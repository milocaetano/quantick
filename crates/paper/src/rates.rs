//! Money in another currency: only on request, always labelled, never
//! guessed.
//!
//! A merged report converts each trade's money into one currency at the
//! rate of the civil day it closed on. The rate comes through
//! [`RateSource`] — "one `from` is worth how much `to` on day `D`" — read
//! from the local rates store ([`quantick_trading::rates::RateStore`]), which
//! the feed side fills and this crate only ever reads. Which codes count as
//! one currency (USDT and USDC as USD) and which pairs exist are the
//! instrument catalog's `[currencies]` and `[[rates]]`, never code.
//!
//! A stored pair converts both ways: USDT→BRL at 5.80 is BRL→USDT at 1/5.80.
//! A day with no stored rate has none — no neighbour is borrowed — and the
//! caller lists what it could not convert.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quantick_civil::CivilDate;
pub use quantick_trading::Currency;
pub use quantick_trading::rates::{RatePair, RateStore};
use rust_decimal::Decimal;
use serde::Deserialize;

/// The rates store's file name, beside the journal folders.
pub const RATES_FILE: &str = "rates.csv";

/// The currency every merged report can always be read in.
pub const USD: &str = "USD";

/// "How much `to` one `from` is worth on civil day `day`", or that no rate
/// is known. Equivalent codes are worth exactly one of each other.
pub trait RateSource {
    fn rate(&self, from: &Currency, to: &Currency, day: CivilDate) -> Option<Decimal>;
}

/// The catalog's currency rules: which codes are one currency, and which
/// pairs the rates store is filled with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurrencyRules {
    /// A code and the currency it counts as: `USDT` → `USD`.
    same: BTreeMap<Currency, Currency>,
    pairs: Vec<RatePair>,
}

/// `[currencies]` as the catalog file writes it.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct CurrenciesRecord {
    #[serde(default)]
    same: BTreeMap<String, String>,
}

/// One `[[rates]]` entry as the catalog file writes it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RateRecord {
    from: String,
    to: String,
    venue: String,
    symbol: String,
}

impl CurrencyRules {
    /// The rules a catalog file declares, naming the first thing wrong.
    pub(crate) fn parse(
        currencies: &CurrenciesRecord,
        rates: &[RateRecord],
    ) -> Result<Self, String> {
        let code = |field: &str, text: &str| {
            Currency::new(text).ok_or_else(|| format!("{field}: a currency code must not be blank"))
        };
        let mut same = BTreeMap::new();
        for (alias, currency) in &currencies.same {
            let (alias, currency) = (
                code("currencies.same", alias)?,
                code("currencies.same", currency)?,
            );
            if alias == currency {
                return Err(format!(
                    "currencies.same: {} counts as itself",
                    alias.code()
                ));
            }
            same.insert(alias, currency);
        }
        if let Some(chained) = same.values().find(|currency| same.contains_key(*currency)) {
            return Err(format!(
                "currencies.same: {} is an alias and a currency both",
                chained.code()
            ));
        }
        let mut pairs = Vec::new();
        for record in rates {
            let pair = RatePair {
                from: code("rates.from", &record.from)?,
                to: code("rates.to", &record.to)?,
                venue: record.venue.trim().to_owned(),
                symbol: record.symbol.trim().to_owned(),
            };
            if pair.venue.is_empty() || pair.symbol.is_empty() {
                return Err("rates: venue and symbol must not be blank".to_owned());
            }
            pairs.push(pair);
        }
        let rules = Self { same, pairs };
        if let Some(pair) = rules
            .pairs
            .iter()
            .find(|pair| rules.canonical(&pair.from) == rules.canonical(&pair.to))
        {
            return Err(format!(
                "rates: {} and {} are one currency",
                pair.from.code(),
                pair.to.code()
            ));
        }
        Ok(rules)
    }

    /// A trader's rules laid over these: their equivalences added, their
    /// pairs after the shipped ones they do not repeat.
    pub(crate) fn overlay(&mut self, other: Self) {
        self.same.extend(other.same);
        for pair in other.pairs {
            if !self.pairs.contains(&pair) {
                self.pairs.push(pair);
            }
        }
    }

    /// The currency `code` counts as: USDT reads as USD.
    #[must_use]
    pub fn canonical<'a>(&'a self, code: &'a Currency) -> &'a Currency {
        self.same.get(code).unwrap_or(code)
    }

    /// The pairs the feed side keeps the store filled with.
    #[must_use]
    pub fn pairs(&self) -> &[RatePair] {
        &self.pairs
    }
}

/// The rates store read through the catalog's equivalences: the
/// [`RateSource`] a merged report converts with.
pub struct StoredRates<'a> {
    pub rules: &'a CurrencyRules,
    pub store: &'a RateStore,
}

impl RateSource for StoredRates<'_> {
    fn rate(&self, from: &Currency, to: &Currency, day: CivilDate) -> Option<Decimal> {
        let (from, to) = (self.rules.canonical(from), self.rules.canonical(to));
        if from == to {
            return Some(Decimal::ONE);
        }
        self.store.pairs().find_map(|(base, quote)| {
            let (canon_base, canon_quote) =
                (self.rules.canonical(base), self.rules.canonical(quote));
            let stored = || self.store.get(base, quote, day);
            if (canon_base, canon_quote) == (from, to) {
                stored()
            } else if (canon_base, canon_quote) == (to, from) {
                stored().and_then(|rate| Decimal::ONE.checked_div(rate))
            } else {
                None
            }
        })
    }
}

/// Where the rates store of the journal under `journal_dir` lives: beside
/// it, as the broker journal is, so a scratch journal has scratch rates.
#[must_use]
pub fn store_path(journal_dir: &Path) -> PathBuf {
    journal_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(journal_dir)
        .join(RATES_FILE)
}

/// The rates store at `path`. A missing file is the common case and holds
/// no rate; one that does not parse holds none either and is logged by
/// name, so a merge lists every trade as unconverted rather than guess.
#[must_use]
pub fn read_store(path: &Path) -> RateStore {
    let Ok(text) = std::fs::read_to_string(path) else {
        return RateStore::default();
    };
    RateStore::parse(&text).unwrap_or_else(|warning| {
        tracing::warn!(
            target: "quantick::paper",
            event_code = "RATES_STORE",
            path = %path.display(),
            %warning,
            "rates store not read"
        );
        RateStore::default()
    })
}

#[cfg(test)]
#[path = "rates_tests.rs"]
mod rates_tests;
