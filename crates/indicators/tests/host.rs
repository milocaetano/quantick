//! Host behavior: rollback discipline, error isolation, catch-up, replace
//! and rebuild — the §6 property tests of the plan, written against the
//! public host API only.

use quantick_engine::{Bar, BarBuilder as _, TickBarBuilder, fixture, golden as engine_golden};
use quantick_indicators::{
    Ctx, EvalError, Indicator, IndicatorBar, IndicatorDescriptor, IndicatorHost, PlotBuffer,
    PlotId, PreviewFrame, SourceId,
    native::{Cvd, Ema},
};

const TRADES: &str = include_str!("fixtures/trades_ramp.csv");

/// Closed tick(N) bars plus a forming bar built from the leftover trades —
/// the exact shape a live chart hands the host.
fn bars_and_partial(tick: u64) -> (Vec<Bar>, Option<Bar>) {
    let trades = fixture::parse_trades(TRADES).expect("fixture parses");
    let mut builder = TickBarBuilder::new(tick);
    let closed = engine_golden::replay(&mut builder, &trades);
    (closed, builder.partial().cloned())
}

fn plot0(host: &IndicatorHost, id: quantick_indicators::InstanceId) -> Vec<f64> {
    host.plots(id)
        .expect("instance exists")
        .column(PlotId::new(0))
        .to_vec()
}

#[test]
fn previews_never_advance_committed_state() {
    let (bars, _) = bars_and_partial(1);

    // Reference: closes only.
    let mut plain = IndicatorHost::new();
    let plain_id = plain.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        plain.push_closed_bar(bar);
    }

    // Same closes, but with three previews of a moving partial before every
    // close — the double-advance bug this design exists to prevent.
    let mut previewed = IndicatorHost::new();
    let previewed_id = previewed.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        for wiggle in [0.5, -0.25, 1.0] {
            let mut forming = bar.clone();
            forming.close += rust_decimal::Decimal::try_from(wiggle).unwrap();
            previewed.set_partial(Some(&forming));
        }
        previewed.push_closed_bar(bar);
    }

    let expected = plot0(&plain, plain_id);
    let got = plot0(&previewed, previewed_id);
    assert_eq!(
        format!("{expected:?}"),
        format!("{got:?}"),
        "an EMA over `close, preview x3, close` must equal the EMA over `close, close`"
    );
}

#[test]
fn preview_stages_cvd_without_committing_it() {
    // tick(5) over 12 trades: 2 closed bars + 2 leftover trades forming.
    let (bars, partial) = bars_and_partial(5);
    let partial = partial.expect("12 trades leave a 2-trade partial at tick(5)");

    let mut host = IndicatorHost::new();
    let cvd_id = host.add(Box::new(Cvd::new()));
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    let committed = host.cvd().to_vec();

    host.set_partial(Some(&partial));
    let frame = host.preview(cvd_id).expect("preview frame exists");
    let staged = committed.last().copied().unwrap_or(0.0) + IndicatorBar::from(&partial).delta();
    assert_eq!(frame.values, vec![staged], "preview sees the staged cvd");
    assert_eq!(host.cvd(), committed.as_slice(), "committed cvd untouched");

    host.set_partial(None);
    assert!(host.preview(cvd_id).is_none(), "cleared with the partial");
}

/// Fails its commit run at a chosen bar — the neighbour-isolation probe.
struct FailingAt {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
    fail_at: usize,
}

impl FailingAt {
    fn new(fail_at: usize) -> Self {
        Self {
            descriptor: IndicatorDescriptor {
                title: "failing (test)".to_owned(),
                short_title: None,
                overlay: false,
                plots: Vec::new(),
                inputs: Vec::new(),
            },
            plots: PlotBuffer::new(0),
            fail_at,
        }
    }
}

impl Indicator for FailingAt {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }
    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }
    fn on_close(&mut self, _bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        if ctx.bar_index == self.fail_at {
            return Err(EvalError {
                bar_index: ctx.bar_index,
                message: "deliberate test failure".to_owned(),
            });
        }
        self.plots.push_row(&[]);
        Ok(())
    }
    fn preview(
        &mut self,
        _partial: &IndicatorBar,
        _ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        Ok(PreviewFrame::new(Vec::new()))
    }
    fn reset(&mut self) {
        self.plots.clear();
    }
}

#[test]
fn one_failing_indicator_never_poisons_its_neighbours() {
    let (bars, _) = bars_and_partial(1);
    let mut host = IndicatorHost::new();
    let failing = host.add(Box::new(FailingAt::new(2)));
    let healthy = host.add(Box::new(Cvd::new()));

    for bar in &bars {
        host.push_closed_bar(bar);
    }

    let error = host
        .error(failing)
        .expect("failing instance holds its error");
    assert_eq!(error.bar_index, 2);
    assert!(error.message.contains("deliberate"), "{error}");
    assert_eq!(
        host.plots(failing).unwrap().len(),
        2,
        "rows stop at the failure; no half-appended row"
    );
    assert_eq!(
        host.plots(healthy).unwrap().len(),
        bars.len(),
        "the healthy neighbour saw every bar"
    );
}

#[test]
fn adding_late_catches_up_identically() {
    let (bars, _) = bars_and_partial(1);
    let split = 7;

    let mut from_start = IndicatorHost::new();
    let a = from_start.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        from_start.push_closed_bar(bar);
    }

    let mut late = IndicatorHost::new();
    for bar in &bars[..split] {
        late.push_closed_bar(bar);
    }
    let b = late.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars[split..] {
        late.push_closed_bar(bar);
    }

    assert_eq!(
        format!("{:?}", plot0(&from_start, a)),
        format!("{:?}", plot0(&late, b)),
        "an indicator added mid-stream must be indistinguishable from one added at bar zero"
    );
}

#[test]
fn replace_recomputes_the_full_history() {
    let (bars, _) = bars_and_partial(1);

    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        host.push_closed_bar(bar);
    }

    // "Change the length input" = construct anew, replace, host replays.
    assert!(host.replace(id, Box::new(Ema::new(2, SourceId::Close))));

    let mut reference = IndicatorHost::new();
    let ref_id = reference.add(Box::new(Ema::new(2, SourceId::Close)));
    for bar in &bars {
        reference.push_closed_bar(bar);
    }

    assert_eq!(
        format!("{:?}", plot0(&host, id)),
        format!("{:?}", plot0(&reference, ref_id)),
        "replace must replay history as if the new inputs had always been set"
    );
    assert_eq!(
        host.descriptor(id).unwrap().title,
        "EMA(2)",
        "descriptor follows the replacement"
    );
}

#[test]
fn rebuild_replays_identically_and_clears_errors() {
    let (bars, partial) = bars_and_partial(5);
    assert!(
        partial.is_some(),
        "the rebuild must re-preview a real partial"
    );

    let mut host = IndicatorHost::new();
    let ema = host.add(Box::new(Ema::new(3, SourceId::Close)));
    let failing = host.add(Box::new(FailingAt::new(1)));
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    let before = format!("{:?}", plot0(&host, ema));
    assert!(host.error(failing).is_some());

    host.rebuild(&bars, partial.as_ref());
    assert_eq!(
        format!("{:?}", plot0(&host, ema)),
        before,
        "same bars in, same plots out — through a rebuild too"
    );
    let re_error = host
        .error(failing)
        .expect("deterministic failure re-surfaces");
    assert_eq!(re_error.bar_index, 1);
    assert!(
        host.preview(ema).is_some(),
        "rebuild re-previews the partial"
    );

    host.rebuild(&bars[..1], None);
    assert_eq!(host.bar_count(), 1);
    assert_eq!(
        host.plots(ema).unwrap().len(),
        1,
        "seek back = shorter history"
    );
    assert!(host.preview(ema).is_none(), "no partial after the seek");
}

#[test]
fn remove_forgets_the_instance() {
    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(Cvd::new()));
    assert_eq!(host.indicator_count(), 1);
    assert!(host.remove(id));
    assert_eq!(host.indicator_count(), 0);
    assert!(host.plots(id).is_none());
    assert!(
        !host.remove(id),
        "an id is never reused, a stale one just misses"
    );
}
