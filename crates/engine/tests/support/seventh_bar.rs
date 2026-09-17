//! A neutral extension fixture imported by engine, chart and runner tests.
use quantick_engine::bar_registry::{
    BarDefinition, NumberEditor, ParameterDescriptor, definitions::TICK,
};
use quantick_engine::{Bar, BarBuilder, TickBarBuilder, Trade};
use rust_decimal::prelude::ToPrimitive;

pub static SEVENTH: BarDefinition = BarDefinition {
    id: "probe",
    parameter: ParameterDescriptor {
        editor: NumberEditor {
            label: "probe count",
            ..TICK.parameter.editor
        },
        ..TICK.parameter
    },
    factory: |value, _| Box::new(Probe(TickBarBuilder::new(value.to_u64().unwrap()))),
    ..TICK
};

struct Probe(TickBarBuilder);
impl BarBuilder for Probe {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        self.0.push(trade)
    }
    fn partial(&self) -> Option<&Bar> {
        self.0.partial()
    }
}
