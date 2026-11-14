use super::*;

/// A directory of this test's own thread. It used to be one fixed folder
/// shared by every test in this module *and* by every concurrent run on
/// the machine — two worktrees running the suite at once wrote each
/// other's fixtures. `thread_dir` rather than a `ScratchDir` because
/// every caller spells `scratch().join(..)` in a single statement, where
/// a value would be dropped before the file it names is written.
fn scratch() -> PathBuf {
    crate::scratch::thread_dir("chart-layers")
}

#[test]
fn a_missing_file_opens_on_the_shipped_default() {
    let fresh = load(&scratch().join("missing.toml"));
    assert_eq!(
        fresh,
        shipped_default(),
        "a fresh install opens on the config, not on a struct initialiser"
    );
    assert!(!fresh.is_empty(), "the shipped file has to reach the app");
}

/// The file is tracked and compiled in, so anything wrong with it is a
/// build-time mistake — and one that would otherwise degrade silently into
/// "the code decides", which is the state this file exists to end.
#[test]
fn the_shipped_default_parses_and_opens_the_flow_layers() {
    let shipped = shipped_default();
    for layer in [
        ChartLayer::Heatmap,
        ChartLayer::Footprint,
        ChartLayer::LiveStrip,
    ] {
        assert_eq!(
            shipped.get(&layer),
            Some(&true),
            "{} is what the chart is for; a first launch draws it",
            layer.id()
        );
    }
    assert_eq!(
        shipped.get(&ChartLayer::BackfillDivider),
        Some(&false),
        "the one layer a fresh chart holds back"
    );
    // The compass, on. The bars here are cut by ticks and volume, so
    // without it the only way to learn when a candle happened is to arm
    // the crosshair tool — a fresh install that cannot answer "when was
    // this?" while you point at a candle is the gap this project's own
    // bar types create.
    for layer in [ChartLayer::PointerPrice, ChartLayer::PointerTime] {
        assert_eq!(
            shipped.get(&layer),
            Some(&true),
            "{} opens on, and each axis's menu is where it is switched off",
            layer.id()
        );
    }
    // Every layer this file owns is stated, so no reader has to know which
    // ones fall through to a struct initialiser.
    for layer in ChartLayer::ALL.into_iter().filter(|l| l.persisted()) {
        assert!(
            shipped.contains_key(&layer),
            "{} has no entry in config/chart-layers.toml",
            layer.id()
        );
    }
    assert!(
        !shipped.contains_key(&ChartLayer::LaneMarks),
        "the preset is the lane marks' only home"
    );
    assert!(
        !shipped.contains_key(&ChartLayer::Bubbles),
        "the aggression bubbles are each asset's own"
    );
}

/// What `load` must return for a file holding exactly `states`: the
/// trader's answers over the shipped ones. Written once because three
/// tests need it and a fourth copy of `shipped_default().extend(...)` is
/// three chances to encode the *old* contract by accident.
fn shipped_with(states: &BTreeMap<ChartLayer, bool>) -> BTreeMap<ChartLayer, bool> {
    let mut expected = shipped_default();
    expected.extend(states.iter().filter(|(layer, _)| layer.persisted()));
    expected
}

#[test]
fn visibility_round_trips_through_disk() {
    let path = scratch().join("round-trip.toml");
    let states = BTreeMap::from([
        (ChartLayer::Crosshair, false),
        (ChartLayer::Grid, false),
        (ChartLayer::Heatmap, true),
        (ChartLayer::Drawings, true),
        (ChartLayer::PointerPrice, true),
        (ChartLayer::PointerTime, false),
    ]);
    save(&path, &states);
    assert_eq!(load(&path), shipped_with(&states));
    std::fs::remove_file(&path).ok();
}
