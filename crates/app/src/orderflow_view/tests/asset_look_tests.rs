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

#[test]
fn whole_preset_api_matches_picker_navigation_visibility_and_save_scope() {
    let store = AssetBubblesStore::default().shared();
    let mut api = bound_to(&store, ("binance-futures", "BTCUSDT.p"));
    let mut other = bound_to(&store, ("binance", "BTCUSDT"));
    let unrelated = appearance(&other);
    let mut ui = bound_to(
        &AssetBubblesStore::default().shared(),
        ("binance-futures", "BTCUSDT.p"),
    );
    let name = "mini index regions";
    for view in [&mut api, &mut ui] {
        view.edit_config(|config| {
            config.live_lane.width_share = 0.4;
            config.show_aggressions = false;
            config.live_lane.stable_size = false;
        });
    }
    api.set_save_asset_changes(false).unwrap();
    let before = ui.config.clone();
    assert!(ui.apply_preset(name));
    ui.commit_config_changes(before);
    assert!(api.apply_named_preset(name).unwrap());
    assert_eq!(
        api.config, ui.config,
        "same picker operation and worker commit"
    );
    assert_eq!(
        api.look.lane_window_setting(),
        ui.look.lane_window_setting()
    );
    assert!(
        !api.config.show_aggressions,
        "a whole look preserves layer visibility"
    );
    let chosen = appearance(&api);
    api.sync_asset(false);
    assert_eq!(
        appearance(&api),
        chosen,
        "save-off changes stay on this view"
    );
    other.sync_asset(false);
    assert_eq!(appearance(&other), unrelated);
    let reopened = bound_to(&store, ("binance-futures", "BTCUSDT.p"));
    assert_ne!(
        appearance(&reopened),
        chosen,
        "save-off does not replace the stored asset look"
    );
    api.set_save_asset_changes(true).unwrap();
    api.sync_asset(false);
    let restored = bound_to(&store, ("binance-futures", "BTCUSDT.p"));
    assert_eq!(
        appearance(&restored),
        chosen,
        "save-on follows the existing asset persistence path"
    );
    assert!(!api.apply_named_preset(name).unwrap());
}

#[test]
fn whole_preset_api_commits_the_changed_configuration_to_the_actual_worker() {
    let mut view = bound_to(
        &AssetBubblesStore::default().shared(),
        ("binance-futures", "BTCUSDT.p"),
    );
    view.edit_config(|config| config.live_lane.stable_size = false);
    fn revision(view: &mut OrderflowView) -> u64 {
        view.flush_for_test();
        view.cached_health().config_revision
    }
    let before = revision(&mut view);
    assert!(
        view.apply_named_preset("Binance BTC perpetual spheres")
            .unwrap()
    );
    assert!(
        revision(&mut view) > before,
        "API commit reaches the worker"
    );
    let after = revision(&mut view);
    assert!(
        !view
            .apply_named_preset("Binance BTC perpetual spheres")
            .unwrap()
    );
    assert_eq!(
        revision(&mut view),
        after,
        "a no-op sends no repeated config effect"
    );
}

fn ignore_opening(view: &mut OrderflowView, on: bool) -> bool {
    view.edit_config(|config| config.volume_dots.ignore_opening_burst_in_scale = on)
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
    let active = "Binance BTC Spot spheres";
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
        AssetSource::Declared,
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

/// Review #19/#29: the sphere picker asks the rule tick.set asks. A mode
/// asset offers its spheres on a tick chart and not on a volume chart; an
/// asset without the mode offers none, but spheres it already wears can be
/// switched off there.
#[test]
fn the_sphere_picker_follows_the_tick_sphere_rule() {
    let texts = |view: &mut OrderflowView, tick_chart: bool| -> String {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                view.draw_bubbles_tab(ui, tick_chart);
            });
        });
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let store = AssetBubblesStore::default().shared();
    let mut btc = bound_to(&store, ("binance", "BTCUSDT"));
    let spheres = btc.config.volume_dots.tick_bubbles;
    assert!(spheres.is_some(), "the declared look wears spheres");
    assert!(texts(&mut btc, true).contains("quantity in BTC"));
    assert!(
        !texts(&mut btc, false).contains("Tick chart spheres"),
        "a volume chart offers no spheres"
    );
    let mut eth = bound_to(&store, ("binance", "ETHUSDT"));
    assert!(!texts(&mut eth, true).contains("Tick chart spheres"));
    eth.edit_config(|config| config.volume_dots.tick_bubbles = spheres);
    let offered = texts(&mut eth, true);
    assert!(offered.contains("Tick chart spheres"), "{offered}");
    assert!(!offered.contains("quantity in"), "{offered}");
    assert_eq!(eth.apply_tick_preset(None), Ok(true));
    assert_eq!(eth.config.volume_dots.tick_bubbles, None);
}
