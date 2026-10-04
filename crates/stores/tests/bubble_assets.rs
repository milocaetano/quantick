//! Each asset keeps its own bubble settings: the config declares which look an
//! asset opens on, the trader's changes are filed under that asset alone, and
//! a filed asset reopens exactly as it was left.

use quantick_orderflow::{HeatmapConfig, LaneWindow};
use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore, SharedAssetBubbles};
use quantick_stores::bubble_assets::{self, AssetBubbles, AssetSource};
use quantick_stores::bubble_presets::{self, BubblePresetFile};
use quantick_stores::config::{AppConfig, BubbleAsset};

const NATIVE_PRESET: &str = "mini index regions";

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
    AssetBinding::bind(store.clone(), &shipped_config(), symbol, presets, false)
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
    assert_eq!(btc.preset, None);
}

#[test]
fn only_the_win_family_opens_on_the_native_tape() {
    let store = memory();
    let config = shipped_config();
    for feed in &config.feeds {
        for symbol in &feed.symbols {
            let (binding, settings) = bind(&store, symbol);
            let native = settings.look.live_lane.native_tape && settings.look.overlap_merge;
            assert_eq!(
                native,
                symbol.starts_with("WIN"),
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
    let (binding, settings) = AssetBinding::bind(memory(), &config, "BTCUSDT", &presets, false);
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
    let replayed = config.bubble_asset("WINV26");
    assert_eq!(replayed.key, "WIN*");
    assert_eq!(replayed.preset.as_deref(), Some(NATIVE_PRESET));
    let btc = config.bubble_asset("BTCUSDT");
    assert_eq!(btc.key, "BTCUSDT");
    assert_eq!(btc.preset, None);
    let unknown = config.bubble_asset("NOSUCHSYMBOL");
    assert_eq!(unknown.key, "NOSUCHSYMBOL");
    assert_eq!(unknown.preset, None);
}

/// Review round 1, finding 9: a key resolves against one declared look,
/// whichever tab shows it — a WDO$N recording in a crypto tab is the B3
/// feed's WDO$N, on that feed's preset.
#[test]
fn one_asset_key_resolves_to_one_declared_look_whichever_feed_shows_it() {
    let config = shipped_config();
    let wdo = config.bubble_asset("WDO$N");
    assert_eq!(wdo, asset("metatrader-b3", "WDO$N"));
    assert_eq!(wdo.preset.as_deref(), Some("live lane pie"));
    assert_eq!(
        config.bubble_asset("XAUUSD").preset.as_deref(),
        Some("live lane pie")
    );
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
    a.note_change();
    assert!(a.file(edited.clone()));
    assert_eq!(a.adoption(), None, "a view does not adopt its own filing");

    let (adopted, lane_moved) = b.adoption().expect("B learns A's edit");
    assert_eq!(adopted, edited);
    assert!(!lane_moved);
    assert_eq!(b.adoption(), None, "once");

    let mut later = adopted.clone();
    later.flow_ignore_opening = true;
    b.note_change();
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
    win.note_change();
    assert!(win.file(zoomed.clone()));
    let stored = store.borrow().get("WIN*").cloned().expect("stored");
    assert_eq!(stored.look.live_lane.window, opened.look.live_lane.window);

    let before = HeatmapConfig::default();
    let mut after = before.clone();
    after.live_lane.window = LaneWindow::Fixed { ms: 3_000 };
    win.note_edit(&before, &after);
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

    // The presets file now declares exactly that look as the default.
    let active = presets.active.clone();
    let preset = presets
        .presets
        .iter_mut()
        .find(|preset| preset.name == active)
        .expect("the active preset");
    preset.bubbles.max_radius = 33.0;
    let fresh = btc
        .refresh_declared(&presets)
        .expect("an untuned asset follows its declared look");
    assert_eq!(fresh, tuned);
    btc.note_change();
    assert!(!btc.file(tuned), "the new declared look is not an edit");
    assert!(store.borrow().get("BTCUSDT").is_none());
    assert_eq!(btc.source(), AssetSource::Default);
}

/// Review round 1, finding 4: a reload leaves an asset's own settings alone.
#[test]
fn a_reload_does_not_dress_a_tuned_asset() {
    let store = memory();
    let presets = bubble_presets::embedded();
    let (mut win, opened) = bind_with(&store, "WINV26", &presets);
    let mut tuned = opened;
    tuned.flow_ignore_opening = true;
    win.note_change();
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
