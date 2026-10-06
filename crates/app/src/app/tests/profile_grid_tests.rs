//! Market-grid inheritance and exact profile comparisons across bar rules.
use super::*;
use quantick_anchored_studies::{LevelPrices, ProfileOutput};
use quantick_engine::VolumeProfile;
use quantick_engine::bar_registry::BUILTIN_BARS;

const SIDES: [PaneSide; 3] = [PaneSide::Flow, PaneSide::Time(0), PaneSide::Time(1)];

fn profile_fixture(ctx: &egui::Context, collapsed: bool) -> QuantickApp {
    let mut app = profile_fixture_unadopted(ctx, collapsed);
    app.active_tab_mut().tape_mut().flush_for_test();
    app
}

fn profile_fixture_unadopted(ctx: &egui::Context, collapsed: bool) -> QuantickApp {
    let (mut app, events, _commands, _book) = test_app();
    app.active_tab_mut().restore_canvas(
        CanvasLayout::TimeTimeAndFlow,
        Some(0.45),
        crate::tab::CanvasCollapseRestore {
            context: collapsed,
            flow: false,
            heights: &[0.4, 0.6],
            collapsed_slots: &[false, collapsed],
        },
        Some(PaneSide::Flow),
        &[60_000, 300_000],
        crate::tab::LegendFold::default(),
    );
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    assert_eq!(app.active_tab().pane_count(), 3);
    for (side, spec) in SIDES.into_iter().zip(["tick:4", "tick:8", "renko:2"]) {
        app.active_tab_mut()
            .set_pane_bar_spec(side.index(), BUILTIN_BARS.parse(spec).unwrap())
            .unwrap();
        let pane = app.active_tab_mut().pane_mut(side);
        for layer in [
            ChartLayer::Footprint,
            ChartLayer::CandleAggression,
            ChartLayer::LiveStrip,
            ChartLayer::TapeChart,
        ] {
            pane.set_layer_visible(layer, false, &mut Default::default());
        }
    }
    // Twelve cycles on a five-point grid; enough price changes to settle
    // Renko's inferred step. Row volumes per cycle: 2, 6, 13, 12, 7.
    // At the market grid the value area expands above the POC; on the
    // default cent grid the empty adjacent buckets instead tie downward.
    let prices = [
        100_000, 100_005, 100_010, 100_015, 100_020, 100_015, 100_010, 100_005,
    ];
    let quantities = [2, 2, 7, 6, 7, 6, 6, 4];
    let prints = (0..96)
        .map(|index| quantick_engine::Trade {
            price: Decimal::from(prices[index % prices.len()]),
            quantity: Decimal::from(quantities[index % quantities.len()]),
            ..trade(index as u64 + 1)
        })
        .collect();
    events.try_send(FeedEvent::Backfilled(prints)).unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    assert_eq!(
        app.active_tab_mut()
            .tape_mut()
            .published_capture_grouping_for_test(),
        Decimal::from(5)
    );
    app
}

fn place_profile(pane: &mut ChartPane, start: f32, end: f32) {
    let tool = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .unwrap();
    assert!(!pane.drawings.place(tool, ChartPoint::at(start, 100_000.0)));
    assert!(pane.drawings.place(tool, ChartPoint::at(end, 100_020.0)));
    let payload = pane
        .drawings
        .items_mut()
        .last_mut()
        .unwrap()
        .payload
        .as_any_mut()
        .downcast_mut::<crate::drawings::FrvpPayload>()
        .unwrap();
    payload.value_area_pct = 70;
    payload.approximate_history = false;
    pane.drawings.select(None);
}

fn output(pane: &ChartPane) -> ProfileOutput<'_> {
    pane.drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<crate::drawings::FrvpPayload>()
        .unwrap()
        .cache
        .as_ref()
        .expect("the normal frame refreshed the profile")
        .output()
}

fn profile(pane: &ChartPane) -> (&VolumeProfile, LevelPrices) {
    let result = output(pane);
    assert!(!result.folding);
    assert_eq!(result.bars_approximated, 0);
    let (profile, area) = result
        .profile
        .expect("exact prints cover the selected bars");
    (profile, LevelPrices::of(profile, area.unwrap()))
}

fn assert_market_grid(app: &QuantickApp) {
    for side in SIDES {
        assert_eq!(
            app.active_tab().pane(side).state.footprint_group(),
            Decimal::from(5),
            "{side:?} must inherit the instrument capture grid"
        );
    }
}

#[test]
fn collapsed_flow_adopts_published_grid_before_context_profiles_draw() {
    let ctx = egui::Context::default();
    let mut app = profile_fixture_unadopted(&ctx, false);
    app.active_tab_mut().set_flow_collapsed(true);
    let before = app.active_tab_mut().tape_mut().base_capture_grouping();
    assert_ne!(before, Decimal::from(5), "the view mirror must start stale");
    for side in [PaneSide::Time(0), PaneSide::Time(1)] {
        let pane = app.active_tab_mut().pane_mut(side);
        let end = pane.slots().saturating_sub(1) as f32;
        place_profile(pane, 0.0, end);
    }
    // Fix the summary clock so diagnostics cannot adopt the worker's grid.
    let now = Instant::now();
    app.health.last_summary = now;
    app.health.last_frame = None;
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
            ..Default::default()
        },
        |ctx| app.draw_frame(ctx, now),
    );
    assert!(app.active_tab().pane(PaneSide::Flow).frame.area.is_none());
    assert_eq!(app.health.last_summary, now);
    for side in [PaneSide::Time(0), PaneSide::Time(1)] {
        let pane = app.active_tab().pane(side);
        assert!(pane.frame.area.is_some());
        assert_eq!(pane.state.footprint_group(), Decimal::from(5), "{side:?}");
        let (profile, levels) = profile(pane);
        assert_eq!(profile.group(), Decimal::from(5));
        assert_eq!(profile.total_volume(), Decimal::from(480));
        assert_eq!(
            levels,
            LevelPrices {
                poc: Decimal::new(1_000_125, 1),
                vah: Decimal::from(100_025),
                val: Decimal::from(100_010),
            }
        );
    }
}

#[test]
fn equal_prints_and_bounds_match_profiles_on_non_first_renko_context() {
    let ctx = egui::Context::default();
    let mut app = profile_fixture(&ctx, false);
    for side in SIDES {
        let pane = app.active_tab_mut().pane_mut(side);
        assert_eq!(pane.state.trades().len(), 96);
        let end = pane.slots().saturating_sub(1) as f32;
        place_profile(pane, 0.0, end);
    }
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    for side in SIDES {
        let (profile, levels) = profile(app.active_tab().pane(side));
        eprintln!(
            "{side:?}: group={} volume={} levels={levels:?}",
            profile.group(),
            profile.total_volume()
        );
    }
    assert_market_grid(&app);
    let (flow, levels) = profile(app.active_tab().pane(PaneSide::Flow));
    let rows: Vec<_> = flow
        .levels()
        .iter()
        .map(|(bucket, level)| (*bucket, level.volume()))
        .collect();
    assert_eq!(
        rows,
        [
            (20_000, 24),
            (20_001, 72),
            (20_002, 156),
            (20_003, 144),
            (20_004, 84)
        ]
        .map(|(bucket, volume)| (bucket, Decimal::from(volume)))
    );
    assert_eq!(flow.total_volume(), Decimal::from(480));
    assert_eq!(
        levels,
        LevelPrices {
            poc: Decimal::new(1_000_125, 1),
            vah: Decimal::from(100_025),
            val: Decimal::from(100_010),
        }
    );
    for side in [PaneSide::Time(0), PaneSide::Time(1)] {
        let (context, context_levels) = profile(app.active_tab().pane(side));
        assert_eq!(context, flow, "{side:?}: rows, side volumes and grouping");
        assert_eq!(context_levels, levels, "{side:?}: POC, VAH and VAL");
    }
    // A footprint refold bumps the series revision. A profile refold replaces
    // its row map. Measure closed ranges after the whole-tape comparison:
    // the forming ladder has its own scheduled snapshot cadence.
    for side in SIDES {
        let pane = app.active_tab_mut().pane_mut(side);
        pane.drawings.items_mut()[0].points[1].bar =
            pane.state.bars().len().saturating_sub(1) as f32;
    }
    run_frame(&mut app, &ctx);
    let before: Vec<_> = SIDES
        .iter()
        .map(|side| {
            let pane = app.active_tab().pane(*side);
            let result = output(pane);
            (
                pane.state.series_revision(),
                result.key,
                result
                    .profile
                    .unwrap()
                    .0
                    .levels()
                    .first_key_value()
                    .unwrap()
                    .1 as *const _,
            )
        })
        .collect();
    let anchors: Vec<_> = SIDES
        .iter()
        .map(|side| {
            app.active_tab().pane(*side).drawings.items()[0]
                .points
                .clone()
        })
        .collect();
    for _ in 0..5 {
        run_frame(&mut app, &ctx);
    }
    for ((side, expected), anchors) in SIDES.into_iter().zip(before).zip(anchors) {
        let pane = app.active_tab().pane(side);
        let result = output(pane);
        assert_eq!(
            (
                pane.state.series_revision(),
                result.key,
                result
                    .profile
                    .unwrap()
                    .0
                    .levels()
                    .first_key_value()
                    .unwrap()
                    .1 as *const _
            ),
            expected
        );
        assert_eq!(pane.drawings.items()[0].points, anchors);
    }
}

#[test]
fn restored_collapsed_contexts_inherit_grid_before_they_draw_and_after_recut() {
    let ctx = egui::Context::default();
    let mut app = profile_fixture(&ctx, true);
    place_profile(app.active_tab_mut().pane_mut(PaneSide::Flow), 0.0, 23.0);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab()
            .pane(PaneSide::Time(1))
            .frame
            .area
            .is_none()
    );
    assert_market_grid(&app);
    app.active_tab_mut().set_context_collapsed(false);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab()
            .pane(PaneSide::Time(1))
            .frame
            .area
            .is_none()
    );
    assert_market_grid(&app);
    app.active_tab_mut()
        .restore_context_heights(&[0.4, 0.6], &[false, false]);
    app.active_tab_mut().set_flow_collapsed(true);
    for spec in ["tick:3", "renko:3"] {
        app.active_tab_mut()
            .set_pane_bar_spec(2, BUILTIN_BARS.parse(spec).unwrap())
            .unwrap();
        run_frame(&mut app, &ctx);
        assert_market_grid(&app);
        assert!(
            app.active_tab()
                .pane(PaneSide::Time(1))
                .frame
                .area
                .is_some()
        );
        assert_eq!(
            app.active_tab()
                .pane(PaneSide::Time(1))
                .state
                .trades()
                .len(),
            96
        );
    }
}

#[test]
fn different_first_candle_bounds_keep_their_actual_print_volumes() {
    let ctx = egui::Context::default();
    let mut app = profile_fixture(&ctx, false);
    for side in [PaneSide::Flow, PaneSide::Time(1)] {
        place_profile(app.active_tab_mut().pane_mut(side), 0.0, 0.0);
    }
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_market_grid(&app);
    let (ticks, tick_levels) = profile(app.active_tab().pane(PaneSide::Flow));
    let (renko, renko_levels) = profile(app.active_tab().pane(PaneSide::Time(1)));
    assert_eq!(ticks.total_volume(), Decimal::from(17));
    assert_eq!(renko.total_volume(), Decimal::from(4));
    assert_eq!(
        tick_levels,
        LevelPrices {
            poc: Decimal::new(1_000_125, 1),
            vah: Decimal::from(100_020),
            val: Decimal::from(100_010),
        }
    );
    assert_eq!(
        renko_levels,
        LevelPrices {
            poc: Decimal::new(1_000_025, 1),
            vah: Decimal::from(100_010),
            val: Decimal::from(100_000),
        }
    );
    for (side, trades, end_ms) in [(PaneSide::Flow, 4, 1_400), (PaneSide::Time(1), 2, 1_200)] {
        let bar = &app.active_tab().pane(side).state.bars()[0];
        assert_eq!(
            (bar.open_time, bar.close_time, bar.trade_count),
            (1_100, end_ms, trades)
        );
    }
    eprintln!(
        "different first candle bounds: ticks volume={} levels={tick_levels:?}; Renko volume={} levels={renko_levels:?}",
        ticks.total_volume(),
        renko.total_volume()
    );
}
