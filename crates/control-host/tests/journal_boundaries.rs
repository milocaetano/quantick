use quantick_control::id::{EventKind, ModuleId};
use quantick_control::limits::CONTROL_EVENT_MAX_BYTES;
use quantick_control::wire::ActorKind;
use quantick_control_host::journal::{EventActor, EventJournal, NewEvent};
use serde_json::{Value, json};

fn event(payload: Value) -> NewEvent {
    NewEvent {
        module_id: ModuleId::new("workspace").unwrap(),
        kind: EventKind::new("workspace.changed").unwrap(),
        actor: None,
        payload,
    }
}

#[test]
fn zero_item_limit_preserves_the_cursor_and_reports_available_data() {
    let (mut journal, _ticks) = EventJournal::new();
    journal.record(event(json!({"name": "desk"})), 100);
    let withheld = journal.read(1, 0, usize::MAX);
    assert!(withheld.events.is_empty());
    assert_eq!(withheld.next_sequence, 1);
    assert!(withheld.has_more);
    let retried = journal.read(withheld.next_sequence, 1, usize::MAX);
    assert_eq!(retried.events[0].payload, json!({"name": "desk"}));
    assert_eq!(retried.next_sequence, 2);
    assert!(!retried.has_more);
}

#[test]
fn reading_an_empty_journal_or_future_position_does_not_rewind_the_cursor() {
    let (mut journal, _ticks) = EventJournal::new();
    let empty = journal.read(0, 10, usize::MAX);
    assert!(empty.events.is_empty());
    assert_eq!(empty.next_sequence, 1);
    assert!(!empty.has_more);
    journal.record(event(json!({})), 1);
    let future = journal.read(u64::MAX, 10, usize::MAX);
    assert!(future.events.is_empty());
    assert_eq!(future.next_sequence, u64::MAX);
    assert!(!future.has_more);
    assert_eq!(journal.bounds().next_sequence.get(), 2);
}

#[test]
fn zero_capacity_keeps_the_newest_event_and_publishes_the_eviction() {
    let (mut journal, _ticks) = EventJournal::with_bounds(0, usize::MAX);
    let signal = journal.signal();
    journal.record(event(json!({"state": "old"})), 10);
    journal.record(event(json!({"state": "new"})), 20);
    let page = journal.read(1, 10, usize::MAX);
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].payload, json!({"state": "new"}));
    assert_eq!(page.events[0].sequence.get(), 2);
    assert_eq!(page.next_sequence, 3);
    assert_eq!(signal.bounds(), journal.bounds());
    assert_eq!(signal.oldest_sequence(), 2);
    assert!(!page.has_more);
}

#[test]
fn zero_byte_storage_budget_still_retains_one_event_for_reader_progress() {
    let (mut journal, _ticks) = EventJournal::with_bounds(10, 0);
    journal.record(event(json!({"state": "old"})), 1);
    journal.record(event(json!({"state": "new"})), 2);
    let page = journal.read(0, 10, 0);
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].sequence.get(), 2);
    assert_eq!(page.events[0].payload, json!({"state": "new"}));
    assert_eq!(page.next_sequence, 3);
    assert!(!page.has_more);
}

#[test]
fn exact_read_byte_boundary_includes_the_last_fitting_event() {
    let (mut journal, _ticks) = EventJournal::new();
    journal.record(event(json!({"name": "one"})), 1);
    journal.record(event(json!({"name": "two"})), 2);
    let all = journal.read(1, 10, usize::MAX);
    let exact: usize = all
        .events
        .iter()
        .map(|entry| serde_json::to_vec(entry).unwrap().len())
        .sum();
    let fits = journal.read(1, 10, exact);
    assert_eq!(fits.events, all.events);
    assert!(!fits.has_more);
    let short = journal.read(1, 10, exact - 1);
    assert_eq!(short.events.len(), 1);
    assert_eq!(short.next_sequence, 2);
    assert!(short.has_more);
    let rest = journal.read(short.next_sequence, 10, exact - 1);
    assert_eq!(rest.events, all.events[1..]);
    assert!(!rest.has_more);
}

#[test]
fn exact_storage_byte_boundary_retains_two_events_until_another_arrives() {
    let (mut probe, _ticks) = EventJournal::new();
    probe.record(event(json!({"name": "one"})), 1);
    probe.record(event(json!({"name": "two"})), 2);
    let page = probe.read(1, 10, usize::MAX);
    let exact: usize = page
        .events
        .iter()
        .map(|entry| serde_json::to_vec(entry).unwrap().len())
        .sum();
    let (mut journal, _ticks) = EventJournal::with_bounds(10, exact);
    journal.record(event(json!({"name": "one"})), 1);
    journal.record(event(json!({"name": "two"})), 2);
    assert_eq!(journal.read(1, 10, usize::MAX).events, page.events);
    journal.record(event(json!({"name": "six"})), 3);
    let retained = journal.read(1, 10, usize::MAX);
    assert_eq!(retained.events.len(), 2);
    assert_eq!(retained.events[0].sequence.get(), 2);
    assert_eq!(retained.events[1].sequence.get(), 3);
    assert_eq!(journal.bounds().oldest_sequence.get(), 2);
}

#[test]
fn a_large_replacement_reclaims_the_bytes_of_all_evicted_events() {
    let (mut probe, _ticks) = EventJournal::new();
    probe.record(event(json!({"blob": "a".repeat(100)})), 1);
    let event_bytes = serde_json::to_vec(&probe.read(1, 1, usize::MAX).events[0])
        .unwrap()
        .len();
    let (mut journal, _ticks) = EventJournal::with_bounds(10, event_bytes * 2);
    journal.record(event(json!({"blob": "a".repeat(100)})), 1);
    journal.record(event(json!({"blob": "b".repeat(100)})), 2);
    journal.record(event(json!({"blob": "c".repeat(500)})), 3);
    assert_eq!(journal.read(1, 10, usize::MAX).events.len(), 1);
    journal.record(event(json!({"blob": "d".repeat(100)})), 4);
    journal.record(event(json!({"blob": "e".repeat(100)})), 5);
    let page = journal.read(1, 10, usize::MAX);
    assert_eq!(
        page.events.len(),
        2,
        "evicted bytes must no longer consume the budget"
    );
    assert_eq!(page.events[0].sequence.get(), 4);
    assert_eq!(page.events[1].sequence.get(), 5);
}

#[test]
fn oversized_payload_summary_preserves_actor_identity_time_and_kind() {
    let (mut journal, _ticks) = EventJournal::new();
    let actor = EventActor {
        kind: ActorKind::Agent,
        client_name: "test operator".to_owned(),
    };
    let mut change = event(json!({"blob": "x".repeat(CONTROL_EVENT_MAX_BYTES)}));
    change.actor = Some(actor.clone());
    journal.record(change, -123);
    let page = journal.read(1, 1, usize::MAX);
    let retained = &page.events[0];
    assert_eq!(retained.actor, Some(actor));
    assert_eq!(retained.recorded_at_unix_ms, -123);
    assert_eq!(retained.module_id.as_str(), "workspace");
    assert_eq!(retained.kind.as_str(), "workspace.changed");
    assert_eq!(retained.sequence.get(), 1);
    assert_eq!(
        retained.payload["unavailable"],
        "payload_exceeds_event_limit"
    );
    assert!(retained.payload["payload_bytes"].as_u64().unwrap() > CONTROL_EVENT_MAX_BYTES as u64);
    assert_eq!(retained.payload["limit_bytes"], CONTROL_EVENT_MAX_BYTES);
    assert!(retained.payload.get("blob").is_none());
    assert!(serde_json::to_vec(retained).unwrap().len() < CONTROL_EVENT_MAX_BYTES);
}

#[test]
fn event_size_limit_is_inclusive_and_counts_utf8_encoded_bytes() {
    let (mut probe, _ticks) = EventJournal::new();
    probe.record(event(json!({"blob": ""})), 1);
    let overhead = serde_json::to_vec(&probe.read(1, 1, usize::MAX).events[0])
        .unwrap()
        .len();
    let bytes = CONTROL_EVENT_MAX_BYTES - overhead;
    let payload = format!("{}{}", "\u{00e9}".repeat(bytes / 2), "a".repeat(bytes % 2));
    assert_eq!(payload.len(), bytes);
    let (mut journal, _ticks) = EventJournal::new();
    journal.record(event(json!({"blob": payload})), 1);
    let at_limit = journal.read(1, 1, usize::MAX);
    assert_eq!(
        at_limit.events[0].payload["blob"].as_str().unwrap().len(),
        bytes
    );
    assert_eq!(
        serde_json::to_vec(&at_limit.events[0]).unwrap().len(),
        CONTROL_EVENT_MAX_BYTES
    );
    let (mut journal, _ticks) = EventJournal::new();
    journal.record(event(json!({"blob": format!("{payload}a")})), 1);
    let over = journal.read(1, 1, usize::MAX);
    assert_eq!(
        over.events[0].payload["payload_bytes"],
        CONTROL_EVENT_MAX_BYTES + 1
    );
    assert!(over.events[0].payload.get("blob").is_none());
}

#[test]
fn a_drained_tick_is_rearmed_by_the_next_append() {
    let (mut journal, ticks) = EventJournal::new();
    journal.record(event(json!({"revision": 1})), 1);
    assert!(ticks.try_recv().is_ok());
    assert!(ticks.try_recv().is_err());
    journal.record(event(json!({"revision": 2})), 2);
    assert!(ticks.try_recv().is_ok());
    assert_eq!(journal.signal().next_sequence(), 3);
    assert!(ticks.try_recv().is_err());
}

#[test]
fn a_disconnected_waiter_does_not_prevent_retention_or_signal_publication() {
    let (mut journal, ticks) = EventJournal::with_bounds(1, usize::MAX);
    let signal = journal.signal();
    drop(ticks);
    journal.record(event(json!({"revision": 1})), 1);
    journal.record(event(json!({"revision": 2})), 2);
    assert_eq!(signal.oldest_sequence(), 2);
    assert_eq!(signal.next_sequence(), 3);
    assert_eq!(signal.bounds(), journal.bounds());
    assert_eq!(
        journal.read(1, 10, usize::MAX).events[0].payload,
        json!({"revision": 2})
    );
}

#[test]
fn returned_events_are_owned_snapshots_that_cannot_edit_retained_history() {
    let (mut journal, _ticks) = EventJournal::new();
    journal.record(event(json!({"nested": {"name": "original"}})), 1);
    let mut first = journal.read(1, 1, usize::MAX);
    first.events[0].payload["nested"]["name"] = json!("edited");
    journal.record(event(json!({"name": "later"})), 2);
    let again = journal.read(1, 1, usize::MAX);
    assert_eq!(
        again.events[0].payload,
        json!({"nested": {"name": "original"}})
    );
    assert_eq!(again.next_sequence, 2);
    assert!(again.has_more);
    assert_eq!(first.events[0].payload["nested"]["name"], "edited");
}
