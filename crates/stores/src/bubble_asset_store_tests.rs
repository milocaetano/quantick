//! The store on disk: written once per change, read back by a restart and
//! by a workspace import, and never claimed saved when it is not.
use super::*;
use crate::bubble_assets::{ASSETS_FILE, AssetBubblesFile, save_to};
use crate::bubble_presets;
use crate::scratch::ScratchDir;
use quantick_orderflow::LaneWindow;

const NATIVE_PRESET: &str = "mini index regions";

pub(super) fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../app/config/feeds.toml")).expect("shipped feeds parse")
}

/// Bind a view to `symbol`'s asset — the mini index on its B3 feed, any
/// other symbol on Binance — over the shipped presets.
pub(super) fn bind(store: &SharedAssetBubbles, symbol: &str) -> (AssetBinding, AssetBubbles) {
    let presets = bubble_presets::embedded();
    bind_with(store, symbol, &presets)
}

fn bind_with(
    store: &SharedAssetBubbles,
    symbol: &str,
    presets: &BubblePresetFile,
) -> (AssetBinding, AssetBubbles) {
    let feed = if symbol.starts_with("WIN") {
        "metatrader-b3"
    } else {
        "binance"
    };
    AssetBinding::bind(
        store.clone(),
        &shipped_config(),
        (feed, symbol),
        presets,
        false,
    )
}

#[test]
fn a_win_edit_is_filed_for_win_alone_and_survives_a_restart() {
    let dir = ScratchDir::new("asset-restart");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    assert_eq!(win.source(), AssetSource::Declared);
    assert_eq!(opened.look.name, NATIVE_PRESET);

    let mut edited = opened.clone();
    edited.candle_aggression = true;
    edited.flow_ignore_opening = true;
    edited.look.volume_dot_ignore_opening_burst_in_scale = true;
    edited.look.live_lane.tape_only = true;
    win.note_edit();
    assert!(win.file(edited.clone()), "an edit is filed");
    assert!(!win.file(edited.clone()), "an unchanged look files nothing");
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    assert_eq!(store.borrow_mut().flush(), None, "one write per change");
    assert_eq!(win.unsaved(), None);

    // A restart reads the file back.
    let reread = AssetBubblesStore::load(path).shared();
    let (roll, restored) = bind(&reread, "WINZ26");
    assert_eq!(roll.source(), AssetSource::Stored);
    assert_eq!(
        restored, edited,
        "the next WIN contract opens as WIN was left"
    );

    // BTC never saw any of it.
    let (btc, settings) = bind(&reread, "BTCUSDT");
    assert_eq!(btc.source(), AssetSource::Default);
    assert_eq!(settings.look.name, bubble_presets::embedded().active);
    assert!(!settings.candle_aggression && !settings.flow_ignore_opening);
    assert!(!settings.look.live_lane.tape_only && !settings.look.live_lane.native_tape);
}

/// Review round 1, finding 7: when the file cannot be read or written the
/// settings are kept in memory and reported unsaved, never claimed saved.
#[test]
fn an_unreadable_or_unwritable_store_reports_unsaved_settings() {
    let dir = ScratchDir::new("asset-unreadable");
    let path = dir.join(ASSETS_FILE);
    std::fs::write(&path, "assets = [").expect("a broken store");
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    assert!(win.unsaved().is_some(), "an unreadable store is not saved");
    let mut edited = opened;
    edited.flow_ignore_opening = true;
    win.note_edit();
    assert!(win.file(edited));
    assert!(matches!(store.borrow_mut().flush(), Some(Err(_))));
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "assets = [",
        "an unreadable store is never written over"
    );
    assert!(win.unsaved().expect("unsaved").contains("unreadable"));
    let (btc, _) = bind(&store, "BTCUSDT");
    assert!(
        btc.unsaved().is_some(),
        "nothing on an unreadable store is known to be saved"
    );

    // A folder where the file should be: the write fails, and says so.
    let blocked = dir.join("blocked.toml");
    std::fs::create_dir_all(&blocked).expect("a folder in the way");
    let store = AssetBubblesStore::load(blocked).shared();
    let (mut btc, opened) = bind(&store, "BTCUSDT");
    let mut edited = opened;
    edited.look.bubbles.max_radius = 28.0;
    assert!(btc.file(edited));
    assert!(matches!(store.borrow_mut().flush(), Some(Err(_))));
    assert_eq!(store.borrow_mut().flush(), None, "not retried every frame");
    assert!(btc.unsaved().is_some());
    assert_eq!(btc.source(), AssetSource::Stored, "kept in memory");
}

/// Review round 2, finding 5: a failed write is one asset's, not every
/// asset's, and it is retried after a growing number of frames until the
/// disk takes it.
#[test]
fn a_failed_write_names_only_its_asset_and_is_retried_with_a_growing_wait() {
    let dir = ScratchDir::new("asset-retry");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    std::fs::create_dir_all(&path).expect("a folder where the file goes");
    let (mut win, opened) = bind(&store, "WINV26");
    let (btc, _) = bind(&store, "BTCUSDT");
    let mut edited = opened;
    edited.flow_ignore_opening = true;
    win.note_edit();
    assert!(win.file(edited.clone()));
    assert_eq!(win.unsaved().as_deref(), Some("not written yet"));
    assert_eq!(btc.unsaved(), None, "BTC changed nothing");

    let mut attempts = Vec::new();
    for _ in 0..7 {
        attempts.push(store.borrow_mut().flush().map(|outcome| outcome.is_ok()));
    }
    let (fail, wait) = (Some(false), None);
    assert_eq!(
        attempts,
        [fail, wait, fail, wait, wait, fail, wait],
        "tried, then after one frame, then after two"
    );
    assert!(win.unsaved().is_some());
    assert_eq!(btc.unsaved(), None, "WIN's failed write is not BTC's");
    assert!(
        store
            .borrow()
            .unsaved_any()
            .expect("unsaved")
            .starts_with("WIN*")
    );

    // The disk takes it at the next attempt once the way is clear.
    std::fs::remove_dir(&path).expect("the folder goes");
    let retried = std::iter::repeat_with(|| store.borrow_mut().flush())
        .take(usize::try_from(MAX_WRITE_BACKOFF).expect("small") + 1)
        .flatten()
        .next();
    assert_eq!(retried, Some(Ok(())));
    assert_eq!(win.unsaved(), None);
    assert_eq!(store.borrow().unsaved_any(), None);
    let reread = AssetBubblesStore::load(path);
    assert_eq!(reread.get("WIN*"), Some(&edited));
}

/// Review round 2, finding 5: an export writes what waits, backoff or not,
/// and says when the file still does not hold it.
#[test]
fn writing_now_ignores_the_backoff_and_reports_what_is_still_unsaved() {
    let dir = ScratchDir::new("asset-write-now");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    std::fs::create_dir_all(&path).expect("a folder where the file goes");
    let (mut win, opened) = bind(&store, "WINV26");
    let mut edited = opened;
    edited.flow_ignore_opening = true;
    win.note_edit();
    assert!(win.file(edited));
    assert!(matches!(store.borrow_mut().flush(), Some(Err(_))));
    let still = store.borrow_mut().write_now().expect("still unsaved");
    assert!(still.starts_with("WIN*"), "{still}");
    std::fs::remove_dir(&path).expect("the folder goes");
    assert_eq!(store.borrow_mut().write_now(), None, "written at once");
    assert!(path.is_file());
}

/// Review round 2, finding 1: presets one view reloads reach every other
/// view on the store — each refreshes its declared baseline, and an asset
/// nobody tuned wears the new look — so a later edit elsewhere does not
/// file the old look and pin the asset.
#[test]
fn a_presets_reload_in_one_view_refreshes_every_view() {
    let store = memory_store();
    let presets = bubble_presets::embedded();
    let (mut here, opened) = bind_with(&store, "BTCUSDT", &presets);
    let (mut there, _) = bind_with(&store, "BTCUSDT", &presets);
    let (mut tuned, win) = bind_with(&store, "WINV26", &presets);
    let mut own = win;
    own.flow_ignore_opening = true;
    tuned.note_edit();
    assert!(tuned.file(own));
    assert_eq!(there.take_reloaded_presets(), None, "nothing reloaded yet");

    let mut reloaded = presets.clone();
    let active = reloaded.active.clone();
    let look = reloaded
        .presets
        .iter_mut()
        .find(|preset| preset.name == active)
        .expect("the active preset");
    look.bubbles.max_radius = 34.0;
    here.publish_presets(&reloaded);
    assert_eq!(here.take_reloaded_presets(), None, "its own reload");

    let taken = there
        .take_reloaded_presets()
        .expect("the other view learns it");
    assert_eq!(taken, reloaded);
    assert_eq!(there.take_reloaded_presets(), None, "once");
    let (fresh, _) = there
        .refresh_declared(&taken)
        .expect("an untuned asset wears the new look");
    assert_eq!(fresh.look.bubbles.max_radius, 34.0);
    assert_ne!(fresh, opened);

    // An edit there that returns to the new look pins nothing.
    there.note_edit();
    assert!(!there.file(fresh));
    assert!(store.borrow().get("BTCUSDT").is_none());

    let taken = tuned
        .take_reloaded_presets()
        .expect("the WIN view learns it");
    assert_eq!(tuned.refresh_declared(&taken), None, "WIN keeps its own");
}

/// Review round 2, finding 4: a lane window set on purpose is filed even
/// when navigation already shows it.
#[test]
fn a_lane_window_set_on_purpose_to_the_navigated_one_is_filed() {
    let store = memory_store();
    let (mut win, opened) = bind(&store, "WINV26");
    let mut navigated = opened.clone();
    navigated.look.live_lane.window = LaneWindow::Fixed { ms: 4_000 };
    assert_ne!(
        navigated.look.live_lane.window,
        opened.look.live_lane.window
    );
    win.note_lane_set();
    assert!(win.file(navigated));
    let stored = store.borrow().get("WIN*").cloned().expect("stored");
    assert_eq!(
        stored.look.live_lane.window,
        LaneWindow::Fixed { ms: 4_000 }
    );
}

/// Review round 2, finding 6: two views edit one asset before either files.
/// The later filing wins whole and says so; neither edit vanishes unlogged,
/// and an import still wins over an unfiled edit.
#[test]
fn the_later_filing_wins_and_an_import_wins_over_an_unfiled_edit() {
    let dir = ScratchDir::new("asset-supersede");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut a, opened) = bind(&store, "WIN$N");
    let (mut b, _) = bind(&store, "WINV26");
    let mut first = opened.clone();
    first.flow_ignore_opening = true;
    let mut second = opened.clone();
    second.candle_aggression = true;
    a.note_edit();
    b.note_edit();
    assert!(a.file(first));
    assert!(b.file(second.clone()), "B's edit is filed, not dropped");
    assert_eq!(store.borrow().get("WIN*"), Some(&second));
    let adopted = a.adoption().expect("A wears B's").settings;
    assert_eq!(adopted, second);

    let mut file = AssetBubblesFile::default();
    file.assets.insert("WIN*".to_owned(), opened.clone());
    save_to(&path, &file).expect("an imported store");
    store.borrow_mut().reload();
    let mut late = second;
    late.look.bubbles.max_radius = 26.0;
    b.note_edit();
    assert!(!b.file(late), "the import wins over B's unfiled edit");
    assert_eq!(b.adoption().map(|adopted| adopted.settings), Some(opened));
}

fn memory_store() -> SharedAssetBubbles {
    AssetBubblesStore::default().shared()
}

/// Review round 1, finding 6: a reload — a workspace import wrote the file —
/// reaches every bound view.
#[test]
fn a_reloaded_store_reaches_every_bound_view() {
    let dir = ScratchDir::new("asset-reload");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    let mut imported = opened.clone();
    imported.look.bubbles.max_radius = 27.0;
    let mut file = AssetBubblesFile::default();
    file.assets.insert("WIN*".to_owned(), imported.clone());
    save_to(&path, &file).expect("an imported store");

    store.borrow_mut().reload();
    let adopted = win
        .adoption()
        .expect("the import reaches the view")
        .settings;
    assert_eq!(adopted, imported);
    assert_eq!(win.source(), AssetSource::Stored);
}
