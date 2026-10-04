//! Each asset keeps its own bubble settings: the config declares which look an
//! asset opens on, the trader's changes are filed under that asset alone, and
//! a filed asset reopens exactly as it was left.

use quantick_stores::bubble_assets::{self, AssetBubblesFile, AssetSource, AssetTrack};
use quantick_stores::bubble_presets;
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
    let presets = bubble_presets::embedded();
    let store = AssetBubblesFile::default();
    let config = shipped_config();
    for feed in &config.feeds {
        for symbol in &feed.symbols {
            let (track, settings) =
                AssetTrack::resolve(feed.bubble_asset(symbol), &presets, &store, false);
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
            assert_ne!(track.source(), AssetSource::Stored, "{symbol}");
        }
    }
}

#[test]
fn a_win_edit_is_filed_for_win_alone_and_survives_a_restart() {
    let presets = bubble_presets::embedded();
    let mut store = AssetBubblesFile::default();
    let (mut win, opened) =
        AssetTrack::resolve(asset("metatrader-b3", "WINV26"), &presets, &store, false);
    assert_eq!(win.source(), AssetSource::Declared);
    assert_eq!(opened.look.name, NATIVE_PRESET);

    let mut edited = opened.clone();
    edited.candle_aggression = true;
    edited.flow_ignore_opening = true;
    edited.look.volume_dot_ignore_opening_burst_in_scale = true;
    edited.look.live_lane.tape_only = true;
    assert!(win.file(&edited, &mut store), "an edit is filed");
    assert!(
        !win.file(&edited, &mut store),
        "an unchanged look files nothing"
    );

    // A restart reads the file back.
    let text = bubble_assets::render(&store).expect("the store renders");
    let reread = bubble_assets::parse(&text).expect("the store reparses");
    assert_eq!(reread, store);
    let (roll, restored) =
        AssetTrack::resolve(asset("metatrader-b3", "WINZ26"), &presets, &reread, false);
    assert_eq!(roll.source(), AssetSource::Stored);
    assert_eq!(
        restored, edited,
        "the next WIN contract opens as WIN was left"
    );

    // BTC never saw any of it.
    let (btc, settings) =
        AssetTrack::resolve(asset("binance", "BTCUSDT"), &presets, &reread, false);
    assert_eq!(btc.source(), AssetSource::Default);
    assert_eq!(settings.look.name, presets.active);
    assert!(!settings.candle_aggression && !settings.flow_ignore_opening);
    assert!(!settings.look.live_lane.tape_only && !settings.look.live_lane.native_tape);
}

#[test]
fn returning_to_the_declared_look_forgets_the_filed_entry() {
    let presets = bubble_presets::embedded();
    let mut store = AssetBubblesFile::default();
    let (mut btc, opened) =
        AssetTrack::resolve(asset("binance", "BTCUSDT"), &presets, &store, false);
    let mut edited = opened.clone();
    edited.look.bubbles.max_radius = 30.0;
    assert!(btc.file(&edited, &mut store));
    assert!(store.get("BTCUSDT").is_some());
    assert!(btc.file(&opened, &mut store));
    assert!(
        store.get("BTCUSDT").is_none(),
        "a look equal to the declared one is not a setting of the asset's own"
    );
}

#[test]
fn an_unknown_declared_preset_opens_on_the_default_look() {
    let presets = bubble_presets::embedded();
    let ghost = BubbleAsset {
        key: "GHOST".to_owned(),
        preset: Some("no such preset".to_owned()),
    };
    let (track, settings) =
        AssetTrack::resolve(ghost, &presets, &AssetBubblesFile::default(), false);
    assert_eq!(track.source(), AssetSource::Default);
    assert_eq!(settings.look.name, presets.active);
}

#[test]
fn a_malformed_store_is_reported_rather_than_parsed() {
    assert!(bubble_assets::parse("assets = [").is_err());
    assert!(bubble_assets::validate("version = 1\n").is_ok());
}
