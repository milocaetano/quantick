use super::*;
use crate::indicators::IndicatorViews;
use quantick_indicators::{IndicatorHost, PlotId, SourceId, native::Ema};

/// Applying settings = construct anew + replace + replay: the columns
/// The second implementer of the input port: a script's bound values must
/// move its output, which nothing in the workspace exercised.
#[test]
fn set_inputs_binds_a_scripts_declared_input() {
    let trades: Vec<quantick_engine::Trade> = (1..=8).map(tests::trade).collect();
    let mut builder = quantick_engine::TickBarBuilder::new(2);
    let bars = quantick_engine::golden::replay(&mut builder, &trades);

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Script {
            name: "scaled.pine".to_owned(),
            text: "//@version=5
indicator(\"scaled\")
k = input.int(1, \"k\")
plot(close * k)
"
            .to_owned(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.send(IndicatorCommand::SetInputs {
        slot,
        values: vec![InputValue::Int(3)],
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }

    let view = &views.all()[0];
    assert_eq!(view.input_values, vec![InputValue::Int(3)], "bound");
    let plotted = &view.columns[0];
    let expected: Vec<f64> = bars
        .iter()
        .map(|b| b.close.to_string().parse::<f64>().unwrap_or(f64::NAN) * 3.0)
        .collect();
    assert_eq!(
        format!("{plotted:?}"),
        format!("{expected:?}"),
        "the script's output moved with its input"
    );
}

/// after SetInputs must equal a host that ran the new inputs from bar
/// zero, and the UI must receive a full Rebuilt carrying the new values.
#[test]
fn set_inputs_recomputes_like_a_fresh_instance() {
    let trades: Vec<quantick_engine::Trade> = (1..=20).map(tests::trade).collect();
    let mut builder = quantick_engine::TickBarBuilder::new(2);
    let bars = quantick_engine::golden::replay(&mut builder, &trades);

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Native {
            id: "native.ema".to_owned(),
            values: vec![InputValue::Int(3), InputValue::Source(SourceId::Close)],
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.send(IndicatorCommand::SetInputs {
        slot,
        values: vec![InputValue::Int(2), InputValue::Source(SourceId::Hl2)],
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }

    let mut reference = IndicatorHost::new();
    let id = reference.add(Box::new(Ema::new(2, SourceId::Hl2)));
    for bar in &bars {
        reference.push_closed_bar(bar);
    }

    let view = &views.all()[0];
    assert_eq!(view.descriptor.title, "EMA(2, hl2)", "descriptor followed");
    assert_eq!(view.input_values[0], InputValue::Int(2), "values followed");
    assert_eq!(
        view.input_values[1],
        InputValue::Source(SourceId::Hl2),
        "the source cell is bound too — applying `Close` to an instance that already had it proved nothing"
    );
    assert_eq!(
        format!("{:?}", view.columns[0]),
        format!("{:?}", reference.plots(id).unwrap().column(PlotId::new(0))),
        "SetInputs must replay as if the new inputs had always been set"
    );
}
