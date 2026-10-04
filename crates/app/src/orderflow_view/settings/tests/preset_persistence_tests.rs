//! Saving a look on one asset writes the preset, never the presets file's
//! default: the look every undeclared asset opens on stays the file's own.
use super::*;
use crate::bubble_presets::{BubblePresetFile, embedded, parse};
use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore};
use quantick_stores::bubble_assets::AssetSource;
use std::path::PathBuf;

fn from_document(document: BubblePresetFile) -> OrderflowView {
    let mut view = OrderflowView::new("TEST");
    // The construction stages, without sharing a process-global presets file.
    view.presets = document;
    let active = view.presets.active.clone();
    assert!(view.apply_preset(&active));
    view
}

fn save_document(view: &mut OrderflowView) -> BubblePresetFile {
    let mut stored = None;
    view.save_preset_with(|document| {
        let encoded = toml::to_string(document).expect("presets serialize");
        stored = Some(parse(&encoded).expect("saved presets reload"));
        Ok(PathBuf::from("isolated-test-bubbles.toml"))
    });
    stored.expect("the save action handed a document to its writer")
}

fn appearance(view: &OrderflowView) -> BubblePreset {
    BubblePreset::capture(&view.look_name, &view.config)
}

fn edited_win() -> (OrderflowView, BubblePreset) {
    let mut view = from_document(embedded());
    assert!(view.apply_preset("mini index regions"));
    assert!(view.set_ignore_opening_burst_in_scale(true));
    let win = appearance(&view);
    (view, win)
}

#[test]
fn saving_the_win_look_keeps_the_files_default_for_every_other_asset() {
    let (mut view, win) = edited_win();
    let stored = save_document(&mut view);
    assert_eq!(
        stored.active,
        embedded().active,
        "the file's default is untouched"
    );
    assert_eq!(stored.get("mini index regions"), Some(&win));
    assert_eq!(appearance(&view), win, "the WIN pane stays put");

    let fresh = from_document(stored);
    assert_eq!(fresh.look_name, embedded().active);
    assert!(!fresh.config.native_tape());
    assert!(!fresh.config.volume_dots.enabled);
    assert!(!fresh.config.volume_dots.ignore_opening_burst_in_scale);
}

#[test]
fn a_new_name_is_saved_and_worn_without_becoming_the_default() {
    let (mut view, _) = edited_win();
    view.preset_name_draft = "my win".to_owned();
    let stored = save_document(&mut view);
    assert_eq!(view.look_name, "my win");
    assert!(stored.get("my win").is_some());
    assert_eq!(stored.active, embedded().active);
}

#[test]
fn reloading_reapplies_the_saved_look_on_screen() {
    let (mut view, win) = edited_win();
    let stored = save_document(&mut view);
    assert!(view.set_ignore_opening_burst_in_scale(false));
    view.reload_presets_from((stored, PresetSource::Embedded, None));
    assert_eq!(appearance(&view), win, "reload discards unsaved edits");
}

#[test]
fn a_failed_save_keeps_the_view_and_says_so() {
    let (mut view, win) = edited_win();
    view.save_preset_with(|document| {
        assert_eq!(document.active, embedded().active);
        Err("test writer refused the save".to_owned())
    });
    assert_eq!(appearance(&view), win);
    assert!(
        view.preset_status
            .as_deref()
            .unwrap()
            .starts_with("not saved")
    );
}

/// A view on the mini index, bound to its asset as a tab binds it.
fn bound_win() -> OrderflowView {
    let mut view = from_document(embedded());
    let config: crate::config::AppConfig =
        toml::from_str(include_str!("../../../../config/feeds.toml")).expect("shipped feeds");
    let store = AssetBubblesStore::default().shared();
    let (binding, settings) = AssetBinding::bind(store, &config, "WINV26", &view.presets, false);
    view.bind_asset(binding, &settings);
    view
}

/// Review round 1, finding 4: reloading the presets keeps the settings an
/// asset made its own, and files nothing over them.
#[test]
fn reloading_keeps_the_settings_an_asset_made_its_own() {
    let mut view = bound_win();
    assert!(view.set_ignore_opening_burst_in_scale(true));
    assert_eq!(view.sync_asset(false), None);
    let edited = appearance(&view);
    view.reload_presets_from((embedded(), PresetSource::Embedded, None));
    assert_eq!(appearance(&view), edited, "WIN's edit is still on screen");
    view.sync_asset(false);
    assert_eq!(view.asset().expect("bound").source(), AssetSource::Stored);
    let status = view.preset_status.clone().expect("a status");
    assert!(status.contains("keeps its own settings"), "{status}");
}

/// Review round 1, finding 5: a look other assets open on is not saved over
/// — nor removed — from one asset's panel.
#[test]
fn a_look_other_assets_open_on_is_not_saved_over_from_one_asset() {
    let mut view = bound_win();
    assert!(view.set_ignore_opening_burst_in_scale(true));
    let active = embedded().active;
    for name in ["mini index regions", active.as_str(), "live lane pie"] {
        view.preset_name_draft = name.to_owned();
        view.save_preset_with(|_| panic!("'{name}' reached the writer"));
        let status = view.preset_status.clone().expect("a status");
        assert!(status.contains("other assets open on"), "{status}");
    }
    view.preset_name_draft = "my win".to_owned();
    assert!(save_document(&mut view).get("my win").is_some());
}

/// Review round 1, finding 8: the panel says what happens — restored
/// defaults are kept for the asset, not left unsaved.
#[test]
fn restored_defaults_are_kept_for_the_asset_and_say_so() {
    let mut view = bound_win();
    view.reset_bubble_visuals();
    let status = view.preset_status.clone().expect("a status");
    assert!(!status.contains("not saved"), "{status}");
    view.sync_asset(false);
    assert_eq!(view.asset().expect("bound").source(), AssetSource::Stored);
}
