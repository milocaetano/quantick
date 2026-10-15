//! The log line a config asset mode's key move writes, read back alone.
//!
//! `tracing` decides whether a line is wanted once, when a thread first reaches
//! it, and while a single subscriber is installed it asks only the subscriber
//! of that thread. A test thread with none that is first to reach the line
//! while this test's subscriber is installed therefore caches it as unwanted
//! for every thread, and the capture comes back empty. `src/asset_mode_tests.rs`
//! runs many threads that move keys, so the line is pinned here instead: this
//! file holds this one test, and its process reaches the line from one thread
//! only. A second test that moves a key belongs in the unit tests, never here.

use std::sync::{Arc, Mutex};

use quantick_stores::bubble_asset_store::AssetBubblesStore;
use quantick_stores::bubble_assets::AssetBubbles;
use quantick_stores::bubble_presets;
use quantick_stores::config::{AppConfig, AssetMode, EMBEDDED_DEFAULT};

/// The shipped config with Hyperliquid also offering `symbol` on a look of its
/// own and a mode switched on for it: a market whose key the mode moves off
/// main's.
fn with_mode(mut config: AppConfig, symbol: &str, unit: &str) -> AppConfig {
    let venue = config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "hyperliquid")
        .unwrap();
    venue.symbols.push(symbol.to_owned());
    venue
        .symbol_bubble_presets
        .insert(symbol.to_owned(), "dense tape".to_owned());
    venue.symbol_asset_modes.insert(
        symbol.to_owned(),
        AssetMode {
            quantity_unit: unit.to_owned(),
            look_key: None,
        },
    );
    config
}

/// Every log line written while it is the subscriber's writer.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Review round 1 #2: a mode the config switches on after the first migration
/// carries its asset's look over and logs it, while the move already settled
/// logs nothing again. Whether the look moves, and survives a write and a
/// reopen, is the unit test `a_mode_added_after_the_first_migration_still_moves_its_look`.
#[test]
fn a_mode_added_after_the_first_migration_logs_its_move_and_a_settled_one_stays_silent() {
    let declared =
        AssetBubbles::declared(bubble_presets::embedded().get("default").unwrap(), false);
    let mut btc = declared.clone();
    btc.look.bubbles.max_radius = 23.0;
    let mut eth = declared.clone();
    eth.look.bubbles.max_radius = 27.0;
    let mut store = AssetBubblesStore::default();
    assert!(store.record("BTCUSDT", &btc, &declared));
    assert!(store.record("ETHUSDT", &eth, &declared));

    let shipped: AppConfig = toml::from_str(EMBEDDED_DEFAULT).unwrap();
    let first = with_mode(shipped, "BTCUSDT", "BTC");
    store.migrate_identities(&first);
    assert_eq!(store.get("hyperliquid:BTCUSDT"), Some(&btc));

    // Feeds.toml later switches a mode on for ETHUSDT on the same venue.
    let later = with_mode(first, "ETHUSDT", "ETH");
    let log = Capture::default();
    let writer = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || store.migrate_identities(&later));
    let logged = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();

    assert_eq!(store.get("hyperliquid:ETHUSDT"), Some(&eth));
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
}
