//! The semantic event journal: a bounded ring of what changed, read by cursor.
//!
//! The journal records semantic changes — a selection, a tab switch, a feed
//! connection, a replay state, a human mark — never trade prints or frames.
//! It is owned by the application thread, which is the only writer; gateway
//! workers never touch it directly. Two things cross to them: a bounded read
//! that travels the UI request queue like every other capture, and a
//! lock-free signal ([`JournalSignal`]) that tells parked waiters the journal
//! has moved, so `wait_for_change` can park off the UI thread and off the
//! request queue (plan §6.4, contract §10).
//!
//! Eviction is by the earlier of entry capacity and total encoded bytes
//! (`CONTROL_EVENT_JOURNAL_CAPACITY`, `CONTROL_EVENT_JOURNAL_MAX_BYTES`); an
//! event larger than `CONTROL_EVENT_MAX_BYTES` keeps a bounded summary in
//! place of its payload. A read that starts before the oldest retained event
//! says so with `dropped_before`.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender, bounded};
use quantick_control::{
    cursor::{EventCursor, EventJournalBounds},
    id::{EventKind, InstanceId, ModuleId},
    limits::{
        CONTROL_EVENT_JOURNAL_CAPACITY, CONTROL_EVENT_JOURNAL_MAX_BYTES, CONTROL_EVENT_MAX_BYTES,
    },
    wire::{ActorKind, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// How the journal summarises a payload it refuses to retain whole.
const OVERSIZED_PAYLOAD_MARKER: &str = "payload_exceeds_event_limit";

/// Who caused an event, as the journal retains it: the kind and a display
/// name, never a principal that could be mistaken for an identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EventActor {
    pub kind: ActorKind,
    pub client_name: String,
}

/// One semantic event as a client reads it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SemanticEvent {
    /// Monotonic, instance-scoped; the cursor token.
    pub sequence: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub recorded_at_unix_ms: i64,
    pub module_id: ModuleId,
    pub kind: EventKind,
    /// Present when a human or an agent caused the event; absent for a
    /// change the application observed on its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<EventActor>,
    pub payload: Value,
}

/// What a new event carries before the journal stamps it.
pub(crate) struct NewEvent {
    pub module_id: ModuleId,
    pub kind: EventKind,
    pub actor: Option<EventActor>,
    pub payload: Value,
}

/// The journal's position as gateway workers may read it without a lock.
/// `next_sequence` advances on every record; parked waiters compare their
/// target against it, and the tick channel wakes the waiter manager.
pub(crate) struct JournalSignal {
    next_sequence: AtomicU64,
    oldest_sequence: AtomicU64,
    ticks: Sender<()>,
}

impl JournalSignal {
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence.load(Ordering::Acquire)
    }

    pub fn oldest_sequence(&self) -> u64 {
        self.oldest_sequence.load(Ordering::Acquire)
    }

    /// The bounds a waiter resolves its start against, read without a lock.
    /// Oldest is read after next so a concurrent append can only widen the
    /// window, never invert it.
    pub fn bounds(&self) -> EventJournalBounds {
        let next = self.next_sequence();
        let oldest = self.oldest_sequence().min(next);
        EventJournalBounds {
            oldest_sequence: WireU64::new(oldest),
            next_sequence: WireU64::new(next),
        }
    }
}

/// A page of events as the UI-thread read returns it, before the envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EventPage {
    pub instance_id: InstanceId,
    pub events: Vec<SemanticEvent>,
    /// One past the last returned event, or the resolved start when empty.
    pub next_cursor: EventCursor,
    /// Present when the requested position had already been evicted; the
    /// page then begins at the oldest retained event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped_before: Option<EventCursor>,
    pub has_more: bool,
    /// Set by `wait_for_change` when its timeout elapsed and the page is
    /// empty; a change that landed between the deadline and the read is
    /// reported as a change.
    pub timed_out: bool,
}

pub(crate) struct EventJournal {
    events: VecDeque<(SemanticEvent, usize)>,
    next_sequence: u64,
    total_bytes: usize,
    capacity: usize,
    max_bytes: usize,
    signal: Arc<JournalSignal>,
}

impl EventJournal {
    /// A journal with the reviewed bounds. The returned receiver is the tick
    /// the waiter manager listens on; it carries no data, only "look again".
    pub fn new() -> (Self, Receiver<()>) {
        Self::with_bounds(
            CONTROL_EVENT_JOURNAL_CAPACITY,
            CONTROL_EVENT_JOURNAL_MAX_BYTES,
        )
    }

    pub fn with_bounds(capacity: usize, max_bytes: usize) -> (Self, Receiver<()>) {
        let (ticks, tick_rx) = bounded(1);
        let journal = Self {
            events: VecDeque::new(),
            next_sequence: 1,
            total_bytes: 0,
            capacity: capacity.max(1),
            max_bytes,
            signal: Arc::new(JournalSignal {
                next_sequence: AtomicU64::new(1),
                oldest_sequence: AtomicU64::new(1),
                ticks,
            }),
        };
        (journal, tick_rx)
    }

    pub fn signal(&self) -> Arc<JournalSignal> {
        Arc::clone(&self.signal)
    }

    pub fn bounds(&self) -> EventJournalBounds {
        EventJournalBounds {
            oldest_sequence: WireU64::new(self.oldest_sequence()),
            next_sequence: WireU64::new(self.next_sequence),
        }
    }

    fn oldest_sequence(&self) -> u64 {
        self.events
            .front()
            .map_or(self.next_sequence, |(event, _)| event.sequence.get())
    }

    /// Append one event, evict what the bounds no longer hold, publish the
    /// new position and tick the waiters. Returns the event's sequence.
    pub fn record(&mut self, event: NewEvent, recorded_at_unix_ms: i64) -> WireU64 {
        let sequence = WireU64::new(self.next_sequence);
        self.next_sequence = self.next_sequence.saturating_add(1);
        let mut stamped = SemanticEvent {
            sequence,
            recorded_at_unix_ms,
            module_id: event.module_id,
            kind: event.kind,
            actor: event.actor,
            payload: event.payload,
        };
        let mut encoded_bytes = encoded_size(&stamped);
        if encoded_bytes > CONTROL_EVENT_MAX_BYTES {
            // The contract points a large event at a resource; resources land
            // with evidence capture. Until then the event keeps its identity
            // and says what it could not carry, never a silently cut payload.
            stamped.payload = json!({
                "unavailable": OVERSIZED_PAYLOAD_MARKER,
                "payload_bytes": encoded_bytes,
                "limit_bytes": CONTROL_EVENT_MAX_BYTES,
            });
            encoded_bytes = encoded_size(&stamped);
        }
        self.events.push_back((stamped, encoded_bytes));
        self.total_bytes = self.total_bytes.saturating_add(encoded_bytes);
        while self.events.len() > self.capacity
            || (self.total_bytes > self.max_bytes && self.events.len() > 1)
        {
            if let Some((_, bytes)) = self.events.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(bytes);
            }
        }
        // Publish oldest before next: a reader that sees the new next and
        // the old oldest only sees a wider window, which resolve_event_read
        // treats as retained rather than dropped.
        self.signal
            .oldest_sequence
            .store(self.oldest_sequence(), Ordering::Release);
        self.signal
            .next_sequence
            .store(self.next_sequence, Ordering::Release);
        let _ = self.signal.ticks.try_send(());
        sequence
    }

    /// Events from `from_sequence` on, at most `limit` items and roughly
    /// `max_bytes` of encoded payload. `from_sequence` is a resolved position
    /// (see `resolve_event_read`); a position below the oldest retained event
    /// starts at the oldest.
    pub fn read(&self, from_sequence: u64, limit: usize, max_bytes: usize) -> ReadSlice {
        let oldest = self.oldest_sequence();
        let start = from_sequence.max(oldest);
        let skip = usize::try_from(start.saturating_sub(oldest)).unwrap_or(usize::MAX);
        let mut events = Vec::new();
        let mut bytes = 0usize;
        let mut next = start;
        let mut has_more = false;
        for (event, size) in self.events.iter().skip(skip) {
            if events.len() >= limit
                || (!events.is_empty() && bytes.saturating_add(*size) > max_bytes)
            {
                has_more = true;
                break;
            }
            bytes = bytes.saturating_add(*size);
            next = event.sequence.get().saturating_add(1);
            events.push(event.clone());
        }
        ReadSlice {
            events,
            next_sequence: next,
            has_more,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }
}

/// What one read returns before the envelope fields are added.
pub(crate) struct ReadSlice {
    pub events: Vec<SemanticEvent>,
    pub next_sequence: u64,
    pub has_more: bool,
}

fn encoded_size(event: &SemanticEvent) -> usize {
    serde_json::to_vec(event).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, payload: Value) -> NewEvent {
        NewEvent {
            module_id: ModuleId::new("test").unwrap(),
            kind: EventKind::new(kind).unwrap(),
            actor: None,
            payload,
        }
    }

    #[test]
    fn sequences_are_monotonic_and_the_signal_follows_the_journal() {
        let (mut journal, ticks) = EventJournal::with_bounds(8, 1 << 20);
        let signal = journal.signal();
        assert_eq!(signal.next_sequence(), 1);
        let first = journal.record(event("test.one", json!({})), 1);
        let second = journal.record(event("test.two", json!({})), 2);
        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        assert_eq!(signal.next_sequence(), 3);
        assert_eq!(signal.oldest_sequence(), 1);
        // One tick is enough: the channel holds one and the second record
        // found it already pending, which is what "look again" means.
        assert!(ticks.try_recv().is_ok());
        assert!(ticks.try_recv().is_err());
    }

    #[test]
    fn capacity_evicts_the_oldest_and_a_read_below_the_window_starts_at_the_oldest() {
        let (mut journal, _ticks) = EventJournal::with_bounds(3, 1 << 20);
        for index in 0..5 {
            journal.record(event("test.n", json!({ "index": index })), i64::from(index));
        }
        assert_eq!(journal.len(), 3);
        assert_eq!(journal.bounds().oldest_sequence.get(), 3);
        assert_eq!(journal.bounds().next_sequence.get(), 6);
        let slice = journal.read(1, 10, 1 << 20);
        assert_eq!(
            slice
                .events
                .iter()
                .map(|event| event.sequence.get())
                .collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(slice.next_sequence, 6);
        assert!(!slice.has_more);
    }

    #[test]
    fn byte_budget_evicts_too_and_an_oversized_payload_keeps_a_summary() {
        let (mut journal, _ticks) = EventJournal::with_bounds(100, 600);
        for index in 0..10 {
            journal.record(
                event("test.n", json!({ "pad": "x".repeat(100), "index": index })),
                0,
            );
        }
        assert!(
            journal.len() < 10,
            "the byte budget evicts before capacity does"
        );
        assert!(journal.len() >= 1);

        let (mut journal, _ticks) = EventJournal::with_bounds(4, 1 << 30);
        let huge = "y".repeat(CONTROL_EVENT_MAX_BYTES + 1);
        journal.record(event("test.big", json!({ "blob": huge })), 0);
        let slice = journal.read(1, 1, 1 << 30);
        assert_eq!(
            slice.events[0].payload["unavailable"],
            OVERSIZED_PAYLOAD_MARKER
        );
        assert!(
            slice.events[0].payload["payload_bytes"].as_u64().unwrap()
                > CONTROL_EVENT_MAX_BYTES as u64
        );
    }

    #[test]
    fn a_page_is_bounded_by_items_and_by_bytes_and_says_when_more_remain() {
        let (mut journal, _ticks) = EventJournal::with_bounds(100, 1 << 20);
        for index in 0..6 {
            journal.record(event("test.n", json!({ "index": index })), 0);
        }
        let page = journal.read(1, 2, 1 << 20);
        assert_eq!(page.events.len(), 2);
        assert_eq!(page.next_sequence, 3);
        assert!(page.has_more);
        let rest = journal.read(page.next_sequence, 10, 1 << 20);
        assert_eq!(rest.events.len(), 4);
        assert!(!rest.has_more);
        // A byte budget smaller than one event still returns that one event,
        // so a reader always makes progress.
        let tiny = journal.read(1, 10, 1);
        assert_eq!(tiny.events.len(), 1);
        assert!(tiny.has_more);
    }
}
