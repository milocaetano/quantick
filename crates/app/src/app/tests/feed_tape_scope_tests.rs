//! Built-in crypto sources retain their existing look after visiting WIN.
use super::*;
use crate::orderflow_view::OrderflowView;

fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../../config/feeds.toml")).expect("shipped feeds parse")
}

fn assert_existing_crypto_look(view: &OrderflowView) {
    let presets = crate::bubble_presets::embedded();
    assert_eq!(presets.active, "default", "pin the preexisting shipped look");
    let mut expected = quantick_orderflow::HeatmapConfig::default();
    presets.get("default").unwrap().apply_to(&mut expected);
    let actual = view.cached_config();
    assert!(!actual.live_lane.tape_only, "WIN pane mode cannot follow onto crypto");
    assert!(!actual.volume_dots.enabled, "the existing crypto bubbles remain legacy");
    assert_eq!(actual.bubbles, expected.bubbles, "do not alter BTC styling");
    assert_eq!(actual.bubble_cluster_ms, expected.bubble_cluster_ms);
    assert_eq!(actual.bubble_candle_summary, expected.bubble_candle_summary);
}

#[test]
fn leaving_win_for_builtin_crypto_restores_its_existing_bubbles() {
    for (feed, symbol) in [
        ("binance", "BTCUSDT"),
        ("binance", "ETHUSDT"),
        ("hyperliquid", "BTC"),
        ("hyperliquid", "ETH"),
    ] {
        let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
        assert!(app.active_tab().tape().cached_config().tape_only());
        let tab = app.active_tab_mut();
        tab.feed_id = feed.to_owned();
        tab.symbol = symbol.to_owned();
        tab.tape_mut().reset_for_symbol(symbol);
        with_config(&mut app, |tab, config| {
            tab.apply_feed_bubble_preset_after_switch(config, "metatrader-b3", "WINV26");
        });
        assert_existing_crypto_look(app.active_tab().tape());
    }
}

#[test]
fn crypto_declaration_overrides_a_saved_win_look_when_a_tab_opens() {
    for (feed, symbol) in [("binance", "BTCUSDT"), ("hyperliquid", "BTC")] {
        let mut app = app_on(shipped_config(), feed, symbol);
        // Loading an active WIN preset precedes this same owner call during
        // tab creation; no global presets file needs to be changed by a test.
        assert!(app.active_tab_mut().tape_mut().apply_preset("mini index regions"));
        with_config(&mut app, |tab, config| tab.apply_feed_bubble_preset(config));
        assert_existing_crypto_look(app.active_tab().tape());
    }
}

#[test]
fn an_undeclared_custom_feed_still_keeps_the_users_active_look() {
    let mut app = app_on(test_config(), "binance", "TESTUSDT");
    assert!(app.active_tab_mut().tape_mut().apply_preset("mini index regions"));
    with_config(&mut app, |tab, config| {
        tab.apply_feed_bubble_preset_after_switch(config, "custom-feed", "WINV26");
    });
    assert!(app.active_tab().tape().cached_config().tape_only());
    assert_eq!(app.active_tab().tape().active_preset_for_test(), "mini index regions");
}
