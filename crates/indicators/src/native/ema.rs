//! `ta.ema` as a native [`Indicator`] — the overlay reference.

use super::{EMA_COLOR, EMA_DEFAULT_LEN, PLOT_WIDTH_PT};
use crate::bar::IndicatorBar;
use crate::indicator::{Ctx, EvalError, Indicator, IndicatorDescriptor};
use crate::input::{InputSpec, InputValue, SourceId};
use crate::output::{PlotBuffer, PlotId, PlotSpec, PlotStyle, PreviewFrame};
use crate::ta;

/// Exponential moving average of a selectable source.
///
/// Overlays the price chart when the source is price-scaled; an EMA of a
/// flow series (delta, cvd, …) renders in its own pane — the scale decides,
/// not the indicator kind.
pub struct Ema {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
    kernel: ta::Ema,
    len: usize,
    source: SourceId,
}

impl Ema {
    /// An EMA of `source` over `len` bars.
    ///
    /// # Panics
    ///
    /// Panics if `len == 0` (the kernel's honesty rule).
    #[must_use]
    pub fn new(len: usize, source: SourceId) -> Self {
        let title = if source == SourceId::Close {
            format!("EMA({len})")
        } else {
            format!("EMA({len}, {})", source.as_str())
        };
        Self {
            descriptor: IndicatorDescriptor {
                title: title.clone(),
                short_title: None,
                overlay: source.is_price_scaled(),
                plots: vec![PlotSpec {
                    id: PlotId::new(0),
                    title: "ema".to_owned(),
                    style: PlotStyle::Line,
                    base_color: EMA_COLOR,
                    width: PLOT_WIDTH_PT,
                    offset: 0,
                    marker: None,
                }],
                fills: Vec::new(),
                inputs: vec![
                    InputSpec::Int {
                        name: "len".to_owned(),
                        title: "Length".to_owned(),
                        default: EMA_DEFAULT_LEN,
                        min: Some(1),
                        max: None,
                        step: None,
                        options: Vec::new(),
                    },
                    InputSpec::Source {
                        name: "source".to_owned(),
                        title: "Source".to_owned(),
                        default: SourceId::Close,
                    },
                ],
            },
            plots: PlotBuffer::new(1),
            kernel: ta::Ema::new(len),
            len,
            source,
        }
    }
}

impl Default for Ema {
    /// `EMA(9)` of close — the classic.
    fn default() -> Self {
        Self::new(EMA_DEFAULT_LEN as usize, SourceId::Close)
    }
}

impl Indicator for Ema {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        let value = self.kernel.push(self.source.value(bar, ctx.cvd_now()));
        self.plots.push_row(&[value]);
        Ok(())
    }

    fn preview(
        &mut self,
        partial: &IndicatorBar,
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        let mut kernel = self.kernel.clone();
        Ok(PreviewFrame::new(vec![
            kernel.push(self.source.value(partial, ctx.cvd_now())),
        ]))
    }

    fn input_values(&self) -> Vec<InputValue> {
        // What this instance is actually running with, not what a fresh one
        // would declare.
        vec![
            InputValue::Int(i64::try_from(self.len).unwrap_or(i64::MAX)),
            InputValue::Source(self.source),
        ]
    }

    fn rebind(&self, values: &[InputValue]) -> Option<Box<dyn Indicator>> {
        let len = match values.first() {
            Some(InputValue::Int(v)) => usize::try_from((*v).max(1)).unwrap_or(1),
            _ => self.len,
        };
        let source = match values.get(1) {
            Some(InputValue::Source(s)) => *s,
            _ => self.source,
        };
        Some(Box::new(Self::new(len, source)))
    }

    fn reset(&mut self) {
        self.plots.clear();
        self.kernel = ta::Ema::new(self.len);
    }
}
