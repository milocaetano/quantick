// External measurement suffix. Not product source; identical on both revisions.
const OWNER_ACTION_WARMUPS: usize = 10;
const OWNER_ACTION_OBSERVATIONS: usize = 100;

fn owner_action_measurement(count_allocations: bool) {
    use crate::surfaces::drawing_chrome::QuickRangeAction;
    for (action, tool, anchors) in [
        (QuickRangeAction::Profile, crate::frvp::TOOL_ID, 2),
        (QuickRangeAction::Retracement, "fib-retracement", 2),
        (QuickRangeAction::Projection, "fib-extension", 3),
    ] {
        for observation in 0..OWNER_ACTION_WARMUPS + OWNER_ACTION_OBSERVATIONS {
            let ctx = egui::Context::default();
            let (mut app, _commands) = app_with_history(200);
            run_frame_at(&mut app, &ctx, TEST_WINDOW);
            let (_, start, end) = quick_range_ends(&app);
            drag_quick_range(&mut app, &ctx, start, end);
            run_frame(&mut app, &ctx);
            let control = quick_range_action(&app, action);
            assert!(control.enabled);
            assert!(app.active_tab().flow_pane.drawings.items().is_empty());
            let pressed_at = control.rect.center();
            run_frame_with_events(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pressed_at),
                    pointer_button(pressed_at, true),
                ],
            );
            let released_at = quick_range_action(&app, action).rect.center();
            let events = vec![
                egui::Event::PointerMoved(released_at),
                pointer_button(released_at, false),
            ];
            let (output, metric) = if count_allocations {
                crate::work_meter::reset_largest();
                let before = crate::work_meter::tally();
                let output = run_frame_with_events(&mut app, &ctx, events);
                let heap = crate::work_meter::tally().since(before);
                (
                    output,
                    serde_json::json!({
                        "allocs": heap.allocs, "alloc_bytes": heap.alloc_bytes,
                        "reallocs": heap.reallocs, "realloc_copy_bytes": heap.realloc_copy_bytes,
                        "largest_realloc_copy": heap.largest_realloc_copy,
                    }),
                )
            } else {
                let started = std::time::Instant::now();
                let output = run_frame_with_events(&mut app, &ctx, events);
                let elapsed = started.elapsed();
                (
                    output,
                    serde_json::json!({"release_frame_ns": elapsed.as_nanos()}),
                )
            };
            assert!(crate::app::control_quick_range(&app).is_none());
            let drawings = app.active_tab().flow_pane.drawings.items();
            assert_eq!(drawings.len(), 1);
            assert_eq!(drawings[0].tool.id(), tool);
            assert_eq!(drawings[0].points.len(), anchors);
            assert!((drawings[0].points[0].bar - 80.5).abs() < 0.01);
            assert!((drawings[0].points[1].bar - 160.5).abs() < 0.01);
            assert!(drawings[0].author.is_none());
            assert_ne!(
                app.surfaces.toast.message(),
                Some("The drawing could not be placed; the temporary range is still available.")
            );
            if action == QuickRangeAction::Projection {
                assert_eq!(drawings[0].points[2], drawings[0].points[1]);
            }
            let points: Vec<_> = drawings[0]
                .points
                .iter()
                .map(|point| {
                    serde_json::json!({"bar_bits":point.bar.to_bits(),
                    "price_bits":point.price.to_bits(),"time_ms":point.time_ms})
                })
                .collect();
            println!(
                "OWNER_ACTION {}",
                serde_json::json!({
                    "action":tool,"observation":observation,
                    "warmup":observation<OWNER_ACTION_WARMUPS,
                    "mode":if count_allocations {"allocations"} else {"timing"},
                    "metric":metric,"output":{"tool":tool,"points":points,
                        "feed_id":app.active_tab().feed_id,"symbol":app.active_tab().symbol,"pane":"flow"},
                })
            );
            drop(output);
        }
    }
}

#[test]
#[ignore = "fixed paired owner-delivery release-frame measurement"]
fn owner_action_release_frame_measurement() {
    owner_action_measurement(false);
}

#[test]
#[ignore = "fixed separate owner-delivery allocation measurement"]
fn owner_action_release_frame_allocations() {
    owner_action_measurement(true);
}
