//! "Save changes for this asset": the asset's own switch. Off, what the
//! trader changes stays on the screen it was made on, for the session only;
//! on again, that screen is filed.
use super::*;
use crate::bubble_assets::ASSETS_FILE;
use crate::bubble_presets;
use crate::scratch::ScratchDir;
use quantick_orderflow::LaneWindow;

fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../app/config/feeds.toml")).expect("shipped feeds parse")
}

fn bind(store: &SharedAssetBubbles, symbol: &str) -> (AssetBinding, AssetBubbles) {
    let feed = if symbol.starts_with("WIN") {
        "metatrader-b3"
    } else {
        "binance"
    };
    AssetBinding::bind(
        store.clone(),
        &shipped_config(),
        (feed, symbol),
        &bubble_presets::embedded(),
        false,
    )
}

#[test]
fn saving_off_keeps_changes_on_one_screen_and_a_restart_keeps_only_the_switch() {
    let dir = ScratchDir::new("asset-save-off");
    let path = dir.join(ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut win, opened) = bind(&store, "WINV26");
    let (mut other, _) = bind(&store, "WIN$N");
    let (btc, _) = bind(&store, "BTCUSDT");
    assert!(win.saves_changes(), "saving is on until switched off");
    assert!(win.set_save_changes(false));
    assert!(!win.set_save_changes(false), "already off");
    assert!(!other.saves_changes(), "the switch is the asset's, in every tab");
    assert!(btc.saves_changes(), "and that asset's alone");
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())), "the switch is kept");

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
    assert!(written.contains("save_changes_off = [\"WIN*\"]"), "{written}");
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
    assert!(win.set_save_changes(false));

    let mut mine = opened.clone();
    mine.flow_ignore_opening = true;
    mine.look.live_lane.window = LaneWindow::Fixed { ms: 6_000 };
    win.note_lane_set();
    assert!(!win.file(mine.clone()));
    let mut theirs = opened.clone();
    theirs.candle_aggression = true;
    other.note_edit();
    assert!(!other.file(theirs));

    assert!(win.set_save_changes(true));
    assert!(win.edited(), "the screen waits for the next filing");
    assert!(win.file(mine.clone()), "what is on screen is filed");
    assert_eq!(
        store.borrow().get("WIN*"),
        Some(&mine),
        "the lane window set while saving was off is filed with it"
    );
    let (adopted, _) = other
        .adoption()
        .expect("the other tab drops what it held and wears the filed look");
    assert_eq!(adopted, mine);
    assert_eq!(win.adoption(), None, "the screen that turned it on stays");
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    assert_eq!(win.unsaved(), None);
    assert_eq!(other.unsaved(), None);
    let reread = AssetBubblesStore::load(path);
    assert!(reread.saves_changes("WIN*"));
    assert_eq!(reread.get("WIN*"), Some(&mine));
}
