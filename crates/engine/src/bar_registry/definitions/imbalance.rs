use crate::{
    ImbalanceBarBuilder, ImbalanceUnit,
    bar_registry::{
        BarDefinition, ChoiceDescriptor, NumberEditor, NumberKind, ParameterDescriptor,
    },
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
pub static IMBALANCE: BarDefinition = BarDefinition {
    id: "imbalance",
    parameter: ParameterDescriptor {
        name: "target",
        unit: "target_trades",
        kind: NumberKind::Count,
        default: Decimal::from_parts(100, 0, 0, false, 0),
        editor: NumberEditor {
            label: "target trades",
            min: 2.0,
            max: 1_000_000.0,
            step: 25.0,
            hover: "expected trades per bar in balanced flow — a real expectation, not a fixed length: one-sided aggression closes a bar well short of it, which is what this bar type is for",
            ..super::COUNT_EDITOR
        },
    },
    choice_parameter: Some("imbalance_unit"),
    choices: &[
        ChoiceDescriptor {
            id: "trades",
            hover: "θ sums ±1 per trade — tick imbalance bars",
            requirements: super::PRINTS,
        },
        ChoiceDescriptor {
            id: "volume",
            hover: "θ sums ±quantity — volume imbalance bars",
            requirements: super::VOLUME_INPUT,
        },
        ChoiceDescriptor {
            id: "dollar",
            hover: "θ sums ±(price × quantity) — dollar imbalance bars",
            requirements: super::VOLUME_INPUT,
        },
    ],
    default_choice: Some("trades"),
    requirements: super::PRINTS,
    progress_unit: "ticks",
    fixed_time_interval: false,
    factory: |value, choice| {
        Box::new(ImbalanceBarBuilder::with_unit(
            value.to_u64().expect("count representation"),
            ImbalanceUnit::parse_token(choice.expect("imbalance unit"))
                .expect("registered imbalance unit"),
        ))
    },
};
