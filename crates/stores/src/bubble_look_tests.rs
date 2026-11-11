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
/// must not wear where a shipped preset is declared.
fn saved_over() -> BubblePresetFile {
    let mut file = embedded();
    for preset in &mut file.presets {
        preset.bubbles.max_radius += 7.0;
    }
    file
}

/// A view on `market` under `config`, bound to `store` over `presets` and
/// wearing what the binding opened with.
fn bound_in(
    store: &SharedAssetBubbles,
    config: &crate::config::AppConfig,
    market: (&str, &str),
    presets: &BubblePresetFile,
) -> Screen {
    let mut screen = Screen::open(presets.clone());
    let (binding, settings) = AssetBinding::bind(store.clone(), config, market, presets, false);
    screen.look.bind(binding);
    screen.look.wear(&settings, false, &mut screen.config);
    screen
}

fn bound(store: &SharedAssetBubbles, market: (&str, &str), presets: &BubblePresetFile) -> Screen {
    bound_in(store, &shipped_config(), market, presets)
}

/// The look a fresh open of `market` shows over `presets`.
fn fresh_look(market: (&str, &str), presets: &BubblePresetFile) -> BubblePreset {
    let store = AssetBubblesStore::default().shared();
    let (_, settings) = AssetBinding::bind(store, &shipped_config(), market, presets, false);
    settings.look
}

/// `config` in `look` as the reset wears it: the look only, tape only kept.
fn reset_to(config: &HeatmapConfig, look: &BubblePreset) -> HeatmapConfig {
    let mut expected = config.clone();
    look.apply_to(&mut expected);
    expected.live_lane.tape_only = config.live_lane.tape_only;
    expected.sanitize();
    expected
}

/// A panel value the trader moved, as the view commits one.
fn tune(screen: &mut Screen) {
    edit(screen, |config| config.bubbles.max_radius += 3.0);
}

fn edit(screen: &mut Screen, change: impl FnOnce(&mut HeatmapConfig)) {
    let before = screen.config.clone();
    change(&mut screen.config);
    screen.look.note_config_change(&before, &screen.config);
}

/// The trader's request: "reset bubble visuals" returns an asset to the look
/// it ships with — the preset the feed config declares for it, as shipped,
/// even after the operator's presets file saved over that name — and files
/// it, so the asset opens on it again. A second reset changes nothing.
#[test]
fn the_reset_wears_the_assets_shipped_factory_look_and_files_it() {
    let operator = saved_over();
    for (market, name) in [
        (("binance-futures", "BTCUSDT.p"), "Crypto default"),
        (("binance", "ETHUSDT"), "Crypto default"),
        (("hyperliquid", "ETH"), "Crypto default"),
        (("metatrader-tickmill", "XAUUSD"), "live lane pie"),
        (("metatrader-b3", "WINZ26"), "mini index regions"),
    ] {
        let store = AssetBubblesStore::default().shared();
        let mut screen = bound(&store, market, &operator);
        let factory = fresh_look(market, &embedded());
        assert_eq!(factory.name, name);
        let copy = operator.get(name).expect("the operator's copy");
        assert_ne!(copy.bubbles, factory.bubbles, "{name}: saved over");
        tune(&mut screen);
        let expected = reset_to(&screen.config, &factory);

        assert!(screen.look.restore_factory(&mut screen.config));

        assert_eq!(screen.config, expected, "{name}: the shipped look");
        assert_eq!(screen.config.bubbles.max_radius, factory.bubbles.max_radius);
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

        let shown = screen.config.clone();
        assert!(!screen.look.restore_factory(&mut screen.config), "{name}");
        assert_eq!(screen.config, shown);
        assert!(!screen.look.asset().expect("bound").edited(), "{name}");
        let status = screen.look.status().expect("a status");
        assert!(status.contains("already"), "{status}");
    }
}

/// The reset puts back the look, not the pane's view: a tape-only pane stays
/// tape only. The native tape is the look's, as a fresh open sets it.
#[test]
fn the_reset_keeps_tape_only_and_wears_the_factory_native_tape() {
    let market = ("binance-futures", "BTCUSDT.p");
    let factory = fresh_look(market, &embedded());
    assert!(factory.live_lane.native_tape, "the shipped look's tape");
    let store = AssetBubblesStore::default().shared();
    let mut screen = bound(&store, market, &embedded());
    edit(&mut screen, |config| {
        config.live_lane.tape_only = true;
        config.live_lane.native_tape = false;
    });
    tune(&mut screen);
    assert!(screen.look.restore_factory(&mut screen.config));
    assert!(screen.config.live_lane.tape_only, "still tape only");
    assert!(
        screen.config.live_lane.native_tape,
        "the factory native tape"
    );

    let win = ("metatrader-b3", "WINZ26");
    let factory = fresh_look(win, &embedded());
    assert!(factory.live_lane.native_tape, "the mini index's tape");
    let mut screen = bound(&AssetBubblesStore::default().shared(), win, &embedded());
    let picked = "consumption focus";
    assert!(screen.look.apply_preset(picked, &mut screen.config, false));
    assert!(!screen.config.live_lane.native_tape, "{picked} has none");
    assert!(screen.look.restore_factory(&mut screen.config));
    assert!(
        screen.config.live_lane.native_tape,
        "WIN's native tape is back"
    );
}

/// The factory look's spheres come with it whatever pane resets: the mini
/// index gets its spheres back, and an asset without the mode gets none.
#[test]
fn the_reset_wears_the_factory_spheres_the_asset_opens_with() {
    let win = ("metatrader-b3", "WINZ26");
    let factory = fresh_look(win, &embedded());
    assert!(factory.tick_bubbles.is_some(), "the mini index has spheres");
    let mut screen = bound(&AssetBubblesStore::default().shared(), win, &embedded());
    edit(&mut screen, |config| config.volume_dots.tick_bubbles = None);
    assert!(screen.look.restore_factory(&mut screen.config));
    assert_eq!(screen.config.volume_dots.tick_bubbles, factory.tick_bubbles);

    let gold = ("metatrader-tickmill", "XAUUSD");
    let mut screen = bound(&AssetBubblesStore::default().shared(), gold, &embedded());
    edit(&mut screen, |config| {
        config.volume_dots.tick_bubbles = factory.tick_bubbles;
    });
    assert!(screen.look.restore_factory(&mut screen.config));
    assert_eq!(screen.config.volume_dots.tick_bubbles, None, "no mode");
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
        assert!(screen.look.restore_factory(&mut screen.config));
        screen.look.sync(&screen.config, false, false);
        assert_eq!(store.borrow().get(&key), None, "{key}: nothing stored");
        let binding = screen.look.asset().expect("bound");
        assert_eq!(binding.source(), AssetSource::Declared, "{key}");
    }
}

/// A declared name the shipped presets lack opens on the operator's preset
/// of that name, so that is the asset's factory look; an asset nothing is
/// declared for opens on the operator file's active look.
#[test]
fn without_a_shipped_declared_preset_the_reset_wears_what_the_operator_file_opens() {
    let mut operator = saved_over();
    let mut own = operator.get("dense tape").cloned().expect("a preset");
    own.name = "my look".to_owned();
    own.bubbles.max_radius = 19.0;
    operator.presets.push(own.clone());
    let mut config = shipped_config();
    let hyperliquid = config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "hyperliquid");
    hyperliquid.expect("a shipped feed").bubble_preset = Some("my look".to_owned());
    let store = AssetBubblesStore::default().shared();
    let mut screen = bound_in(&store, &config, ("hyperliquid", "ETH"), &operator);
    tune(&mut screen);
    assert!(screen.look.restore_factory(&mut screen.config));
    assert_eq!(screen.look.look_name(), "my look");
    assert_eq!(screen.config.bubbles.max_radius, own.bubbles.max_radius);

    // Binance with no look declared, feed-wide or for ETHUSDT.
    let mut undeclared = shipped_config();
    let binance = undeclared
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "binance");
    let binance = binance.expect("a shipped feed");
    binance.bubble_preset = None;
    assert!(!binance.symbol_bubble_presets.contains_key("ETHUSDT"));
    let mut eth = bound_in(&store, &undeclared, ("binance", "ETHUSDT"), &operator);
    let active = operator.get(&operator.active).expect("the active look");
    tune(&mut eth);
    assert!(eth.look.restore_factory(&mut eth.config));
    assert_eq!(eth.look.look_name(), active.name);
    assert_eq!(eth.config.bubbles.max_radius, active.bubbles.max_radius);
}

/// No asset bound: the reset restores the code defaults, as it always did.
#[test]
fn an_unbound_view_resets_to_the_code_defaults() {
    let mut screen = Screen::open(embedded());
    tune(&mut screen);
    let mut expected = screen.config.clone();
    expected.reset_bubble_visuals();
    expected.sanitize();
    assert!(screen.look.restore_factory(&mut screen.config));
    assert_eq!(screen.config, expected);
    assert_eq!(screen.look.look_name(), "");
    assert_eq!(
        screen.look.status(),
        Some("bubble defaults restored for the asset on screen")
    );
    assert!(!screen.look.restore_factory(&mut screen.config));
}
