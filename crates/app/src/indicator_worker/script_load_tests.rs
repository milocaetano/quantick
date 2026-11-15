use super::*;
use crate::indicators::IndicatorViews;

/// The M2 acceptance: a script using request.security loads into an
/// error slot whose message carries the line number and the stable code.
#[test]
fn a_rejected_script_surfaces_its_error_with_line_and_code() {
    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: "sec.pine".to_owned(),
            text: "//@version=5\nindicator(\"t\")\ns = request.security(close)\nplot(s)\n"
                .to_owned(),
        },
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    let view = &views.all()[0];
    let error = view.error.as_ref().expect("the slot carries the error");
    assert!(error.message.contains("sec.pine:3:"), "{}", error.message);
    assert!(
        error.message.contains("PINE_NO_SECURITY"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("activity-sampled"),
        "{}",
        error.message
    );
}

/// An embedded starter script runs end to end through the worker: bars
/// in, plot columns out — the whole scripted pipe.
#[test]
fn an_embedded_script_plots_through_the_worker() {
    let (name, text) = crate::indicators::library::EMBEDDED_SCRIPTS[0];
    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: name.to_owned(),
            text: text.to_owned(),
        },
    });
    let trades: Vec<quantick_engine::Trade> = (1..=24).map(tests::trade).collect();
    let mut builder = quantick_engine::TickBarBuilder::new(2);
    let bars = quantick_engine::golden::replay(&mut builder, &trades);
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    let view = &views.all()[0];
    assert!(view.error.is_none(), "{:?}", view.error);
    assert_eq!(view.descriptor.title, "EMA");
    assert!(view.descriptor.overlay);
    assert_eq!(view.rows, bars.len());
    // EMA(9) over 12 bars: warmup NaN then values.
    let column = &view.columns[0];
    assert!(column[0].is_nan());
    assert!(column.last().is_some_and(|v| !v.is_nan()));
}
