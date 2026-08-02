//! Golden test for the smoothing kernels through the full pipeline: trades →
//! tick bars → an indicator built on `ta::{Sma, Ema, Wma}` → committed plot
//! CSV, twice, byte-identical, against hand-computed expectations.
//!
//! The unit tests in `ta::smooth` prove each kernel against hand references;
//! this file proves the kernels survive the *contract* — warmup rows commit
//! as NaN, values flow through commit runs in order, and the whole pipeline
//! is deterministic.

use quantick_engine::TickBarBuilder;
use quantick_indicators::{
    Ctx, EvalError, Indicator, IndicatorBar, IndicatorDescriptor, PlotBuffer, PlotId, PlotSpec,
    PlotStyle, PreviewFrame, Rgba8, golden,
    ta::{Ema, Sma, Wma},
};

const TRADES: &str = include_str!("fixtures/trades_ramp.csv");
const EXPECTED_PLOTS: &str = include_str!("fixtures/ta_smooth_plots.csv");
const LEN: usize = 3;

/// sma(close, 3) + ema(close, 3) + wma(close, 3), the M1 golden trio.
struct TaSmooth {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
    sma: Sma,
    ema: Ema,
    wma: Wma,
}

impl TaSmooth {
    fn new() -> Self {
        let plot = |index: usize, title: &str| PlotSpec {
            id: PlotId::new(index),
            title: title.to_owned(),
            style: PlotStyle::Line,
            base_color: Rgba8::opaque(255, 255, 255),
            width: 1.0,
            offset: 0,
        };
        Self {
            descriptor: IndicatorDescriptor {
                title: "ta smooth (test)".to_owned(),
                short_title: None,
                overlay: true,
                plots: vec![plot(0, "sma3"), plot(1, "ema3"), plot(2, "wma3")],
                inputs: Vec::new(),
            },
            plots: PlotBuffer::new(3),
            sma: Sma::new(LEN),
            ema: Ema::new(LEN),
            wma: Wma::new(LEN),
        }
    }
}

impl Indicator for TaSmooth {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, bar: &IndicatorBar, _ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        let row = [
            self.sma.push(bar.close),
            self.ema.push(bar.close),
            self.wma.push(bar.close),
        ];
        self.plots.push_row(&row);
        Ok(())
    }

    fn preview(
        &mut self,
        partial: &IndicatorBar,
        _ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        // Clone-small-state snapshot (the locked preview discipline): kernels
        // are a few f64s each, so previewing on copies costs nothing and the
        // committed kernels never advance.
        let mut sma = self.sma.clone();
        let mut ema = self.ema.clone();
        let mut wma = self.wma.clone();
        Ok(PreviewFrame::new(vec![
            sma.push(partial.close),
            ema.push(partial.close),
            wma.push(partial.close),
        ]))
    }

    fn reset(&mut self) {
        self.plots.clear();
        self.sma = Sma::new(LEN);
        self.ema = Ema::new(LEN);
        self.wma = Wma::new(LEN);
    }
}

#[test]
fn golden_smoothing_kernels_over_ramp() {
    golden::assert_golden(
        TaSmooth::new,
        || TickBarBuilder::new(1),
        TRADES,
        EXPECTED_PLOTS,
    );
}

#[test]
fn kernel_preview_never_advances_committed_state() {
    let bars = golden::bars_from_trades_csv(&mut TickBarBuilder::new(1), TRADES);
    let mut indicator = TaSmooth::new();

    // Interleave three previews before every commit — the classic
    // double-advance bug this design exists to prevent. If preview advanced
    // any kernel, the committed columns would diverge from the golden run.
    let mut cvd_scratch = Vec::new();
    let mut sum = 0.0;
    for (bar_index, bar) in bars.iter().enumerate() {
        for _ in 0..3 {
            let mut ctx = Ctx {
                bar_index,
                cvd: &cvd_scratch,
            };
            indicator.preview(bar, &mut ctx).expect("preview evaluates");
        }
        sum += bar.delta();
        cvd_scratch.push(sum);
        let mut ctx = Ctx {
            bar_index,
            cvd: &cvd_scratch,
        };
        indicator.on_close(bar, &mut ctx).expect("commit evaluates");
    }

    assert!(
        golden::diff_plot_csv(EXPECTED_PLOTS, &golden::plot_csv(&indicator)).is_none(),
        "previews between commits must not change committed output"
    );
}
