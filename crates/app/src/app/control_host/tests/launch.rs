//! The launch hooks read into the gateway's seat.

use super::super::*;

fn launch(inputs: &[(&str, &str)]) -> ControlLaunch {
    ControlLaunch::capture(|name| {
        inputs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| std::ffi::OsString::from(value))
    })
}

fn state(launch: ControlLaunch, access: crate::control::ControlAccess) -> ControlState {
    let mut state = ControlState {
        control_access: None,
        scenarios: LaunchScenarios::default(),
    };
    state.apply_launch(launch);
    state.control_access = Some(access);
    state
}

fn prepare(state: &mut ControlState) -> Option<quantick_control_host::launch::EvidenceCapture> {
    let access = state
        .control_access
        .take()
        .expect("the test installs access");
    let capture = state.prepare_evidence(&access);
    state.control_access = Some(access);
    capture
}

#[test]
fn captured_enable_and_mark_are_consumed_once() {
    let mut launch = launch(&[
        ("QUANTICK_CONTROL_ACCESS", "1"),
        ("QUANTICK_CONTROL_MARK", " 1 "),
    ]);
    assert!(launch.scenarios.take_enable());
    assert!(!launch.scenarios.take_enable());
    assert_eq!(launch.scenarios.take_mark().as_deref(), Some(" 1 "));
    assert_eq!(launch.scenarios.take_mark(), None);
}

#[test]
fn annotation_retains_raw_text_until_an_anchor_exists() {
    let mut launch = launch(&[("QUANTICK_CONTROL_ANNOTATE", " keep this ")]);
    assert!(launch.scenarios.annotation(None).is_none());
    assert!(launch.scenarios.has_annotation());
    let anchor = serde_json::json!({"time_unix_ms": 1800, "price": "100.8"});
    assert_eq!(
        launch.scenarios.annotation(Some(anchor.clone())),
        Some(serde_json::json!({"anchors": [anchor], "text": " keep this "}))
    );
    assert!(!launch.scenarios.has_annotation());
}

#[test]
fn notification_preserves_message_and_types_unknown_channel() {
    let mut launch = launch(&[("QUANTICK_CONTROL_NOTIFY", " popup :a:b ")]);
    let Some(NotificationStep::Ready { capability, input }) = launch.scenarios.notification()
    else {
        panic!("registered popup must be ready");
    };
    assert_eq!(capability, "notify.popup");
    assert_eq!(
        input,
        serde_json::json!({"message": "a:b ", "title": "From your assistant"})
    );
    assert!(launch.scenarios.notification().is_none());
    launch
        .scenarios
        .queue_notification(" unknown :message".into());
    let Some(NotificationStep::Refused { channel }) = launch.scenarios.notification() else {
        panic!("unknown channel must be typed refusal");
    };
    assert_eq!(channel, "unknown");
    assert!(launch.scenarios.notification().is_none());
}

#[test]
fn evidence_reexpands_current_grants_and_keeps_its_lifetime_wait_count() {
    let mut access = crate::control::ControlAccess::new();
    access
        .configure_scopes("all-reads,observe.evidence,observe.screenshot")
        .unwrap();
    let mut state = state(
        launch(&[("QUANTICK_CONTROL_EVIDENCE", "all,screenshot")]),
        access,
    );
    for _ in 0..CONTROL_EVIDENCE_HOOK_FRAMES {
        let capture = prepare(&mut state).unwrap();
        assert!(capture.wants_screenshot);
        assert!(!capture.screenshot_not_granted);
        assert!(matches!(
            state.scenarios.finish_evidence(capture, false),
            EvidenceStep::Waiting
        ));
    }
    state
        .control_access
        .as_mut()
        .unwrap()
        .configure_scopes("observe.events,observe.evidence,observe.screenshot")
        .unwrap();
    let capture = prepare(&mut state).unwrap();
    let expected: std::collections::BTreeSet<_> = state
        .control_access
        .as_ref()
        .unwrap()
        .readable_scopes()
        .into_iter()
        .map(|scope| scope.to_string())
        .collect();
    assert_eq!(capture.scopes, expected);
    assert!(matches!(
        state.scenarios.finish_evidence(capture, false),
        EvidenceStep::Capture {
            screenshot: true,
            image_timed_out: true,
            ..
        }
    ));
    assert!(!state.scenarios.has_evidence());
    state.scenarios.queue_evidence("all,screenshot".into());
    let capture = prepare(&mut state).unwrap();
    assert!(matches!(
        state.scenarios.finish_evidence(capture, false),
        EvidenceStep::Capture {
            image_timed_out: true,
            ..
        }
    ));
}

#[test]
fn screenshot_permission_is_checked_before_waiting() {
    let mut state = state(
        launch(&[("QUANTICK_CONTROL_EVIDENCE", "all,screenshot")]),
        crate::control::ControlAccess::new(),
    );
    let capture = prepare(&mut state).unwrap();
    assert!(capture.screenshot_not_granted);
    assert!(!capture.wants_screenshot);
    assert!(matches!(
        state.scenarios.finish_evidence(capture, false),
        EvidenceStep::Capture {
            screenshot: false,
            image_timed_out: false,
            ..
        }
    ));
    assert_eq!(state.scenarios.evidence_frames(), 0);
}
