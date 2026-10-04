//! The WIN tape look is WIN's: every other market keeps, and gets back, its own.
use super::*;
use crate::bubble_presets::BubblePreset;
use quantick_orderflow::LaneWindow;

fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../../config/feeds.toml")).expect("shipped feeds parse")
}

fn appearance(app: &QuantickApp) -> BubblePreset {
    let tape = app.active_tab().tape();
    BubblePreset::capture(tape.active_preset_for_test(), tape.cached_config())
}

fn select_market(app: &mut QuantickApp, feed: &str, symbol: &str) {
    let tab = app.active_tab_mut();
    tab.feed_id = feed.to_owned();
    tab.symbol = symbol.to_owned();
    tab.tape_mut().reset_for_symbol(symbol);
    with_config(app, |tab, config| {
        tab.apply_asset_bubbles_after_switch(config)
    });
}

fn customize_crypto(app: &mut QuantickApp) -> BubblePreset {
    let tape = app.active_tab_mut().tape_mut();
    assert!(tape.apply_preset("dense tape"));
    tape.set_live_lane_window(LaneWindow::Fixed { ms: 17_000 });
    appearance(app)
}

#[test]
fn automatic_win_look_restores_the_prior_custom_crypto_appearance() {
    let mut config = shipped_config();
    let mut custom = config.feed("binance").unwrap().clone();
    custom.id = "custom-btc".to_owned();
    custom.bubble_preset = None;
    config.feeds.push(custom);
    for (feed, symbol) in [
        ("binance", "BTCUSDT"),
        ("binance", "ETHUSDT"),
        ("hyperliquid", "BTC"),
        ("custom-btc", "BTCUSDT"),
    ] {
        let mut app = app_on(config.clone(), feed, symbol);
        let before = customize_crypto(&mut app);
        select_market(&mut app, "metatrader-b3", "WINV26");
        assert!(app.active_tab().tape().cached_config().native_tape());
        select_market(&mut app, feed, symbol);
        assert_eq!(appearance(&app), before, "restore the exact look on {feed}");
    }
}

#[test]
fn a_crypto_tab_reopens_on_its_filed_custom_look() {
    for (feed, symbol) in [("binance", "BTCUSDT"), ("hyperliquid", "BTC")] {
        let mut app = app_on(shipped_config(), feed, symbol);
        let before = customize_crypto(&mut app);
        app.active_tab_mut().file_asset_bubbles();
        // What a restart runs for the tab it opens.
        with_config(&mut app, |tab, config| tab.apply_asset_bubbles(config));
        assert_eq!(appearance(&app), before);
    }
}

#[test]
fn a_declared_look_stays_with_its_market() {
    let mut app = app_on(shipped_config(), "binance", "BTCUSDT");
    let before = customize_crypto(&mut app);
    select_market(&mut app, "metatrader-b3", "WINV26");
    select_market(&mut app, "metatrader-b3", "WDO$N");
    let declared = appearance(&app);
    assert_eq!(declared.name, "live lane pie");
    assert_ne!(declared, before);
    select_market(&mut app, "binance", "BTCUSDT");
    assert_eq!(
        appearance(&app),
        before,
        "BTC gets its own look back, not the mini dollar's"
    );
}

#[test]
fn a_tape_scope_ends_on_an_undeclared_symbol_inside_the_same_feed() {
    let mut config = shipped_config();
    config
        .feeds
        .iter_mut()
        .find(|f| f.id == "metatrader-b3")
        .unwrap()
        .bubble_preset = None;
    let mut app = app_on(config, "metatrader-b3", "WDO$N");
    let before = customize_crypto(&mut app);
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert!(app.active_tab().tape().cached_config().native_tape());
    select_market(&mut app, "metatrader-b3", "WDO$N");
    assert_eq!(appearance(&app), before);
}

#[test]
fn a_hop_inside_the_win_family_keeps_edits_and_btc_its_own_look() {
    let mut app = app_on(shipped_config(), "binance", "BTCUSDT");
    let before = customize_crypto(&mut app);
    select_market(&mut app, "metatrader-b3", "WIN$N");
    app.active_tab_mut()
        .tape_mut()
        .set_live_lane_window(LaneWindow::Fixed { ms: 7_000 });
    let edited_win = appearance(&app);
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert_eq!(appearance(&app), edited_win, "one asset, one set of edits");
    select_market(&mut app, "binance", "BTCUSDT");
    assert_eq!(appearance(&app), before, "BTC's own look");
}

#[test]
fn an_undeclared_custom_feed_still_keeps_the_users_manual_tape_look() {
    let mut app = app_on(test_config(), "binance", "TESTUSDT");
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_preset("mini index regions")
    );
    let before = appearance(&app);
    with_config(&mut app, |tab, config| {
        tab.apply_asset_bubbles_after_switch(config)
    });
    assert_eq!(
        appearance(&app),
        before,
        "a manual choice is the asset's own"
    );
}
