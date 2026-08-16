//! The bar paint channel under the golden harness.
//!
//! Two things are proven here, and they are different things:
//!
//! 1. **Determinism** — same trades in, same colours out, bit-exact, twice
//!    (`assert_golden` replays the whole pipeline from scratch and diffs).
//! 2. **Bar-type independence** — the identical indicator over imbalance bars
//!    obeys the identical rule. The channel never asks what built the bar,
//!    and this is the test that would fail if it started to.
//!
//! `PaintedDirection` is deliberately trivial and deliberately *native*: what
//! is under test is the contract, not a script.

use quantick_engine::{ImbalanceBarBuilder, TickBarBuilder};
use quantick_indicators::{
    Ctx, EvalError, Indicator, IndicatorBar, IndicatorDescriptor, PlotBuffer, PlotId, PlotSpec,
    PlotStyle, PreviewFrame, Rgba8, golden,
};

const TRADES: &str = include_str!("fixtures/trades_small.csv");
const RAMP: &str = include_str!("fixtures/trades_ramp.csv");
const EXPECTED: &str = include_str!("fixtures/painted_direction_plots.csv");

const GREEN: Rgba8 = Rgba8::opaque(0, 200, 83);
const RED: Rgba8 = Rgba8::opaque(213, 0, 0);

/// Plots `close` and paints every bar after the first by direction.
///
/// Skipping bar 0 is the point: it makes the fixture pin an unpainted bar
/// beside painted ones, which is the case a channel that quietly shifted its
/// rows would get wrong.
struct PaintedDirection {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
}

impl PaintedDirection {
    fn new() -> Self {
        let descriptor = IndicatorDescriptor {
            title: "Painted direction (test)".to_owned(),
            short_title: None,
            overlay: true,
            plots: vec![PlotSpec {
                id: PlotId::new(0),
                title: "close".to_owned(),
                style: PlotStyle::Line,
                base_color: Rgba8::opaque(255, 255, 255),
                width: 1.0,
                offset: 0,
                marker: None,
            }],
            inputs: Vec::new(),
            fills: Vec::new(),
        };
        Self {
            plots: PlotBuffer::for_plots(&descriptor.plots),
            descriptor,
        }
    }

    fn paint_for(bar: &IndicatorBar, bar_index: usize) -> Option<Rgba8> {
        if bar_index == 0 {
            return None;
        }
        Some(if bar.close >= bar.open { GREEN } else { RED })
    }
}

impl Indicator for PaintedDirection {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        let paint = Self::paint_for(bar, ctx.bar_index);
        self.plots.push_row_painted(&[bar.close], paint);
        Ok(())
    }

    fn preview(
        &mut self,
        partial: &IndicatorBar,
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        let mut frame = PreviewFrame::new(vec![partial.close]);
        frame.paint = Self::paint_for(partial, ctx.bar_index);
        Ok(frame)
    }

    fn reset(&mut self) {
        self.plots.clear();
    }
}

#[test]
fn golden_paint_channel_round_trip() {
    golden::assert_golden(
        PaintedDirection::new,
        || TickBarBuilder::new(2),
        TRADES,
        EXPECTED,
    );
}

/// Every bar the channel painted wears the colour its own direction asks for,
/// and bar 0 wears none — whatever built the bars.
fn assert_paint_follows_direction(bars: &[IndicatorBar], indicator: &PaintedDirection) {
    assert!(bars.len() >= 3, "need a few bars to say anything");
    for (index, bar) in bars.iter().enumerate() {
        let expected = PaintedDirection::paint_for(bar, index);
        assert_eq!(
            indicator.plots().bar_paint(index),
            expected,
            "bar {index} (open {}, close {})",
            bar.open,
            bar.close
        );
    }
}

#[test]
fn the_channel_works_the_same_over_imbalance_bars() {
    // Imbalance bars close on adaptive signed-tick imbalance, not on a trade
    // count: the bar boundaries here are nothing like the tick(2) ones above.
    let bars = golden::bars_from_trades_csv(&mut ImbalanceBarBuilder::new(1), RAMP);
    let mut indicator = PaintedDirection::new();
    golden::run_indicator(&mut indicator, &bars).expect("fixture evaluates");

    assert!(indicator.plots().paints_any(), "imbalance bars get painted");
    assert_eq!(indicator.plots().len(), bars.len());
    assert_paint_follows_direction(&bars, &indicator);
}

#[test]
fn the_channel_works_the_same_over_tick_bars() {
    // The control for the test above: same indicator, same fixture, a bar
    // type with completely different boundaries.
    let bars = golden::bars_from_trades_csv(&mut TickBarBuilder::new(2), RAMP);
    let mut indicator = PaintedDirection::new();
    golden::run_indicator(&mut indicator, &bars).expect("fixture evaluates");

    assert_paint_follows_direction(&bars, &indicator);
}

#[test]
fn a_reset_replays_to_the_same_colours() {
    let bars = golden::bars_from_trades_csv(&mut TickBarBuilder::new(2), TRADES);
    let mut indicator = PaintedDirection::new();
    golden::run_indicator(&mut indicator, &bars).expect("fixture evaluates");
    indicator.reset();
    assert!(
        !indicator.plots().paints_any(),
        "reset clears the channel with everything else"
    );

    golden::run_indicator(&mut indicator, &bars).expect("fixture re-evaluates");
    assert!(
        golden::diff_plot_csv(EXPECTED, &golden::plot_csv(&indicator)).is_none(),
        "replay after reset reproduces the same colours"
    );
}

#[test]
fn an_unpainted_indicator_keeps_its_golden_column_layout() {
    // The conditional `bar_paint` column is what lets every existing fixture
    // stay byte-identical. This is that promise, written down.
    struct Silent(PlotBuffer, IndicatorDescriptor);
    impl Indicator for Silent {
        fn descriptor(&self) -> &IndicatorDescriptor {
            &self.1
        }
        fn plots(&self) -> &PlotBuffer {
            &self.0
        }
        fn on_close(&mut self, bar: &IndicatorBar, _: &mut Ctx<'_>) -> Result<(), EvalError> {
            self.0.push_row(&[bar.close]);
            Ok(())
        }
        fn preview(
            &mut self,
            b: &IndicatorBar,
            _: &mut Ctx<'_>,
        ) -> Result<PreviewFrame, EvalError> {
            Ok(PreviewFrame::new(vec![b.close]))
        }
        fn reset(&mut self) {
            self.0.clear();
        }
    }

    let painted = PaintedDirection::new();
    let descriptor = IndicatorDescriptor {
        title: "Silent (test)".to_owned(),
        ..painted.descriptor.clone()
    };
    let mut silent = Silent(PlotBuffer::for_plots(&descriptor.plots), descriptor);
    let bars = golden::bars_from_trades_csv(&mut TickBarBuilder::new(2), TRADES);
    golden::run_indicator(&mut silent, &bars).expect("fixture evaluates");

    let csv = golden::plot_csv(&silent);
    assert_eq!(
        csv.lines().next(),
        Some("bar_index,close"),
        "no paint, no column: {csv}"
    );
}
