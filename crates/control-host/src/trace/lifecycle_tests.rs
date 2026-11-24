use super::*;
use crate::scratch::ScratchDir;
use quantick_control::id::ModuleId;
use serde_json::json;

fn intent(sequence: u64, elapsed: i64) -> TraceEntry {
    TraceEntry {
        trace_version: TRACE_VERSION,
        replay_elapsed_ms: elapsed,
        sequence: WireU64::new(sequence),
        actor_kind: ActorKind::Agent,
        client_name: "trace test".to_owned(),
        capability_id: CapabilityId::new("attention.mark.create").unwrap(),
        capability_version: 1,
        canonical_input: json!({"label": "review"}),
        expected_revisions: vec![ModuleRevision {
            module_id: ModuleId::new("attention").unwrap(),
            revision: WireU64::new(7),
        }],
        result_code: None,
        result_digest: None,
    }
}

fn terminal(mut entry: TraceEntry, code: &str) -> TraceEntry {
    entry.result_code = Some(ErrorCode::new(code).unwrap());
    entry.result_digest = result_digest(&json!({"created": 1}));
    entry
}

#[test]
fn reopening_a_trace_appends_without_truncating_prior_actions_or_the_recording() {
    let directory = ScratchDir::new("trace-reopen");
    let session = directory.join("session.csv");
    let recording = b"timestamp,price\n1000,100\n";
    std::fs::write(&session, recording).unwrap();
    {
        let mut writer = ReplayTraceFile::open(&session).unwrap();
        writer.append_intent(&intent(1, 100)).unwrap();
        writer
            .append_result(&terminal(intent(1, 100), "control.ok"))
            .unwrap();
    }
    let before = std::fs::read(ReplayTraceFile::path_for(&session)).unwrap();
    {
        let mut writer = ReplayTraceFile::open(&session).unwrap();
        writer.append_intent(&intent(2, 200)).unwrap();
        writer
            .append_result(&terminal(intent(2, 200), "control.ok"))
            .unwrap();
    }
    let after = std::fs::read(ReplayTraceFile::path_for(&session)).unwrap();
    assert!(after.starts_with(&before));
    assert_eq!(std::fs::read(&session).unwrap(), recording);
    let loaded = TraceReplay::load(&session).unwrap();
    assert!(loaded.is_complete());
    assert_eq!(
        loaded.completed,
        [
            terminal(intent(1, 100), "control.ok"),
            terminal(intent(2, 200), "control.ok")
        ]
    );
    assert_eq!(loaded.max_sequence, 2);
}

#[test]
fn an_orphan_result_cannot_become_a_replayable_action_but_reserves_its_sequence() {
    let directory = ScratchDir::new("trace-orphan");
    let session = directory.join("session.csv");
    let mut writer = ReplayTraceFile::open(&session).unwrap();
    writer
        .append_result(&terminal(intent(99, 100), "control.ok"))
        .unwrap();
    drop(writer);
    let loaded = TraceReplay::load(&session).unwrap();
    assert!(loaded.completed.is_empty());
    assert!(loaded.incomplete.is_empty());
    assert!(loaded.is_complete());
    assert_eq!(loaded.max_sequence, 99);
}

#[test]
fn a_result_before_its_intent_does_not_retroactively_complete_it() {
    let directory = ScratchDir::new("trace-order");
    let session = directory.join("session.csv");
    let mut writer = ReplayTraceFile::open(&session).unwrap();
    writer
        .append_result(&terminal(intent(1, 100), "control.ok"))
        .unwrap();
    writer.append_intent(&intent(1, 100)).unwrap();
    drop(writer);
    let loaded = TraceReplay::load(&session).unwrap();
    assert!(loaded.completed.is_empty());
    assert_eq!(loaded.incomplete, [1]);
    assert!(!loaded.is_complete());
}

#[test]
fn terminal_errors_complete_intents_and_keep_their_result_evidence() {
    let directory = ScratchDir::new("trace-refusal");
    let session = directory.join("session.csv");
    let mut writer = ReplayTraceFile::open(&session).unwrap();
    let request = intent(1, 500);
    let result = terminal(
        request.clone(),
        quantick_control::error::codes::INVALID_REQUEST,
    );
    writer.append_intent(&request).unwrap();
    writer.append_result(&result).unwrap();
    drop(writer);
    let loaded = TraceReplay::load(&session).unwrap();
    assert!(loaded.is_complete());
    assert_eq!(loaded.completed, [result]);
    assert_eq!(
        loaded.completed[0].expected_revisions,
        request.expected_revisions
    );
    assert_eq!(loaded.completed[0].canonical_input, request.canonical_input);
    assert_eq!(loaded.max_sequence, 1);
}

#[test]
fn completed_actions_sort_by_replay_time_then_sequence_not_append_order() {
    let directory = ScratchDir::new("trace-time-order");
    let session = directory.join("session.csv");
    let mut writer = ReplayTraceFile::open(&session).unwrap();
    for (sequence, elapsed) in [(9, 500), (7, 100), (3, 100), (4, -1)] {
        writer.append_intent(&intent(sequence, elapsed)).unwrap();
        writer
            .append_result(&terminal(intent(sequence, elapsed), "control.ok"))
            .unwrap();
    }
    drop(writer);
    let loaded = TraceReplay::load(&session).unwrap();
    assert_eq!(
        loaded
            .completed
            .iter()
            .map(|entry| entry.sequence.get())
            .collect::<Vec<_>>(),
        [4, 3, 7, 9]
    );
    assert_eq!(
        loaded
            .completed
            .iter()
            .map(|entry| entry.replay_elapsed_ms)
            .collect::<Vec<_>>(),
        [-1, 100, 100, 500]
    );
    assert_eq!(loaded.max_sequence, 9);
    assert!(loaded.is_complete());
}

#[test]
fn duplicate_terminal_lines_do_not_replay_one_intent_twice() {
    let directory = ScratchDir::new("trace-duplicate-result");
    let session = directory.join("session.csv");
    let mut writer = ReplayTraceFile::open(&session).unwrap();
    writer.append_intent(&intent(1, 100)).unwrap();
    let result = terminal(intent(1, 100), "control.ok");
    writer.append_result(&result).unwrap();
    writer.append_result(&result).unwrap();
    drop(writer);
    let loaded = TraceReplay::load(&session).unwrap();
    assert_eq!(loaded.completed, [result]);
    assert!(loaded.is_complete());
    assert_eq!(loaded.max_sequence, 1);
}

#[test]
fn a_truncated_line_refuses_the_whole_trace_instead_of_returning_partial_success() {
    let directory = ScratchDir::new("trace-truncated");
    let session = directory.join("session.csv");
    let first = serde_json::to_string(&intent(1, 100)).unwrap();
    let result = serde_json::to_string(&terminal(intent(1, 100), "control.ok")).unwrap();
    std::fs::write(
        ReplayTraceFile::path_for(&session),
        format!("{first}\n{result}\n{{\"trace_version\":"),
    )
    .unwrap();
    let error = TraceReplay::load(&session).unwrap_err();
    assert!(matches!(error, TraceError::Malformed(_)));
    assert!(error.to_string().contains("line 3"));
}

#[test]
fn a_future_trace_version_reports_its_physical_line_after_blank_lines() {
    let directory = ScratchDir::new("trace-version");
    let session = directory.join("session.csv");
    let mut entry = intent(1, 100);
    entry.trace_version = TRACE_VERSION + 1;
    std::fs::write(
        ReplayTraceFile::path_for(&session),
        format!("\n \t\n{}\n", serde_json::to_string(&entry).unwrap()),
    )
    .unwrap();
    let error = TraceReplay::load(&session).unwrap_err();
    assert!(matches!(error, TraceError::Malformed(_)));
    assert!(error.to_string().contains("line 3 has version 2"));
    assert!(error.to_string().contains("expected 1"));
}

#[test]
fn blank_lines_and_a_missing_final_newline_do_not_hide_valid_records() {
    let directory = ScratchDir::new("trace-newlines");
    let session = directory.join("session.csv");
    let first = serde_json::to_string(&intent(1, 100)).unwrap();
    let result = terminal(intent(1, 100), "control.ok");
    let encoded = serde_json::to_string(&result).unwrap();
    std::fs::write(
        ReplayTraceFile::path_for(&session),
        format!("\n \t\n{first}\r\n\n{encoded}"),
    )
    .unwrap();
    let loaded = TraceReplay::load(&session).unwrap();
    assert_eq!(loaded.completed, [result]);
    assert!(loaded.is_complete());
}

#[test]
fn invalid_utf8_is_an_io_read_failure_and_does_not_become_an_empty_trace() {
    let directory = ScratchDir::new("trace-utf8");
    let session = directory.join("session.csv");
    std::fs::write(ReplayTraceFile::path_for(&session), [0xff, 0xfe, b'\n']).unwrap();
    let error = TraceReplay::load(&session).unwrap_err();
    assert!(matches!(error, TraceError::Io(_)));
    assert!(error.to_string().contains("read control trace"));
}

#[test]
fn a_missing_parent_directory_refuses_append_without_creating_a_recording() {
    let directory = ScratchDir::new("trace-missing-parent");
    let session = directory.join("absent").join("session.csv");
    let error = ReplayTraceFile::open(&session).err().unwrap();
    assert!(matches!(error, TraceError::Io(_)));
    assert!(error.to_string().contains("open control trace"));
    assert!(!session.exists());
    assert!(!ReplayTraceFile::path_for(&session).exists());
}

#[test]
fn json_strings_with_newlines_stay_inside_one_physical_trace_record() {
    let directory = ScratchDir::new("trace-escaped-newlines");
    let session = directory.join("session.csv");
    let mut entry = intent(1, 100);
    entry.canonical_input = json!({"label": "first\nsecond\r\nthird"});
    let mut writer = ReplayTraceFile::open(&session).unwrap();
    writer.append_intent(&entry).unwrap();
    let result = terminal(entry, "control.ok");
    writer.append_result(&result).unwrap();
    drop(writer);
    let text = std::fs::read_to_string(ReplayTraceFile::path_for(&session)).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert_eq!(TraceReplay::load(&session).unwrap().completed, [result]);
}

#[test]
fn result_digest_ignores_object_key_order_but_preserves_array_order() {
    let first = json!({"status": "ok", "nested": {"a": 1, "b": [2, 3]}});
    let reordered: Value =
        serde_json::from_str(r#"{"nested":{"b":[2,3],"a":1},"status":"ok"}"#).unwrap();
    let changed = json!({"status": "ok", "nested": {"a": 1, "b": [3, 2]}});
    let digest = result_digest(&first).unwrap();
    assert_eq!(Some(digest.clone()), result_digest(&reordered));
    assert_ne!(Some(digest), result_digest(&changed));
}
