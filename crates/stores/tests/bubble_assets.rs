//! Each asset keeps its own bubble settings: the config declares which look an
//! asset opens on, the trader's changes are filed under that asset alone, and
//! a filed asset reopens exactly as it was left.

use quantick_orderflow::{HeatmapConfig, LaneFold, LaneView, LaneWindow};
use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore, SharedAssetBubbles};
use quantick_stores::bubble_assets::{self, AssetBubbles, AssetBubblesFile, AssetSource};
use quantick_stores::bubble_look::BubbleLook;
use quantick_stores::bubble_presets::{self, BubblePresetFile, PresetSource};
use quantick_stores::config::{AppConfig, BubbleAsset};

const NATIVE_PRESET: &str = "mini index regions";
const BTC_SPOT_PRESET: &str = "Binance BTC Spot spheres";

fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../app/config/feeds.toml")).expect("shipped feeds parse")
}

fn asset(feed: &str, symbol: &str) -> BubbleAsset {
    shipped_config()
        .feed(feed)
        .expect("a shipped feed")
        .bubble_asset(symbol)
}

fn memory() -> SharedAssetBubbles {
    AssetBubblesStore::default().shared()
}

fn bind(store: &SharedAssetBubbles, symbol: &str) -> (AssetBinding, AssetBubbles) {
    bind_with(store, symbol, &bubble_presets::embedded())
}

fn bind_with(
    store: &SharedAssetBubbles,
    symbol: &str,
    presets: &BubblePresetFile,
) -> (AssetBinding, AssetBubbles) {
    let config = shipped_config();
    let feed = config
        .feeds
        .iter()
        .find(|feed| feed.symbols.iter().any(|offered| offered == symbol))
        .map_or("binance", |feed| feed.id.as_str())
        .to_owned();
    AssetBinding::bind(store.clone(), &config, (&feed, symbol), presets, false)
}

#[test]
fn the_win_family_is_one_asset_declared_by_a_config_pattern() {
    for symbol in ["WIN$N", "WINV26", "WINZ26", "WINF27"] {
        let win = asset("metatrader-b3", symbol);
        assert_eq!(win.key, "WIN*", "{symbol} belongs to the mini index");
        assert_eq!(win.preset.as_deref(), Some(NATIVE_PRESET), "{symbol}");
    }
    let wdo = asset("metatrader-b3", "WDO$N");
    assert_eq!(wdo.key, "WDO$N");
    assert_eq!(wdo.preset.as_deref(), Some("live lane pie"));
    let btc = asset("binance", "BTCUSDT");
    assert_eq!(btc.key, "BTCUSDT");
    assert_eq!(btc.preset.as_deref(), Some(BTC_SPOT_PRESET));
}

#[test]
fn only_win_and_declared_binance_btc_looks_open_on_native_tape() {
    let store = memory();
    let config = shipped_config();
    for feed in &config.feeds {
        for symbol in &feed.symbols {
            let (binding, settings) = bind(&store, symbol);
            let native = settings.look.live_lane.native_tape && settings.look.overlap_merge;
            assert_eq!(
                native,
                symbol.starts_with("WIN")
                    || (matches!(feed.id.as_str(), "binance" | "binance-futures")
                        && matches!(symbol.as_str(), "BTCUSDT" | "BTCUSDT.p")),
                "{}/{symbol} opens on the native tape: {native}",
                feed.id
            );
            assert!(!settings.look.live_lane.tape_only, "{symbol}");
            assert!(
                !settings.look.volume_dot_ignore_opening_burst_in_scale,
                "{symbol}"
            );
            assert!(!settings.candle_aggression, "{symbol}");
            assert!(!settings.flow_ignore_opening, "{symbol}");
            assert_ne!(binding.source(), AssetSource::Stored, "{symbol}");
        }
    }
}

#[test]
fn returning_to_the_declared_look_forgets_the_filed_entry() {
    let store = memory();
    let (mut btc, opened) = bind(&store, "BTCUSDT");
    let mut edited = opened.clone();
    edited.look.bubbles.max_radius = 30.0;
    assert!(btc.file(edited));
    assert!(store.borrow().get("BTCUSDT").is_some());
    assert!(btc.file(opened));
    assert!(
        store.borrow().get("BTCUSDT").is_none(),
        "a look equal to the declared one is not a setting of the asset's own"
    );
}

#[test]
fn an_unknown_declared_preset_opens_on_the_default_look() {
    let mut config = shipped_config();
    config.feeds[0]
        .symbol_bubble_presets
        .insert("BTCUSDT".to_owned(), "no such preset".to_owned());
    let presets = bubble_presets::embedded();
    let (binding, settings) =
        AssetBinding::bind(memory(), &config, ("binance", "BTCUSDT"), &presets, false);
    assert_eq!(binding.source(), AssetSource::Default);
    assert_eq!(settings.look.name, presets.active);
}

#[test]
fn a_malformed_store_is_reported_rather_than_parsed() {
    assert!(bubble_assets::parse("assets = [").is_err());
    assert!(bubble_assets::validate("version = 1\n").is_ok());
}

#[test]
fn a_win_recording_in_a_btc_tab_is_still_the_mini_index() {
    let config = shipped_config();
    for tab_feed in ["binance", "metatrader-tickmill", "metatrader-b3"] {
        let replayed = config.bubble_asset(tab_feed, "WINV26");
        assert_eq!(replayed.key, "WIN*", "in a {tab_feed} tab");
        assert_eq!(replayed.preset.as_deref(), Some(NATIVE_PRESET));
    }
    let btc = config.bubble_asset("binance", "BTCUSDT");
    assert_eq!(btc.key, "BTCUSDT");
    assert_eq!(btc.preset.as_deref(), Some(BTC_SPOT_PRESET));
    let unknown = config.bubble_asset("binance", "NOSUCHSYMBOL");
    assert_eq!(unknown.key, "NOSUCHSYMBOL");
    assert_eq!(unknown.preset, None);
}

/// Review round 1, finding 9: a key resolves against one declared look,
/// whichever tab shows it — a WDO$N recording in a crypto tab is the B3
/// feed's WDO$N, on that feed's preset.
#[test]
fn one_asset_key_resolves_to_one_declared_look_whichever_feed_shows_it() {
    let config = shipped_config();
    let wdo = config.bubble_asset("binance", "WDO$N");
    assert_eq!(wdo, asset("metatrader-b3", "WDO$N"));
    assert_eq!(wdo.preset.as_deref(), Some("live lane pie"));
    assert_eq!(
        config.bubble_asset("binance", "XAUUSD").preset.as_deref(),
        Some("live lane pie")
    );
}

/// Review round 2, finding 2: the tab's feed decides a symbol it offers, so
/// a family pattern on one venue never captures a same-prefix symbol on
/// another.
#[test]
fn a_pattern_on_one_venue_does_not_capture_another_venues_symbol() {
    let mut config = shipped_config();
    config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "hyperliquid")
        .expect("the Hyperliquid feed")
        .symbol_bubble_presets
        .insert("BTC*".to_owned(), "live lane pie".to_owned());
    let btc = config.bubble_asset("binance", "BTCUSDT");
    assert_eq!(btc.key, "BTCUSDT", "Binance's BTCUSDT is its own asset");
    assert_eq!(btc.preset.as_deref(), Some(BTC_SPOT_PRESET));
    let perp = config.bubble_asset("hyperliquid", "BTC");
    assert_eq!(perp.key, "BTC*");
    assert_eq!(perp.preset.as_deref(), Some("live lane pie"));
}

/// Review round 2, finding 2: a symbol no feed lists — a dated mini dollar
/// roll typed in the source picker, a recording of one — opens on its tab
/// feed's own preset, as it did before assets existed.
#[test]
fn an_unlisted_symbol_opens_on_its_tab_feeds_preset() {
    let config = shipped_config();
    let roll = config.bubble_asset("metatrader-b3", "WDOX26");
    assert_eq!(roll.key, "WDOX26");
    assert_eq!(roll.preset.as_deref(), Some("live lane pie"));
    let silver = config.bubble_asset("metatrader-tickmill", "XAGUSD");
    assert_eq!(silver.preset.as_deref(), Some("live lane pie"));
    assert_eq!(config.bubble_asset("binance", "WDOX26").preset, None);
}

/// Review round 2, finding 2: one key declared on two feeds opens on one
/// look — the first feed's — whichever tab shows it. A feed that switches
/// the asset's mode on decides instead: the mode's look is its own.
#[test]
fn a_key_two_feeds_declare_opens_on_the_first_ones_preset() {
    let mut config = shipped_config();
    let tickmill = config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "metatrader-tickmill")
        .expect("the Tickmill feed");
    tickmill
        .symbol_bubble_presets
        .insert("WIN*".to_owned(), "live lane pie".to_owned());
    for tab_feed in ["metatrader-b3", "metatrader-tickmill", "binance"] {
        let win = config.bubble_asset(tab_feed, "WINV26");
        assert_eq!(win.key, "WIN*");
        assert_eq!(
            win.preset.as_deref(),
            Some("mini index regions"),
            "in a {tab_feed} tab, the mode feed's look"
        );
    }
    config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "metatrader-b3")
        .expect("the B3 feed")
        .symbol_asset_modes
        .clear();
    for tab_feed in ["metatrader-b3", "metatrader-tickmill", "binance"] {
        let win = config.bubble_asset(tab_feed, "WINV26");
        assert_eq!(win.key, "WIN*");
        assert_eq!(
            win.preset.as_deref(),
            Some("live lane pie"),
            "in a {tab_feed} tab"
        );
    }
}

/// Review round 1, finding 2: an edit filed from one view reaches every
/// other view on the asset, and their next filing starts from it instead of
/// writing their stale copy over it.
#[test]
fn an_edit_in_one_view_reaches_another_view_on_the_same_asset() {
    let store = memory();
    let (mut a, opened) = bind(&store, "WIN$N");
    let (mut b, _) = bind(&store, "WINV26");
    let mut edited = opened.clone();
    edited.look.bubbles.max_radius = 31.0;
    a.note_edit();
    assert!(a.file(edited.clone()));
    assert_eq!(a.adoption(), None, "a view does not adopt its own filing");

    let adoption = b.adoption().expect("B learns A's edit");
    assert!(!adoption.lane_moved && !adoption.dropped);
    let adopted = adoption.settings;
    assert_eq!(adopted, edited);
    assert_eq!(b.adoption(), None, "once");

    let mut later = adopted.clone();
    later.flow_ignore_opening = true;
    b.note_edit();
    assert!(b.file(later));
    let stored = store.borrow().get("WIN*").cloned().expect("stored");
    assert_eq!(
        stored.look.bubbles.max_radius, 31.0,
        "A's edit survives B's"
    );
    assert!(stored.flow_ignore_opening);
}

/// Review round 1, finding 1: wheeling or dragging the tape's window moves
/// the view, not the asset; a window set on purpose is the asset's.
#[test]
fn a_navigated_lane_window_is_not_filed_but_a_chosen_one_is() {
    let store = memory();
    let (mut win, opened) = bind(&store, "WINV26");
    let mut zoomed = opened.clone();
    zoomed.look.live_lane.window = LaneWindow::Fixed { ms: 3_000 };
    assert!(!win.file(zoomed.clone()), "navigation alone files nothing");
    assert_ne!(win.source(), AssetSource::Stored);

    // A real edit on a zoomed view files the edit, not the zoom.
    zoomed.look.bubbles.max_radius = 29.0;
    win.note_edit();
    assert!(win.file(zoomed.clone()));
    let stored = store.borrow().get("WIN*").cloned().expect("stored");
    assert_eq!(stored.look.live_lane.window, opened.look.live_lane.window);

    win.note_lane_set();
    assert!(win.file(zoomed));
    let stored = store.borrow().get("WIN*").cloned().expect("stored");
    assert_eq!(
        stored.look.live_lane.window,
        LaneWindow::Fixed { ms: 3_000 }
    );
}

/// Review round 1, finding 3: a reloaded presets file is the baseline the
/// next filing compares against, so the asset is not pinned with an entry
/// equal to its new declared look.
#[test]
fn a_reloaded_declared_look_is_the_new_baseline() {
    let store = memory();
    let mut presets = bubble_presets::embedded();
    let (mut btc, opened) = bind_with(&store, "BTCUSDT", &presets);
    let mut tuned = opened.clone();
    tuned.look.bubbles.max_radius = 33.0;

    // The presets file now declares exactly that look for this asset.
    let preset = presets
        .presets
        .iter_mut()
        .find(|preset| preset.name == opened.look.name)
        .expect("the asset's declared preset");
    preset.bubbles.max_radius = 33.0;
    let (fresh, _) = btc
        .refresh_declared(&presets)
        .expect("an untuned asset follows its declared look");
    assert_eq!(fresh, tuned);
    btc.note_edit();
    assert!(!btc.file(tuned), "the new declared look is not an edit");
    assert!(store.borrow().get("BTCUSDT").is_none());
    assert_eq!(btc.source(), AssetSource::Declared);
}

/// Review round 1, finding 4: a reload leaves an asset's own settings alone.
#[test]
fn a_reload_does_not_dress_a_tuned_asset() {
    let store = memory();
    let presets = bubble_presets::embedded();
    let (mut win, opened) = bind_with(&store, "WINV26", &presets);
    let mut tuned = opened;
    tuned.flow_ignore_opening = true;
    win.note_edit();
    assert!(win.file(tuned));
    assert_eq!(win.refresh_declared(&presets), None);
    assert_eq!(win.source(), AssetSource::Stored);
}

/// Review round 1, finding 5: a look other assets open on is not saved over
/// from one asset.
#[test]
fn the_looks_assets_open_on_are_refused_as_save_names() {
    let store = memory();
    let presets = bubble_presets::embedded();
    let (btc, _) = bind_with(&store, "BTCUSDT", &presets);
    for name in [presets.active.as_str(), NATIVE_PRESET, "live lane pie"] {
        assert!(btc.refuses_preset_name(name, &presets).is_some(), "{name}");
    }
    assert_eq!(btc.refuses_preset_name("my btc", &presets), None);
}

/// A view on BTCUSDT wearing `settings`, as a tab opens on them.
fn btc_screen(settings: &AssetBubbles) -> (BubbleLook, HeatmapConfig) {
    let mut config = HeatmapConfig::default();
    let loaded = (bubble_presets::embedded(), PresetSource::Embedded, None);
    let mut look = BubbleLook::open(loaded, &mut config);
    look.wear(settings, false, &mut config);
    (look, config)
}

/// A store written while a closed side was filed apart from its view: what an old
/// `fold` key under an asset's entry reads as.
fn old_store_entry(fold: &str) -> AssetBubbles {
    let (_, declared) = bind(&memory(), "BTCUSDT");
    let mut file = AssetBubblesFile::default();
    let off = AssetBubbles {
        bubbles: false,
        ..declared.clone()
    };
    assert!(file.record("BTCUSDT", &off, &declared));
    let text = bubble_assets::render(&file).expect("the store renders");
    let old = text.replacen(
        "[assets.BTCUSDT]\n",
        &format!("[assets.BTCUSDT]\nfold = \"{fold}\"\n"),
        1,
    );
    assert_ne!(old, text, "the entry's table is where it is expected");
    let reloaded = bubble_assets::parse(&old).expect("an old store reads");
    reloaded.get("BTCUSDT").expect("its entry").clone()
}

/// An old store's closed candles open as tape only, at the share it saved; written
/// again the store says the view and no fold, and it reads as the same side.
#[test]
fn an_old_stores_candle_fold_opens_as_tape_only_and_is_never_written_back() {
    let stored = old_store_entry("candles");
    assert_eq!(stored.fold, LaneFold::Candles);
    let saved_share = stored.look.live_lane.width_share;
    let (look, reopened) = btc_screen(&stored);
    assert_eq!(reopened.live_lane.view(), LaneView::TapeOnly);
    assert_eq!(reopened.live_lane.width_share, saved_share);

    let settings = look.settings(&reopened, stored.candle_aggression, false);
    assert_eq!(settings.fold, LaneFold::Open, "no fold left to file");
    let (_, declared) = bind(&memory(), "BTCUSDT");
    let mut file = AssetBubblesFile::default();
    assert!(file.record("BTCUSDT", &settings, &declared));
    let text = bubble_assets::render(&file).expect("the store renders");
    assert!(!text.contains("\nfold ="), "{text}");
    let reread = bubble_assets::parse(&text).expect("the store reads");
    let (_, again) = btc_screen(reread.get("BTCUSDT").expect("filed"));
    assert_eq!(again.live_lane.view(), LaneView::TapeOnly, "the same side");
}

/// An old store's closed tape waits for the layer restore that owns the tape's
/// switch, then closes once, where the view is beside; a view other than beside
/// won on screen and wins here.
#[test]
fn an_old_stores_tape_fold_closes_the_tape_once_and_only_beside() {
    let stored = old_store_entry("tape");
    assert_eq!(stored.fold, LaneFold::Tape);
    let (mut look, config) = btc_screen(&stored);
    assert_eq!(config.live_lane.view(), LaneView::Beside, "not worn yet");
    assert!(look.take_old_tape_fold(&config));
    assert!(!look.take_old_tape_fold(&config), "asked once");

    let (mut look, mut alone) = btc_screen(&stored);
    alone.live_lane.tape_only = true;
    assert!(!look.take_old_tape_fold(&alone), "tape only won");

    let settings = look.settings(&alone, stored.candle_aggression, false);
    assert_eq!(settings.fold, LaneFold::Open);
}

/// An old store's closed tape stays in the asset's file until the Tape layer can
/// save it, so a tab no mask has observed never loses it; once settled it is gone,
/// and only a tape still closed asks for the layer to be saved.
#[test]
fn an_old_tape_fold_stays_filed_until_the_layer_mask_has_seen_it() {
    let stored = old_store_entry("tape");
    let (mut look, mut config) = btc_screen(&stored);
    let filed = |look: &BubbleLook, config: &HeatmapConfig| {
        look.settings(config, stored.candle_aggression, false).fold
    };
    assert_eq!(filed(&look, &config), LaneFold::Tape, "waiting");
    assert!(!look.settle_old_tape_fold(&config), "nothing closed yet");

    assert!(look.take_old_tape_fold(&config));
    config.live_lane.set_view(LaneView::CandlesOnly);
    assert_eq!(filed(&look, &config), LaneFold::Tape, "closed, still kept");
    let text = toml::to_string(&look.settings(&config, stored.candle_aggression, false));
    assert!(text.unwrap().contains("fold = \"tape\""), "and written");

    assert!(look.settle_old_tape_fold(&config), "the mask saves it");
    assert_eq!(filed(&look, &config), LaneFold::Open, "gone from the file");
    assert!(!look.settle_old_tape_fold(&config), "once");

    // Reopened by a word before the mask saw it: nothing is left to carry.
    let (mut look, config) = btc_screen(&stored);
    assert!(look.take_old_tape_fold(&config));
    assert_eq!(config.live_lane.view(), LaneView::Beside, "a word kept it");
    assert_eq!(filed(&look, &config), LaneFold::Open);
    assert!(!look.settle_old_tape_fold(&config));
}

const OLD_PRESET: &str = "active = \"old\"\n\n[[presets]]\nname = \"old\"\n\
                          [presets.live_lane]\nwidth_share = 0.42\nfold = \"tape\"\n\n\
                          [[presets]]\nname = \"off\"\n[presets.live_lane]\nenabled = false\n";

/// A preset's old closed tape closes it once, at the first apply after the file is
/// read, and the file saves without it; picks after that never close it again, and
/// a hand-edited switch never did.
#[test]
fn a_presets_old_tape_fold_closes_once_and_a_pick_after_reopening_does_not() {
    let presets = bubble_presets::parse(OLD_PRESET).expect("reads");
    assert!(
        !toml::to_string(&presets).unwrap().contains("fold ="),
        "saved without"
    );

    // Opened on it: closed, and waiting for the layer restore to have spoken.
    let mut config = HeatmapConfig::default();
    let mut look = BubbleLook::open((presets.clone(), PresetSource::Embedded, None), &mut config);
    assert_eq!(config.live_lane.view(), LaneView::CandlesOnly);
    assert_eq!(config.live_lane.width_share, 0.42);
    config.live_lane.restore_enabled(true);
    assert!(look.take_old_tape_fold(&config), "it survives the restore");
    assert!(look.apply_preset("old", &mut config, false));
    assert!(
        config.live_lane.enabled,
        "spent: the trader's reopened tape"
    );

    // Picked first: closed by that pick, and only that pick.
    let mut unmarked = presets.clone();
    unmarked.active.clear();
    let mut config = HeatmapConfig::default();
    let mut look = BubbleLook::open((unmarked, PresetSource::Embedded, None), &mut config);
    assert!(config.live_lane.enabled);
    assert!(look.apply_preset("old", &mut config, false));
    assert_eq!(config.live_lane.view(), LaneView::CandlesOnly, "the first");
    assert!(config.live_lane.set_view(LaneView::Beside), "reopened");
    assert!(look.apply_preset("old", &mut config, false));
    assert_eq!(config.live_lane.view(), LaneView::Beside, "not again");

    // A hand-edited switch is ignored, at every pick.
    assert!(look.apply_preset("off", &mut config, false));
    assert_eq!(config.live_lane.view(), LaneView::Beside, "no old fold");
}

/// A fold's own view, tape only won over a candle fold saved with it too: the view
/// the look already shows is not moved.
#[test]
fn an_old_stores_fold_leaves_a_view_other_than_beside() {
    let mut stored = old_store_entry("candles");
    stored.look.live_lane.enabled = true;
    stored.look.live_lane.tape_only = true;
    let (_, config) = btc_screen(&stored);
    assert_eq!(config.live_lane.view(), LaneView::TapeOnly);
}

/// What the market holds is not the asset's: a held pane files what it would
/// have filed open, and the setting is filed only when it is off.
#[test]
fn a_market_hold_is_never_filed_and_the_setting_is() {
    let (_, declared) = bind(&memory(), "BTCUSDT");
    let (look, mut config) = btc_screen(&declared);
    config.live_lane.market_hold.held = true;
    assert_eq!(config.live_lane.shown_view(), LaneView::CandlesOnly);
    let held = look.settings(&config, declared.candle_aggression, false);
    assert_eq!(held, declared, "the store cannot tell it was held");
    let mut file = AssetBubblesFile::default();
    assert!(!file.record("BTCUSDT", &held, &declared), "nothing to file");

    let (look, mut config) = btc_screen(&declared);
    config.live_lane.fold_market_closed = false;
    let off = look.settings(&config, declared.candle_aggression, false);
    assert_ne!(off, declared);
    let mut file = AssetBubblesFile::default();
    assert!(file.record("BTCUSDT", &off, &declared));
    let text = bubble_assets::render(&file).expect("the store renders");
    let reloaded = bubble_assets::parse(&text).expect("the store reloads");
    let (_, reopened) = btc_screen(reloaded.get("BTCUSDT").expect("filed"));
    assert!(!reopened.live_lane.fold_market_closed, "survives a restart");
}

#[test]
fn a_stored_asset_without_a_fold_reopens_open() {
    let (_, declared) = bind(&memory(), "BTCUSDT");
    // An entry as a store from before the fold reads: the bubbles switched
    // off, and no fold anywhere in the file.
    let off = AssetBubbles {
        bubbles: false,
        ..declared.clone()
    };
    let mut file = AssetBubblesFile::default();
    assert!(file.record("BTCUSDT", &off, &declared));
    let text = bubble_assets::render(&file).expect("the store renders");
    assert!(!text.contains("fold"), "{text}");
    let file = bubble_assets::parse(&text).expect("an old store reads");
    let stored = file.get("BTCUSDT").expect("its entry");
    let (mut look, mut screen) = btc_screen(&declared);
    screen.live_lane.set_view(LaneView::CandlesOnly);
    look.wear(stored, false, &mut screen);
    assert_eq!(screen.live_lane.view(), LaneView::CandlesOnly, "not moved");
    assert_eq!(stored.fold, LaneFold::Open, "no fold reads open");
}
