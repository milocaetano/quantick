use crate::{
    DollarBarBuilder,
    bar_registry::{BarDefinition, NumberEditor, NumberKind, ParameterDescriptor},
};
use rust_decimal::Decimal;
pub static DOLLAR: BarDefinition = BarDefinition {
    id: "dollar",
    parameter: ParameterDescriptor {
        name: "notional",
        unit: "quote_asset_notional",
        kind: NumberKind::Decimal,
        default: Decimal::from_parts(500_000, 0, 0, false, 0),
        editor: NumberEditor {
            label: "notional",
            min: 1000.0,
            max: 1_000_000_000.0,
            step: 1000.0,
            decimals: Some(0),
            ..super::COUNT_EDITOR
        },
    },
    choice_parameter: None,
    choices: &[],
    default_choice: None,
    requirements: super::VOLUME_INPUT,
    progress_unit: "notional",
    fixed_time_interval: false,
    factory: |value, _| Box::new(DollarBarBuilder::new(value)),
};
