//! Pane ownership exercised through the same settings operation as the editor.
use super::*;
use quantick_control_schema::chart_settings::ChartSettingsInput;
use serde_json::{Value, json};

fn dispatch(
    app: &mut QuantickApp,
    input: Value,
) -> Result<Value, crate::candle_presets::PresetError> {
    let input = serde_json::from_value::<ChartSettingsInput>(input).unwrap();
    app.layer_wiring()
        .chart_settings(input)
        .map(|result| serde_json::to_value(result).unwrap())
}

fn call(app: &mut QuantickApp, input: Value) -> Value {
    dispatch(app, input).expect("the addressed settings operation succeeds")
}

fn target(app: &QuantickApp, tab: usize, side: PaneSide) -> Value {
    json!({"tab_id":app.runtime.tabs.id_at(tab).to_string(),
           "pane_id":app.runtime.tabs[tab].pane(side).id.to_string()})
}

fn inspect(app: &mut QuantickApp, target: &Value) -> Value {
    call(app, json!({"action":"inspect", "target":target}))["effective_style"].clone()
}

fn three_charts(app: &mut QuantickApp, ctx: &egui::Context) -> [Value; 3] {
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    run_frame(app, ctx);
    run_frame(app, ctx);
    assert_eq!(app.active_tab().time_panes.len(), 2);
    let tab = app.runtime.tabs.active_index();
    [PaneSide::Flow, PaneSide::Time(0), PaneSide::Time(1)].map(|side| target(app, tab, side))
}

fn styles(app: &mut QuantickApp, targets: &[Value]) -> Vec<Value> {
    targets.iter().map(|target| inspect(app, target)).collect()
}

#[test]
fn local_candles_edit_each_chart_without_changing_siblings_or_another_market() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let first = three_charts(&mut app, &ctx);
    let _other = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let targets = [
        first[0].clone(),
        first[1].clone(),
        first[2].clone(),
        target(&app, 1, PaneSide::Flow),
    ];
    let mut expected = styles(&mut app, &targets);
    let legacy = app.runtime.style;
    for (index, selected) in targets.iter().enumerate() {
        let mut edited = expected[index].clone();
        edited["candles"]["gap_px"] = json!((3 + index).to_string());
        edited["canvas"]["grid_enabled"] = json!(index % 2 == 0);
        edited["candles"]["bull_fill"] = json!([31 + index, 71, 119]);
        edited["ownership"]["bull_fill"] = json!("explicit");
        let result = call(
            &mut app,
            json!({"action":"update_candles", "target":selected, "style":edited}),
        );
        assert_eq!(result["target"], *selected);
        assert_eq!(result["changed"], true);
        expected[index] = edited;
        assert_eq!(
            styles(&mut app, &targets),
            expected,
            "only chart {index} changes"
        );
        assert_eq!(
            app.runtime.style, legacy,
            "the migration seed stays unchanged"
        );
    }
}

#[test]
fn local_candles_open_editor_stays_pinned_across_focus_market_and_preset_changes() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let first = three_charts(&mut app, &ctx);
    let before = styles(&mut app, &first);
    call(
        &mut app,
        json!({"action":"open", "tab":"candles", "target":first[1]}),
    );
    app.active_tab_mut().focus = PaneSide::Time(1);
    let _other = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let other = target(&app, 1, PaneSide::Flow);
    let other_before = inspect(&mut app, &other);
    let applied = call(
        &mut app,
        json!({"action":"apply_candle_preset", "name":"glass"}),
    );
    assert_eq!(applied["target"], first[1]);
    assert_eq!(applied["candle_preset"], "Glass");
    assert_eq!(applied["effective_style"]["canvas"], before[1]["canvas"]);
    assert_eq!(inspect(&mut app, &first[0]), before[0]);
    assert_eq!(inspect(&mut app, &first[2]), before[2]);
    assert_eq!(inspect(&mut app, &other), other_before);
    assert!(
        dispatch(
            &mut app,
            json!({"action":"reset_candles", "target":first[2]})
        )
        .is_err()
    );
    let reset = call(&mut app, json!({"action":"reset_candles"}));
    assert_eq!(reset["target"], first[1]);
    assert_eq!(reset["effective_style"], before[1]);
    assert_eq!(inspect(&mut app, &other), other_before);
}

#[test]
fn local_candles_invalid_documents_and_cross_tab_targets_are_atomic() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let first = three_charts(&mut app, &ctx);
    let _other = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let targets = [
        first[0].clone(),
        first[1].clone(),
        first[2].clone(),
        target(&app, 1, PaneSide::Flow),
    ];
    call(
        &mut app,
        json!({"action":"open", "tab":"candles", "target":first[1]}),
    );
    let before = styles(&mut app, &targets);
    let revision = app.runtime.style_revision;
    let saved = toml::to_string(&app.workspace_state().capture_workspace()).unwrap();
    for (pointer, invalid) in [
        ("/candles/gap_px", json!("-1")),
        ("/candles/wick_width", json!("10")),
        ("/candles/fill_opacity", json!("1.1")),
        ("/candles/outline_width", json!("0")),
        ("/candles/body_width_frac", json!("2")),
        ("/canvas/grid_opacity", Value::Null),
        ("/candles/bull_fill", json!([256, 0, 0])),
        ("/ownership/bull_fill", json!("guessed")),
    ] {
        let mut document = before[1].clone();
        *document.pointer_mut(pointer).unwrap() = invalid;
        assert!(
            dispatch(
                &mut app,
                json!({"action":"update_candles", "target":first[1], "style":document})
            )
            .is_err(),
            "{pointer}"
        );
        assert_eq!(styles(&mut app, &targets), before);
        assert_eq!(app.runtime.style_revision, revision);
    }
    let mut document = before[1].clone();
    document["candles"]["silently_ignored"] = json!(true);
    assert!(
        dispatch(
            &mut app,
            json!({"action":"update_candles", "target":first[1], "style":document})
        )
        .is_err()
    );
    let mismatched = json!({"tab_id":first[0]["tab_id"], "pane_id":targets[3]["pane_id"]});
    assert!(
        dispatch(
            &mut app,
            json!({"action":"apply_candle_preset", "target":mismatched, "name":"classic"})
        )
        .is_err()
    );
    assert_eq!(styles(&mut app, &targets), before);
    assert_eq!(app.runtime.style_revision, revision);
    assert_eq!(
        toml::to_string(&app.workspace_state().capture_workspace()).unwrap(),
        saved
    );
    assert_eq!(
        call(&mut app, json!({"action":"inspect"}))["target"],
        first[1]
    );
}

#[test]
fn local_candles_stale_editor_never_redirects_after_a_frame_and_shared_actions_recover() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let first = target(&app, 0, PaneSide::Flow);
    call(
        &mut app,
        json!({"action":"save_candle_preset", "name":"retained", "target":first}),
    );
    call(
        &mut app,
        json!({"action":"open", "tab":"candles", "target":first}),
    );
    let _other = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let other = target(&app, 1, PaneSide::Flow);
    let before = inspect(&mut app, &other);
    app.arrangement_adapter().close_tab(0);
    run_frame(&mut app, &ctx);
    assert!(
        dispatch(
            &mut app,
            json!({"action":"apply_candle_preset", "name":"classic"})
        )
        .is_err()
    );
    assert!(dispatch(&mut app, json!({"action":"update_candles", "style":before})).is_err());
    assert_eq!(inspect(&mut app, &other), before);
    let deleted = call(
        &mut app,
        json!({"action":"delete_candle_preset", "name":"retained"}),
    );
    assert_eq!(deleted["custom_presets"], json!([]));
    let closed = call(&mut app, json!({"action":"close"}));
    assert_eq!(closed["open"], false);
    assert_eq!(inspect(&mut app, &other), before);
}

#[test]
fn local_candles_closed_startup_never_pins_a_ghost_editor_across_replay_and_tab_changes() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    assert!(app.surfaces.hooks_pending());
    assert!(!app.surfaces.chart_settings.model_state.is_open());
    run_frame(&mut app, &ctx);
    assert_eq!(app.surfaces.chart_settings.model_state.editor.target, None);
    let flow = target(&app, 0, PaneSide::Flow);
    let time = target(&app, 0, PaneSide::Time(0));
    let session = quantick_replay::Session::from_text(
        std::path::Path::new("ETHUSDT_2026-03-16.csv"),
        "# quantick,csv,1\n# symbol=ETHUSDT\n# timezone=UTC\nDate,Time,Price,Volume,Side\n2026-03-16,10:01:08.000,2000,12,B\n",
        quantick_replay::ParseOptions::default(),
    ).unwrap();
    with_config(&mut app, |tab, config| {
        tab.open_replay(
            config,
            quantick_feed::ReplayRequest {
                session: std::sync::Arc::new(session),
                options: quantick_feed::ReplayOptions {
                    autoplay: false,
                    ..Default::default()
                },
            },
        )
    });
    run_frame(&mut app, &ctx);
    for selected in [&flow, &time] {
        assert_eq!(
            call(&mut app, json!({"action":"inspect", "target":selected}))["target"],
            *selected
        );
    }
    let _other = open_second_tab(&mut app, &ctx, "TESTUSDT");
    let focused = target(&app, 1, app.active_tab().focused_side());
    assert_eq!(
        call(&mut app, json!({"action":"inspect"}))["target"],
        focused
    );
    assert_eq!(
        call(&mut app, json!({"action":"inspect", "target":flow}))["target"],
        flow
    );
    assert!(!app.surfaces.chart_settings.model_state.is_open());
    assert_eq!(app.surfaces.chart_settings.model_state.editor.target, None);
    assert!(!app.surfaces.chart_settings.model_state.editor.stale);
}

#[test]
fn local_candles_first_frame_hook_pins_only_the_opened_chart_and_captures_its_style() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let flow = target(&app, 0, PaneSide::Flow);
    let mut expected = inspect(&mut app, &flow);
    expected["candles"]["wick_width"] = json!("3");
    expected["canvas"]["background"] = json!([11, 22, 33]);
    expected["ownership"]["background"] = json!("explicit");
    call(
        &mut app,
        json!({"action":"update_candles", "target":flow, "style":expected}),
    );
    crate::hooks::captured::set_on_this_thread(Some(&[("QUANTICK_STYLE_PANEL", "1")]));
    run_frame(&mut app, &ctx);
    crate::hooks::captured::set_on_this_thread(None);
    assert!(app.surfaces.chart_settings.model_state.is_open());
    assert_eq!(
        serde_json::to_value(
            app.surfaces
                .chart_settings
                .model_state
                .editor
                .target
                .map(|target| {
                    quantick_control_schema::chart_settings::ChartSettingsTarget {
                        tab_id: quantick_control::wire::WireU64(target.tab_id),
                        pane_id: quantick_control::wire::WireU64(target.pane_id),
                    }
                })
        )
        .unwrap(),
        flow
    );
    let _other = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let changed = call(
        &mut app,
        json!({"action":"apply_candle_preset", "name":"glass"}),
    );
    assert_eq!(changed["target"], flow);
    let reset = call(&mut app, json!({"action":"reset_candles"}));
    assert_eq!(reset["effective_style"], expected);
}

#[test]
fn local_candles_source_retarget_invalidates_the_sitting_until_explicit_reopen() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let selected = target(&app, 0, PaneSide::Flow);
    call(
        &mut app,
        json!({"action":"open", "tab":"candles", "target":selected}),
    );
    // The actual feed switch reuses the pane identity, but retires its source context.
    app.active_tab_mut().symbol = "ETHUSDT".into();
    let (tab, config) = app.active_with_config();
    tab.maybe_switch_feed(config);
    assert_eq!(target(&app, 0, PaneSide::Flow), selected);
    assert!(
        dispatch(
            &mut app,
            json!({"action":"apply_candle_preset", "name":"glass"})
        )
        .is_err()
    );
    run_frame(&mut app, &ctx);
    assert!(app.surfaces.chart_settings.model_state.editor.stale);
    assert!(dispatch(&mut app, json!({"action":"reset_candles"})).is_err());
    let reopened = call(
        &mut app,
        json!({"action":"open", "tab":"candles", "target":selected}),
    );
    assert!(!app.surfaces.chart_settings.model_state.editor.stale);
    let applied = call(
        &mut app,
        json!({"action":"apply_candle_preset", "name":"glass"}),
    );
    assert_eq!(applied["target"], selected);
    let reset = call(&mut app, json!({"action":"reset_candles"}));
    assert_eq!(reset["effective_style"], reopened["effective_style"]);
}

#[test]
fn local_candles_same_rgb_explicit_color_survives_theme_roundtrips() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let targets = three_charts(&mut app, &ctx);
    let mut edited = inspect(&mut app, &targets[1]);
    let authored = edited["candles"]["bull_fill"].clone();
    assert_eq!(edited["ownership"]["bull_fill"], "theme");
    // The bytes are unchanged: provenance alone makes this an explicit choice.
    edited["ownership"]["bull_fill"] = json!("explicit");
    call(
        &mut app,
        json!({"action":"update_candles", "target":targets[1], "style":edited}),
    );
    for source in [
        quantick_appearance::LIGHT,
        quantick_appearance::DARK,
        quantick_appearance::LIGHT,
    ] {
        app.runtime
            .appearance
            .apply_candidate(source, false)
            .unwrap();
        run_frame(&mut app, &ctx);
        let appearance = app.runtime.appearance.resolved().clone();
        let local = inspect(&mut app, &targets[1]);
        assert_eq!(local["candles"]["bull_fill"], authored);
        assert_eq!(local["ownership"]["bull_fill"], "explicit");
        for index in [0, 2] {
            let theme = if index == 0 {
                appearance.bookmap()
            } else {
                &appearance
            };
            let untouched = inspect(&mut app, &targets[index]);
            let buy = theme.color(quantick_appearance::Token::Buy).to_array();
            assert_eq!(untouched["candles"]["bull_fill"], json!(&buy[..3]));
            assert_eq!(untouched["ownership"]["bull_fill"], "theme");
        }
    }
}

#[test]
fn local_candles_workspace_save_and_restart_restore_each_chart_and_ownership() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let targets = three_charts(&mut app, &ctx);
    for (index, selected) in targets.iter().enumerate() {
        let mut edited = inspect(&mut app, selected);
        edited["candles"]["gap_px"] = json!((3 + index).to_string());
        edited["canvas"]["grid_enabled"] = json!(index % 2 == 0);
        edited["candles"]["bull_fill"] = json!([19, 59 + index, 89]);
        edited["ownership"]["bull_fill"] = json!("explicit");
        call(
            &mut app,
            json!({"action":"update_candles", "target":selected, "style":edited}),
        );
    }
    let expected = styles(&mut app, &targets);
    app.workspace_save_adapter()
        .save_workspace("local-candles-restart");
    let saved = ui_state::load(app.runtime.workspace.ui_state_path());
    let document = toml::to_string(&saved).unwrap();
    assert!(
        document.contains("candle_styles"),
        "the normal workspace document carries local styles"
    );
    let (mut restarted, _events, _commands, _book) =
        test_app_with_workspace_and_launch(saved, AppLaunch::default());
    let restarted_ctx = egui::Context::default();
    run_frame(&mut restarted, &restarted_ctx);
    run_frame(&mut restarted, &restarted_ctx);
    assert_eq!(restarted.active_tab().time_panes.len(), 2);
    let restored = [PaneSide::Flow, PaneSide::Time(0), PaneSide::Time(1)]
        .map(|side| target(&restarted, 0, side));
    assert_eq!(styles(&mut restarted, &restored), expected);
    restarted
        .runtime
        .appearance
        .apply_candidate(quantick_appearance::LIGHT, false)
        .unwrap();
    run_frame(&mut restarted, &restarted_ctx);
    for (index, selected) in restored.iter().enumerate() {
        let actual = inspect(&mut restarted, selected);
        assert_eq!(
            actual["candles"]["bull_fill"],
            expected[index]["candles"]["bull_fill"]
        );
        assert_eq!(
            actual["candles"]["gap_px"],
            expected[index]["candles"]["gap_px"]
        );
        assert_eq!(actual["ownership"]["bull_fill"], "explicit");
        assert_eq!(actual["ownership"]["bear_fill"], "theme");
        assert_eq!(
            actual["canvas"]["grid_enabled"],
            expected[index]["canvas"]["grid_enabled"]
        );
    }
}

#[test]
fn local_candles_legacy_global_settings_seed_every_unedited_pane_without_rgb_inference() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let mut legacy = app.runtime.style.snapshot();
    legacy.candles.gap_px = 4.5;
    legacy.candles.bull_fill = [38, 166, 154];
    legacy.canvas.background = [211, 212, 213];
    app.runtime.style = legacy;
    let targets = three_charts(&mut app, &ctx);
    for source in [quantick_appearance::DARK, quantick_appearance::LIGHT] {
        app.runtime
            .appearance
            .apply_candidate(source, false)
            .unwrap();
        run_frame(&mut app, &ctx);
        for selected in &targets {
            let actual = inspect(&mut app, selected);
            assert_eq!(actual["candles"]["bull_fill"], json!([38, 166, 154]));
            assert_eq!(actual["candles"]["gap_px"], "4.5");
            assert_eq!(actual["canvas"]["background"], json!([211, 212, 213]));
            assert_eq!(actual["ownership"]["bull_fill"], "explicit");
        }
    }
    call(
        &mut app,
        json!({"action":"apply_candle_preset", "target":targets[1], "name":"classic"}),
    );
    for index in [0, 2] {
        let actual = inspect(&mut app, &targets[index]);
        assert_eq!(actual["candles"]["gap_px"], "4.5");
        assert_eq!(actual["canvas"]["background"], json!([211, 212, 213]));
    }
    assert_eq!(app.runtime.style, legacy);
}

#[test]
fn local_candles_hooks_capture_source_before_an_immediate_retarget() {
    for hook in ["QUANTICK_STYLE_PANEL", "QUANTICK_FOOTPRINT_PANEL"] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(20);
        let selected = target(&app, 0, PaneSide::Flow);
        crate::hooks::captured::set_on_this_thread(Some(&[(hook, "1")]));
        run_frame(&mut app, &ctx);
        crate::hooks::captured::set_on_this_thread(None);
        // No intervening frame or control call may initialize the old context.
        app.active_tab_mut().symbol = "ETHUSDT".into();
        let (tab, config) = app.active_with_config();
        tab.maybe_switch_feed(config);
        assert!(
            dispatch(
                &mut app,
                json!({"action":"apply_candle_preset", "name":"glass"})
            )
            .is_err()
        );
        assert!(dispatch(&mut app, json!({"action":"reset_candles"})).is_err());
        assert!(app.surfaces.chart_settings.model_state.editor.stale);
        let reopened = call(
            &mut app,
            json!({"action":"open", "tab":"candles", "target":selected}),
        );
        let applied = call(
            &mut app,
            json!({"action":"apply_candle_preset", "name":"glass"}),
        );
        assert_eq!(applied["target"], selected);
        let reset = call(&mut app, json!({"action":"reset_candles"}));
        assert_eq!(reset["effective_style"], reopened["effective_style"]);
    }
}
