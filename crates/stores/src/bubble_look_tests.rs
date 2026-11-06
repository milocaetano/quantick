//! Saving a look on one asset writes the preset, never the presets file's
//! default: the look every undeclared asset opens on stays the file's own.
use super::*;
use crate::bubble_asset_store::bubble_asset_store_tests::{bind, shipped_config};
use crate::bubble_asset_store::{AssetBubblesStore, SharedAssetBubbles};
use crate::bubble_assets::AssetSource;
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
                .apply_preset("mini index regions", &mut screen.config, false)
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

/// The tape window a strip double-click returns to: the run's hold, else the
/// asset's own setting, else the default — never the navigated window.
#[test]
fn the_lane_window_setting_is_the_hold_then_the_asset_then_the_default() {
    use quantick_orderflow::LaneWindow;
    let mut screen = Screen::open(embedded());
    assert_eq!(
        screen.look.lane_window_setting(),
        screen.config.live_lane.window,
        "the window the active preset opened on"
    );

    let (binding, _) = bind(&AssetBubblesStore::default().shared(), "WINV26");
    screen.look.bind(binding);
    // Set on purpose, as the window menu does, and filed at the next sync.
    screen.config.live_lane.window = LaneWindow::Fixed { ms: 6_000 };
    screen.look.note_lane_set(&screen.config);
    screen.look.sync(&screen.config, false, false);
    // Then the wheel: navigation, never filed.
    screen.config.live_lane.window = LaneWindow::Fixed { ms: 40_000 };
    screen.look.sync(&screen.config, false, false);
    assert_eq!(
        screen.look.lane_window_setting(),
        LaneWindow::Fixed { ms: 6_000 },
        "the asset's setting, not the wheel's window"
    );

    screen.look.hold_for_run(
        |held| held.window = Some(LaneWindow::Fixed { ms: 2_000 }),
        &mut screen.config,
    );
    assert_eq!(
        screen.look.lane_window_setting(),
        LaneWindow::Fixed { ms: 2_000 }
    );
}

/// Review round 2, B1: an unbound view resets to the window its active
/// preset opened on, and to a window set on purpose since.
#[test]
fn an_unbound_view_resets_to_its_presets_window_then_to_one_set_on_purpose() {
    use quantick_orderflow::LaneWindow;
    let mut document = embedded();
    let active = document.active.clone();
    let preset = document
        .presets
        .iter_mut()
        .find(|preset| preset.name == active)
        .expect("the active preset is stored");
    preset.live_lane.window = LaneWindow::Fixed { ms: 9_000 };
    let mut screen = Screen::open(document);
    assert_eq!(
        screen.config.live_lane.window,
        LaneWindow::Fixed { ms: 9_000 }
    );
    screen.config.live_lane.window = LaneWindow::Fixed { ms: 30_000 };
    assert_eq!(
        screen.look.lane_window_setting(),
        LaneWindow::Fixed { ms: 9_000 },
        "the wheel is not a setting"
    );
    screen.config.live_lane.window = LaneWindow::Fixed { ms: 3_000 };
    screen.look.note_lane_set(&screen.config);
    screen.config.live_lane.window = LaneWindow::Fixed { ms: 30_000 };
    assert_eq!(
        screen.look.lane_window_setting(),
        LaneWindow::Fixed { ms: 3_000 }
    );
}

/// Review round 2, B2: a window set and reset before the next sync resets to
/// the window just set, which is what the sync then files for the asset.
#[test]
fn a_reset_before_the_sync_sees_the_window_just_set() {
    use quantick_orderflow::LaneWindow;
    let mut screen = Screen::open(embedded());
    let (binding, _) = bind(&AssetBubblesStore::default().shared(), "WINV26");
    screen.look.bind(binding);
    screen.config.live_lane.window = LaneWindow::Fixed { ms: 6_000 };
    screen.look.note_lane_set(&screen.config);
    // The reset in the same frame: navigation back onto the setting.
    screen.config.live_lane.window = screen.look.lane_window_setting();
    assert_eq!(
        screen.config.live_lane.window,
        LaneWindow::Fixed { ms: 6_000 }
    );
    screen.look.sync(&screen.config, false, false);
    screen.config.live_lane.window = LaneWindow::Fixed { ms: 40_000 };
    assert_eq!(
        screen.look.lane_window_setting(),
        LaneWindow::Fixed { ms: 6_000 },
        "the trader's choice was filed, not lost"
    );
    assert_eq!(
        screen
            .look
            .asset()
            .expect("bound")
            .on_screen()
            .look
            .live_lane
            .window,
        LaneWindow::Fixed { ms: 6_000 }
    );
}

#[test]
fn deliberate_same_name_btc_preset_selection_replaces_old_saved_lane_flags() {
    let mut screen = Screen::open(embedded());
    let name = "Binance BTC perpetual spheres";
    assert!(screen.look.apply_preset(name, &mut screen.config, false));
    let approved_tick = screen.config.volume_dots.tick_bubbles;
    screen.config.live_lane.native_tape = false;
    screen.config.volume_dots.enabled = false;
    screen.config.live_lane.depth_time_averaging = false;
    assert_eq!(screen.look.look_name(), name);
    assert!(screen.look.apply_preset(name, &mut screen.config, false));
    assert!(screen.config.live_lane.native_tape);
    assert!(screen.config.volume_dots.enabled);
    assert!(screen.config.live_lane.depth_time_averaging);
    assert_eq!(screen.config.volume_dots.tick_bubbles, approved_tick);
}

/// The operator's presets file with every preset saved over: what a reset
/// must not wear.
fn saved_over() -> BubblePresetFile {
    let mut file = embedded();
    for preset in &mut file.presets {
        preset.bubbles.max_radius += 7.0;
    }
    file
}

/// A view on `market`, bound to `store` over `presets` and wearing what the
/// binding opened with.
fn bound(store: &SharedAssetBubbles, market: (&str, &str), presets: &BubblePresetFile) -> Screen {
    let mut screen = Screen::open(presets.clone());
    let (binding, settings) =
        AssetBinding::bind(store.clone(), &shipped_config(), market, presets, false);
    screen.look.bind(binding);
    screen.look.wear(&settings, false, &mut screen.config);
    screen
}

/// A panel value the trader moved, as the view commits one.
fn tune(screen: &mut Screen) {
    let before = screen.config.clone();
    screen.config.bubbles.max_radius += 3.0;
    screen.look.note_config_change(&before, &screen.config);
}

/// The trader's request: "reset bubble visuals" returns an asset to the look
/// it ships with — the preset the feed config declares for it, as shipped,
/// even after the operator's presets file saved over that name — and files
/// it, so the asset opens on it again.
#[test]
fn the_reset_wears_the_assets_shipped_factory_look_and_files_it() {
    let appearance = quantick_appearance::ResolvedTheme::bundled_dark().orderflow;
    let operator = saved_over();
    for (market, name) in [
        (
            ("binance-futures", "BTCUSDT.p"),
            "Binance BTC perpetual spheres",
        ),
        (("metatrader-tickmill", "XAUUSD"), "live lane pie"),
        (("metatrader-b3", "WINZ26"), "mini index regions"),
    ] {
        let store = AssetBubblesStore::default().shared();
        let mut screen = bound(&store, market, &operator);
        let shipped = embedded().get(name).cloned().expect("a shipped preset");
        assert_ne!(operator.get(name), Some(&shipped), "{name}: saved over");
        tune(&mut screen);
        let spheres = screen.look.asset_mode().is_some();
        let mut expected = screen.config.clone();
        super::dress(&shipped, &mut expected, spheres, &appearance);
        // As a fresh open wears it: the palette is not the theme's snapshot.
        expected.palette = shipped.palette;

        let worn = screen.look.restore_factory(&mut screen.config, spheres);

        assert_eq!(worn.as_deref(), Some(name));
        assert_eq!(screen.config, expected, "{name}: the shipped look");
        assert_eq!(screen.config.bubbles, shipped.bubbles, "{name}");
        assert_eq!(
            screen.config.palette,
            quantick_orderflow::config::theme::PaletteChoice::Inherited,
            "{name}: the palette still follows the theme"
        );
        assert_eq!(screen.look.look_name(), name, "the picker names it");
        assert_eq!(screen.look.name_draft(), name);
        let key = screen.look.asset().expect("bound").key().to_owned();
        assert_eq!(
            screen.look.status(),
            Some(format!("factory look '{name}' restored for {key}").as_str())
        );
        screen.look.sync(&screen.config, false, false);
        let (_, reopened) =
            AssetBinding::bind(store.clone(), &shipped_config(), market, &operator, false);
        assert_eq!(
            reopened.look,
            BubblePreset::capture(name, &screen.config),
            "{name}: a fresh bind opens on the factory look"
        );
    }
}

/// Over an unedited presets file, a reset after a trader's edit returns the
/// asset to exactly what a fresh open wears: nothing of its own is stored.
#[test]
fn a_reset_over_the_shipped_presets_leaves_no_stored_entry() {
    for market in [
        ("binance-futures", "BTCUSDT.p"),
        ("metatrader-tickmill", "XAUUSD"),
        ("metatrader-b3", "WINZ26"),
    ] {
        let store = AssetBubblesStore::default().shared();
        let mut screen = bound(&store, market, &embedded());
        tune(&mut screen);
        screen.look.sync(&screen.config, false, false);
        let key = screen.look.asset().expect("bound").key().to_owned();
        assert!(
            store.borrow().get(&key).is_some(),
            "{key}: the edit is filed"
        );
        let spheres = screen.look.asset_mode().is_some();
        assert!(
            screen
                .look
                .restore_factory(&mut screen.config, spheres)
                .is_some()
        );
        screen.look.sync(&screen.config, false, false);
        assert_eq!(store.borrow().get(&key), None, "{key}: nothing stored");
        let binding = screen.look.asset().expect("bound");
        assert_eq!(binding.source(), AssetSource::Declared, "{key}");
    }
}

/// No declared look, or no asset at all: the reset restores the code
/// defaults, as it always did.
#[test]
fn without_a_declared_look_the_reset_restores_the_code_defaults() {
    let store = AssetBubblesStore::default().shared();
    let mut eth = bound(&store, ("binance", "ETHUSDT"), &embedded());
    assert_eq!(eth.look.asset().expect("bound").factory_preset(), None);
    let mut unbound = Screen::open(embedded());
    for screen in [&mut eth, &mut unbound] {
        tune(screen);
        let mut expected = screen.config.clone();
        expected.reset_bubble_visuals();
        let worn = screen.look.restore_factory(&mut screen.config, false);
        assert_eq!(worn, None);
        assert_eq!(screen.config, expected);
        assert_eq!(screen.look.look_name(), "");
        assert_eq!(
            screen.look.status(),
            Some("bubble defaults restored for the asset on screen")
        );
    }
}
