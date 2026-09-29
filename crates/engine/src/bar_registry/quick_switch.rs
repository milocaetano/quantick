//! A typed number read as every registered bar at that size — ProfitChart's
//! quick period switch. The registry, not a fixed list, decides what is offered.
use super::{BarConfiguration, BarDefinition, BarRegistry, NumberKind};
use rust_decimal::Decimal;

/// How a bare number reads as a duration, in the order offered: minutes, as
/// ProfitChart reads it first, then seconds and hours.
pub const QUICK_DURATION_SCALES_MS: [i64; 3] = [60_000, 1_000, 3_600_000];

impl BarRegistry {
    /// Every configuration `number` can mean, duration kinds first, then the
    /// others in registration order with each choice of a choice parameter.
    /// A reading its definition rejects (an interval past a day) is left out.
    #[must_use]
    pub fn quick_candidates(&self, number: u64) -> Vec<BarConfiguration> {
        let (durations, others): (Vec<_>, Vec<_>) = self
            .definitions()
            .iter()
            .copied()
            .partition(|definition| definition.parameter.kind == NumberKind::Duration);
        durations
            .into_iter()
            .chain(others)
            .flat_map(|definition| readings(definition, number))
            .collect()
    }
}

fn readings(definition: &'static BarDefinition, number: u64) -> Vec<BarConfiguration> {
    let values: Vec<Decimal> = if definition.parameter.kind == NumberKind::Duration {
        QUICK_DURATION_SCALES_MS
            .iter()
            .filter_map(|scale| i64::try_from(number).ok()?.checked_mul(*scale))
            .map(Decimal::from)
            .collect()
    } else {
        vec![Decimal::from(number)]
    };
    let choices: Vec<Option<&str>> = if definition.choices.is_empty() {
        vec![None]
    } else {
        definition
            .choices
            .iter()
            .map(|choice| Some(choice.id))
            .collect()
    };
    values
        .into_iter()
        .flat_map(|value| {
            choices
                .iter()
                .filter_map(move |choice| definition.configure(value, *choice).ok())
        })
        .collect()
}
