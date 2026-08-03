//! Cumulative volume delta as a native [`Indicator`] — the pane reference.

use super::{CVD_COLOR, PLOT_WIDTH_PT};
use crate::bar::IndicatorBar;
use crate::indicator::{Ctx, EvalError, Indicator, IndicatorDescriptor};
use crate::output::{PlotBuffer, PlotId, PlotSpec, PlotStyle, PreviewFrame};

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
                    width: PLOT_WIDTH_PT,
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
        Ok(PreviewFrame {
            values: vec![ctx.cvd_now()],
        })
    }

    fn reset(&mut self) {
        self.plots.clear();
    }
}
