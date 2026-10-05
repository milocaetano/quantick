//! "Save changes for this asset": the asset's own switch. Off, what the
//! trader changes stays on the screen it was made on, for the session only;
//! on again, that screen is filed.
use super::bubble_asset_store_tests::bind;
use super::*;
use crate::bubble_assets::{ASSETS_FILE, AssetBubblesFile, save_to};
use crate::bubble_presets;
use crate::scratch::ScratchDir;
use quantick_orderflow::LaneWindow;

#[test]
fn saving_off_keeps_changes_on_one_screen_and_a_restart_keeps_only_the_switch() {
    let dir = ScratchDir::new("asset-save-off");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    let (mut other, _) = bind(&store, "WIN$N");
    let (btc, _) = bind(&store, "BTCUSDT");
    assert!(win.saves_changes(), "saving is on until switched off");
    assert_eq!(win.set_save_changes(false), SaveSwitch::Off);
    assert_eq!(
        win.set_save_changes(false),
        SaveSwitch::Unmoved,
        "already off"
    );
    assert!(
        !other.saves_changes(),
        "the switch is the asset's, in every tab"
    );
    assert!(btc.saves_changes(), "and that asset's alone");
    assert_eq!(
        store.borrow_mut().flush(),
        Some(Ok(())),
        "the switch is kept"
    );

    let mut edited = opened.clone();
    edited.flow_ignore_opening = true;
    edited.look.bubbles.max_radius = 29.0;
    win.note_edit();
    let why = win.unsaved().expect("an edit with saving off is not saved");
    assert!(why.contains("saving is off"), "{why}");
    assert!(!win.file(edited.clone()), "nothing is filed");
    assert_eq!(win.on_screen(), &edited, "the change stays on this screen");
    assert!(!win.edited(), "held, not waiting to be filed every frame");
    assert_eq!(store.borrow().get("WIN*"), None);
    assert_eq!(store.borrow_mut().flush(), None, "nothing to write");
    let why = win.unsaved().expect("still not saved");
    assert!(why.contains("saving is off"), "{why}");
    assert_eq!(other.adoption(), None, "another tab keeps the stored look");
    assert_eq!(other.unsaved(), None, "and it is what the file holds");
    assert_eq!(
        win.refresh_declared(&bubble_presets::embedded()),
        None,
        "a presets reload does not wipe changes held on screen"
    );

    let written = std::fs::read_to_string(&path).expect("the store");
    assert!(
        written.contains("save_changes_off = [\"WIN*\"]"),
        "{written}"
    );
    let reread = AssetBubblesStore::load(path).shared();
    let (win, restored) = bind(&reread, "WINV26");
    assert!(!win.saves_changes(), "a restart keeps the switch");
    assert_eq!(restored, opened, "and drops what was never saved");
    assert_eq!(win.unsaved(), None);
    assert!(bind(&reread, "BTCUSDT").0.saves_changes());
}

#[test]
fn saving_on_again_files_the_screen_that_turned_it_on_and_other_tabs_wear_it() {
    let dir = ScratchDir::new("asset-save-on");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    let (mut other, _) = bind(&store, "WIN$N");
    assert_eq!(win.set_save_changes(false), SaveSwitch::Off);

    let mut mine = opened.clone();
    mine.flow_ignore_opening = true;
    mine.look.live_lane.window = LaneWindow::Fixed { ms: 6_000 };
    win.note_lane_set();
    assert!(!win.file(mine.clone()));
    let mut theirs = opened.clone();
    theirs.candle_aggression = true;
    other.note_edit();
    assert!(!other.file(theirs));

    assert_eq!(win.set_save_changes(true), SaveSwitch::ScreenStored);
    assert!(win.edited(), "the screen waits for the next filing");
    assert!(win.file(mine.clone()), "what is on screen is filed");
    assert_eq!(
        store.borrow().get("WIN*"),
        Some(&mine),
        "the lane window set while saving was off is filed with it"
    );
    let adopted = other
        .adoption()
        .expect("the other tab drops what it held and wears the filed look");
    assert_eq!(adopted.settings, mine);
    assert!(adopted.dropped, "and is told its held change is gone");
    assert_eq!(win.adoption(), None, "the screen that turned it on stays");
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    assert_eq!(win.unsaved(), None);
    assert_eq!(other.unsaved(), None);
    let reread = AssetBubblesStore::load(path);
    assert!(reread.saves_changes("WIN*"));
    assert_eq!(reread.get("WIN*"), Some(&mine));
}

/// Review of d326c7b7, finding 1: switching saving on takes what the store
/// holds first — another tab's filing or an import — and files nothing of
/// this tab's stale screen over it.
#[test]
fn switching_saving_on_behind_the_store_wears_the_store_and_files_nothing() {
    let dir = ScratchDir::new("asset-save-behind");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    let (mut other, _) = bind(&store, "WIN$N");
    assert_eq!(win.set_save_changes(false), SaveSwitch::Off);
    let mut stale = opened.clone();
    stale.flow_ignore_opening = true;
    win.note_edit();
    assert!(!win.file(stale.clone()));

    // Another tab switches saving on, files its own screen, and off again
    // before this one looked.
    let mut theirs = opened.clone();
    theirs.candle_aggression = true;
    assert_eq!(other.set_save_changes(true), SaveSwitch::ScreenStored);
    assert!(other.file(theirs.clone()));
    assert_eq!(other.set_save_changes(false), SaveSwitch::Off);

    assert_eq!(
        win.set_save_changes(true),
        SaveSwitch::StoredTaken,
        "behind the store, the screen gives way and the switch says so"
    );
    win.note_edit();
    assert!(!win.file(stale.clone()), "the stale screen is not filed");
    let adopted = win.adoption().expect("the store is worn first");
    assert_eq!(adopted.settings, theirs);
    assert!(
        adopted.dropped,
        "what this tab held is gone, and it says so"
    );
    assert_eq!(store.borrow().get("WIN*"), Some(&theirs));
    assert_eq!(win.unsaved(), Some("not written yet".to_owned()));

    // An import while saving is off wins the same way.
    assert_eq!(win.set_save_changes(false), SaveSwitch::Off);
    win.note_edit();
    assert!(!win.file(stale.clone()));
    let mut imported = opened.clone();
    imported.look.bubbles.max_radius = 27.0;
    let mut file = AssetBubblesFile::default();
    file.save_changes_off.insert("WIN*".to_owned());
    file.assets.insert("WIN*".to_owned(), imported.clone());
    save_to(&path, &file).expect("an imported store");
    store.borrow_mut().reload();
    assert_eq!(win.set_save_changes(true), SaveSwitch::StoredTaken);
    win.note_edit();
    assert!(!win.file(stale), "the import is the newer word");
    assert_eq!(
        win.adoption().map(|adopted| adopted.settings),
        Some(imported.clone())
    );
    assert_eq!(store.borrow().get("WIN*"), Some(&imported));
}

/// Review of d326c7b7, finding 2: with saving off a lane set on purpose is
/// held as set; later wheeling or dragging moves the view only, and is
/// neither held nor filed when saving comes back on.
#[test]
fn with_saving_off_navigation_after_a_lane_set_is_neither_held_nor_filed() {
    let store = AssetBubblesStore::default().shared();
    let (mut win, opened) = bind(&store, "WINV26");
    assert_eq!(win.set_save_changes(false), SaveSwitch::Off);
    let mut set = opened.clone();
    set.look.live_lane.window = LaneWindow::Fixed { ms: 6_000 };
    win.note_lane_set();
    assert!(!win.file(set.clone()));

    let mut wheeled = set.clone();
    wheeled.look.live_lane.window = LaneWindow::Fixed { ms: 20_000 };
    wheeled.look.live_lane.width_share = 0.61;
    wheeled.look.bubbles.max_radius = 31.0;
    win.note_edit();
    assert!(!win.file(wheeled.clone()));
    let mut expected = set;
    expected.look.bubbles.max_radius = 31.0;
    assert_eq!(
        win.on_screen(),
        &expected,
        "the set lane is held, the wheel's is not"
    );

    assert_eq!(win.set_save_changes(true), SaveSwitch::ScreenStored);
    assert!(win.file(wheeled));
    assert_eq!(store.borrow().get("WIN*"), Some(&expected), "nor filed");
}

/// Review of d326c7b7, finding 3: a store that refused the switch's own
/// write says so before "saving is off" — at a restart the switch would be
/// on again.
#[test]
fn a_failed_write_is_reported_before_saving_is_off() {
    let dir = ScratchDir::new("asset-save-refused");
    let path = dir.join(ASSETS_FILE);
    std::fs::create_dir_all(path.with_extension("toml.tmp")).expect("a folder in the write's way");
    let store = AssetBubblesStore::load(path).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    assert_eq!(win.set_save_changes(false), SaveSwitch::Off);
    assert!(matches!(store.borrow_mut().flush(), Some(Err(_))));
    let mut edited = opened;
    edited.flow_ignore_opening = true;
    win.note_edit();
    assert!(!win.file(edited));
    let why = win.unsaved().expect("not saved");
    let (store_part, screen_part) = why.split_once("; ").expect("both reasons");
    assert!(store_part.contains("bubble-assets.toml"), "{why}");
    assert!(screen_part.contains("saving is off"), "{why}");
}
