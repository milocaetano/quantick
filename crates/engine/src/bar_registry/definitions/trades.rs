use crate::{
    DealBarBuilder,
    bar_registry::{
        BarDefinition, InputRequirements, NumberEditor, NumberKind, ParameterDescriptor,
    },
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
pub static TRADES: BarDefinition = BarDefinition {
    id: "trades",
    parameter: ParameterDescriptor {
        name: "count",
        unit: "deals",
        kind: NumberKind::Count,
        default: Decimal::from_parts(2000, 0, 0, false, 0),
        editor: NumberEditor {
            label: "N deals",
            max: 100_000.0,
            step: 50.0,
            ..super::COUNT_EDITOR
        },
    },
    choice_parameter: None,
    choices: &[],
    default_choice: None,
    requirements: InputRequirements {
        deal_counter: true,
        ..super::PRINTS
    },
    progress_unit: "deals",
    fixed_time_interval: false,
    factory: |value, _| {
        Box::new(DealBarBuilder::new(
            value.to_u64().expect("count representation"),
        ))
    },
};
