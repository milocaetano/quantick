//! A typed number read as every registered bar at that size — ProfitChart's
//! quick period switch. The registry, not a fixed list, decides what is offered.
//! A letter after the number (`50R`) narrows the list to the kind declaring it;
//! a unit after it (`5m`, `1d`, `1w`, `1mo`) to the time bars in that unit.
use super::{BarConfiguration, BarDefinition, BarRegistry, NumberKind, interval_unit_ms};
use rust_decimal::Decimal;

/// How a bare number reads as a duration, in the order offered: minutes, as
/// ProfitChart reads it first, then seconds and hours.
pub const QUICK_DURATION_SCALES_MS: [i64; 3] = [60_000, 1_000, 3_600_000];

/// The most digits a typed number keeps: more than any bar rule reads.
const QUICK_MAX_DIGITS: usize = 9;

/// A kind's own name in the quick switch: the letter typed after the number
/// that lists this kind alone (`50R`), and the noun its row reads after the
/// number (`50 Ticks (Renko)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickAlias {
    pub suffix: char,
    pub noun: &'static str,
}

/// The one two-letter suffix the switch reads: calendar months (`1mo`). Every
/// other suffix is a single letter, so `m` stays minutes.
const MONTH_SUFFIX: &str = "mo";

/// What the switch keeps of typed text: its digits, at most nine, then the
/// last letter typed — a second letter replaces the first rather than
/// spelling a word no kind declares. The one exception is `mo`, kept whole
/// when it ends the text, so typing `1m` then `o` reads as a month.
///
/// This is the switch's only length limit. A field capping its own length
/// would refuse the letter typed after a full query, which has to reach this
/// to replace the letter there.
#[must_use]
pub fn quick_query_text(typed: &str) -> String {
    let mut query: String = typed
        .chars()
        .filter(char::is_ascii_digit)
        .take(QUICK_MAX_DIGITS)
        .collect();
    let letters: String = typed.chars().filter(char::is_ascii_alphabetic).collect();
    let tail = letters.len().saturating_sub(MONTH_SUFFIX.len());
    if letters.len() >= MONTH_SUFFIX.len() && letters[tail..].eq_ignore_ascii_case(MONTH_SUFFIX) {
        query.push_str(&letters[tail..]);
    } else {
        query.extend(letters.chars().last());
    }
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
    /// case, keeps the kind whose [`QuickAlias`] declares that letter, and a
    /// duration unit (`s`, `m`, `h`, `d`, `w`, `mo`) keeps the duration kinds
    /// read in that unit — `1d`, `1w` and `1mo` are the daily, weekly and
    /// monthly time bars. Anything else — no number, two letters — means
    /// nothing.
    #[must_use]
    pub fn quick_matches(&self, query: &str) -> Vec<BarConfiguration> {
        let digits = query.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let suffix = &query[digits.len()..];
        let unit = interval_unit_ms(suffix).filter(|_| !suffix.eq_ignore_ascii_case("ms"));
        if (suffix.len() > 1 && unit.is_none()) || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Vec::new();
        }
        let Ok(number) = digits.parse::<u64>() else {
            return Vec::new();
        };
        let Some(letter) = suffix.chars().next() else {
            return self.quick_candidates(number);
        };
        let durations = unit.into_iter().flat_map(|scale| {
            self.definitions()
                .iter()
                .copied()
                .filter(|definition| definition.parameter.kind == NumberKind::Duration)
                .flat_map(move |definition| scaled_readings(definition, number, &[scale]))
        });
        let aliased = self.quick_candidates(number).into_iter().filter(|config| {
            config
                .definition()
                .quick_alias
                .is_some_and(|alias| alias.suffix.eq_ignore_ascii_case(&letter))
        });
        durations.chain(aliased).collect()
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
    scaled_readings(definition, number, &QUICK_DURATION_SCALES_MS)
}

/// `number` read as `definition` at each duration scale given (a count kind
/// ignores the scales), with every choice, dropping what the definition
/// rejects.
fn scaled_readings(
    definition: &'static BarDefinition,
    number: u64,
    scales: &[i64],
) -> Vec<BarConfiguration> {
    let values: Vec<Decimal> = if definition.parameter.kind == NumberKind::Duration {
        scales
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
