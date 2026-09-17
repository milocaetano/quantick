//! Characterization of the actual demo consumers, before ownership changes.
//! The external baseline runner supplies each literal environment case in a
//! separate process. The normal workspace run retains the gallery fixture.
use super::*;

#[test]
fn stress_without_a_flow_anchor_retries_then_uses_the_real_delivery_path() {
    let launch = AppLaunch {
        drawing_chrome: crate::surfaces::drawing_chrome::DrawingChromeLaunch::capture(|name| {
            (name == "QUANTICK_FRVP_DEMO").then(|| "stress".into())
        }),
        ..Default::default()
    };
    let (mut app, _commands) = app_with_history_and_launch(20_000, launch);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    let QuantickApp {
        tabs,
        config,
        style,
        pane_ids,
        ..
    } = &mut app;
    for tab in tabs.iter_mut() {
        tab.apply_pending_layout(config, style, pane_ids);
    }
    assert!(app.active_tab().time_pane().unwrap().slots() >= 12);
    let flow = std::mem::replace(
        &mut app.active_tab_mut().flow_pane.state,
        crate::state::ChartState::new(crate::state::BarSpec::Tick(1)),
    );
    app.apply_frvp_demo();
    assert!(
        app.surfaces
            .drawing_chrome
            .demos()
            .profile_requested()
            .is_some()
    );
    assert!(
        app.active_tab()
            .time_pane()
            .unwrap()
            .drawings
            .items()
            .is_empty()
    );
    app.active_tab_mut().flow_pane.state = flow;
    app.apply_frvp_demo();
    assert!(
        app.surfaces
            .drawing_chrome
            .demos()
            .profile_requested()
            .is_none()
    );
    let pane = app.active_tab().time_pane().unwrap();
    assert!(pane.history_prefix.len() > 24_000);
    assert_eq!(pane.drawings.items().len(), 1);
}

#[test]
fn nine_hook_consumer_baseline() {
    let case = std::env::var("H2B_BASELINE_CASE").unwrap_or_else(|_| "gallery-default".into());
    let count = match case.as_str() {
        "compare-wait" => 30,
        "wait" => 5,
        "stress" => 20_000,
        _ => 200,
    };
    let launch = AppLaunch {
        drawing_chrome: crate::surfaces::drawing_chrome::DrawingChromeLaunch::capture(|name| {
            std::env::var_os(name)
        }),
        ..Default::default()
    };
    let (mut app, _commands) = app_with_history_and_launch(count, launch);
    if case == "gallery-default" {
        app.surfaces
            .drawing_chrome
            .demos_mut()
            .arm_drawings_demo(DrawingsDemo::default());
    }
    app.active_tab_mut().flow_pane.frame.chart_area = Some(egui::Rect::from_min_size(
        egui::pos2(100.0, 100.0),
        egui::vec2(800.0, 400.0),
    ));
    app.active_tab_mut().flow_pane.frame.auto_range = Some((90.0, 110.0));
    if case == "bands" {
        add_pane_indicator(&mut app, "Baseline band", vec![8.0; 200]);
    }
    match case.as_str() {
        "gallery-default" | "gallery" | "shared" | "bands" | "recut" => {
            if case == "shared" {
                app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
                app.active_tab_mut().context_collapsed = true;
            }
            app.apply_drawing_demo();
            assert!(!app.surfaces.drawing_chrome.demos().gallery_requested());
            let pane = &app.active_tab().flow_pane;
            assert!(pane.drawings.items().len() >= drawings::DRAWING_TOOLS.len());
            if case != "recut" {
                let first = &pane.drawings.items()[0];
                assert_eq!(
                    first.points[0],
                    ChartPoint::at_time(110.5, 102.0, Some(12_100))
                );
                assert_eq!(
                    first.points[1],
                    ChartPoint::at_time(132.5, 104.4, Some(14_300))
                );
            } else {
                assert_eq!(
                    pane.spec.retained(crate::state::BarKind::Tick).parameter(),
                    2_u64.into()
                );
                assert!(
                    pane.drawings.items().iter().any(|drawing| drawing
                        .points
                        .iter()
                        .any(|p| p.time_ms == Some(-3_598_900)))
                );
            }
            if case == "shared" {
                assert!(!app.active_tab().context_collapsed);
                assert!(
                    app.active_tab().time_panes.is_empty(),
                    "the canvas switch defers materialization"
                );
                assert!(
                    pane.drawings
                        .items()
                        .iter()
                        .filter(|d| d.shareable())
                        .all(|d| d.scope == drawings::DrawingScope::AllCharts)
                );
                assert_eq!(
                    pane.drawings.items()[pane.drawings.selected().unwrap()]
                        .tool
                        .id(),
                    "horizontal-line"
                );
                app.apply_frvp_demo();
                assert!(
                    app.surfaces
                        .drawing_chrome
                        .demos()
                        .profile_requested()
                        .is_some(),
                    "stress waits for an actual time pane"
                );
            }
            if case == "bands" {
                let pane = &app.active_tab().flow_pane;
                let band_drawings: Vec<_> = pane
                    .drawings
                    .items()
                    .iter()
                    .filter(|d| matches!(d.band, drawings::DrawingBand::Indicator(_)))
                    .collect();
                assert_eq!(band_drawings.len(), 2);
                let level = band_drawings
                    .iter()
                    .find(|d| d.tool.id() == "horizontal-line")
                    .unwrap();
                assert_eq!(
                    level.points,
                    vec![ChartPoint::at_time(155.5, 8.0, Some(16_600))]
                );
                let line = band_drawings
                    .iter()
                    .find(|d| d.tool.id() == "trend-line")
                    .unwrap();
                assert_eq!(
                    line.points,
                    vec![
                        ChartPoint::at_time(121.5, 4.0, Some(13_200)),
                        ChartPoint::at_time(177.5, 12.0, Some(18_800))
                    ]
                );
            }
            let before = app.active_tab().flow_pane.drawings.items().len();
            app.apply_drawing_demo();
            assert_eq!(app.active_tab().flow_pane.drawings.items().len(), before);
        }
        "draft" | "draft-zero" => {
            app.apply_drawing_draft();
            assert!(
                app.surfaces
                    .drawing_chrome
                    .demos()
                    .draft_requested()
                    .is_some(),
                "no armed tool means retry"
            );
            let id = if case == "draft-zero" {
                "horizontal-line"
            } else {
                "parallel-channel"
            };
            app.toolrail.arm(Tool::Drawing(
                drawings::DRAWING_TOOLS
                    .into_iter()
                    .find(|tool| tool.id() == id)
                    .unwrap(),
            ));
            app.apply_drawing_draft();
            assert!(
                app.surfaces
                    .drawing_chrome
                    .demos()
                    .draft_requested()
                    .is_none()
            );
            let pane = &app.active_tab().flow_pane;
            assert_eq!(
                pane.drawings.draft_len(),
                if case == "draft-zero" { 0 } else { 2 }
            );
            let hand = pane.gestures.parked_hand.as_ref().unwrap();
            assert_eq!(
                hand.position,
                if case == "draft-zero" {
                    egui::pos2(660.0, 240.0)
                } else {
                    egui::pos2(500.0, 300.0)
                }
            );
            assert_eq!(hand.constrain, drawings::Constrain::Level);
            if case == "draft" {
                let points = &pane.drawings.draft().unwrap().points;
                assert_eq!(points[0], ChartPoint::at_time(141.5, 99.4, Some(15_200)));
                assert_eq!(points[1], ChartPoint::at_time(168.5, 100.6, Some(17_900)));
            }
        }
        "profile" | "compare" | "compare-wait" => {
            app.apply_frvp_demo();
            if case == "compare-wait" {
                assert!(
                    app.surfaces
                        .drawing_chrome
                        .demos()
                        .profile_requested()
                        .is_some()
                );
                assert!(app.active_tab().flow_pane.drawings.items().is_empty());
                return;
            }
            assert!(
                app.surfaces
                    .drawing_chrome
                    .demos()
                    .profile_requested()
                    .is_none()
            );
            let pane = &app.active_tab().flow_pane;
            let ranges: &[(f32, f32)] = if case == "compare" {
                &[(150.0, 174.0), (175.0, 199.0)]
            } else {
                &[(170.0, 199.0)]
            };
            assert_eq!(pane.drawings.items().len(), ranges.len());
            for (drawing, &(left, right)) in pane.drawings.items().iter().zip(ranges) {
                assert_eq!(
                    drawing.points[0],
                    ChartPoint::at_time(left, 100.0, Some(1_100 + left as i64 * 100))
                );
                assert_eq!(
                    drawing.points[1],
                    ChartPoint::at_time(right, 100.0, Some(1_100 + right as i64 * 100))
                );
            }
            assert_eq!(pane.drawings.selected(), Some(ranges.len() - 1));
        }
        "avwap" => {
            app.apply_avwap_demo();
            assert!(!app.surfaces.drawing_chrome.demos().avwap_requested());
            let pane = &app.active_tab().flow_pane;
            assert_eq!(pane.drawings.items().len(), 1);
            let drawing = &pane.drawings.items()[0];
            assert_eq!(
                drawing.points[0],
                ChartPoint::at_time(160.0, 100.1, Some(17_100))
            );
            assert!(
                drawing
                    .payload
                    .as_any()
                    .downcast_ref::<drawings::AvwapPayload>()
                    .unwrap()
                    .bands[1]
                    .on
            );
        }
        "wait" => {
            app.apply_drawing_demo();
            app.apply_frvp_demo();
            app.apply_avwap_demo();
            assert!(app.surfaces.drawing_chrome.demos().gallery_requested());
            assert!(
                app.surfaces
                    .drawing_chrome
                    .demos()
                    .profile_requested()
                    .is_some()
            );
            assert!(app.surfaces.drawing_chrome.demos().avwap_requested());
            assert!(app.active_tab().flow_pane.drawings.items().is_empty());
        }
        "invalid" => {
            assert!(!app.surfaces.drawing_chrome.demos().gallery_requested());
            assert!(
                app.surfaces
                    .drawing_chrome
                    .demos()
                    .profile_requested()
                    .is_none()
            );
            assert!(!app.surfaces.drawing_chrome.demos().avwap_requested());
            assert!(
                app.surfaces
                    .drawing_chrome
                    .demos()
                    .draft_requested()
                    .is_none()
            );
            assert!(
                app.surfaces.drawing_chrome.demos().recut_requested(),
                "recut is read independently"
            );
            app.apply_drawing_demo();
            assert_eq!(
                app.active_tab()
                    .flow_pane
                    .spec
                    .retained(crate::state::BarKind::Tick)
                    .parameter(),
                1_u64.into()
            );
        }
        "stress" => {
            app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
            let QuantickApp {
                tabs,
                config,
                style,
                pane_ids,
                ..
            } = &mut app;
            for tab in tabs.iter_mut() {
                tab.apply_pending_layout(config, style, pane_ids);
            }
            assert!(app.active_tab().time_pane().unwrap().slots() >= 12);
            app.apply_frvp_demo();
            assert!(
                app.surfaces
                    .drawing_chrome
                    .demos()
                    .profile_requested()
                    .is_none()
            );
            let pane = app.active_tab().time_pane().unwrap();
            assert!(pane.history_prefix.len() > 24_000);
            assert_eq!(pane.drawings.items().len(), 1);
            let points = &pane.drawings.items()[0].points;
            assert_eq!(points[0].bar, 0.0);
            assert_eq!(points[1].bar, (pane.slots() - 1) as f32);
            assert_eq!(points[0].time_ms, pane.slot_open_time(0));
            assert_eq!(points[1].time_ms, pane.slot_open_time(pane.slots() - 1));
            assert!(app.active_tab().flow_pane.drawings.items().is_empty());
        }
        _ => panic!("unknown baseline case: {case}"),
    }
}
