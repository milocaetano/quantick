//! Literal child-process fixtures against the original launch consumers.
use super::*;

#[test]
fn six_hook_consumer_baseline() {
    let case = std::env::var("H3_BASELINE_CASE").unwrap_or_else(|_| "absent".into());
    if case == "draft-phase" {
        draft_uses_the_tool_selected_during_construction();
        return;
    }
    let (seed, _, _, _) = test_app();
    let mut workspace = seed.capture_workspace();
    workspace.tabs.clear();
    workspace.favorite_tools = vec!["measure".into()];
    workspace.chrome.as_mut().unwrap().rail_dock = ui_state::SavedRailDock::Bottom;
    let (mut app, _, _, _) = test_app_with_workspace_and_launch(
        workspace,
        AppLaunch {
            toolrail: crate::toolrail::ToolRailLaunch::capture(|name| std::env::var_os(name)),
            ..Default::default()
        },
    );

    let mut tool = "pointer";
    let mut magnet = false;
    let mut favorites = vec!["measure"];
    let mut dock = crate::toolrail::ToolboxDock::Bottom;
    let mut target = None;
    let mut flyout = None;
    let mut staged = false;
    match case.as_str() {
        "valid" => {
            tool = "horizontal-line";
            magnet = true;
            favorites = vec!["measure", "horizontal-line"];
            dock = crate::toolrail::ToolboxDock::Top;
            target = Some(42.5);
            flyout = Some("lines");
            staged = true;
        }
        "favorites-empty" | "favorites-unknown" => {
            favorites.clear();
            staged = true;
        }
        "favorites-duplicates" => {
            favorites = vec!["horizontal-line", "measure"];
            staged = true;
        }
        "dock-left" => dock = crate::toolrail::ToolboxDock::Left,
        "dock-top" => dock = crate::toolrail::ToolboxDock::Top,
        "scroll-negative" => target = Some(0.0),
        "scroll-end" => target = Some(f32::INFINITY),
        "flyout-empty" => flyout = Some(""),
        "flyout-unknown" => flyout = Some("unknown"),
        "absent" | "tool-unknown" | "magnet-whitespace" | "magnet-zero" | "dock-bottom"
        | "dock-right" | "dock-uppercase" | "scroll-nan" | "scroll-infinity" | "scroll-invalid" => {
        }
        other => panic!("unregistered literal baseline {other}"),
    }
    assert_eq!(app.toolrail.tool().id(), tool);
    assert_eq!(app.toolrail.magnet(), magnet);
    assert_eq!(app.starred_tool_ids(), favorites);
    assert_eq!(app.toolrail.dock(), dock);
    assert_eq!(app.toolrail.launch_pending_for_test(), (target, flyout));
    assert_eq!(app.workspace.session().favorites_are_staged(), staged);

    let ctx = egui::Context::default();
    if case.starts_with("scroll-") {
        app.toolrail
            .launch_draw_for_test(&ctx, egui::vec2(560.0, 900.0));
        let (scrolling, offset, _) = app.toolrail.launch_rendered_for_test();
        assert!(
            scrolling,
            "the actual renderer must exercise its scroll band"
        );
        assert!(offset.is_finite());
        if case == "scroll-end" {
            assert!(offset > 0.0, "end reaches a real finite far edge");
        } else {
            assert_eq!(offset, 0.0);
        }
        assert_eq!(app.toolrail.launch_pending_for_test().0, None);
        app.toolrail
            .launch_draw_for_test(&ctx, egui::vec2(560.0, 900.0));
        assert_eq!(app.toolrail.launch_rendered_for_test().1, offset);
    } else {
        app.toolrail
            .launch_draw_for_test(&ctx, egui::vec2(2000.0, 2000.0));
        if case == "valid" {
            assert_eq!(app.toolrail.launch_pending_for_test().1, None);
            assert_eq!(app.toolrail.launch_rendered_for_test().2, Some("lines"));
        } else if flyout.is_some() {
            assert_eq!(app.toolrail.launch_pending_for_test().1, flyout);
            assert_eq!(app.toolrail.launch_rendered_for_test().2, None);
            app.toolrail
                .launch_draw_for_test(&ctx, egui::vec2(2000.0, 2000.0));
            assert_eq!(app.toolrail.launch_pending_for_test().1, flyout);
        }
    }
    let path = scratch_ui_state("literal-launch-favorites");
    app.workspace.set_ui_state_path(path.clone());
    app.toolrail.toggle_favorite(starrable_tool());
    run_frame(&mut app, &ctx);
    assert_eq!(path.exists(), !staged, "only ordinary stars reach disk");
    if path.exists() {
        assert_eq!(ui_state::load(&path).favorite_tools, app.starred_tool_ids());
        std::fs::remove_file(path).unwrap();
    }
}

fn draft_uses_the_tool_selected_during_construction() {
    let launch = AppLaunch {
        toolrail: crate::toolrail::ToolRailLaunch::capture(|name| std::env::var_os(name)),
        drawing_chrome: crate::surfaces::drawing_chrome::DrawingChromeLaunch::capture(|name| {
            std::env::var_os(name)
        }),
        ..Default::default()
    };
    let (mut app, _commands) = app_with_history_and_launch(200, launch);
    assert_eq!(app.toolrail.tool().id(), "parallel-channel");
    app.active_tab_mut().flow_pane.frame.chart_area = Some(egui::Rect::from_min_size(
        egui::pos2(100.0, 100.0),
        egui::vec2(800.0, 400.0),
    ));
    app.active_tab_mut().flow_pane.frame.auto_range = Some((90.0, 110.0));
    app.apply_drawing_draft();
    let pane = &app.active_tab().flow_pane;
    assert_eq!(pane.drawings.draft_len(), 2);
    let points = &pane.drawings.draft().unwrap().points;
    assert_eq!(points[0], ChartPoint::at_time(141.5, 99.4, Some(15_200)));
    assert_eq!(points[1], ChartPoint::at_time(168.5, 100.6, Some(17_900)));
    assert!(
        app.surfaces
            .drawing_chrome
            .demos()
            .draft_requested()
            .is_none()
    );
}
