//! The store on disk: written once per change, read back by a restart and
//! by a workspace import, and never claimed saved when it is not.
use super::*;
use crate::bubble_assets::{ASSETS_FILE, AssetBubblesFile, save_to};
use crate::bubble_presets;
use crate::scratch::ScratchDir;

const NATIVE_PRESET: &str = "mini index regions";

fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../app/config/feeds.toml")).expect("shipped feeds parse")
}

fn bind(store: &SharedAssetBubbles, symbol: &str) -> (AssetBinding, AssetBubbles) {
    let presets = bubble_presets::embedded();
    AssetBinding::bind(store.clone(), &shipped_config(), symbol, &presets, false)
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
    win.note_change();
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
    win.note_change();
    assert!(win.file(edited));
    assert!(matches!(store.borrow_mut().flush(), Some(Err(_))));
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "assets = [",
        "an unreadable store is never written over"
    );
    assert!(win.unsaved().expect("unsaved").contains("unreadable"));

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
    let (adopted, _) = win.adoption().expect("the import reaches the view");
    assert_eq!(adopted, imported);
    assert_eq!(win.source(), AssetSource::Stored);
}
