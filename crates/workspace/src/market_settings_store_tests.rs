//! The held store: autosave off keeps drawings and anchored studies in memory
//! until Save.
use super::MarketSettingsStore;
use crate::layout_commit::LayoutSave;
use crate::layout_document::{SavedBand, SavedDrawing, SavedPoint};
use crate::market_settings::{DrawingKey, MarketKey, MarketSettings, PaneRole};
use std::time::{Duration, Instant};

fn key(venue: &str, role: PaneRole) -> DrawingKey {
    DrawingKey {
        market: MarketKey {
            venue: venue.to_owned(),
            asset: "WIN*".to_owned(),
        },
        role,
    }
}

fn context(bars: &str) -> PaneRole {
    PaneRole::Context {
        bars: bars.to_owned(),
    }
}

fn known(_tool: &str) -> bool {
    true
}

fn level(id: Option<u64>, price: f64) -> SavedDrawing {
    SavedDrawing {
        id,
        tool: "horizontal-line".to_owned(),
        name: None,
        author: None,
        points: vec![SavedPoint {
            time_ms: Some(1_000),
            price,
            bar: 3.0,
        }],
        band: SavedBand::Price,
        color: [255, 200, 0, 255],
        width_px: 1.5,
        fill_alpha: 0,
        locked: false,
        hidden: false,
        magnet: false,
        shared: false,
        payload: None,
        text: None,
        origin: None,
    }
}

/// A store over an empty document, writes allowed.
fn store() -> MarketSettingsStore {
    MarketSettingsStore::new(
        MarketSettings::default(),
        std::path::PathBuf::from("market-settings.toml"),
        false,
    )
}

fn draw(store: &mut MarketSettingsStore, key: &DrawingKey) {
    let now = Instant::now();
    store.settings_mut().replace(
        key,
        vec![level(Some(1), 100.0), level(Some(2), 90.0)],
        known,
    );
    store.mark_changed(now);
}

/// Autosave off: a drawing waits for Save, is counted for its tab, and Save
/// writes it whatever the hold says.
#[test]
fn a_held_store_writes_only_on_save() {
    let flow = key("mt5", PaneRole::Flow);
    let mut store = store();
    store.set_held(true);
    draw(&mut store, &flow);
    let later = Instant::now() + Duration::from_secs(5);
    assert_eq!(store.take_save(later), LayoutSave::Wait);
    assert_eq!(store.take_flush(), LayoutSave::Wait);
    assert_eq!(store.held_changes([&flow]), 2);
    assert_eq!(store.held_changes([&key("mt5", context("1m"))]), 0);
    assert_eq!(store.take_commit(), LayoutSave::Write);
    assert_eq!(store.held_changes([&flow]), 0, "saved is the new baseline");
}

/// Autosave on: nothing is held, the debounce writes as it always did.
#[test]
fn an_unheld_store_writes_after_the_debounce() {
    let flow = key("mt5", PaneRole::Flow);
    let mut store = store();
    draw(&mut store, &flow);
    assert_eq!(store.held_changes([&flow]), 0);
    let later = Instant::now() + Duration::from_secs(5);
    assert_eq!(store.take_save(later), LayoutSave::Write);
}

/// Close without saving drops the closed tab's held drawings, unless another
/// open pane still shows that very set.
#[test]
fn close_without_saving_drops_only_what_no_open_pane_shows() {
    let (flow, time) = (key("mt5", PaneRole::Flow), key("mt5", context("5m")));
    let mut store = store();
    store.set_held(true);
    draw(&mut store, &flow);
    draw(&mut store, &time);
    assert!(!store.discard(std::slice::from_ref(&time), std::slice::from_ref(&time)));
    assert_eq!(store.held_changes([&time]), 2, "an open twin keeps it");
    assert!(store.discard(&[flow.clone(), time.clone()], &[]));
    assert!(store.settings().drawings(&flow).is_empty());
    assert!(!store.is_dirty(), "nothing is owed once nothing differs");
    store.set_held(false);
    assert_eq!(store.take_flush(), LayoutSave::Wait);
}
