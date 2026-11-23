use super::*;
use crate::scratch::ScratchDir;

const DEFAULTS: &str = "version = 1\n[layers]\nheatmap = true\ngrid = true\ncrosshair = true\n";

fn registry() -> LayerRegistry {
    LayerRegistry::new(ChartLayer::ALL).unwrap()
}

#[test]
fn a_missing_or_unreadable_file_opens_on_the_supplied_default() {
    let scratch = ScratchDir::new("layer-defaults");
    let path = scratch.path().join("layers.toml");
    let expected = shipped_default(DEFAULTS, &registry());
    assert!(!expected.is_empty());
    assert_eq!(load(&path, DEFAULTS, &registry()), expected);
    for invalid in ["not toml", "version = 2\n[layers]\ngrid = false"] {
        std::fs::write(&path, invalid).unwrap();
        assert_eq!(load(&path, DEFAULTS, &registry()), expected);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
    assert_eq!(load(scratch.path(), DEFAULTS, &registry()), expected);
}

#[test]
fn visibility_round_trips_through_disk_over_the_supplied_default() {
    let scratch = ScratchDir::new("layer-roundtrip");
    let path = scratch.path().join("layers.toml");
    let states = BTreeMap::from([
        (ChartLayer::Crosshair, false),
        (ChartLayer::Grid, false),
        (ChartLayer::Heatmap, true),
        (ChartLayer::Drawings, true),
        (ChartLayer::PointerPrice, true),
        (ChartLayer::PointerTime, false),
        (ChartLayer::LaneMarks, true),
    ]);
    save(&path, &states);
    let mut expected = shipped_default(DEFAULTS, &registry());
    expected.extend(states.iter().filter(|(layer, _)| layer.persisted()));
    assert_eq!(load(&path, DEFAULTS, &registry()), expected);
    assert!(!path.with_extension("toml.tmp").exists());
}

#[test]
fn the_callers_registry_resolves_only_known_persisted_layers() {
    let scratch = ScratchDir::new("layer-registry");
    let path = scratch.path().join("layers.toml");
    std::fs::write(
        &path,
        "version = 1\n[layers]\ngrid = false\nheatmap = false\nunknown = true\n",
    )
    .unwrap();
    let registry = LayerRegistry::new([ChartLayer::Grid]).unwrap();
    assert_eq!(
        load(&path, DEFAULTS, &registry),
        BTreeMap::from([(ChartLayer::Grid, false)])
    );
}

#[test]
fn refused_writes_preserve_the_document_and_create_no_temp_file() {
    use quantick_workspace::write_refusal::{WritesRefused, refuse_writes_on_this_thread};
    let scratch = ScratchDir::new("layer-refused");
    let path = scratch.path().join("layers.toml");
    std::fs::write(&path, DEFAULTS).unwrap();
    refuse_writes_on_this_thread(Some(WritesRefused {
        hooks: vec!["QUANTICK_UI_STATE".into()],
    }));
    save(&path, &BTreeMap::from([(ChartLayer::Grid, false)]));
    refuse_writes_on_this_thread(None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULTS);
    assert!(!path.with_extension("toml.tmp").exists());
}

#[test]
fn failed_publication_cleans_the_temp_sibling() {
    let scratch = ScratchDir::new("layer-publish");
    let path = scratch.path().join("layers.toml");
    std::fs::create_dir(&path).unwrap();
    save(&path, &BTreeMap::from([(ChartLayer::Grid, false)]));
    assert!(path.is_dir());
    assert!(!path.with_extension("toml.tmp").exists());
}
