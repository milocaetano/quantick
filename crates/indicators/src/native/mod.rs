//! Native reference indicators.
//!
//! Hand-written Rust implementations of the [`Indicator`] contract. They
//! exist for three reasons: they prove the whole pipe (host → worker →
//! render) before the script frontend lands (M1 of the plan), they are the
//! performance reference a scripted equivalent is benchmarked against, and
//! they double as living documentation of how an indicator is written.
//!
//! Both follow the preview discipline the trait demands: kernels are cloned
//! for the preview push (clone-small-state, a few f64s) so committed state
//! never advances inside a forming bar.

use crate::bar::IndicatorBar;
use crate::indicator::{Ctx, EvalError, Indicator, IndicatorDescriptor};
use crate::input::{InputSpec, SourceId};
use crate::output::{PlotBuffer, PlotId, PlotSpec, PlotStyle, PreviewFrame, Rgba8};
use crate::ta;

/// Default EMA look and defaults (settings UI overrides come with M4).
const EMA_COLOR: Rgba8 = Rgba8::opaque(255, 179, 0);
const EMA_DEFAULT_LEN: i64 = 9;
const PLOT_WIDTH: f32 = 1.5;

/// CVD pane color.
const CVD_COLOR: Rgba8 = Rgba8::opaque(41, 182, 246);

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
                    width: PLOT_WIDTH,
                    offset: 0,
                }],
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

    fn reset(&mut self) {
        self.plots.clear();
        self.kernel = ta::Ema::new(self.len);
    }
}

/// Cumulative volume delta, read straight from the host's shared series —
/// the "hello, order flow" of the pane indicators.
pub struct Cvd {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
}

impl Cvd {
    /// A CVD pane indicator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            descriptor: IndicatorDescriptor {
                title: "CVD".to_owned(),
                short_title: None,
                overlay: false,
                plots: vec![PlotSpec {
                    id: PlotId::new(0),
                    title: "cvd".to_owned(),
                    style: PlotStyle::Line,
                    base_color: CVD_COLOR,
                    width: PLOT_WIDTH,
                    offset: 0,
                }],
                inputs: Vec::new(),
            },
            plots: PlotBuffer::new(1),
        }
    }
}

impl Default for Cvd {
    fn default() -> Self {
        Self::new()
    }
}

impl Indicator for Cvd {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, _bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        self.plots.push_row(&[ctx.cvd_now()]);
        Ok(())
    }

    fn preview(
        &mut self,
        _partial: &IndicatorBar,
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        // The host stages the forming bar's cvd as the slice's last element;
        // there is no state of our own to snapshot.
        Ok(PreviewFrame::new(vec![ctx.cvd_now()]))
    }

    fn reset(&mut self) {
        self.plots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_overlay_follows_the_source_scale() {
        assert!(Ema::new(9, SourceId::Close).descriptor().overlay);
        assert!(Ema::new(9, SourceId::Hl2).descriptor().overlay);
        assert!(
            !Ema::new(9, SourceId::Delta).descriptor().overlay,
            "an EMA of delta lives on the flow scale, not the price scale"
        );
        assert!(!Ema::new(9, SourceId::Cvd).descriptor().overlay);
    }

    #[test]
    fn ema_title_names_non_default_sources() {
        assert_eq!(Ema::new(9, SourceId::Close).descriptor().title, "EMA(9)");
        assert_eq!(
            Ema::new(21, SourceId::Delta).descriptor().title,
            "EMA(21, delta)"
        );
    }

    #[test]
    fn descriptors_declare_their_settings() {
        let ema = Ema::default();
        assert_eq!(ema.descriptor().inputs.len(), 2);
        assert_eq!(ema.descriptor().inputs[0].name(), "len");
        assert!(Cvd::new().descriptor().inputs.is_empty());
    }
}
