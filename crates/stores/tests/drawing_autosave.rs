//! Disk-level independence of drawing persistence and workspace configuration.
use quantick_stores::{layouts, market_settings};
use quantick_workspace::indicator_document::{SavedIndicator, SavedKind};
use quantick_workspace::layout_commit::LAYOUTS_SAVE_DEBOUNCE;
use quantick_workspace::layout_document::{LayoutBook, LayoutId, SavedDrawing};
use quantick_workspace::layout_store::LayoutStore;
use quantick_workspace::market_settings::{DrawingKey, DrawingScope, MarketSettings};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[path = "../src/scratch.rs"]
mod scratch;

fn key(workspace: u64, layout: LayoutId) -> DrawingKey {
    DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), Some("time:5m".to_owned()))
        .in_scope(DrawingScope { workspace, layout })
}

fn level(price: f64) -> SavedDrawing {
    toml::from_str(&format!(
        r#"
id = 1
name = "line-1"
tool = "horizontal-line"
points = [{{time_ms = 1000, price = {price}, bar = 0.0}}]
band = {{axis = "price"}}
color = [255, 200, 0, 255]
width_px = 1.5
fill_alpha = 0
bars = "time:5m"
"#
    ))
    .unwrap()
}

fn fixture(label: &str) -> (PathBuf, LayoutStore) {
    let dir = scratch::thread_dir(label);
    let book = LayoutBook::default();
    let path = dir.join("layouts.toml");
    assert!(layouts::save(&path, &book));
    (dir, LayoutStore::new(book, path, false))
}

fn read_layouts(dir: &Path) -> LayoutBook {
    match layouts::load(&dir.join("layouts.toml")) {
        layouts::Loaded::Book(book) => book,
        other => panic!("expected saved layouts: {other:?}"),
    }
}

fn read_drawings(dir: &Path) -> MarketSettings {
    market_settings::open(
        &dir.join("market-settings.toml"),
        &dir.join("layouts.toml"),
        &mut read_layouts(dir),
        |_, symbol| symbol.to_owned(),
    )
    .settings
}

fn replace(store: &mut LayoutStore, key: &DrawingKey, items: Vec<SavedDrawing>, now: Instant) {
    store
        .market_mut()
        .settings_mut()
        .replace(key, items, |_| true);
    store.market_mut().mark_changed(now);
}

#[test]
fn drawing_writes_and_explicit_layout_save_keep_separate_disk_snapshots() {
    let (dir, mut store) = fixture("separate-autosave");
    let now = Instant::now();
    let layout = store.book().active_id();
    let original_name = store.book().active().name.clone();
    let own = key(1, layout);
    let other_workspace = key(2, layout);
    replace(&mut store, &own, vec![level(100.0)], now);
    store.session_mut().rename(layout, "Pending name").unwrap();
    store.mark_changed(now);
    market_settings::flush(&mut store, now);
    assert_eq!(read_layouts(&dir).active().name, original_name);
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(100.0)]);

    assert!(store.market_mut().set_drawing_autosave(&own, false, now));
    replace(&mut store, &own, vec![level(200.0)], now);
    replace(&mut store, &other_workspace, vec![level(300.0)], now);
    market_settings::flush(&mut store, now);
    let saved = read_drawings(&dir);
    assert!(!saved.drawing_autosave(&own));
    assert_eq!(saved.drawings(&own), &[level(100.0)]);
    assert_eq!(saved.drawings(&other_workspace), &[level(300.0)]);

    market_settings::commit_layouts(&mut store).unwrap();
    assert_eq!(read_layouts(&dir).active().name, "Pending name");
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(100.0)]);

    store.market_mut().set_drawing_autosave(&own, true, now);
    market_settings::flush(&mut store, now);
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(200.0)]);
    replace(&mut store, &own, Vec::new(), now);
    market_settings::flush(&mut store, now);
    let saved = read_drawings(&dir);
    assert!(saved.drawings(&own).is_empty());
    assert_eq!(saved.drawings(&other_workspace), &[level(300.0)]);
}

#[test]
fn a_failed_drawing_write_keeps_disk_state_and_retries_the_pending_edit() {
    let (dir, mut store) = fixture("drawing-write-retry");
    let now = Instant::now();
    let own = key(1, store.book().active_id());
    replace(&mut store, &own, vec![level(100.0)], now);
    market_settings::flush(&mut store, now);
    let obstacle = dir.join("market-settings.toml.tmp");
    std::fs::create_dir(&obstacle).unwrap();
    replace(&mut store, &own, vec![level(200.0)], now);
    market_settings::flush(&mut store, now);
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(100.0)]);
    assert!(store.market().owes_write());
    assert!(store.market().is_dirty());

    std::fs::remove_dir(obstacle).unwrap();
    market_settings::save_settled(&mut store, now + LAYOUTS_SAVE_DEBOUNCE);
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(200.0)]);
    assert!(!store.market().owes_write());
}

#[test]
fn a_new_layout_keeps_its_drawing_identity_without_saving_its_configuration() {
    let (dir, mut store) = fixture("unsaved-layout-identity");
    let now = Instant::now();
    let previous_layout = store.book().active_id();
    let new_layout = store.session_mut().create(Some("Research")).unwrap();
    store
        .session_mut()
        .indicators_mut(new_layout)
        .unwrap()
        .push(SavedIndicator {
            kind: SavedKind::native("native.ema"),
            hidden: false,
            mouse_vertical_line: false,
            inputs: Vec::new(),
            plot_styles: Vec::new(),
        });
    store.mark_changed(now);
    let own = key(1, new_layout);
    replace(&mut store, &own, vec![level(100.0)], now);
    market_settings::flush(&mut store, now);

    let mut reopened = read_layouts(&dir);
    assert_eq!(reopened.active_id(), previous_layout);
    assert_eq!(reopened.layouts().len(), 1);
    assert!(reopened.get(new_layout).is_none());
    assert!(reopened.by_name("Research").is_none());
    assert!(reopened.create(Some("Research")).unwrap().0 > new_layout.0);
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(100.0)]);
    market_settings::commit_layouts(&mut store).unwrap();
    let committed = read_layouts(&dir);
    let layout = committed
        .get(new_layout)
        .expect("explicit Save keeps the layout");
    assert_eq!(layout.name, "Research");
    assert_eq!(layout.indicators.len(), 1);
    assert_eq!(committed.layouts().len(), 2);
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(100.0)]);
}

#[test]
fn drawings_wait_for_a_new_layout_identity_to_reach_disk() {
    let (dir, mut store) = fixture("drawing-identity-write-retry");
    let now = Instant::now();
    let new_layout = store.session_mut().create(Some("Research")).unwrap();
    store.mark_changed(now);
    let own = key(1, new_layout);
    replace(&mut store, &own, vec![level(100.0)], now);
    let blocked_temp = dir.join("layouts.toml.tmp");
    std::fs::create_dir(&blocked_temp).unwrap();

    market_settings::flush(&mut store, now);
    assert!(read_layouts(&dir).get(new_layout).is_none());
    assert!(read_drawings(&dir).drawings(&own).is_empty());
    assert!(store.is_dirty());
    assert!(store.market().owes_write());

    std::fs::remove_dir(&blocked_temp).unwrap();
    market_settings::save_settled(&mut store, now + LAYOUTS_SAVE_DEBOUNCE);
    let mut reopened = read_layouts(&dir);
    assert!(reopened.get(new_layout).is_none());
    assert!(reopened.create(Some("Research")).unwrap().0 > new_layout.0);
    assert_eq!(read_drawings(&dir).drawings(&own), &[level(100.0)]);
    assert!(!store.market().owes_write());
}
