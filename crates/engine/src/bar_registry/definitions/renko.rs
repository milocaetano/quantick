use crate::{
    RenkoBarBuilder,
    bar_registry::{
        BarDefinition, InputRequirements, NumberEditor, NumberKind, ParameterDescriptor,
        QuickAlias,
    },
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
pub static RENKO: BarDefinition = BarDefinition {
    id: "renko",
    parameter: ParameterDescriptor {
        name: "ticks",
        unit: "ticks",
        kind: NumberKind::Count,
        default: Decimal::from_parts(50, 0, 0, false, 0),
        minimum: Some(Decimal::from_parts(2, 0, 0, false, 0)),
        editor: NumberEditor {
            min: 2.0,
            hover: "ProfitChart's Renko: a brick is N - 1 price steps tall and closes once a print \
                    trades a step past its far edge",
            ..super::COUNT_EDITOR
        },
    },
    choice_parameter: None,
    choices: &[],
    default_choice: None,
    requirements: InputRequirements {
        price_step: true,
        ..super::PRINTS
    },
    progress_unit: "ticks",
    fixed_time_interval: false,
    quick_alias: Some(QuickAlias {
        suffix: 'R',
        noun: "Ticks (Renko)",
    }),
    factory: |value, _, facts| {
        Box::new(RenkoBarBuilder::new(
            value.to_u64().expect("count representation"),
            facts.price_step,
        ))
    },
};
