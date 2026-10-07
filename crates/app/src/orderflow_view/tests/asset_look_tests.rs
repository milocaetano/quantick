//! A view wears its asset's look through the headless owner
//! ([`quantick_stores::bubble_look::BubbleLook`]): presets reloaded in one
//! tab reach every tab on an untuned asset, and an asset's own settings
//! survive a reload. The owner's save, delete and refusal rules are tested
//! beside it.
use super::*;
use crate::bubble_presets::{BubblePreset, BubblePresetFile, PresetSource, embedded};
use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore, SharedAssetBubbles};
use quantick_stores::bubble_assets::AssetSource;

fn from_document(document: BubblePresetFile) -> OrderflowView {
    let mut view = OrderflowView::new("TEST");
    // The construction stages, without sharing a process-global presets file.
    view.look = BubbleLook::open(
        (document, PresetSource::Embedded, None),
        &mut HeatmapConfig::default(),
    );
    let active = view.look.presets().active.clone();
    assert!(view.apply_preset(&active));
    view
}

fn appearance(view: &OrderflowView) -> BubblePreset {
    BubblePreset::capture(view.look.look_name(), &view.config)
}

fn ignore_opening(view: &mut OrderflowView, on: bool) -> bool {
    view.edit_config(|config| config.set_ignore_opening_burst_in_scale(on))
}

/// A view on the mini index, bound to its asset as a tab binds it.
fn bound_win() -> OrderflowView {
    bound_to(
        &AssetBubblesStore::default().shared(),
        ("metatrader-b3", "WINV26"),
    )
}

/// A view on `market`, bound to its asset in `store` as a tab binds it.
fn bound_to(store: &SharedAssetBubbles, market: (&str, &str)) -> OrderflowView {
    let mut view = from_document(embedded());
    let config: crate::config::AppConfig =
        toml::from_str(include_str!("../../../config/feeds.toml")).expect("shipped feeds");
    let (binding, settings) =
        AssetBinding::bind(store.clone(), &config, market, view.look.presets(), false);
    view.bind_asset(binding, &settings);
    view
}

/// Review round 2, finding 1: presets reloaded in one tab reach every tab.
/// Another view on the same untuned asset wears the new look and compares
/// its next edit against it, so that edit does not pin the old look.
#[test]
fn a_presets_reload_in_one_tab_dresses_every_tab_on_an_untuned_asset() {
    let store = AssetBubblesStore::default().shared();
    let mut here = bound_to(&store, ("binance", "BTCUSDT"));
    let mut there = bound_to(&store, ("binance", "BTCUSDT"));
    let mut reloaded = embedded();
    let active = reloaded.active.clone();
    let look = reloaded
        .presets
        .iter_mut()
        .find(|preset| preset.name == active)
        .expect("the active preset");
    look.bubbles.max_radius = 35.0;
    here.reload_presets_from((reloaded.clone(), PresetSource::Embedded, None));
    assert_eq!(appearance(&here).bubbles.max_radius, 35.0);

    assert_eq!(there.sync_asset(false), None);
    assert_eq!(
        appearance(&there).bubbles.max_radius,
        35.0,
        "the other tab follows"
    );
    assert_eq!(
        there.look.presets(),
        &reloaded,
        "and holds the reloaded presets"
    );
    assert!(ignore_opening(&mut there, true));
    assert!(ignore_opening(&mut there, false));
    there.sync_asset(false);
    assert_eq!(
        there.look.asset().expect("bound").source(),
        AssetSource::Default,
        "an edit undone on the new look pins nothing"
    );
}

/// Review round 1, finding 4: reloading the presets keeps the settings an
/// asset made its own, and files nothing over them.
#[test]
fn reloading_keeps_the_settings_an_asset_made_its_own() {
    let mut view = bound_win();
    assert!(ignore_opening(&mut view, true));
    assert_eq!(view.sync_asset(false), None);
    let edited = appearance(&view);
    view.reload_presets_from((embedded(), PresetSource::Embedded, None));
    assert_eq!(appearance(&view), edited, "WIN's edit is still on screen");
    view.sync_asset(false);
    assert_eq!(
        view.look.asset().expect("bound").source(),
        AssetSource::Stored
    );
    let status = view.look.status().expect("a status");
    assert!(status.contains("keeps its own settings"), "{status}");
}

/// Review round 1, finding 8: the panel says what happens — restored
/// defaults are kept for the asset, not left unsaved.
#[test]
fn restored_defaults_are_kept_for_the_asset_and_say_so() {
    let mut view = bound_win();
    view.press_reset_bubble_visuals_for_test();
    let status = view.look.status().expect("a status");
    assert!(!status.contains("not saved"), "{status}");
    view.sync_asset(false);
    assert_eq!(
        view.look.asset().expect("bound").source(),
        AssetSource::Stored
    );
}
