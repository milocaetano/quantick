use super::*;

/// The file is tracked and compiled in, so anything wrong with it is a
/// build-time mistake — and one that would otherwise degrade silently into
/// "the code decides", which is the state this file exists to end.
#[test]
fn the_shipped_default_parses_and_opens_the_flow_layers() {
    let shipped = load(&crate::scratch::thread_dir("chart-layers").join("missing.toml"));
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
