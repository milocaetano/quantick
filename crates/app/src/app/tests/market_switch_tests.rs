//! The toolbar's market button and the palette's switch mode: one tab-market
//! state, one switch path, drawings kept with each market.
use super::*;
use quantick_workspace::market_list::{PaletteIntent, SwitchChoice};

pub(super) fn painted_starting(output: &egui::FullOutput, prefix: &str) -> Vec<String> {
    let mut found = painted_text(output);
    found.retain(|text| text.starts_with(prefix));
    found
}

fn switch_to(app: &QuantickApp, symbol: &str) -> PaletteIntent {
    PaletteIntent::Switch(SwitchChoice {
        tab_id: app.runtime.tabs.active_id(),
        feed_id: "binance".to_owned(),
        symbol: symbol.to_owned(),
        bar: None,
        workspace: None,
        add: false,
    })
}

/// Each market keeps its own drawings: a level on A is gone on B and back
/// on A, and the tab keeps its bar through both switches.
#[test]
fn a_switch_keeps_drawings_with_each_market() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    place_level(&mut app, PaneSide::Flow, 100.0);
    run_frame(&mut app, &ctx);
    let bars = format!("{:?}", app.active_tab().flow_pane.spec.spec());

    let intent = switch_to(&app, "ETHUSDT");
    super::super::frame::apply_palette_intent(&mut app, intent);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert!(
        drawings_on(&app, PaneSide::Flow).is_empty(),
        "B has none of A's"
    );

    let intent = switch_to(&app, "TESTUSDT");
    super::super::frame::apply_palette_intent(&mut app, intent);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "TESTUSDT");
    assert_eq!(
        drawings_on(&app, PaneSide::Flow),
        vec![100.0],
        "A's came back"
    );
    assert_eq!(
        format!("{:?}", app.active_tab().flow_pane.spec.spec()),
        bars
    );
}

/// A bar picked in switch mode lands on the switched tab's flow pane.
#[test]
fn a_switch_with_a_picked_bar_puts_the_flow_pane_on_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let PaletteIntent::Switch(mut choice) = switch_to(&app, "ETHUSDT") else {
        unreachable!()
    };
    choice.bar = Some("tick:7".to_owned());
    super::super::frame::apply_palette_intent(&mut app, PaletteIntent::Switch(choice));
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert_eq!(
        app.active_tab().flow_pane.spec.spec().to_config_string(),
        "tick:7"
    );
    assert!(!app.surfaces.market_palette.model_state.is_open());
}

/// The market button opens the palette in switch mode on the active tab,
/// offering the roll the feed config names; a recording has no button to
/// press and the call refuses it.
#[test]
fn the_market_button_opens_the_palette_in_switch_mode_with_the_roll() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    app.config.feeds[0].contract_rolls =
        std::collections::BTreeMap::from([("TESTUSDT".to_owned(), "ETHUSDT".to_owned())]);
    run_frame(&mut app, &ctx);
    app.apply_toolbar_action(ToolbarAction::SwitchMarket);
    let palette = app
        .surfaces
        .market_palette
        .model_state
        .open
        .as_ref()
        .expect("open");
    let target = palette.switch_target().expect("switch mode");
    assert_eq!(target.tab_id, app.runtime.tabs.active_id());
    assert_eq!(target.symbol, "TESTUSDT");
    assert_eq!(
        target.roll.as_ref().map(|roll| roll.target.as_str()),
        Some("ETHUSDT")
    );
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    assert!(app.surfaces.market_palette.model_state.is_open());
    assert!(
        !painted_starting(&output, "Switching TESTUSDT \u{b7} Binance on this tab").is_empty(),
        "the caption names the tab's market: {:?}",
        painted_text(&output)
    );
}

/// The roll row runs the roll's own rule and act: refused with a position
/// open, rolled once flat, keeping the level.
#[test]
fn the_palette_roll_is_the_capability_s_act_and_refuses_with_a_position_open() {
    let ctx = egui::Context::default();
    let (mut app, evt_tx, _commands, _book) = test_app();
    let scratch = crate::scratch::ScratchDir::new("contract-roll-palette");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(scratch.path().to_path_buf());
    app.config.feeds[0].contract_rolls =
        std::collections::BTreeMap::from([("TESTUSDT".to_owned(), "ETHUSDT".to_owned())]);
    let tab_id = app.runtime.tabs.active_id();
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    assert!(!app.active_tab().paper.is_flat(), "a position is open");

    let roll = || PaletteIntent::Roll {
        tab_id,
        from_symbol: "TESTUSDT".to_owned(),
    };
    app.apply_toolbar_action(ToolbarAction::SwitchMarket);
    super::super::frame::apply_palette_intent(&mut app, roll());
    assert_eq!(app.active_tab().active.1, "TESTUSDT", "the roll refused");
    let refusal = (app.surfaces.market_palette.model_state.open.as_ref())
        .and_then(|palette| palette.refusal().map(str::to_owned));
    assert!(
        refusal
            .as_deref()
            .is_some_and(|reason| reason.contains("paper position")),
        "{refusal:?}"
    );

    app.active_tab_mut().paper.close_position();
    evt_tx.try_send(FeedEvent::Live(trade(5))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    assert!(app.active_tab().paper.is_flat(), "the position closed");
    super::super::frame::apply_palette_intent(&mut app, roll());
    assert_eq!(app.active_tab().active.1, "ETHUSDT", "flat, it rolled");
    assert!(
        !app.surfaces.market_palette.model_state.is_open(),
        "a roll closes the palette"
    );
    run_frame(&mut app, &ctx);
}
