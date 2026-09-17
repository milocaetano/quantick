use crate::{
    TimeBarBuilder,
    bar_registry::{
        BarDefinition, DEFAULT_TIME_INTERVAL_MS, MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS,
        NumberEditor, NumberKind, ParameterDescriptor, TIME_INTERVAL_DRAG_SPEED, TIME_PRESETS,
    },
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
pub static TIME: BarDefinition = BarDefinition {
    id: "time",
    parameter: ParameterDescriptor {
        name: "interval_ms",
        unit: "milliseconds",
        kind: NumberKind::Duration,
        default: Decimal::from_parts(DEFAULT_TIME_INTERVAL_MS as u32, 0, 0, false, 0),
        editor: NumberEditor {
            label: "",
            min: MIN_TIME_INTERVAL_MS as f64,
            max: MAX_TIME_INTERVAL_MS as f64,
            step: TIME_INTERVAL_DRAG_SPEED,
            hover: "custom interval, in milliseconds",
            presets: &TIME_PRESETS,
            ..super::COUNT_EDITOR
        },
    },
    choice_parameter: None,
    choices: &[],
    default_choice: None,
    requirements: super::PRINTS,
    progress_unit: "ms",
    fixed_time_interval: true,
    factory: |value, _| {
        Box::new(TimeBarBuilder::new(
            value.to_i64().expect("interval representation"),
        ))
    },
};
