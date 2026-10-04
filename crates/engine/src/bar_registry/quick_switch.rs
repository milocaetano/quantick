//! A typed number read as every registered bar at that size — ProfitChart's
//! quick period switch. The registry, not a fixed list, decides what is offered.
//! A letter after the number (`50R`) narrows the list to the kind declaring it.
use super::{BarConfiguration, BarDefinition, BarRegistry, NumberKind};
use rust_decimal::Decimal;

/// How a bare number reads as a duration, in the order offered: minutes, as
/// ProfitChart reads it first, then seconds and hours.
pub const QUICK_DURATION_SCALES_MS: [i64; 3] = [60_000, 1_000, 3_600_000];

/// The most digits a typed number keeps: more than any bar rule reads.
const QUICK_MAX_DIGITS: usize = 9;

/// The longest query the switch holds: the number, then one suffix letter.
pub const QUICK_QUERY_MAX_CHARS: usize = QUICK_MAX_DIGITS + 1;

/// A kind's own name in the quick switch: the letter typed after the number
/// that lists this kind alone (`50R`), and the noun its row reads after the
/// number (`50 Ticks (Renko)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickAlias {
    pub suffix: char,
    pub noun: &'static str,
}

/// What the switch keeps of typed text: its digits, at most nine, then the
/// last letter typed — a second letter replaces the first rather than
/// spelling a word no kind declares.
#[must_use]
pub fn quick_query_text(typed: &str) -> String {
    let mut query: String = typed
        .chars()
        .filter(char::is_ascii_digit)
        .take(QUICK_MAX_DIGITS)
        .collect();
    query.extend(typed.chars().rev().find(char::is_ascii_alphabetic));
    query
}

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

    /// Every configuration typed text can mean: the number alone reads as
    /// every kind ([`Self::quick_candidates`]); a letter after it, in either
    /// case, keeps the kind whose [`QuickAlias`] declares that letter.
    /// Anything else — no number, two letters — means nothing.
    #[must_use]
    pub fn quick_matches(&self, query: &str) -> Vec<BarConfiguration> {
        let digits = query.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let mut letters = query[digits.len()..].chars();
        let suffix = letters.next();
        if letters.next().is_some() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Vec::new();
        }
        let Ok(number) = digits.parse::<u64>() else {
            return Vec::new();
        };
        let candidates = self.quick_candidates(number);
        match suffix {
            None => candidates,
            Some(letter) => candidates
                .into_iter()
                .filter(|config| {
                    config
                        .definition()
                        .quick_alias
                        .is_some_and(|alias| alias.suffix.eq_ignore_ascii_case(&letter))
                })
                .collect(),
        }
    }
}

impl BarConfiguration {
    /// The row the quick switch lists this configuration as: the kind's own
    /// noun after the number where it declares one (`50 Ticks (Renko)`),
    /// otherwise its summary (`tick(50)`).
    #[must_use]
    pub fn quick_label(self) -> String {
        let definition = self.definition();
        match definition.quick_alias {
            Some(alias) => format!(
                "{} {}",
                definition.parameter.format(self.parameter()),
                alias.noun
            ),
            None => self.summary(),
        }
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
