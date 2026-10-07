//! Saving a look on one asset writes the preset, never the presets file's
//! default: the look every undeclared asset opens on stays the file's own.
use super::*;
use crate::bubble_asset_store::AssetBubblesStore;
use crate::bubble_asset_store::bubble_asset_store_tests::bind;
use crate::bubble_presets::{embedded, parse};

/// One view's screen: the look and the config it dresses.
struct Screen {
    look: BubbleLook,
    config: HeatmapConfig,
}

impl Screen {
    fn open(document: BubblePresetFile) -> Self {
        let mut config = HeatmapConfig::default();
        let look = BubbleLook::open((document, PresetSource::Embedded, None), &mut config);
        Self { look, config }
    }

    /// The edited mini index look: its preset, then the tape's opening scale.
    fn edited_win() -> (Self, BubblePreset) {
        let mut screen = Self::open(embedded());
        assert!(
            screen
                .look
                .apply_preset("mini index regions", &mut screen.config)
        );
        screen.ignore_opening(true);
        let win = screen.appearance();
        (screen, win)
    }

    /// The trader's edit, as the view commits one.
    fn ignore_opening(&mut self, on: bool) {
        let before = self.config.clone();
        assert!(self.config.set_ignore_opening_burst_in_scale(on));
        self.look.note_config_change(&before, &self.config);
    }

    fn appearance(&self) -> BubblePreset {
        BubblePreset::capture(self.look.look_name(), &self.config)
    }

    fn save(&mut self) -> BubblePresetFile {
        let mut stored = None;
        self.look.save_preset(&self.config, |document| {
            let encoded = toml::to_string(document).expect("presets serialize");
            stored = Some(parse(&encoded).expect("saved presets reload"));
            Ok(PathBuf::from("isolated-test-bubbles.toml"))
        });
        stored.expect("the save action handed a document to its writer")
    }

    fn name(&mut self, name: &str) {
        name.clone_into(self.look.name_draft_mut());
    }
}

#[test]
fn opening_wears_the_files_active_preset_and_drafts_its_name() {
    let screen = Screen::open(embedded());
    assert_eq!(screen.look.look_name(), embedded().active);
    assert!(screen.look.draft_is_stored());
    assert_eq!(screen.look.status(), None);
    let unreadable = BubbleLook::open(
        (
            embedded(),
            PresetSource::Embedded,
            Some("bad toml".to_owned()),
        ),
        &mut HeatmapConfig::default(),
    );
    assert_eq!(unreadable.status(), Some("presets not loaded — bad toml"));
}

#[test]
fn saving_the_win_look_keeps_the_files_default_for_every_other_asset() {
    let (mut screen, win) = Screen::edited_win();
    let stored = screen.save();
    assert_eq!(
        stored.active,
        embedded().active,
        "the file's default is untouched"
    );
    assert_eq!(stored.get("mini index regions"), Some(&win));
    assert_eq!(screen.appearance(), win, "the WIN pane stays put");

    let fresh = Screen::open(stored);
    assert_eq!(fresh.look.look_name(), embedded().active);
    assert!(!fresh.config.native_tape());
    assert!(!fresh.config.volume_dots.enabled);
    assert!(!fresh.config.volume_dots.ignore_opening_burst_in_scale);
}

#[test]
fn a_new_name_is_saved_and_worn_without_becoming_the_default() {
    let (mut screen, _) = Screen::edited_win();
    screen.name("my win");
    let stored = screen.save();
    assert_eq!(screen.look.look_name(), "my win");
    assert!(stored.get("my win").is_some());
    assert_eq!(stored.active, embedded().active);
}

#[test]
fn an_unnamed_save_reaches_no_writer() {
    let (mut screen, _) = Screen::edited_win();
    screen.name("  ");
    screen.look.save_preset(&screen.config, |_| {
        panic!("an unnamed save reached the writer")
    });
    assert_eq!(screen.look.status(), Some("name the preset before saving"));
}

#[test]
fn reloading_reapplies_the_saved_look_on_screen() {
    let (mut screen, win) = Screen::edited_win();
    let stored = screen.save();
    screen.ignore_opening(false);
    let worn = screen
        .look
        .reload((stored, PresetSource::Embedded, None), &mut screen.config);
    assert_eq!(worn, None, "an unbound view wears nothing from an asset");
    assert_eq!(screen.appearance(), win, "reload discards unsaved edits");
    assert_eq!(
        screen.look.status(),
        Some("reloaded · 'mini index regions' applied")
    );
}

#[test]
fn a_failed_save_keeps_the_view_and_says_so() {
    let (mut screen, win) = Screen::edited_win();
    screen.look.save_preset(&screen.config, |document| {
        assert_eq!(document.active, embedded().active);
        Err("test writer refused the save".to_owned())
    });
    assert_eq!(screen.appearance(), win);
    assert!(screen.look.status().unwrap().starts_with("not saved"));
}

#[test]
fn deleting_removes_the_drafted_preset_and_writes_the_file() {
    let (mut screen, _) = Screen::edited_win();
    screen.name("my win");
    screen.save();
    assert!(screen.look.draft_is_stored());
    let mut written = None;
    screen.look.delete_preset(|document| {
        written = Some(document.clone());
        Ok(PathBuf::from("isolated-test-bubbles.toml"))
    });
    assert!(written.expect("written").get("my win").is_none());
    assert!(!screen.look.draft_is_stored());
}

/// Review round 1, finding 5: a look other assets open on is not saved over
/// — nor removed — from one asset's panel.
#[test]
fn a_look_other_assets_open_on_is_not_saved_over_from_one_asset() {
    let (mut screen, _) = Screen::edited_win();
    let (binding, _) = bind(&AssetBubblesStore::default().shared(), "WINV26");
    screen.look.bind(binding);
    let active = embedded().active;
    for name in ["mini index regions", active.as_str(), "live lane pie"] {
        screen.name(name);
        screen
            .look
            .save_preset(&screen.config, |_| panic!("'{name}' reached the writer"));
        let status = screen.look.status().expect("a status");
        assert!(status.contains("other assets open on"), "{status}");
        screen
            .look
            .delete_preset(|_| panic!("'{name}' was removed"));
    }
    screen.name("my win");
    assert!(screen.save().get("my win").is_some());
}

#[test]
fn a_launch_hold_is_released_by_the_traders_own_switch() {
    let mut screen = Screen::open(embedded());
    screen
        .look
        .hold_for_run(|held| held.bubbles = Some(false), &mut screen.config);
    assert!(!screen.config.show_aggressions);
    let before = screen.config.clone();
    screen.config.show_aggressions = true;
    screen.look.note_config_change(&before, &screen.config);
    assert!(
        screen.look.settings(&screen.config, false, false).bubbles,
        "the trader's switch is filed, not the hold"
    );
}
