use super::*;

#[test]
fn launch_capture_reads_each_input_once_in_legacy_order_without_mutation() {
    let mut reads = Vec::new();
    let launch = ToolRailLaunch::capture(|name| {
        reads.push(name.to_owned());
        match name {
            "QUANTICK_DRAWING_TOOL" => Some(" horizontal-line ".into()),
            "QUANTICK_DRAWING_MAGNET" => Some("1".into()),
            "QUANTICK_TOOL_FAVORITES" => Some("measure,measure,unknown".into()),
            "QUANTICK_TOOLBOX_DOCK" => Some(" top ".into()),
            "QUANTICK_TOOLBAR_SCROLL" => Some(" end ".into()),
            "QUANTICK_TOOLBOX_FLYOUT" => Some(" lines ".into()),
            _ => unreachable!(),
        }
    });
    assert_eq!(
        reads,
        [
            "QUANTICK_DRAWING_TOOL",
            "QUANTICK_DRAWING_MAGNET",
            "QUANTICK_TOOL_FAVORITES",
            "QUANTICK_TOOLBOX_DOCK",
            "QUANTICK_TOOLBAR_SCROLL",
            "QUANTICK_TOOLBOX_FLYOUT",
        ]
    );
    let mut rail = ToolRail::new();
    assert_eq!(rail.tool(), Tool::Pointer);
    assert!(!rail.magnet());
    assert!(rail.favorites().is_empty());
    let outcome = rail.apply_launch(launch);
    assert!(outcome.favorites_staged);
    assert_eq!(rail.tool().id(), "horizontal-line");
    assert!(rail.magnet());
    assert_eq!(
        rail.favorites()
            .iter()
            .map(|tool| tool.id())
            .collect::<Vec<_>>(),
        ["measure"]
    );
    assert_eq!(rail.dock(), ToolboxDock::Top);
    assert_eq!(
        rail.launch_pending_for_test(),
        (Some(f32::INFINITY), Some("lines"))
    );
}

#[test]
fn absent_or_invalid_launch_inputs_preserve_the_existing_recipient_state() {
    let mut rail = ToolRail::new();
    rail.arm(Tool::Drawing(DrawingTool::by_id("measure").unwrap()));
    rail.set_magnet(true);
    rail.set_favorites(&["measure".into()]);
    rail.set_dock(ToolboxDock::Bottom);
    rail.set_band_offset(27.0);
    rail.request_flyout("lines".into());
    let outcome = rail.apply_launch(ToolRailLaunch::capture(|name| match name {
        "QUANTICK_DRAWING_TOOL" => Some("unknown".into()),
        "QUANTICK_DRAWING_MAGNET" => Some(" 1 ".into()),
        "QUANTICK_TOOLBOX_DOCK" => Some("right".into()),
        "QUANTICK_TOOLBAR_SCROLL" => Some("NaN".into()),
        _ => None,
    }));
    assert!(!outcome.favorites_staged);
    assert_eq!(rail.tool().id(), "measure");
    assert!(rail.magnet());
    assert_eq!(rail.favorites().len(), 1);
    assert_eq!(rail.dock(), ToolboxDock::Bottom);
    assert_eq!(rail.launch_pending_for_test(), (Some(27.0), Some("lines")));
}

#[test]
fn malformed_unicode_is_absence_for_every_launch_input() {
    #[cfg(windows)]
    let invalid = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    };
    #[cfg(unix)]
    let invalid = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff])
    };
    let mut rail = ToolRail::new();
    rail.set_favorites(&["measure".into()]);
    let outcome = rail.apply_launch(ToolRailLaunch::capture(|_| Some(invalid.clone())));
    assert!(!outcome.favorites_staged);
    assert_eq!(rail.tool(), Tool::Pointer);
    assert!(!rail.magnet());
    assert_eq!(rail.favorites().len(), 1);
    assert_eq!(rail.dock(), ToolboxDock::Left);
    assert_eq!(rail.launch_pending_for_test(), (None, None));
}

#[test]
fn a_pending_flyout_can_be_rearmed_after_unknown_family_retry() {
    let mut rail = ToolRail::new();
    let ctx = egui::Context::default();
    rail.apply_launch(ToolRailLaunch::capture(|name| {
        (name == "QUANTICK_TOOLBOX_FLYOUT").then(|| "unknown".into())
    }));
    rail.launch_draw_for_test(&ctx, egui::vec2(2000.0, 2000.0));
    assert_eq!(rail.launch_pending_for_test().1, Some("unknown"));
    rail.apply_launch(ToolRailLaunch::capture(|name| {
        (name == "QUANTICK_TOOLBOX_FLYOUT").then(|| "lines".into())
    }));
    assert_eq!(rail.launch_pending_for_test().1, Some("lines"));
    rail.launch_draw_for_test(&ctx, egui::vec2(2000.0, 2000.0));
    assert_eq!(rail.launch_pending_for_test().1, None);
    assert_eq!(rail.launch_rendered_for_test().2, Some("lines"));
}
