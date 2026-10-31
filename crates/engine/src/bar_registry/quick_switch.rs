//! A typed number read as every registered bar at that size — ProfitChart's
//! quick period switch. The registry, not a fixed list, decides what is offered.
//! A letter after the number narrows the list to the kinds declaring it (`2000t`
//! lists ticks, then trades); a unit after it (`5m`, `1w`, `1mo`) to the time bars
//! in that unit. A letter that is both (`1d`) lists the time bar first.
use super::{BarConfiguration, BarDefinition, BarRegistry, NumberKind, interval_unit_ms};
use rust_decimal::Decimal;

/// How a bare number reads as a duration, in the order offered: minutes, as
/// ProfitChart reads it first, then seconds and hours.
pub const QUICK_DURATION_SCALES_MS: [i64; 3] = [60_000, 1_000, 3_600_000];

/// The most digits a typed number keeps: more than any bar rule reads.
const QUICK_MAX_DIGITS: usize = 9;

/// A kind's own name in the quick switch: the letter typed after the number
/// that lists this kind and any other declaring it (`2000t`), and the noun its
/// row reads after the number (`1 Tick`, `50 Ticks (Renko)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickAlias {
    pub suffix: char,
    pub singular: &'static str,
    pub plural: &'static str,
}

/// The noun of each unit a time bar's row is read in, singular; the plural adds
/// an `s`. Spelled as [`super::fmt_time_interval`] writes the unit.
const DURATION_NOUNS: [(&str, &str); 6] = [
    ("mo", "Month"),
    ("w", "Week"),
    ("d", "Day"),
    ("h", "Hour"),
    ("m", "Minute"),
    ("s", "Second"),
];

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
    /// case, keeps the kinds whose [`QuickAlias`] declares that letter, and a
    /// duration unit (`s`, `m`, `h`, `d`, `w`, `mo`) keeps the duration kinds
    /// read in that unit — `1d`, `1w` and `1mo` are the daily, weekly and
    /// monthly time bars. Duration readings come first, so `1d` offers the
    /// daily bar before one dollar. Anything else — no number, two letters —
    /// means nothing.
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
    /// The row the quick switch lists this configuration as: the number, the
    /// noun of its unit or kind, and its choice where it has one (`15 Minutes`,
    /// `1 Tick`, `20 Imbalance (Volume)`). A rule with no noun to read — a kind
    /// without an alias, an interval in no named unit — shows its summary.
    #[must_use]
    pub fn quick_label(self) -> String {
        let definition = self.definition();
        let formatted = definition.parameter.format(self.parameter());
        if definition.parameter.kind == NumberKind::Duration {
            return duration_label(&formatted).unwrap_or_else(|| self.summary());
        }
        let Some(alias) = definition.quick_alias else {
            return self.summary();
        };
        let noun = if self.parameter() == Decimal::ONE {
            alias.singular
        } else {
            alias.plural
        };
        match self.choice().map(capitalized) {
            Some(choice) => format!("{formatted} {noun} ({choice})"),
            None => format!("{formatted} {noun}"),
        }
    }
}

/// `15m` as `15 Minutes`, `1d` as `1 Day`; `None` for a unit with no noun.
fn duration_label(interval: &str) -> Option<String> {
    let digits = interval.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let unit = &interval[digits.len()..];
    let noun = DURATION_NOUNS
        .iter()
        .find_map(|(written, noun)| (*written == unit).then_some(*noun))?;
    let plural = if digits == "1" { "" } else { "s" };
    Some(format!("{digits} {noun}{plural}"))
}

fn capitalized(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + letters.as_str()
    })
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
