//! Binance market isolation and preservation of formerly shared BTC settings.

use super::*;
use crate::bubble_assets::{ASSETS_FILE, save_to};
use crate::bubble_presets;
use crate::scratch::ScratchDir;

fn config() -> AppConfig {
    let mut config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
    let other = config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "hyperliquid")
        .unwrap();
    other.symbols.push("BTCUSDT".to_owned());
    other
        .symbol_bubble_presets
        .insert("BTCUSDT".to_owned(), "dense tape".to_owned());
    config
}

fn bind(
    store: &SharedAssetBubbles,
    config: &AppConfig,
    feed: &str,
    symbol: &str,
) -> (AssetBinding, AssetBubbles) {
    AssetBinding::bind(
        store.clone(),
        config,
        (feed, symbol),
        &bubble_presets::embedded(),
        false,
    )
}

#[test]
fn btc_identity_is_market_specific_and_current_feed_presets_cannot_leak() {
    let config = config();
    let spot = config.bubble_asset("binance", "BTCUSDT");
    let future = config.bubble_asset("binance-futures", "BTCUSDT.p");
    let other = config.bubble_asset("hyperliquid", "BTCUSDT");
    assert_eq!(spot.key, "BTCUSDT");
    assert_eq!(spot.preset, None);
    assert_eq!(future.key, "binance-futures:BTCUSDT.p");
    assert_ne!(other.key, spot.key);
    assert_ne!(other.key, future.key);
    assert_eq!(other.preset.as_deref(), Some("dense tape"));
    assert_eq!(config.bubble_asset("binance", "btcusdt").key, spot.key);
    assert_eq!(
        config.bubble_asset("binance-futures", "BTCUSDT.P").key,
        future.key
    );
    assert_eq!(
        config.bubble_asset("binance", "BTCUSDT.p"),
        future,
        "a perpetual recording remains its market"
    );
    assert_eq!(
        config.bubble_asset("binance-futures", "BTCUSDT"),
        spot,
        "a spot recording remains its market"
    );
}

#[test]
fn futures_is_selectable_without_changing_spot_startup_and_refuses_bad_aliases() {
    let config = config();
    assert_eq!(config.default_feed, "binance");
    assert_eq!(config.default_symbol, "BTCUSDT");
    let future = config.feed("binance-futures").unwrap();
    assert_eq!(future.provider, crate::config::ProviderKind::BinanceFutures);
    assert_eq!(future.symbols, ["BTCUSDT.p"]);
    assert!(config.validate().is_ok());
    for invalid in ["BTCUSDT", "btcusdt.p", "BTCUSDT.P", "BTC USDT.p"] {
        let mut invalid_config = config.clone();
        invalid_config
            .feeds
            .iter_mut()
            .find(|feed| feed.id == "binance-futures")
            .unwrap()
            .symbols = vec![invalid.to_owned()];
        let error = invalid_config.validate().unwrap_err();
        assert!(
            error.contains("requires canonical USD-M symbols"),
            "{error}"
        );
    }
}

#[test]
fn legacy_other_venue_appearance_survives_spot_edit_reset_and_restart() {
    let dir = ScratchDir::new("btc-legacy-venue");
    let path = dir.join(ASSETS_FILE);
    let config = config();
    let mut file = AssetBubblesFile::default();
    let mut legacy =
        AssetBubbles::declared(bubble_presets::embedded().get("default").unwrap(), false);
    legacy.look.bubbles.max_radius = 21.0;
    file.assets.insert("BTCUSDT".to_owned(), legacy.clone());
    save_to(&path, &file).unwrap();
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut spot, opened) = bind(&store, &config, "binance", "BTCUSDT");
    let (mut other, preserved) = bind(&store, &config, "hyperliquid", "BTCUSDT");
    assert_eq!(opened, legacy);
    assert_eq!(preserved, legacy);
    let mut edit = opened;
    edit.look.bubbles.max_radius = 29.0;
    spot.note_edit();
    assert!(spot.file(edit.clone()));
    assert_eq!(
        other.adoption(),
        None,
        "spot edits never reach another venue"
    );
    assert_eq!(
        bind(&store, &config, "binance-futures", "BTCUSDT.p")
            .1
            .look
            .bubbles
            .max_radius,
        15.0
    );
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    let restarted = AssetBubblesStore::load(path.clone()).shared();
    assert_eq!(
        bind(&restarted, &config, "hyperliquid", "BTCUSDT").1,
        legacy
    );
    assert_eq!(bind(&restarted, &config, "binance", "BTCUSDT").1, edit);
    let other_default = other.declared.clone();
    other.note_edit();
    assert!(other.file(other_default.clone()));
    spot.note_edit();
    assert!(spot.file(spot.declared.clone()));
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    let restarted = AssetBubblesStore::load(path).shared();
    assert_eq!(
        bind(&restarted, &config, "hyperliquid", "BTCUSDT").1,
        other_default
    );
    assert_eq!(
        bind(&restarted, &config, "binance", "BTCUSDT").1,
        spot.declared
    );
}

#[test]
fn saving_off_without_a_look_migrates_and_stays_local_to_the_venue() {
    let dir = ScratchDir::new("btc-legacy-saving-off");
    let path = dir.join(ASSETS_FILE);
    let config = config();
    let mut file = AssetBubblesFile::default();
    file.save_changes_off.insert("BTCUSDT".to_owned());
    save_to(&path, &file).unwrap();
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (spot, _) = bind(&store, &config, "binance", "BTCUSDT");
    let (mut other, opened) = bind(&store, &config, "hyperliquid", "BTCUSDT");
    let (mut other_tab, _) = bind(&store, &config, "hyperliquid", "BTCUSDT");
    assert!(!spot.saves_changes());
    assert!(!other.saves_changes());
    assert!(
        bind(&store, &config, "binance-futures", "BTCUSDT.p")
            .0
            .saves_changes()
    );
    let mut edit = opened.clone();
    edit.look.bubbles.max_radius = 31.0;
    other.note_edit();
    assert!(!other.file(edit.clone()));
    assert_eq!(other_tab.adoption(), None);
    assert_eq!(other.on_screen(), &edit);
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    let restarted = AssetBubblesStore::load(path).shared();
    let (other, restored) = bind(&restarted, &config, "hyperliquid", "BTCUSDT");
    assert!(!other.saves_changes());
    assert_eq!(restored, opened);
}

#[test]
fn migration_preserves_an_existing_scoped_override() {
    let dir = ScratchDir::new("btc-already-scoped");
    let path = dir.join(ASSETS_FILE);
    let config = config();
    let key = config.bubble_asset("hyperliquid", "BTCUSDT").key;
    let mut file = AssetBubblesFile::default();
    let legacy = AssetBubbles::declared(bubble_presets::embedded().get("default").unwrap(), false);
    let scoped =
        AssetBubbles::declared(bubble_presets::embedded().get("dense tape").unwrap(), false);
    file.assets.insert("BTCUSDT".to_owned(), legacy);
    file.assets.insert(key, scoped.clone());
    file.save_changes_off.insert("BTCUSDT".to_owned());
    save_to(&path, &file).unwrap();
    let store = AssetBubblesStore::load(path).shared();
    let (other, restored) = bind(&store, &config, "hyperliquid", "BTCUSDT");
    assert_eq!(restored, scoped);
    assert!(other.saves_changes());
}
