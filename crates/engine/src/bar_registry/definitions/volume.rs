use crate::{
    VolumeBarBuilder,
    bar_registry::{BarDefinition, NumberEditor, NumberKind, ParameterDescriptor},
};
use rust_decimal::Decimal;
pub static VOLUME: BarDefinition = BarDefinition {
    id: "volume",
    parameter: ParameterDescriptor {
        name: "quantity",
        unit: "base_asset_quantity",
        kind: NumberKind::Decimal,
        default: Decimal::from_parts(5, 0, 0, false, 0),
        editor: NumberEditor {
            label: "units",
            min: 0.1,
            max: 1000.0,
            step: 0.1,
            decimals: Some(1),
            ..super::COUNT_EDITOR
        },
    },
    choice_parameter: None,
    choices: &[],
    default_choice: None,
    requirements: super::VOLUME_INPUT,
    progress_unit: "vol",
    fixed_time_interval: false,
    factory: |value, _| Box::new(VolumeBarBuilder::new(value)),
};
