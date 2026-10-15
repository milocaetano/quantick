//! Per-asset modes the config switches on (`symbol_asset_modes`): which
//! assets wear tick spheres, the unit their sizes count, and the key their
//! saved look lives under. No symbol, feed id or provider in code decides
//! any of it.

use super::*;
use crate::bubble_assets::{ASSETS_FILE, save_to};
use crate::bubble_look::BubbleLook;
use crate::bubble_presets;
use crate::config::AssetMode;
use crate::scratch::ScratchDir;
use quantick_orderflow::HeatmapConfig;

const SPOT_SPHERES: &str = "Binance BTC Spot spheres";

fn shipped() -> AppConfig {
    toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap()
}

fn mode(unit: &str) -> AssetMode {
    AssetMode {
        quantity_unit: unit.to_owned(),
        look_key: None,
    }
}

fn feed<'a>(config: &'a mut AppConfig, id: &str) -> &'a mut crate::config::FeedConfig {
    config.feeds.iter_mut().find(|feed| feed.id == id).unwrap()
}

/// The shipped config, with ETHUSDT and Hyperliquid's BTC switched on by
/// config alone.
fn opted_in() -> AppConfig {
    let mut config = shipped();
    let binance = feed(&mut config, "binance");
    binance
        .symbol_asset_modes
        .insert("ETHUSDT".to_owned(), mode("ETH"));
    let hyperliquid = feed(&mut config, "hyperliquid");
    hyperliquid
        .symbol_asset_modes
        .insert("BTC".to_owned(), mode("BTC"));
    config
}

/// Hyperliquid also offering BTCUSDT on a look of its own, with the mode
/// switched on for it: a market whose key the mode moves off main's.
fn venue_config() -> AppConfig {
    let mut config = shipped();
    let other = feed(&mut config, "hyperliquid");
    other.symbols.push("BTCUSDT".to_owned());
    other
        .symbol_bubble_presets
        .insert("BTCUSDT".to_owned(), "dense tape".to_owned());
    other
        .symbol_asset_modes
        .insert("BTCUSDT".to_owned(), mode("BTC"));
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

fn look(config: &AppConfig, feed: &str, symbol: &str) -> (BubbleLook, HeatmapConfig) {
    let mut screen = HeatmapConfig::default();
    let mut look = BubbleLook::open(
        (
            bubble_presets::embedded(),
            bubble_presets::PresetSource::Embedded,
            None,
        ),
        &mut screen,
    );
    let (binding, settings) = bind(&AssetBubblesStore::default().shared(), config, feed, symbol);
    look.bind(binding);
    look.wear(&settings, false, &mut screen);
    (look, screen)
}

/// The store at `path`, as the app opens it: read, then migrated once.
fn open(path: &std::path::Path, config: &AppConfig) -> SharedAssetBubbles {
    let mut store = AssetBubblesStore::load(path.to_path_buf());
    store.migrate_identities(config);
    store.shared()
}

#[test]
fn the_shipped_config_switches_the_mode_on_for_binance_btc_alone() {
    let config = shipped();
    let spot = config.bubble_asset("binance", "BTCUSDT");
    let perp = config.bubble_asset("binance-futures", "BTCUSDT.p");
    assert_eq!(spot.key, "BTCUSDT", "the key main always saved it under");
    assert_eq!(perp.key, "binance-futures:BTCUSDT.p", "feed id and symbol");
    assert_eq!(spot.preset.as_deref(), Some(SPOT_SPHERES));
    assert_eq!(
        perp.preset.as_deref(),
        Some("Binance BTC perpetual spheres")
    );
    for asset in [&spot, &perp] {
        assert_eq!(asset.mode.as_ref().unwrap().quantity_unit, "BTC");
    }
    let mut on = Vec::new();
    for offering in &config.feeds {
        for symbol in &offering.symbols {
            if config.bubble_asset(&offering.id, symbol).mode.is_some() {
                on.push(format!("{}/{symbol}", offering.id));
            }
        }
    }
    assert_eq!(on, ["binance/BTCUSDT", "binance-futures/BTCUSDT.p"]);
    assert_eq!(
        config.bubble_asset("binance", "BTCUSDT.p"),
        perp,
        "a perpetual recording remains its market"
    );
    assert_eq!(
        config.bubble_asset("binance-futures", "BTCUSDT"),
        spot,
        "a spot recording remains its market"
    );
}

#[test]
fn any_other_asset_gets_the_mode_by_config_alone() {
    assert!(shipped().bubble_asset("binance", "ETHUSDT").mode.is_none());
    assert_eq!(shipped().bubble_asset("hyperliquid", "BTC").key, "BTC");
    let config = opted_in();
    let eth = config.bubble_asset("binance", "ETHUSDT");
    assert_eq!(eth.key, "binance:ETHUSDT");
    assert_eq!(eth.mode.unwrap().quantity_unit, "ETH");
    let btc = config.bubble_asset("hyperliquid", "BTC");
    assert_eq!(btc.key, "hyperliquid:BTC");
    assert_eq!(btc.mode.unwrap().quantity_unit, "BTC");
    let mut keyed = config.clone();
    feed(&mut keyed, "binance")
        .symbol_asset_modes
        .get_mut("ETHUSDT")
        .unwrap()
        .look_key = Some("ETH-spot".to_owned());
    assert_eq!(keyed.bubble_asset("binance", "ETHUSDT").key, "ETH-spot");
    assert!(keyed.validate().is_ok());
}

#[test]
fn a_mode_names_an_offered_symbol_a_unit_and_a_usable_key() {
    for (key, unit, look_key, refusal) in [
        ("NOSUCH", "ETH", None, "matches none of the symbols"),
        ("ETHUSDT", " ", None, "empty quantity_unit"),
        ("ETHUSDT", "ETH", Some(" "), "empty look_key"),
    ] {
        let mut config = shipped();
        feed(&mut config, "binance").symbol_asset_modes.insert(
            key.to_owned(),
            AssetMode {
                quantity_unit: unit.to_owned(),
                look_key: look_key.map(str::to_owned),
            },
        );
        let error = config.validate().unwrap_err();
        assert!(error.contains(refusal), "{key}: {error}");
    }
    let typo = crate::config::EMBEDDED_DEFAULT.replace(
        "symbol_asset_modes = { \"BTCUSDT\" = {",
        "symbol_asset_modes = { \"BTCUSDT\" = { quantity_units = \"BTC\",",
    );
    assert_ne!(typo, crate::config::EMBEDDED_DEFAULT);
    let error = toml::from_str::<AppConfig>(&typo).unwrap_err().to_string();
    assert!(
        error.contains("quantity_units"),
        "a misspelt mode key is refused: {error}"
    );
}

#[test]
fn tick_choices_and_the_unit_follow_the_mode() {
    let config = opted_in();
    for (feed, symbol, unit) in [
        ("binance", "BTCUSDT", Some("BTC")),
        ("binance-futures", "BTCUSDT.p", Some("BTC")),
        ("binance", "ETHUSDT", Some("ETH")),
        ("hyperliquid", "BTC", Some("BTC")),
        ("hyperliquid", "ETH", None),
        ("metatrader-b3", "WIN$N", None),
    ] {
        let (look, _) = look(&config, feed, symbol);
        assert_eq!(look.quantity_unit(), unit, "{feed}/{symbol}");
        assert_eq!(!look.tick_presets().is_empty(), unit.is_some());
        assert_eq!(
            look.tick_style(Some(SPOT_SPHERES)).is_ok(),
            unit.is_some(),
            "{feed}/{symbol}"
        );
        assert_eq!(
            look.tick_style(None),
            Ok(None),
            "dropping spheres is always allowed"
        );
        assert!(look.tick_style(Some("missing")).is_err());
        let hint = look.tick_hint();
        match unit {
            Some(unit) => assert!(hint.contains(&format!("quantity in {unit}")), "{hint}"),
            None => assert!(!hint.contains("quantity in"), "{hint}"),
        }
    }
}

/// The verdict is the tick sphere rule's (control-handlers TickSpherePane),
/// which every door asks: it admits no asset without the mode.
#[test]
fn a_whole_preset_brings_spheres_only_where_the_rule_admits_them() {
    let config = shipped();
    for (feed, symbol, spheres) in [
        ("binance", "BTCUSDT", true),
        ("binance-futures", "BTCUSDT.p", false),
        ("binance", "ETHUSDT", false),
        ("metatrader-b3", "WIN$N", false),
    ] {
        let (mut look, mut screen) = look(&config, feed, symbol);
        assert!(look.apply_preset(SPOT_SPHERES, &mut screen, spheres));
        assert_eq!(
            screen.volume_dots.tick_bubbles.is_some(),
            spheres,
            "{feed}/{symbol}"
        );
    }
    let mut declared = shipped();
    feed(&mut declared, "binance")
        .symbol_bubble_presets
        .insert("ETHUSDT".to_owned(), SPOT_SPHERES.to_owned());
    let (_, opened) = bind(
        &AssetBubblesStore::default().shared(),
        &declared,
        "binance",
        "ETHUSDT",
    );
    assert_eq!(opened.look.name, SPOT_SPHERES);
    assert_eq!(
        opened.look.tick_bubbles, None,
        "a declared look brings no spheres without the mode"
    );
}

/// Review round 1 #4: a sphere preset worn without its spheres reads back as
/// itself, so picking the same one again is a no-op: no change, no edit for
/// the asset to file, no status.
#[test]
fn reapplying_a_sphere_preset_without_its_spheres_changes_nothing() {
    let (mut look, mut screen) = look(&shipped(), "binance", "ETHUSDT");
    let pick = |look: &mut BubbleLook, screen: &mut HeatmapConfig| {
        look.apply_named_preset(SPOT_SPHERES, screen, false)
    };
    assert_eq!(pick(&mut look, &mut screen), Ok(true));
    assert_eq!(screen.volume_dots.tick_bubbles, None);
    look.sync(&screen, false, false);
    assert!(!look.asset().unwrap().edited());
    let (before, status) = (screen.clone(), look.status().map(str::to_owned));

    assert_eq!(pick(&mut look, &mut screen), Ok(false));
    assert_eq!(screen, before);
    assert!(!look.asset().unwrap().edited(), "nothing to file");
    assert_eq!(look.status().map(str::to_owned), status);
}

#[test]
fn venue_identity_is_market_specific_and_current_feed_presets_cannot_leak() {
    let config = venue_config();
    let spot = config.bubble_asset("binance", "BTCUSDT");
    let other = config.bubble_asset("hyperliquid", "BTCUSDT");
    assert_eq!(other.key, "hyperliquid:BTCUSDT");
    assert_ne!(other.key, spot.key);
    assert_eq!(other.preset.as_deref(), Some("dense tape"));
    assert_eq!(spot.preset.as_deref(), Some(SPOT_SPHERES));
}

#[test]
fn futures_is_selectable_without_changing_spot_startup_and_refuses_bad_aliases() {
    let config = shipped();
    assert_eq!(config.default_feed, "binance");
    assert_eq!(config.default_symbol, "BTCUSDT");
    let future = config.feed("binance-futures").unwrap();
    assert_eq!(future.provider, crate::config::ProviderKind::BinanceFutures);
    assert_eq!(future.symbols, ["BTCUSDT.p"]);
    assert!(config.validate().is_ok());
    for invalid in ["BTCUSDT", "btcusdt.p", "BTCUSDT.P", "BTC USDT.p"] {
        let mut invalid_config = config.clone();
        let futures = feed(&mut invalid_config, "binance-futures");
        futures.symbols = vec![invalid.to_owned()];
        futures.symbol_bubble_presets.clear();
        futures.symbol_asset_modes.clear();
        let error = invalid_config.validate().unwrap_err();
        assert!(
            error.contains("requires canonical USD-M symbols"),
            "{error}"
        );
    }
}

#[test]
fn legacy_look_survives_a_moved_key_through_spot_edits_resets_and_restarts() {
    let dir = ScratchDir::new("asset-mode-legacy-venue");
    let path = dir.join(ASSETS_FILE);
    let config = venue_config();
    let mut file = AssetBubblesFile::default();
    let mut legacy =
        AssetBubbles::declared(bubble_presets::embedded().get("default").unwrap(), false);
    legacy.look.bubbles.max_radius = 21.0;
    file.assets.insert("BTCUSDT".to_owned(), legacy.clone());
    save_to(&path, &file).unwrap();
    let store = open(&path, &config);
    let (mut spot, opened) = bind(&store, &config, "binance", "BTCUSDT");
    let (mut other, preserved) = bind(&store, &config, "hyperliquid", "BTCUSDT");
    assert_eq!(opened, legacy, "spot's key did not move");
    assert_eq!(preserved, legacy, "the moved key starts on the old look");
    let mut edit = opened;
    edit.look.bubbles.max_radius = 29.0;
    spot.note_edit();
    assert!(spot.file(edit.clone()));
    assert_eq!(
        other.adoption(),
        None,
        "spot edits never reach another venue"
    );
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    let restarted = open(&path, &config);
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
    let restarted = open(&path, &config);
    assert_eq!(
        bind(&restarted, &config, "hyperliquid", "BTCUSDT").1,
        other_default,
        "a reset is not undone by the move"
    );
    assert_eq!(
        bind(&restarted, &config, "binance", "BTCUSDT").1,
        spot.declared
    );
}

#[test]
fn saving_off_moves_with_the_key_and_stays_local_to_the_venue() {
    let dir = ScratchDir::new("asset-mode-saving-off");
    let path = dir.join(ASSETS_FILE);
    let config = venue_config();
    let mut file = AssetBubblesFile::default();
    file.save_changes_off.insert("BTCUSDT".to_owned());
    save_to(&path, &file).unwrap();
    let store = open(&path, &config);
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
}

#[test]
fn a_moved_key_keeps_a_look_already_stored_under_it() {
    let dir = ScratchDir::new("asset-mode-already-scoped");
    let path = dir.join(ASSETS_FILE);
    let config = venue_config();
    let key = config.bubble_asset("hyperliquid", "BTCUSDT").key;
    let mut file = AssetBubblesFile::default();
    let legacy = AssetBubbles::declared(bubble_presets::embedded().get("default").unwrap(), false);
    let scoped =
        AssetBubbles::declared(bubble_presets::embedded().get("dense tape").unwrap(), false);
    file.assets.insert("BTCUSDT".to_owned(), legacy);
    file.assets.insert(key, scoped.clone());
    file.save_changes_off.insert("BTCUSDT".to_owned());
    save_to(&path, &file).unwrap();
    let store = open(&path, &config);
    let (other, restored) = bind(&store, &config, "hyperliquid", "BTCUSDT");
    assert_eq!(restored, scoped);
    assert!(other.saves_changes());
}

#[test]
fn tick_calibration_survives_restart_without_replacing_a_saved_tape() {
    let dir = ScratchDir::new("asset-mode-tick-settings");
    let path = dir.join(ASSETS_FILE);
    let config = venue_config();
    let store = open(&path, &config);
    let (mut spot, mut saved) = bind(&store, &config, "binance", "BTCUSDT");
    saved.look.bubbles.max_radius = 48.0;
    saved.look.volume_dot_full_quantity = 120.0;
    saved.look.tick_bubbles = None;
    spot.note_edit();
    assert!(spot.file(saved.clone()));
    let (_, reopened) = bind(&store, &config, "binance", "BTCUSDT");
    assert_eq!(
        reopened, saved,
        "new presets preserve an existing saved look"
    );
    let mut spheres = saved.clone();
    spheres.look.tick_bubbles = bubble_presets::embedded()
        .get(SPOT_SPHERES)
        .unwrap()
        .tick_bubbles;
    spot.note_edit();
    assert!(spot.file(spheres.clone()));
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    let restart = open(&path, &config);
    assert_eq!(bind(&restart, &config, "binance", "BTCUSDT").1, spheres);
    assert_ne!(
        bind(&restart, &config, "binance-futures", "BTCUSDT.p")
            .1
            .look
            .tick_bubbles,
        spheres.look.tick_bubbles
    );
    assert_ne!(
        bind(&restart, &config, "hyperliquid", "BTCUSDT")
            .1
            .look
            .bubbles
            .max_radius,
        48.0
    );
}

/// Review #14: the move runs when the store is read, before any view binds,
/// and asks for a write only when a copy lands. The written file carries the
/// record of the move; a file with nothing to move is left exactly as it was.
#[test]
fn the_move_runs_at_load_and_writes_only_when_a_copy_lands() {
    let dir = ScratchDir::new("asset-mode-move-at-load");
    let path = dir.join(ASSETS_FILE);
    let mut legacy =
        AssetBubbles::declared(bubble_presets::embedded().get("default").unwrap(), false);
    legacy.look.bubbles.max_radius = 23.0;
    let mut file = AssetBubblesFile::default();
    file.assets.insert("BTCUSDT".to_owned(), legacy.clone());
    save_to(&path, &file).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();

    // The shipped config moves no key that holds a look: nothing to write.
    let shipped_store = open(&path, &shipped());
    assert_eq!(shipped_store.borrow_mut().flush(), None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

    // Binding alone moves nothing any more.
    let config = venue_config();
    let unmigrated = AssetBubblesStore::load(path.clone()).shared();
    let _ = bind(&unmigrated, &config, "hyperliquid", "BTCUSDT");
    assert_eq!(unmigrated.borrow().get("hyperliquid:BTCUSDT"), None);
    assert_eq!(unmigrated.borrow_mut().flush(), None);

    // Read with a moved key: copied before any view, written once, marked.
    let store = open(&path, &config);
    assert_eq!(store.borrow().get("hyperliquid:BTCUSDT"), Some(&legacy));
    assert_eq!(
        store.borrow().get("BTCUSDT"),
        Some(&legacy),
        "the old key stays"
    );
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    let written = std::fs::read_to_string(&path).unwrap();
    let settled = crate::bubble_assets::parse(&written).unwrap().moved_keys;
    assert!(settled.contains("hyperliquid:BTCUSDT"), "{written}");
    assert!(
        written.contains("version = 1"),
        "main's format version: {written}"
    );
    let again = open(&path, &config);
    assert_eq!(again.borrow_mut().flush(), None, "migrated once");
}

/// Review round 1 #2: each move is settled on its own, so a mode the config
/// switches on after the first migration still carries its asset's look over
/// and logs it, while the move already settled is not run again.
#[test]
fn a_mode_added_after_the_first_migration_still_moves_its_look() {
    let dir = ScratchDir::new("asset-mode-added-later");
    let path = dir.join(ASSETS_FILE);
    let mut btc = AssetBubbles::declared(bubble_presets::embedded().get("default").unwrap(), false);
    btc.look.bubbles.max_radius = 23.0;
    let mut eth = btc.clone();
    eth.look.bubbles.max_radius = 27.0;
    let mut file = AssetBubblesFile::default();
    file.assets.insert("BTCUSDT".to_owned(), btc.clone());
    file.assets.insert("ETHUSDT".to_owned(), eth.clone());
    save_to(&path, &file).unwrap();
    let first = open(&path, &venue_config());
    assert_eq!(first.borrow().get("hyperliquid:BTCUSDT"), Some(&btc));
    assert_eq!(first.borrow_mut().flush(), Some(Ok(())));

    // Feeds.toml later switches a mode on for ETHUSDT on the same venue.
    let mut later = venue_config();
    let other = feed(&mut later, "hyperliquid");
    other.symbols.push("ETHUSDT".to_owned());
    other
        .symbol_bubble_presets
        .insert("ETHUSDT".to_owned(), "dense tape".to_owned());
    other
        .symbol_asset_modes
        .insert("ETHUSDT".to_owned(), mode("ETH"));
    let log = Capture::default();
    let writer = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .finish();
    let store = tracing::subscriber::with_default(subscriber, || {
        // Another test's thread may have cached the line as unwanted before
        // this subscriber existed.
        tracing::callsite::rebuild_interest_cache();
        open(&path, &later)
    });
    let logged = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();

    assert_eq!(store.borrow().get("hyperliquid:ETHUSDT"), Some(&eth));
    assert_eq!(store.borrow().get("hyperliquid:BTCUSDT"), Some(&btc));
    assert!(
        logged.contains("ASSET_BUBBLES_KEY_MOVED")
            && logged.contains("asset=\"hyperliquid:ETHUSDT\"")
            && logged.contains("from=\"ETHUSDT\""),
        "{logged}"
    );
    assert!(
        !logged.contains("asset=\"hyperliquid:BTCUSDT\""),
        "a settled move does not run again: {logged}"
    );
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
}

/// Review #14, the trader alternates this build and main (ef882174) on one
/// store. The fixture is main's own rewrite of a file this build migrated:
/// main's loader read that file whole (all three looks, the save-off
/// switch), the trader's WIN edit there set 20, and main wrote it back. This
/// build reads main's file with every look, and the re-check the missing
/// record causes copies nothing over a look main kept.
#[test]
fn a_file_main_rewrote_reads_back_whole() {
    let dir = ScratchDir::new("asset-mode-main-rewrite");
    let path = dir.join(ASSETS_FILE);
    std::fs::write(&path, MAIN_REWRITE).unwrap();
    let file = crate::bubble_assets::parse(MAIN_REWRITE).expect("main's file reads here");
    assert_eq!(
        file.assets.keys().collect::<Vec<_>>(),
        ["BTCUSDT", "WIN*", "hyperliquid:BTCUSDT"]
    );
    assert_eq!((file.version, file.moved_keys.len()), (1, 0));
    assert!(file.save_changes_off.contains("WDO$N"));
    assert_eq!(file.get("WIN*").unwrap().look.bubbles.max_radius, 20.0);
    // Main keeps every look but drops the look fields it does not know,
    // so the BTC looks come back without their spheres (reported in A4).
    assert_eq!(file.get("BTCUSDT").unwrap().look.tick_bubbles, None);
    let store = open(&path, &venue_config());
    for key in ["BTCUSDT", "WIN*", "hyperliquid:BTCUSDT"] {
        assert_eq!(store.borrow().get(key), file.get(key), "{key}");
    }
    assert_eq!(
        store.borrow_mut().flush(),
        None,
        "nothing moved over a kept look"
    );
}

/// Every log line written while it is the subscriber's writer.
#[derive(Clone, Default)]
struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A store as main (ef882174) writes it: format version 1, its own fields and
/// none of this build's. Written by main's `save_to` in a scratch worktree.
const MAIN_REWRITE: &str = include_str!("../tests/fixtures/bubble-assets-main-rewrite.toml");
