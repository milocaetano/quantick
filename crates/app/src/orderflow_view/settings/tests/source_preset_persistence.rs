//! Saving a source-owned WIN look must not change a fresh crypto tab's default.
use super::*;
use crate::bubble_presets::{BubblePresetFile, embedded, parse};
use quantick_orderflow::LaneWindow;
use std::path::PathBuf;

fn from_document(document: BubblePresetFile, source_preset: Option<&str>) -> OrderflowView {
    let mut view = OrderflowView::new("TEST");
    // Use the same loaded-default and subsequent source-selection stages as
    // construction, without sharing a process-global preset file with tests.
    view.presets = document;
    let active = view.presets.active.clone();
    assert!(view.apply_preset(&active));
    assert!(view.apply_source_preset(source_preset));
    view
}

fn crypto_view() -> (OrderflowView, BubblePreset) {
    let mut view = from_document(embedded(), None);
    assert!(view.apply_preset("dense tape"));
    view.config.bubbles.max_radius = 31.0;
    view.config.live_lane.window = LaneWindow::Fixed { ms: 17_000 };
    let previous = BubblePreset::capture("saved crypto", &view.config);
    view.presets.upsert(previous.clone());
    assert!(view.apply_preset(&previous.name));
    (view, previous)
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
    BubblePreset::capture(&view.presets.active, &view.config)
}

#[test]
fn saving_automatic_win_updates_its_preset_without_changing_a_fresh_crypto_default() {
    let (mut view, previous) = crypto_view();
    assert!(view.apply_source_preset(Some("mini index regions")));
    assert!(view.set_ignore_opening_burst_in_scale(true));
    let expected_win = appearance(&view);

    let stored = save_document(&mut view);
    assert_eq!(stored.active, previous.name);
    assert_eq!(stored.get(&previous.name), Some(&previous));
    assert_eq!(stored.get("mini index regions"), Some(&expected_win));
    assert_eq!(
        appearance(&view),
        expected_win,
        "the current WIN pane stays put"
    );

    let crypto = from_document(stored.clone(), None);
    assert_eq!(appearance(&crypto), previous);
    assert!(!crypto.config.tape_only());
    assert!(!crypto.config.volume_dots.enabled);
    assert!(!crypto.config.volume_dots.ignore_opening_burst_in_scale);

    let win = from_document(stored, Some("mini index regions"));
    assert_eq!(appearance(&win), expected_win);
    assert!(win.config.volume_dots.ignore_opening_burst_in_scale);
    assert!(view.apply_source_preset(None));
    assert_eq!(
        appearance(&view),
        previous,
        "the existing view also restores"
    );
}

#[test]
fn an_explicit_manual_choice_after_automatic_win_can_still_become_the_global_default() {
    for chosen in ["dense tape", "mini index regions"] {
        let (mut view, _) = crypto_view();
        assert!(view.apply_source_preset(Some("mini index regions")));
        assert!(view.apply_preset(chosen));
        let manual = appearance(&view);
        let stored = save_document(&mut view);
        assert_eq!(
            stored.active, chosen,
            "manual selection is explicit: {chosen}"
        );
        assert_eq!(appearance(&from_document(stored, None)), manual);
        assert!(view.apply_source_preset(None));
        assert_eq!(
            appearance(&view),
            manual,
            "no stale automatic scope remains"
        );
    }
}

#[test]
fn reloading_a_saved_automatic_win_preset_keeps_its_source_scope_and_crypto_fallback() {
    let (mut view, previous) = crypto_view();
    assert!(view.apply_source_preset(Some("mini index regions")));
    assert!(view.set_ignore_opening_burst_in_scale(true));
    let saved_win = appearance(&view);
    let stored = save_document(&mut view);
    assert!(view.set_ignore_opening_burst_in_scale(false));

    view.reload_presets_from((stored, PresetSource::Embedded, None));
    assert_eq!(
        appearance(&view),
        saved_win,
        "reload discards unsaved WIN edits"
    );
    assert!(view.apply_source_preset(None));
    assert_eq!(appearance(&view), previous);
}

#[test]
fn a_failed_automatic_save_keeps_the_active_win_view_and_its_return_look() {
    let (mut view, previous) = crypto_view();
    assert!(view.apply_source_preset(Some("mini index regions")));
    assert!(view.set_ignore_opening_burst_in_scale(true));
    let win = appearance(&view);
    view.save_preset_with(|document| {
        assert_eq!(document.active, previous.name);
        Err("test writer refused the save".to_owned())
    });
    assert_eq!(appearance(&view), win);
    assert!(
        view.preset_status
            .as_deref()
            .unwrap()
            .starts_with("not saved")
    );
    assert!(view.apply_source_preset(None));
    assert_eq!(appearance(&view), previous);
}

#[test]
fn an_automatic_win_save_cannot_overwrite_its_other_markets_default_name() {
    let (mut view, previous) = crypto_view();
    assert!(view.apply_source_preset(Some("mini index regions")));
    assert!(view.set_ignore_opening_burst_in_scale(true));
    let win = appearance(&view);
    view.preset_name_draft = previous.name.clone();

    view.save_preset_with(|_| panic!("a conflicting name must not reach the writer"));

    assert_eq!(appearance(&view), win);
    assert_eq!(view.presets.get(&previous.name), Some(&previous));
    let status = view
        .preset_status
        .as_deref()
        .expect("explain the conflicting name");
    assert!(status.contains(&previous.name));
    assert!(status.contains("another preset name"));
    assert!(view.apply_source_preset(None));
    assert_eq!(appearance(&view), previous);
}
