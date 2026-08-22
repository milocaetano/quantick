//! The event capabilities: `events.read` and `events.wait`.
//!
//! `events.read` is an ordinary UI-thread read of the journal, bounded like
//! every other capture. `events.wait` is the one tool built for patience: it
//! parks on the gateway side — holding a parked-waiter slot, never a UI
//! request slot — until the journal moves past its position or its timeout
//! elapses, and only then enters the UI queue for the bounded read that
//! completes the call (plan §6.4, contract §10).

use quantick_control::{
    cursor::{EventCursor, EventStart, resolve_event_read},
    error::ControlError,
    id::InstanceId,
    limits::{CONTROL_DEFAULT_PAGE_ITEMS, CONTROL_MAX_RESPONSE_BYTES, CONTROL_WAIT_TIMEOUT_MAX_MS},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::journal::{EventJournal, EventPage};

pub(crate) const READ_CAPABILITY_ID: &str = "events.read";
pub(crate) const WAIT_CAPABILITY_ID: &str = "events.wait";
pub(crate) const EVENTS_MODULE_ID: &str = "events";
pub(crate) const EVENTS_PERMISSION_ID: &str = "observe.events";

/// Keep one page well inside the response limit even when every event is at
/// its own maximum size.
const PAGE_BYTE_BUDGET: usize = CONTROL_MAX_RESPONSE_BYTES / 4;

/// `events.read`: a cursor from a previous page, or an explicit start.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventsReadInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<EventCursor>,
    /// Required on a first read (there is no implicit start position);
    /// forbidden together with a cursor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<EventStart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = CONTROL_DEFAULT_PAGE_ITEMS))]
    pub limit: Option<usize>,
}

/// `events.wait`: the same position, plus how long to wait for it to move.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventsWaitInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<EventCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<EventStart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = CONTROL_DEFAULT_PAGE_ITEMS))]
    pub limit: Option<usize>,
    #[schemars(range(min = 1, max = CONTROL_WAIT_TIMEOUT_MAX_MS), extend("x-unit" = "milliseconds"))]
    pub timeout_ms: u64,
}

/// A resolved read: where to start and what the resolution had to say about
/// retention. Produced on the UI thread from the journal's own bounds, or on
/// the gateway side from the journal's published bounds.
pub(crate) fn read_page(
    journal: &EventJournal,
    instance_id: &InstanceId,
    cursor: Option<&EventCursor>,
    start: Option<EventStart>,
    limit: Option<usize>,
    timed_out: bool,
) -> Result<EventPage, ControlError> {
    let position = resolve_event_read(instance_id, cursor, start, journal.bounds())?;
    let limit = limit
        .unwrap_or(CONTROL_DEFAULT_PAGE_ITEMS)
        .clamp(1, CONTROL_DEFAULT_PAGE_ITEMS);
    let slice = journal.read(position.next_sequence.get(), limit, PAGE_BYTE_BUDGET);
    Ok(EventPage {
        instance_id: instance_id.clone(),
        events: slice.events,
        next_cursor: EventCursor {
            instance_id: instance_id.clone(),
            next_sequence: quantick_control::wire::WireU64::new(slice.next_sequence),
        },
        dropped_before: position.dropped_before,
        has_more: slice.has_more,
        timed_out,
    })
}

/// Finish the page a `wait_for_change` returns. A retention gap the gateway
/// saw while resolving the wait is reported even though the read itself
/// started at the clamped position, and `timed_out` is honest: a change that
/// landed between the deadline and the read is a change, not a timeout.
pub(crate) fn complete_wait_page(
    mut page: EventPage,
    dropped_before: Option<EventCursor>,
) -> EventPage {
    if page.dropped_before.is_none() {
        page.dropped_before = dropped_before;
    }
    if page.timed_out && !page.events.is_empty() {
        page.timed_out = false;
    }
    page
}

#[cfg(test)]
mod tests {
    use quantick_control::{
        error::codes,
        id::{EventKind, ModuleId},
        wire::WireU64,
    };
    use serde_json::json;

    use super::*;
    use crate::control::journal::NewEvent;

    fn instance() -> InstanceId {
        InstanceId::from_bytes([5; 16])
    }

    fn record(journal: &mut EventJournal, index: u64) {
        journal.record(
            NewEvent {
                module_id: ModuleId::new("test").unwrap(),
                kind: EventKind::new("test.n").unwrap(),
                actor: None,
                payload: json!({ "index": index }),
            },
            0,
        );
    }

    #[test]
    fn a_first_read_needs_an_explicit_start_and_pages_from_there() {
        let (mut journal, _ticks) = EventJournal::with_bounds(16, 1 << 20);
        for index in 0..5 {
            record(&mut journal, index);
        }
        let neither = read_page(&journal, &instance(), None, None, None, false).unwrap_err();
        assert_eq!(neither.code.as_str(), codes::CURSOR_INVALID);

        let latest = read_page(
            &journal,
            &instance(),
            None,
            Some(EventStart::Latest),
            None,
            false,
        )
        .unwrap();
        assert!(latest.events.is_empty());
        assert_eq!(latest.next_cursor.next_sequence.get(), 6);

        let oldest = read_page(
            &journal,
            &instance(),
            None,
            Some(EventStart::Oldest),
            Some(2),
            false,
        )
        .unwrap();
        assert_eq!(oldest.events.len(), 2);
        assert!(oldest.has_more);
        assert_eq!(oldest.next_cursor.next_sequence.get(), 3);
        let rest = read_page(
            &journal,
            &instance(),
            Some(&oldest.next_cursor),
            None,
            None,
            false,
        )
        .unwrap();
        assert_eq!(rest.events.len(), 3);
        assert!(!rest.has_more);
        assert!(rest.dropped_before.is_none());
    }

    #[test]
    fn an_expired_cursor_reports_dropped_before_and_a_foreign_cursor_is_invalid() {
        let (mut journal, _ticks) = EventJournal::with_bounds(3, 1 << 20);
        for index in 0..6 {
            record(&mut journal, index);
        }
        let expired = EventCursor {
            instance_id: instance(),
            next_sequence: WireU64::new(1),
        };
        let page = read_page(&journal, &instance(), Some(&expired), None, None, false).unwrap();
        assert_eq!(page.dropped_before.as_ref().unwrap().next_sequence.get(), 4);
        assert_eq!(page.events[0].sequence.get(), 4);

        let foreign = EventCursor {
            instance_id: InstanceId::from_bytes([9; 16]),
            next_sequence: WireU64::new(4),
        };
        let error =
            read_page(&journal, &instance(), Some(&foreign), None, None, false).unwrap_err();
        assert_eq!(error.code.as_str(), codes::CURSOR_INVALID);
    }

    #[test]
    fn a_wait_page_keeps_the_gap_the_gateway_saw_and_is_no_timeout_once_events_landed() {
        let (mut journal, _ticks) = EventJournal::with_bounds(4, 1 << 20);
        for index in 0..8 {
            record(&mut journal, index);
        }
        let bounds = journal.bounds();
        // The gateway resolved a cursor from behind retention to the oldest
        // retained event before parking; the read starts there and, alone,
        // sees no gap.
        let evicted = EventCursor {
            instance_id: instance(),
            next_sequence: bounds.oldest_sequence,
        };
        let page = read_page(&journal, &instance(), Some(&evicted), None, None, true).unwrap();
        assert!(page.dropped_before.is_none());
        assert!(page.timed_out);
        let completed = complete_wait_page(page, Some(evicted.clone()));
        assert_eq!(completed.dropped_before, Some(evicted));
        assert!(
            !completed.timed_out,
            "events that landed between the deadline and the read are a change"
        );
        // An empty page past the deadline stays a timeout.
        let latest = EventCursor {
            instance_id: instance(),
            next_sequence: bounds.next_sequence,
        };
        let empty = read_page(&journal, &instance(), Some(&latest), None, None, true).unwrap();
        assert!(complete_wait_page(empty, None).timed_out);
    }
}
