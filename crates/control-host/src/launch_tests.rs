use super::*;

fn scopes(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn captured(inputs: &[(&str, &str)]) -> ControlLaunch {
    ControlLaunch::capture(|name| {
        let found = inputs.iter().find(|(key, _)| *key == name);
        found.map(|(_, value)| std::ffi::OsString::from(value))
    })
}

#[test]
fn a_capture_reads_the_panel_scopes_and_a_bare_mark() {
    let mut launch = captured(&[
        ("QUANTICK_CONTROL_PANEL", "1"),
        ("QUANTICK_CONTROL_SCOPES", "observe"),
        ("QUANTICK_CONTROL_ACCESS", "1"),
        ("QUANTICK_CONTROL_MARK", "1"),
        ("QUANTICK_CONTROL_NOTIFY", "  "),
    ]);
    assert!(launch.panel);
    assert_eq!(launch.scopes.as_deref(), Some("observe"));
    assert!(launch.scenarios.take_enable());
    assert_eq!(launch.scenarios.take_mark().as_deref(), Some(""));
    assert!(
        launch.scenarios.notification().is_none(),
        "a blank hook is unset"
    );
    let quiet = captured(&[("QUANTICK_CONTROL_PANEL", "0")]);
    assert!(!quiet.panel && quiet.scopes.is_none());
}

#[test]
fn enable_and_mark_are_consumed_once() {
    let mut launch = LaunchScenarios::new(LaunchRequests {
        enable_access: true,
        mark: Some(" 1 ".to_owned()),
        ..LaunchRequests::default()
    });
    assert!(launch.take_enable());
    assert!(!launch.take_enable());
    assert_eq!(launch.take_mark().as_deref(), Some(" 1 "));
    assert_eq!(launch.take_mark(), None);
}

#[test]
fn a_bare_notification_goes_to_the_toast() {
    let mut launch = LaunchScenarios::default();
    launch.queue_notification("hello".to_owned());
    let Some(NotificationStep::Ready { capability, input }) = launch.notification() else {
        panic!("a bare message must be a toast");
    };
    assert_eq!(capability, "notify.toast");
    assert_eq!(input["message"], "hello");
}

#[test]
fn an_explicit_scope_list_never_asks_for_the_readable_scopes() {
    let mut launch = LaunchScenarios::default();
    launch.queue_evidence("observe.chart".to_owned());
    let capture = launch
        .prepare_evidence(
            || panic!("an explicit scope list needs no expansion"),
            false,
        )
        .unwrap();
    assert_eq!(capture.scopes, BTreeSet::from(["observe.chart".to_owned()]));
    assert!(matches!(
        launch.finish_evidence(capture, false),
        EvidenceStep::Capture {
            screenshot: false,
            image_timed_out: false,
            ..
        }
    ));
}

#[test]
fn an_empty_request_reads_every_readable_scope() {
    let mut launch = LaunchScenarios::default();
    launch.queue_evidence(" , ".to_owned());
    let capture = launch
        .prepare_evidence(|| scopes(&["observe.chart", "observe.feed"]), true)
        .unwrap();
    assert_eq!(capture.scopes.len(), 2);
}

#[test]
fn the_evidence_hook_waits_its_budget_and_then_stops() {
    let mut scenarios = LaunchScenarios::default();
    for frame in 1..=CONTROL_EVIDENCE_HOOK_FRAMES {
        assert!(
            scenarios.evidence_frame_waited(),
            "frame {frame} is within the budget"
        );
    }
    assert!(
        !scenarios.evidence_frame_waited(),
        "the window never delivered a frame to rasterise"
    );
}
