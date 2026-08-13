//! Anchored VWAP as a native [`Indicator`] — TradingView's model.
//!
//! From the anchor bar onward (a bar participates when its `close_time`
//! reaches the anchor), three running sums produce everything:
//!
//! ```text
//! vwap  = Σ(src·vol) / Σvol
//! σ²    = Σ(src²·vol) / Σvol − vwap²      (clamped at 0)
//! band  = vwap ± k·σ                       (three optional pairs)
//! ```
//!
//! Bars before the anchor plot `na` and never touch the accumulators, so a
//! re-anchor is a plain rebind + replay — no incremental un-anchoring.

use super::{AVWAP_BAND_MULTS, AVWAP_COLOR, AVWAP_FILL_COLORS, AVWAP_LINE_COLORS, PLOT_WIDTH_PT};
use crate::bar::IndicatorBar;
use crate::indicator::{Ctx, EvalError, Indicator, IndicatorDescriptor};
use crate::input::{InputSpec, InputValue, SourceId};
use crate::output::{FillSpec, PlotBuffer, PlotId, PlotSpec, PlotStyle, PreviewFrame};

/// Number of optional standard-deviation band pairs.
pub const AVWAP_BAND_PAIRS: usize = 3;

/// One plot column per value: vwap + (upper, lower) per band pair.
/// Public for the same reason as [`AVWAP_BAND_PAIRS`]: a consumer caching
/// rows sizes them from here, never from a copied literal.
pub const AVWAP_PLOT_COUNT: usize = 1 + 2 * AVWAP_BAND_PAIRS;

/// The volume-weighted running state — three sums, nothing else.
#[derive(Debug, Clone, Copy, Default)]
struct Sums {
    /// Σ source·volume
    pv: f64,
    /// Σ volume
    v: f64,
    /// Σ source²·volume
    pv2: f64,
}

impl Sums {
    fn add(&mut self, source: f64, volume: f64) {
        self.pv += source * volume;
        self.v += volume;
        self.pv2 += source * source * volume;
    }
}

/// Volume-weighted average price anchored at a user-chosen instant, with up
/// to three σ-band pairs.
///
/// `Clone` is part of the contract: a consumer that snapshots committed
/// state (the chart's per-object cache does, for undo records) clones the
/// whole indicator — plots and sums together, so the copy can keep
/// evaluating from where the original stood.
#[derive(Debug, Clone)]
pub struct AnchoredVwap {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
    /// Epoch-ms instant the accumulation starts at.
    anchor_ms: i64,
    source: SourceId,
    /// Per pair: (enabled, σ multiplier).
    bands: [(bool, f64); AVWAP_BAND_PAIRS],
    sums: Sums,
}

impl AnchoredVwap {
    /// An anchored VWAP of `source` starting at `anchor_ms` (epoch ms).
    #[must_use]
    pub fn new(anchor_ms: i64, source: SourceId, bands: [(bool, f64); AVWAP_BAND_PAIRS]) -> Self {
        let plot = |index: usize, title: String, color| PlotSpec {
            id: PlotId::new(index),
            title,
            style: PlotStyle::Line,
            base_color: color,
            width: PLOT_WIDTH_PT,
            offset: 0,
            marker: None,
        };
        let mut plots = vec![plot(0, "vwap".to_owned(), AVWAP_COLOR)];
        let mut fills = Vec::with_capacity(AVWAP_BAND_PAIRS);
        for pair in 0..AVWAP_BAND_PAIRS {
            let upper = 1 + 2 * pair;
            plots.push(plot(
                upper,
                format!("upper_{}", pair + 1),
                AVWAP_LINE_COLORS[pair],
            ));
            plots.push(plot(
                upper + 1,
                format!("lower_{}", pair + 1),
                AVWAP_LINE_COLORS[pair],
            ));
            fills.push(FillSpec {
                a: PlotId::new(upper),
                b: PlotId::new(upper + 1),
                color: AVWAP_FILL_COLORS[pair],
            });
        }
        let mut inputs = vec![
            InputSpec::Int {
                name: "anchor_time".to_owned(),
                title: "Anchor time (ms)".to_owned(),
                default: 0,
                min: None,
                max: None,
                step: None,
                options: Vec::new(),
            },
            InputSpec::Source {
                name: "source".to_owned(),
                title: "Source".to_owned(),
                default: SourceId::Hlc3,
            },
        ];
        for (pair, mult) in AVWAP_BAND_MULTS.iter().enumerate() {
            inputs.push(InputSpec::Bool {
                name: format!("band_{}", pair + 1),
                title: format!("Bands #{}", pair + 1),
                default: pair == 0,
            });
            inputs.push(InputSpec::Float {
                name: format!("mult_{}", pair + 1),
                title: format!("Bands #{} multiplier", pair + 1),
                default: *mult,
                min: Some(0.0),
                max: None,
                step: Some(0.5),
                options: Vec::new(),
            });
        }
        let descriptor = IndicatorDescriptor {
            title: if source == SourceId::Hlc3 {
                "Anchored VWAP".to_owned()
            } else {
                format!("Anchored VWAP ({})", source.as_str())
            },
            short_title: Some("AVWAP".to_owned()),
            overlay: source.is_price_scaled(),
            plots,
            inputs,
            fills,
        };
        Self {
            plots: PlotBuffer::for_plots(&descriptor.plots),
            descriptor,
            anchor_ms,
            source,
            bands,
            sums: Sums::default(),
        }
    }

    /// One evaluated row from the running sums; all-`na` before any volume.
    fn row(&self, sums: Sums) -> [f64; AVWAP_PLOT_COUNT] {
        let mut row = [f64::NAN; AVWAP_PLOT_COUNT];
        if sums.v <= 0.0 {
            return row;
        }
        let vwap = sums.pv / sums.v;
        let variance = (sums.pv2 / sums.v - vwap * vwap).max(0.0);
        let sigma = variance.sqrt();
        row[0] = vwap;
        for (pair, &(enabled, mult)) in self.bands.iter().enumerate() {
            if enabled {
                row[1 + 2 * pair] = vwap + mult * sigma;
                row[2 + 2 * pair] = vwap - mult * sigma;
            }
        }
        row
    }

    /// The sums as they stand after `bar`, without committing them.
    fn staged(&self, bar: &IndicatorBar, cvd: f64) -> Sums {
        let mut sums = self.sums;
        if bar.close_time >= self.anchor_ms {
            sums.add(self.source.value(bar, cvd), bar.volume());
        }
        sums
    }
}

impl Default for AnchoredVwap {
    /// Anchor at epoch 0 (everything participates), hlc3, 1σ band on.
    fn default() -> Self {
        Self::new(
            0,
            SourceId::Hlc3,
            [
                (true, AVWAP_BAND_MULTS[0]),
                (false, AVWAP_BAND_MULTS[1]),
                (false, AVWAP_BAND_MULTS[2]),
            ],
        )
    }
}

impl Indicator for AnchoredVwap {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        self.sums = self.staged(bar, ctx.cvd_now());
        let row = self.row(self.sums);
        self.plots.push_row(&row);
        Ok(())
    }

    fn preview(
        &mut self,
        partial: &IndicatorBar,
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        let row = self.row(self.staged(partial, ctx.cvd_now()));
        Ok(PreviewFrame::new(row.to_vec()))
    }

    fn input_values(&self) -> Vec<InputValue> {
        let mut values = vec![
            InputValue::Int(self.anchor_ms),
            InputValue::Source(self.source),
        ];
        for &(enabled, mult) in &self.bands {
            values.push(InputValue::Bool(enabled));
            values.push(InputValue::Float(mult));
        }
        values
    }

    fn rebind(&self, values: &[InputValue]) -> Option<Box<dyn Indicator>> {
        let anchor_ms = match values.first() {
            Some(InputValue::Int(v)) => *v,
            _ => self.anchor_ms,
        };
        let source = match values.get(1) {
            Some(InputValue::Source(s)) => *s,
            _ => self.source,
        };
        let mut bands = self.bands;
        for (pair, band) in bands.iter_mut().enumerate() {
            if let Some(InputValue::Bool(enabled)) = values.get(2 + 2 * pair) {
                band.0 = *enabled;
            }
            if let Some(InputValue::Float(mult)) = values.get(3 + 2 * pair) {
                band.1 = mult.max(0.0);
            }
        }
        Some(Box::new(Self::new(anchor_ms, source, bands)))
    }

    fn reset(&mut self) {
        self.plots.clear();
        self.sums = Sums::default();
    }
}
