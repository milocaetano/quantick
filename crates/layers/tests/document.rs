use quantick_layers::{ChartLayer as L, DocumentError, LayerDocument, LayerRegistry};
use std::collections::BTreeMap;

// This consumer needs no app, UI, clock or filesystem.
const DEFAULTS: &str = "version=1\n[layers]\nheatmap=true\nbubbles=true\nfootprint=true\nlive_strip=true\nbackfill_divider=false\n";

#[test]
fn explicit_choice_overrides_defaults_but_absence_inherits() {
    let states = LayerDocument::restore(
        DEFAULTS,
        Some("version=1\n[layers]\nheatmap=false"),
        &LayerRegistry::default(),
    );
    assert_eq!(states.get(&L::Heatmap), Some(&false));
    assert_eq!(states.get(&L::Bubbles), Some(&true));
    assert!(!states.contains_key(&L::LaneMarks));
}
#[test]
fn legacy_files_inherit_new_defaults() {
    let states = LayerDocument::restore(
        DEFAULTS,
        Some(
            "version=1\n[layers]\ngrid=true\ncrosshair=true\nlast_price=true\nbackfill_divider=false",
        ),
        &LayerRegistry::default(),
    );
    for layer in [L::Heatmap, L::Bubbles, L::Footprint, L::LiveStrip] {
        assert_eq!(states.get(&layer), Some(&true));
    }
    assert_eq!(states.get(&L::BackfillDivider), Some(&false));
}
#[test]
fn unknown_versions_and_invalid_documents_preserve_shipped_answers() {
    let registry = LayerRegistry::default();
    let expected = LayerDocument::restore(DEFAULTS, None, &registry);
    for text in ["version=99\n[layers]\ngrid=false", "not even toml ["] {
        assert_eq!(
            LayerDocument::restore(DEFAULTS, Some(text), &registry),
            expected
        );
        assert!(LayerDocument::parse(text).is_err());
    }
    assert!(matches!(
        LayerDocument::parse("version=99"),
        Err(DocumentError::Version(99))
    ));
}
#[test]
fn known_choices_survive_unknown_ids_and_preset_choices_are_ignored() {
    let registry = LayerRegistry::default();
    let text = "version=1\n[layers]\ngrid=false\nfrom_the_future=true\nlane_marks=false";
    let states = LayerDocument::parse(text).unwrap().resolve(&registry);
    assert_eq!(states, BTreeMap::from([(L::Grid, false)]));
}
#[test]
fn serialization_round_trip_keeps_separate_axis_choices_and_excludes_preset() {
    let states = BTreeMap::from([
        (L::PointerPrice, true),
        (L::PointerTime, false),
        (L::LaneMarks, false),
    ]);
    let text = LayerDocument::encode(&states).unwrap();
    assert!(!text.contains("lane_marks"));
    assert_eq!(
        LayerDocument::parse(&text)
            .unwrap()
            .resolve(&LayerRegistry::default()),
        BTreeMap::from([(L::PointerPrice, true), (L::PointerTime, false)])
    );
}
