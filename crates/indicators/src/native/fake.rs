//! A native that exists only to prove the port, behind the `fake-native`
//! feature.
//!
//! This file is the *whole* cost of a new native, and it is here to keep that
//! claim honest: it was written by copying `cvd.rs`, and it reaches the
//! toolbar, the workspace file and the layout restore through one entry in
//! [`super::NATIVES`] and nothing else. A test that drives it end to end is
//! therefore a test that a fourth *real* native needs no edit above this
//! crate. Delete the entry and this file and the port is unchanged.
//!
//! The feature is enabled by `quantick-app` in its `[dev-dependencies]` only,
//! so `cargo build --workspace` never ships it. Do not enable it anywhere a
//! trader can reach; `--all-features` would.

use super::{CVD_COLOR, PLOT_WIDTH_PT};
use crate::bar::IndicatorBar;
use crate::indicator::{Ctx, EvalError, Indicator, IndicatorDescriptor};
use crate::input::{InputSpec, InputValue};
use crate::output::{PlotBuffer, PlotId, PlotSpec, PlotStyle, PreviewFrame};

/// Plots the bar's close scaled by one integer input.
///
/// Declares an input on purpose: a native with none would not exercise the
/// save-and-restore of input values, which is half of what the port has to
/// carry.
pub struct Fake {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
    factor: i64,
}

impl Fake {
    /// A fake native with the given multiplier.
    #[must_use]
    pub fn new(factor: i64) -> Self {
        Self {
            descriptor: IndicatorDescriptor {
                title: format!("FAKE({factor})"),
                short_title: None,
                overlay: true,
                plots: vec![PlotSpec {
                    id: PlotId::new(0),
                    title: "fake".to_owned(),
                    style: PlotStyle::Line,
                    base_color: CVD_COLOR,
                    width: PLOT_WIDTH_PT,
                    offset: 0,
                    marker: None,
                }],
                fills: Vec::new(),
                inputs: vec![InputSpec::Int {
                    name: "factor".to_owned(),
                    title: "Factor".to_owned(),
                    default: 2,
                    min: Some(1),
                    max: None,
                    step: None,
                    options: Vec::new(),
                }],
            },
            plots: PlotBuffer::new(1),
            factor,
        }
    }

    /// The value this indicator commits for a bar.
    fn value(&self, bar: &IndicatorBar) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let factor = self.factor as f64;
        bar.close * factor
    }
}

impl Default for Fake {
    /// `FAKE(2)` — the multiplier the input spec declares.
    fn default() -> Self {
        Self::new(2)
    }
}

impl Indicator for Fake {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }

    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }

    fn on_close(&mut self, bar: &IndicatorBar, _ctx: &mut Ctx<'_>) -> Result<(), EvalError> {
        let value = self.value(bar);
        self.plots.push_row(&[value]);
        Ok(())
    }

    fn preview(
        &mut self,
        partial: &IndicatorBar,
        _ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError> {
        // No state of our own advances, so nothing needs snapshotting.
        Ok(PreviewFrame::new(vec![self.value(partial)]))
    }

    fn rebind(&self, values: &[InputValue]) -> Option<Box<dyn Indicator>> {
        let factor = match values.first() {
            Some(InputValue::Int(value)) => *value,
            _ => self.factor,
        };
        Some(Box::new(Self::new(factor)))
    }

    fn reset(&mut self) {
        self.plots.clear();
    }
}
