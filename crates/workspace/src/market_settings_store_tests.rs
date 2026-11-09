//! The held store: autosave off keeps drawings and anchored studies in memory
//! until Save.
use super::{AssetResolver, MarketSettingsStore};
use crate::layout_commit::LayoutSave;
use crate::layout_document::{SavedBand, SavedDrawing, SavedPoint};
use crate::market_settings::{DrawingKey, MarketKey, MarketSettings, PaneRole};
use std::path::PathBuf;
use std::rc::Rc;
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
        bars: None,
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
    let written = store.commit_document(std::slice::from_ref(&flow)).unwrap();
    assert_eq!(
        store.held_changes([&flow]),
        2,
        "nothing moves before the disk"
    );
    store.note_saved(written);
    assert_eq!(store.held_changes([&flow]), 0, "saved is the new baseline");
    assert!(!store.is_dirty());
}

/// One tab's Save writes its own sets only: another tab's held set stays
/// held, counted and owed, so its Close without saving can still drop it.
#[test]
fn a_save_commits_only_the_keys_it_names() {
    let (flow, time) = (key("mt5", PaneRole::Flow), key("mt5", context("5m")));
    let mut store = store();
    store.set_held(true);
    draw(&mut store, &flow);
    draw(&mut store, &time);
    let written = store.commit_document(std::slice::from_ref(&flow)).unwrap();
    assert_eq!(written.drawings(&flow).len(), 2);
    assert!(
        written.drawings(&time).is_empty(),
        "the other set as the file has it"
    );
    store.note_saved(written);
    assert_eq!(store.held_changes([&flow]), 0);
    assert_eq!(store.held_changes([&time]), 2, "still held");
    assert!(store.is_dirty(), "still owed");
    assert!(store.discard(std::slice::from_ref(&time), &[]));
    assert!(store.settings().drawings(&time).is_empty());
    assert!(store.commit_document(std::slice::from_ref(&flow)).is_none());
}

/// A write that did not reach the disk moves no baseline: the change stays
/// counted and owed, and the debounce asks for it again.
#[test]
fn a_failed_write_keeps_the_change_counted_and_owed() {
    let flow = key("mt5", PaneRole::Flow);
    let mut store = store();
    draw(&mut store, &flow);
    let later = Instant::now() + Duration::from_secs(5);
    assert_eq!(store.take_save(later), LayoutSave::Write);
    store.note_write_failed(later);
    store.set_held(true);
    assert_eq!(store.held_changes([&flow]), 2, "the file never got it");
    store.set_held(false);
    assert!(store.is_dirty());
    assert_eq!(
        store.take_save(later),
        LayoutSave::Wait,
        "a quiet second first"
    );
    assert_eq!(
        store.take_save(later + Duration::from_secs(1)),
        LayoutSave::Write
    );
}

/// A duplicated mark is one added drawing, and a long equal set compares
/// only what changed.
#[test]
fn drawing_changes_count_a_duplicate_and_ignore_equal_ends() {
    let flow = key("mt5", PaneRole::Flow);
    let mut saved = MarketSettings::default();
    let many: Vec<SavedDrawing> = (0..2_000_u32)
        .map(|n| level(Some(u64::from(n)), f64::from(n)))
        .collect();
    saved.replace(&flow, many.clone(), known);
    let mut now = saved.clone();
    let mut doubled = many.clone();
    doubled.insert(1_000, level(Some(1_000), 1_000.0));
    now.replace(&flow, doubled, known);
    assert_eq!(
        now.drawing_changes(&saved, &flow),
        1,
        "the duplicate counts"
    );
    let mut moved = many;
    moved[5].points[0].price = -1.0;
    now.replace(&flow, moved, known);
    assert_eq!(now.drawing_changes(&saved, &flow), 1);
    assert_eq!(saved.drawing_changes(&saved, &flow), 0);
}

/// A re-cut (flow bar size, timeframe) or a relaunch re-derives a dated
/// anchor's bar offset from its market time, and a pane may hand a mark a
/// fresh id: neither is the trader's edit, so a held store counts neither.
/// An undated anchor has only its offset, so moving it still counts.
#[test]
fn a_re_derived_bar_offset_or_id_is_no_drawing_change() {
    let flow = key("mt5", PaneRole::Flow);
    let mut saved = MarketSettings::default();
    saved.replace(&flow, vec![level(None, 100.0), level(Some(2), 90.0)], known);
    let mut recut = vec![level(Some(7), 100.0), level(Some(2), 90.0)];
    for mark in &mut recut {
        mark.points[0].bar = 133.0;
    }
    let mut now = saved.clone();
    now.replace(&flow, recut.clone(), known);
    assert_eq!(now.drawing_changes(&saved, &flow), 0, "a re-cut is no edit");
    recut.push(level(Some(9), 80.0));
    now.replace(&flow, recut.clone(), known);
    assert_eq!(now.drawing_changes(&saved, &flow), 1, "only the new mark");
    let undated = |bar: f64| {
        let mut mark = level(Some(4), 70.0);
        mark.points[0] = SavedPoint {
            time_ms: None,
            price: 70.0,
            bar,
        };
        mark
    };
    saved.replace(&flow, vec![undated(210.0)], known);
    now.replace(&flow, vec![undated(214.0)], known);
    assert_eq!(
        now.drawing_changes(&saved, &flow),
        1,
        "its offset is its anchor"
    );
}

/// The revision moves with every change a held count reads, and only then.
#[test]
fn the_revision_moves_with_what_a_held_count_reads() {
    let flow = key("mt5", PaneRole::Flow);
    let mut store = store();
    let start = store.revision();
    store.set_held(false);
    assert_eq!(store.revision(), start, "no hold change");
    store.set_held(true);
    let held = store.revision();
    assert_ne!(held, start);
    draw(&mut store, &flow);
    assert_ne!(store.revision(), held);
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

/// The switch is kept per asset: two symbols of one asset share it, the
/// same asset on another venue does not, and only a change owes a save.
#[test]
fn bar_progress_is_kept_per_market_and_only_a_change_is_saved() {
    let assets: AssetResolver = Rc::new(|_, symbol: &str| symbol[..3].to_owned());
    let mut store =
        MarketSettingsStore::new(MarketSettings::default(), PathBuf::from("m.toml"), false)
            .with_assets(assets);
    let now = Instant::now();
    assert!(store.bar_progress_on("mt5", "WINZ26"), "on by default");
    assert!(store.set_bar_progress("mt5", "WINZ26", false, now));
    assert!(store.is_dirty(), "a change owes the file a save");
    assert!(!store.bar_progress_on("mt5", "WING27"), "the same asset");
    assert!(store.bar_progress_on("replay", "WINZ26"), "another venue");
    assert!(store.bar_progress_on("mt5", "WDOZ26"), "another asset");

    let _ = store.take_flush();
    assert!(!store.set_bar_progress("mt5", "WING27", false, now));
    assert!(!store.is_dirty(), "a repeat is no change and no save");
}
