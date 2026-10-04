//! Bubble settings belong to an asset: what one market's tab changes never
//! dresses another market, and a market shown again comes back as it was left.
use super::*;
use crate::bubble_presets::BubblePreset;
use quantick_orderflow::LaneWindow;

fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../../config/feeds.toml")).expect("shipped feeds parse")
}

fn look(app: &QuantickApp) -> BubblePreset {
    let tape = app.active_tab().tape();
    BubblePreset::capture(tape.active_preset_for_test(), tape.cached_config())
}

fn candle_aggression(app: &QuantickApp) -> bool {
    app.active_tab()
        .flow_pane
        .layer_switched_on(ChartLayer::CandleAggression, &app.style)
}

/// The tab's own symbol switch, without a live feed behind it.
fn select_market(app: &mut QuantickApp, feed: &str, symbol: &str) {
    let tab = app.active_tab_mut();
    tab.feed_id = feed.to_owned();
    tab.symbol = symbol.to_owned();
    tab.tape_mut().reset_for_symbol(symbol);
    with_config(app, |tab, config| {
        tab.apply_asset_bubbles_after_switch(config);
    });
}

/// Edits made on the mini index while it is on screen.
fn edit_win(app: &mut QuantickApp) -> BubblePreset {
    let tape = app.active_tab_mut().tape_mut();
    tape.set_live_lane_window(LaneWindow::Fixed { ms: 7_000 });
    assert!(tape.set_ignore_opening_burst_in_scale(true));
    look(app)
}

#[test]
fn a_win_edit_survives_a_round_trip_through_btc_in_one_tab() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    assert!(app.active_tab().tape().cached_config().native_tape());
    let edited = edit_win(&mut app);
    select_market(&mut app, "binance", "BTCUSDT");
    assert!(
        !app.active_tab().tape().cached_config().native_tape(),
        "BTC opens without the mini index tape"
    );
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert_eq!(look(&app), edited, "WIN comes back exactly as it was left");
}

#[test]
fn a_win_edit_leaves_btc_on_its_own_look() {
    let mut app = app_on(shipped_config(), "binance", "BTCUSDT");
    let btc = look(&app);
    select_market(&mut app, "metatrader-b3", "WINV26");
    edit_win(&mut app);
    select_market(&mut app, "binance", "BTCUSDT");
    assert_eq!(look(&app), btc, "BTC is untouched by the WIN edit");
    assert!(
        !app.active_tab()
            .tape()
            .cached_config()
            .volume_dots
            .ignore_opening_burst_in_scale
    );
}

#[test]
fn another_markets_declared_look_does_not_follow_the_tab_to_btc() {
    let mut app = app_on(shipped_config(), "binance", "BTCUSDT");
    let btc = look(&app);
    select_market(&mut app, "metatrader-b3", "WDO$N");
    assert_eq!(look(&app).name, "live lane pie");
    select_market(&mut app, "binance", "BTCUSDT");
    assert_eq!(look(&app), btc, "BTC keeps its own look, not WDO's");
}

#[test]
fn candle_aggression_switched_on_win_stays_with_win() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    switch_layer(&mut app, ChartLayer::CandleAggression, true);
    assert!(candle_aggression(&app));
    select_market(&mut app, "binance", "BTCUSDT");
    assert!(
        !candle_aggression(&app),
        "BTC does not inherit WIN's candle aggression"
    );
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert!(candle_aggression(&app), "WIN gets its own switch back");
}

#[test]
fn the_flow_opening_scale_set_on_win_stays_with_win() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_flow_opening(true)
    );
    select_market(&mut app, "binance", "BTCUSDT");
    assert!(!app.active_tab().tape().ignore_flow_opening());
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert!(app.active_tab().tape().ignore_flow_opening());
}

#[test]
fn a_future_win_roll_opens_on_the_mini_index_look() {
    let mut config = shipped_config();
    config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "metatrader-b3")
        .expect("the B3 feed")
        .symbols
        .push("WINZ26".to_owned());
    let app = app_on(config, "metatrader-b3", "WINZ26");
    assert!(
        app.active_tab().tape().cached_config().native_tape(),
        "a dated roll the config never named is still the mini index"
    );
    assert_eq!(look(&app).name, "mini index regions");
}

#[test]
fn the_mini_dollar_beside_it_is_not_the_mini_index() {
    let app = app_on(shipped_config(), "metatrader-b3", "WDO$N");
    assert!(!app.active_tab().tape().cached_config().native_tape());
    assert_eq!(look(&app).name, "live lane pie");
}

/// Every shipped market outside the WIN family opens as it did before the
/// native tape existed: no native tape, no tape-only pane, no volume dots, no
/// opening exclusion and no candle aggression.
#[test]
fn no_market_outside_the_win_family_opens_with_the_tape_features() {
    let config = shipped_config();
    for feed in &config.feeds {
        for symbol in feed
            .symbols
            .iter()
            .filter(|symbol| !symbol.starts_with("WIN"))
        {
            let app = app_on(config.clone(), &feed.id, symbol);
            let tape = app.active_tab().tape().cached_config();
            let at = format!("{}/{symbol}", feed.id);
            assert!(!tape.native_tape(), "{at} opens on the native tape");
            assert!(!tape.live_lane.tape_only, "{at} opens tape only");
            assert!(!tape.volume_dots.enabled, "{at} opens on volume dots");
            assert!(
                !tape.volume_dots.ignore_opening_burst_in_scale,
                "{at} excludes the opening"
            );
            assert!(
                !candle_aggression(&app),
                "{at} opens with candle aggression"
            );
        }
    }
}

/// Two tabs, WIN and BTC: candle aggression switched on in the WIN tab is
/// WIN's, so a restart does not open the BTC tab with it.
#[test]
fn candle_aggression_on_a_win_tab_does_not_reach_a_btc_tab_after_a_restart() {
    let dir = crate::scratch::ScratchDir::new("asset-layers");
    let path = dir.join("chart-layers.toml");
    let mut win = app_on(shipped_config(), "metatrader-b3", "WINV26");
    win.workspace.set_chart_layers_path(path.clone());
    let mask = win.active_tab().flow_pane.layer_mask(&win.style);
    win.workspace.layers_mut().record(mask);
    switch_layer(&mut win, ChartLayer::CandleAggression, true);
    win.layer_wiring().maintain(&egui::Context::default());

    let mut btc = app_on(shipped_config(), "binance", "BTCUSDT");
    btc.workspace.set_chart_layers_path(path);
    btc.layer_wiring().restore();
    assert!(
        !candle_aggression(&btc),
        "the BTC tab opens without WIN's candle aggression"
    );
}
