//! Golden-harness round-trip: a minimal indicator over a committed trade
//! fixture must reproduce a committed plot fixture, byte-exactly, twice.
//!
//! `CloseCvd` is deliberately trivial — plot the bar close and the shared cvd
//! — so what these tests prove is the *harness and contract plumbing*: trades
//! → bars → `IndicatorBar` → commit runs → plot CSV, plus the rule that a
//! preview never commits anything. Real kernels arrive in the next PR and
//! reuse exactly this harness.

use quantick_engine::TickBarBuilder;
use quantick_indicators::{
    Ctx, EvalError, Indicator, IndicatorBar, IndicatorDescriptor, PlotBuffer, PlotId, PlotSpec,
    PlotStyle, PreviewFrame, Rgba8, golden,
};

const TRADES: &str = include_str!("fixtures/trades_small.csv");
const EXPECTED_PLOTS: &str = include_str!("fixtures/close_cvd_plots.csv");

/// Plots `close` (overlay-style) and the host-maintained `cvd`.
struct CloseCvd {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
}

impl CloseCvd {
    fn new() -> Self {
        let plot = |index: usize, title: &str| PlotSpec {
            id: PlotId::new(index),
            title: title.to_owned(),
            style: PlotStyle::Line,
            base_color: Rgba8::opaque(255, 255, 255),
            width: 1.0,
            offset: 0,
        };
        let descriptor = IndicatorDescriptor {
            title: "Close + CVD (test)".to_owned(),
            short_title: None,
            overlay: false,
            plots: vec![plot(0, "close"), plot(1, "cvd")],
            inputs: Vec::new(),
        };
        Self {
            plots: PlotBuffer::for_plots(&descriptor.plots),
            descriptor,
        }
    }
}

impl Indicator for CloseCvd {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        self.plots.push_row(&[bar.close, ctx.cvd_now()]);
        Ok(())
    }

    fn preview(
        &mut self,
        partial: &IndicatorBar,
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        Ok(PreviewFrame::new(vec![partial.close, ctx.cvd_now()]))
    }

    fn reset(&mut self) {
        self.plots.clear();
    }
}

#[test]
fn golden_close_cvd_round_trip() {
    golden::assert_golden(
        CloseCvd::new,
        || TickBarBuilder::new(2),
        TRADES,
        EXPECTED_PLOTS,
    );
}

#[test]
fn preview_commits_nothing() {
    let bars = golden::bars_from_trades_csv(&mut TickBarBuilder::new(2), TRADES);
    let mut indicator = CloseCvd::new();
    golden::run_indicator(&mut indicator, &bars).expect("fixture evaluates");
    assert_eq!(indicator.plots().len(), 3);

    // A forming bar previews without appending a row, however often it runs.
    let partial = IndicatorBar {
        close: 104.0,
        ..bars[2]
    };
    for _ in 0..3 {
        let cvd = [-1.0, 1.0, -1.0, 2.0]; // committed cvd + staged preview value
        let mut ctx = Ctx {
            bar_index: 3,
            cvd: &cvd,
        };
        let frame = indicator
            .preview(&partial, &mut ctx)
            .expect("preview evaluates");
        assert_eq!(frame.values, vec![104.0, 2.0]);
    }
    assert_eq!(
        indicator.plots().len(),
        3,
        "previews must never commit a row"
    );
}

#[test]
fn reset_drops_all_rows() {
    let bars = golden::bars_from_trades_csv(&mut TickBarBuilder::new(2), TRADES);
    let mut indicator = CloseCvd::new();
    golden::run_indicator(&mut indicator, &bars).expect("fixture evaluates");
    indicator.reset();
    assert_eq!(indicator.plots().len(), 0);

    // A replay after reset reproduces the identical output — determinism
    // survives the reset path (spec switch, replay seek).
    golden::run_indicator(&mut indicator, &bars).expect("fixture re-evaluates");
    assert!(
        golden::diff_plot_csv(EXPECTED_PLOTS, &golden::plot_csv(&indicator)).is_none(),
        "replay after reset should match the golden fixture"
    );
}
