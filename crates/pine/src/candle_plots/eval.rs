//! Per-run OHLC and color evaluation; no retained state.
use super::{Eval, Value};
use crate::{
    ast::{Arg, NodeId},
    candle_plots::{BAR_ARGS, CANDLE_ARGS},
    error::PineError,
};
use quantick_indicators::{CandleSample, Rgba8};

impl Eval<'_> {
    pub(super) fn candle_plot(&mut self, id: NodeId, args: &[Arg]) -> Result<Value, PineError> {
        let Some(index) = self.script.plot_of_call[id.index()] else {
            return Ok(Value::Na);
        };
        let is_bar = self.script.candle_plots[index].is_bar;
        let names = if is_bar { BAR_ARGS } else { CANDLE_ARGS };
        let mut prices = [f64::NAN; 4];
        let mut color = Some(Rgba8::opaque(33, 150, 243));
        let mut wick_color = color;
        let mut border_color = color;
        // Evaluate every supplied expression once, in source order, even when
        // another component is na; stateful arguments must still advance.
        let mut position = 0;
        for arg in args {
            let name = if let Some(name) = arg.name.as_deref() {
                name
            } else {
                let name = names[position];
                position += 1;
                name
            };
            let value = self.eval(arg.value)?;
            if let Some(component) = ["open", "high", "low", "close"]
                .iter()
                .position(|n| *n == name)
            {
                prices[component] = match value {
                    Value::Num(n) => n,
                    Value::Na => f64::NAN,
                    other => return Err(self.type_err(id, "a number", &other)),
                };
            } else if matches!(name, "color" | "wickcolor" | "bordercolor") {
                let value = match value {
                    Value::Color(c) => Some(Rgba8::from_u32(c)),
                    value if value.is_na() => None,
                    other => return Err(self.type_err(id, "a color", &other)),
                };
                match name {
                    "color" => color = value,
                    "wickcolor" => wick_color = value,
                    _ => border_color = value,
                }
            }
        }
        // Each omitted color uses the Pine default; explicit na stays absent.
        if is_bar {
            wick_color = None;
            border_color = None;
        }
        self.candles[index] = (!prices.iter().any(|n| n.is_nan())).then(|| CandleSample {
            open: prices[0],
            high: prices.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            low: prices.iter().copied().fold(f64::INFINITY, f64::min),
            close: prices[3],
            color,
            wick_color,
            border_color,
        });
        Ok(Value::Na)
    }
}
