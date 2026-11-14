use super::*;
use crate::indicators::IndicatorViews;

/// The M3 acceptance through the worker: the embedded zigzag script
/// produces committed lines and HH/LH/HL/LL labels, published on the
/// Objects event and surviving a rebuild.
#[test]
fn zigzag_objects_flow_to_the_views_and_survive_rebuild() {
    let (_, text) = crate::indicators::library::EMBEDDED_SCRIPTS
        .iter()
        .find(|(name, _)| *name == "zigzag.pine")
        .expect("zigzag is embedded");
    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: "zigzag.pine".to_owned(),
            text: (*text).to_owned(),
        },
    });
    // A wiggling tape (period-5 price cycle) makes pivots inevitable.
    let trades: Vec<quantick_engine::Trade> = (1..=60).map(tests::trade).collect();
    let mut builder = quantick_engine::TickBarBuilder::new(1);
    let bars = quantick_engine::golden::replay(&mut builder, &trades);
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    let view = &views.all()[0];
    assert!(view.error.is_none(), "{:?}", view.error);
    let objects = view.render_objects();
    assert!(!objects.lines.is_empty(), "swings draw segments");
    assert!(!objects.labels.is_empty(), "pivots draw labels");
    let texts: Vec<&str> = objects.labels.iter().map(|l| l.text.as_str()).collect();
    assert!(
        texts.iter().any(|t| *t == "HH" || *t == "LH"),
        "highs are classified: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| *t == "HL" || *t == "LL"),
        "lows are classified: {texts:?}"
    );

    // A rebuild over the same bars reproduces the identical object set —
    // determinism through the seek/spec-switch path.
    let before = objects.clone();
    worker.send(IndicatorCommand::Rebuild(bars, None));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    assert_eq!(
        *views.all()[0].render_objects(),
        before,
        "same bars in, same objects out"
    );
}
