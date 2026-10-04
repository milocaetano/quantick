//! Saving a look on one asset writes the preset, never the presets file's
//! default: the look every undeclared asset opens on stays the file's own.
use super::*;
use crate::bubble_presets::{BubblePresetFile, embedded, parse};
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
