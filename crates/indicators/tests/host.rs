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

/// Bit-identical comparison of two plot columns. `NaN != NaN` under IEEE, so
/// a plain `assert_eq!` on a column containing warmup cells can never hold;
/// the golden tests compare `{:?}` output for the same reason.
fn same_column(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| format!("{a:?}") == format!("{b:?}"))
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
/// With `fail_preview` it fails the preview run instead, which is the other
/// half of the same contract.
struct FailingAt {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
    fail_at: usize,
    fail_preview: bool,
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
                fills: Vec::new(),
            },
            plots: PlotBuffer::new(0),
            fail_at,
            fail_preview: false,
        }
    }

    /// Commits happily, fails every preview.
    fn failing_preview() -> Self {
        Self {
            fail_preview: true,
            ..Self::new(usize::MAX)
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
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        if self.fail_preview {
            return Err(EvalError {
                bar_index: ctx.bar_index,
                message: "deliberate preview failure".to_owned(),
            });
        }
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
    let (bars, partial) = bars_and_partial(5);
    let mut host = IndicatorHost::new();
    let survivor = host.add(Box::new(Ema::new(3, SourceId::Close)));
    let id = host.add(Box::new(Cvd::new()));
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    host.set_partial(partial.as_ref());
    let before = plot0(&host, survivor);
    let cvd_before = host.cvd().to_vec();

    assert_eq!(host.indicator_count(), 2);
    assert!(host.remove(id));
    assert_eq!(host.indicator_count(), 1);
    assert!(host.plots(id).is_none());
    // Removing mid-stream is the case that would actually hurt: the
    // neighbour keeps its columns and the shared cvd does not shift.
    assert!(
        same_column(&plot0(&host, survivor), &before),
        "neighbour untouched"
    );
    assert_eq!(host.cvd(), cvd_before.as_slice(), "shared cvd untouched");
    assert!(
        !host.remove(id),
        "an id is never reused, a stale one just misses"
    );
}

#[test]
fn an_instance_added_mid_bar_previews_immediately() {
    let (bars, partial) = bars_and_partial(5);
    let partial = partial.expect("the fixture leaves a forming bar");

    let mut host = IndicatorHost::new();
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    host.set_partial(Some(&partial));

    // Added while a bar is forming: the newcomer must be as current as a
    // neighbour that was there all along, not one bar short until the next
    // partial update arrives (which, on a paused replay, never does).
    let late = host.add(Box::new(Ema::new(3, SourceId::Close)));
    let late_preview = host
        .preview(late)
        .expect("a newcomer previews against the retained forming bar");

    let mut reference = IndicatorHost::new();
    let early = reference.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        reference.push_closed_bar(bar);
    }
    reference.set_partial(Some(&partial));
    let early_preview = reference.preview(early).expect("reference previews");
    assert!(same_column(&late_preview.values, &early_preview.values));

    // Same for a replacement, and the retained partial dies with the close.
    let replaced = host.replace(late, Box::new(Ema::new(3, SourceId::Close)));
    assert!(replaced);
    assert!(host.preview(late).is_some(), "a replacement previews too");
    host.push_closed_bar(&partial);
    let fresh = host.add(Box::new(Ema::new(3, SourceId::Close)));
    assert!(
        host.preview(fresh).is_none(),
        "the forming bar is gone once it closes"
    );
}

#[test]
fn a_failing_preview_disables_only_its_own_instance() {
    let (bars, partial) = bars_and_partial(5);
    let partial = partial.expect("the fixture leaves a forming bar");
    let mut host = IndicatorHost::new();
    let broken = host.add(Box::new(FailingAt::failing_preview()));
    let healthy = host.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        host.push_closed_bar(bar);
    }

    host.set_partial(Some(&partial));
    let error = host.error(broken).expect("the preview failure is recorded");
    assert_eq!(error.bar_index, bars.len());
    assert!(host.preview(broken).is_none(), "no frame from a failed run");
    assert!(
        host.preview(healthy).is_some(),
        "the neighbour previews normally"
    );

    // A preview failure disables the instance until it is replaced or the
    // host rebuilds — closing a bar does not quietly re-enable it.
    let rows_before = host.plots(broken).expect("instance exists").len();
    host.push_closed_bar(&partial);
    assert!(host.error(broken).is_some(), "still disabled after a close");
    assert_eq!(
        host.plots(broken).expect("instance exists").len(),
        rows_before,
        "a disabled instance commits nothing"
    );
}

#[test]
fn a_catch_up_failure_lands_in_the_newcomers_error_state() {
    let (bars, _) = bars_and_partial(1);
    assert!(bars.len() > 2, "the fixture has history to replay");
    let mut host = IndicatorHost::new();
    let healthy = host.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    let healthy_rows = plot0(&host, healthy);

    // `add` replays retained history into the newcomer; a failure there is
    // reported exactly like a live one, and the neighbour is untouched.
    let late = host.add(Box::new(FailingAt::new(1)));
    let error = host.error(late).expect("the replay failure is recorded");
    assert_eq!(error.bar_index, 1);
    assert!(host.error(healthy).is_none(), "neighbour unaffected");
    assert!(same_column(&plot0(&host, healthy), &healthy_rows));

    // Same contract for `replace`.
    assert!(host.replace(healthy, Box::new(FailingAt::new(2))));
    assert_eq!(
        host.error(healthy)
            .expect("a failing replacement reports too")
            .bar_index,
        2
    );
}

/// The ladder's core promise: every rung is a real evaluation of a real
/// prefix, so a rung's values equal what a plain `set_partial` of that same
/// prefix produces. If the walk ever interpolated, or leaked one rung's state
/// into the next, this diverges.
#[test]
fn every_rung_equals_a_standalone_preview_of_the_same_prefix() {
    let (bars, _) = bars_and_partial(5);
    let trades = fixture::parse_trades(TRADES).expect("fixture parses");

    // Prefixes of one run of trades, folded the way every builder folds.
    let mut prefixes = Vec::new();
    let mut forming: Option<Bar> = None;
    for trade in trades.iter().take(6) {
        match &mut forming {
            None => forming = Some(Bar::opened_by(trade)),
            Some(bar) => bar.extend(trade),
        }
        prefixes.push(forming.clone().expect("a bar is forming"));
    }

    let mut walked = IndicatorHost::new();
    let walked_id = walked.add(Box::new(Cvd::new()));
    for bar in &bars {
        walked.push_closed_bar(bar);
    }
    let mut rungs = Vec::new();
    walked.walk_partial_prefixes(&prefixes, |prefix, previews| {
        rungs.push((
            prefix.close_time,
            previews
                .get(walked_id)
                .map(|frame| format!("{:?}", frame.values)),
        ));
    });

    let mut one_at_a_time = IndicatorHost::new();
    let solo_id = one_at_a_time.add(Box::new(Cvd::new()));
    for bar in &bars {
        one_at_a_time.push_closed_bar(bar);
    }
    let expected: Vec<_> = prefixes
        .iter()
        .map(|prefix| {
            one_at_a_time.set_partial(Some(prefix));
            (
                prefix.close_time,
                one_at_a_time
                    .preview(solo_id)
                    .map(|frame| format!("{:?}", frame.values)),
            )
        })
        .collect();

    assert_eq!(rungs, expected);
    assert_eq!(rungs.len(), prefixes.len(), "one rung per prefix");
}

/// A walk is a read, not a write: it may not advance committed state, and it
/// must hand the retained forming bar back to whoever reads `preview()` next.
/// Otherwise the chart's headline value would be stuck wherever the ladder
/// happened to stop.
#[test]
fn a_walk_leaves_committed_state_and_the_live_preview_untouched() {
    let (bars, partial) = bars_and_partial(5);
    let partial = partial.expect("the fixture leaves a bar forming");

    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(Cvd::new()));
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    host.set_partial(Some(&partial));

    let committed = plot0(&host, id);
    let live = host
        .preview(id)
        .map(|frame| format!("{:?}", frame.values))
        .expect("the forming bar previews");

    // A ladder that ends somewhere else entirely.
    let mut halfway = partial.clone();
    halfway.close_time -= 1;
    halfway.buy_volume += rust_decimal::Decimal::from(1000);
    host.walk_partial_prefixes(&[halfway], |_, _| {});

    assert!(
        same_column(&plot0(&host, id), &committed),
        "a walk commits nothing"
    );
    assert_eq!(
        host.preview(id).map(|frame| format!("{:?}", frame.values)),
        Some(live),
        "the retained forming bar is previewing again after the walk"
    );
    assert_eq!(host.bar_count(), bars.len());
}

/// With no bar forming, a walk still restores the truth: no preview at all.
/// A leftover rung here would draw a forming value onto a chart that has none.
#[test]
fn a_walk_without_a_forming_bar_clears_the_previews_it_staged() {
    let (bars, partial) = bars_and_partial(5);
    let partial = partial.expect("the fixture leaves a bar forming");

    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(Cvd::new()));
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    host.set_partial(None);

    host.walk_partial_prefixes(&[partial], |_, _| {});

    assert!(
        host.preview(id).is_none(),
        "nothing is forming, so nothing previews"
    );
}

/// An empty ladder is a no-op, and a walk over a host with no indicators
/// visits nothing — the chart asks for a ladder every publish, including the
/// publishes where there is nothing to ask about.
#[test]
fn an_empty_ladder_visits_nothing() {
    let (bars, partial) = bars_and_partial(5);
    let mut host = IndicatorHost::new();
    host.add(Box::new(Cvd::new()));
    for bar in &bars {
        host.push_closed_bar(bar);
    }

    let mut visits = 0;
    host.walk_partial_prefixes(&[], |_, _| visits += 1);
    assert_eq!(visits, 0);

    let mut empty = IndicatorHost::new();
    empty.walk_partial_prefixes(&[partial.expect("forming")], |_, _| visits += 1);
    assert_eq!(visits, 0, "no indicators, nothing to preview");
}
