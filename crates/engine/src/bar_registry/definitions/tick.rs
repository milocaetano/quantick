use crate::{
    TickBarBuilder,
    bar_registry::{BarDefinition, NumberKind, ParameterDescriptor},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
pub static TICK: BarDefinition = BarDefinition {
    id: "tick",
    parameter: ParameterDescriptor {
        name: "count",
        unit: "trades",
        kind: NumberKind::Count,
        default: Decimal::from_parts(50, 0, 0, false, 0),
        editor: super::COUNT_EDITOR,
    },
    choice_parameter: None,
    choices: &[],
    default_choice: None,
    requirements: super::PRINTS,
    progress_unit: "ticks",
    fixed_time_interval: false,
    factory: |value, _| {
        Box::new(TickBarBuilder::new(
            value.to_u64().expect("count representation"),
        ))
    },
};
