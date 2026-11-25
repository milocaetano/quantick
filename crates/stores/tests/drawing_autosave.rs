//! Disk-level independence of drawing persistence and workspace configuration.
use quantick_stores::{layouts, market_settings, ui_state, workspaces::WorkspaceBookStore};
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

fn select(store: &mut LayoutStore, id: LayoutId) {
    let session = store.session_mut();
    session.register(1, Some(session.book().active_id()));
    let selection = session.select(1, id, Default::default()).unwrap();
    let committed = session.commit_selection(selection).unwrap();
    session.finish_selection(committed).unwrap();
}

#[test]
fn cockpit_save_commits_manual_layout_choices_without_committing_drawings() {
    let (dir, mut store) = fixture("cockpit-save-manual");
    let now = Instant::now();
    let original = store.book().active_id();
    let removed = store.session_mut().create(Some("Remove")).unwrap();
    market_settings::commit_layouts(&mut store).unwrap();
    let own = key(1, original);
    replace(&mut store, &own, vec![level(100.0)], now);
    market_settings::drawing_autosave(&mut store, &own, Some(true), now).unwrap();
    market_settings::drawing_autosave(&mut store, &own, Some(false), now).unwrap();
    let drawing_file = std::fs::read(store.market().path()).unwrap();
    replace(&mut store, &own, vec![level(200.0)], now);
    let research = store.session_mut().create(Some("Research")).unwrap();
    store.session_mut().rename(original, "Renamed").unwrap();
    store.session_mut().finish_delete(removed).unwrap();
    select(&mut store, research);
    let tab = toml::from_str(&format!(
        "feed='binance'\nsymbol='BTCUSDT'\nlayout='flow'\nflow_bars='tick:500'\nflow_layout={}\n",
        research.0
    ))
    .unwrap();
    let document = ui_state::Workspace::new(false, None, 0, vec![tab], None);
    assert!(!document.auto_save && !document.save_on_exit);
    let desk = dir.join("ui-state.toml");

    assert!(ui_state::save_cockpit(&desk, &document, &mut store, "test"));
    let reopened = read_layouts(&dir);
    assert_eq!(reopened, *store.book());
    assert_eq!(reopened.active_id(), research);
    assert_eq!(reopened.get(original).unwrap().name, "Renamed");
    assert!(reopened.get(removed).is_none());
    assert_eq!(ui_state::load(&desk).tabs[0].flow_layout, Some(research.0));
    assert_eq!(std::fs::read(store.market().path()).unwrap(), drawing_file);
    assert!(store.market().is_dirty());
}

#[test]
fn cockpit_save_reports_layout_and_desk_failures_and_retries() {
    let (dir, mut store) = fixture("cockpit-save-retry");
    let desk = dir.join("ui-state.toml");
    let before = ui_state::Workspace::default();
    assert!(ui_state::save(&desk, &before));
    let original = store.book().active_id();
    let original_name = store.book().active().name.clone();
    store.session_mut().rename(original, "Pending").unwrap();
    let mut document = before.clone();
    document.window = Some([900.0, 600.0]);
    let layout_obstacle = store.path().with_extension("toml.tmp");
    std::fs::create_dir(&layout_obstacle).unwrap();
    assert!(!ui_state::save_cockpit(
        &desk, &document, &mut store, "test"
    ));
    assert_eq!(ui_state::load(&desk), before);
    assert_eq!(read_layouts(&dir).active().name, original_name);
    assert!(store.configuration_changed());
    std::fs::remove_dir(layout_obstacle).unwrap();
    assert!(ui_state::save_cockpit(&desk, &document, &mut store, "test"));
    assert_eq!(ui_state::load(&desk), document);
    assert_eq!(read_layouts(&dir).active().name, "Pending");

    let saved = document.clone();
    document.window = Some([1000.0, 700.0]);
    store.session_mut().rename(original, "Next").unwrap();
    let desk_obstacle = desk.with_extension("toml.tmp");
    std::fs::create_dir(&desk_obstacle).unwrap();
    assert!(!ui_state::save_cockpit(
        &desk, &document, &mut store, "test"
    ));
    assert_eq!(ui_state::load(&desk), saved);
    std::fs::remove_dir(desk_obstacle).unwrap();
    assert!(ui_state::save_cockpit(&desk, &document, &mut store, "test"));
    assert_eq!(ui_state::load(&desk), document);
    assert_eq!(read_layouts(&dir).active().name, "Next");
}

#[test]
fn immediate_drawing_writes_use_current_configuration_policy_without_a_frame() {
    use quantick_workspace::drawing_identity;
    for entry in ["reserve", "preference", "identity"] {
        for enabled in [false, true] {
            let (dir, mut store) = fixture(&format!("current-policy-{entry}-{enabled}"));
            let now = Instant::now();
            let mut book = WorkspaceBookStore::open_beside(store.path());
            book.set_autosave(!enabled);
            book.persist_drawings(&mut store, now, true);
            book.set_autosave(enabled);
            let before = read_layouts(&dir);
            store
                .session_mut()
                .rename(before.active_id(), "Pending rename")
                .unwrap();
            let research = store.session_mut().create(Some("Research")).unwrap();
            select(&mut store, research);
            store.mark_changed(now);
            let own = key(book.book().workspace_id("Default").unwrap(), research);
            let identity = drawing_identity::issue("document", &own, 1, "entropy").unwrap();
            let mut mark = level(100.0);
            mark.durable_identity = Some(identity.clone());
            replace(&mut store, &own, vec![mark], now);
            match entry {
                "reserve" => book.reserve_drawing_identities(&mut store, now).unwrap(),
                "preference" => {
                    book.drawing_autosave(&mut store, &own, Some(true), now)
                        .unwrap();
                }
                "identity" => book
                    .commit_drawing_identity(&mut store, &own, &identity, now)
                    .unwrap(),
                _ => unreachable!(),
            }
            let mut reopened = read_layouts(&dir);
            let expected = if enabled { store.book() } else { &before };
            assert_eq!(reopened.layouts(), expected.layouts(), "{entry}, {enabled}");
            assert_eq!(reopened.active_id(), expected.active_id());
            assert!(reopened.create(None).unwrap().0 > research.0);
            if entry != "reserve" {
                assert_eq!(
                    read_drawings(&dir).drawings(&own)[0].durable_identity,
                    Some(identity)
                );
            }
        }
    }
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
