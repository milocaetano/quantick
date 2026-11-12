//! Candle volume, with a rollback-safe optional moving average.

use crate::{
    Ctx, EvalError, Indicator, IndicatorBar, IndicatorDescriptor, InputSpec, InputValue,
    PlotBuffer, PlotId, PlotSpec, PlotStyle, PreviewFrame, Rgba8, ta,
};

/// The retained bar quantity can have different provenance in older history.
pub const VOLUME_SOURCE_NOTE: &str = "Reported source volume; historical bars may use tick counts when traded volume is unavailable.";

const DEFAULT_LENGTH: usize = 20;
const MAX_LENGTH: usize = 500;

/// Volume reuses the bar's total quantity, including the feed's tick units.
/// Its consumer labels those units from source capabilities.
pub struct Volume {
    descriptor: IndicatorDescriptor,
    plots: PlotBuffer,
    average: ta::Sma,
    length: usize,
    show_average: bool,
}

impl Volume {
    /// Construct a volume histogram; the SMA is opt-in.
    #[must_use]
    pub fn new(length: usize, show_average: bool) -> Self {
        let length = length.clamp(1, MAX_LENGTH);
        let plot = |index, title: &str, style, color| PlotSpec {
            id: PlotId::new(index),
            title: title.into(),
            style,
            base_color: color,
            width: 1.5,
            offset: 0,
            marker: None,
        };
        Self {
            descriptor: IndicatorDescriptor {
                title: "Volume".into(),
                short_title: None,
                overlay: true,
                plots: vec![
                    plot(
                        0,
                        "Growing",
                        PlotStyle::Columns,
                        Rgba8::new(38, 166, 154, 120),
                    ),
                    plot(
                        1,
                        "Falling",
                        PlotStyle::Columns,
                        Rgba8::new(239, 83, 80, 120),
                    ),
                    plot(
                        2,
                        "Volume MA",
                        PlotStyle::Line,
                        Rgba8::opaque(100, 140, 245),
                    ),
                ],
                inputs: vec![
                    InputSpec::Bool {
                        name: "show_average".into(),
                        title: "Volume MA".into(),
                        default: false,
                    },
                    InputSpec::Int {
                        name: "length".into(),
                        title: "MA length".into(),
                        default: DEFAULT_LENGTH as i64,
                        min: Some(1),
                        max: Some(MAX_LENGTH as i64),
                        step: Some(1),
                        options: Vec::new(),
                    },
                ],
                fills: Vec::new(),
            },
            plots: PlotBuffer::new(3),
            average: ta::Sma::new(length),
            length,
            show_average,
        }
    }

    fn row(bar: &IndicatorBar, average: &mut ta::Sma, show_average: bool) -> [f64; 3] {
        let raw = bar.volume();
        let value = if raw.is_finite() && raw >= 0.0 {
            raw
        } else {
            f64::NAN
        };
        let ma = average.push(value);
        let growing = bar.close >= bar.open;
        [
            if growing { value } else { f64::NAN },
            if growing { f64::NAN } else { value },
            if show_average { ma } else { f64::NAN },
        ]
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::new(DEFAULT_LENGTH, false)
    }
}

impl Indicator for Volume {
    fn descriptor(&self) -> &IndicatorDescriptor {
        &self.descriptor
    }
    fn plots(&self) -> &PlotBuffer {
        &self.plots
    }
    fn on_close(&mut self, bar: &IndicatorBar, _: &mut Ctx<'_>) -> Result<(), EvalError> {
        self.plots
            .push_row(&Self::row(bar, &mut self.average, self.show_average));
        Ok(())
    }
    fn preview(&mut self, bar: &IndicatorBar, _: &mut Ctx<'_>) -> Result<PreviewFrame, EvalError> {
        Ok(PreviewFrame::new(
            Self::row(bar, &mut self.average.clone(), self.show_average).to_vec(),
        ))
    }
    fn input_values(&self) -> Vec<InputValue> {
        vec![
            InputValue::Bool(self.show_average),
            InputValue::Int(self.length as i64),
        ]
    }
    fn rebind(&self, values: &[InputValue]) -> Option<Box<dyn Indicator>> {
        let show = match values.first() {
            Some(InputValue::Bool(v)) => *v,
            _ => self.show_average,
        };
        let length = match values.get(1) {
            Some(InputValue::Int(v)) => (*v).clamp(1, MAX_LENGTH as i64) as usize,
            _ => self.length,
        };
        Some(Box::new(Self::new(length, show)))
    }
    fn reset(&mut self) {
        self.plots.clear();
        self.average = ta::Sma::new(self.length);
    }
}

#[cfg(test)]
#[path = "volume_tests.rs"]
mod tests;
