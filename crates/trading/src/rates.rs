//! The local rates store: one exchange rate per currency pair per civil day.
//!
//! The document both sides of a conversion agree on. The feed side fetches a
//! venue's daily candles and writes their closes here; the paper report reads
//! them back to merge P&L across currencies. Neither reaches the other: this
//! text is the whole contract between them.
//!
//! ```text
//! # quantick-rates v1
//! 2026-11-20,USDT,BRL,5.8123
//! ```
//!
//! `day,from,to,rate`: one unit of `from` is worth `rate` of `to` on that
//! day — a venue's daily close, on the UTC day the venue opened the candle. A day with no line has no rate: nothing here interpolates, carries a
//! rate forward or guesses one.

use std::collections::BTreeMap;

use quantick_civil::{CivilDate, TzOffset, parse_iso_date};
use rust_decimal::Decimal;

use crate::Currency;

/// The first line of a rates file, naming its format.
pub const RATES_HEADER: &str = "# quantick-rates v1";

/// One pair the store is filled with: which venue market prices `from` in
/// `to`, as the configuration declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatePair {
    pub from: Currency,
    pub to: Currency,
    /// The venue that publishes the pair's daily candle, e.g. `binance`.
    pub venue: String,
    /// The venue's market for it, e.g. `USDTBRL`.
    pub symbol: String,
}

/// Every stored rate, by pair and civil day.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateStore {
    series: BTreeMap<(Currency, Currency), BTreeMap<CivilDate, Decimal>>,
}

impl RateStore {
    /// Read a rates file. Blank lines and `#` comments are skipped.
    ///
    /// # Errors
    ///
    /// The first line that is not `day,from,to,rate` with a real
    /// `YYYY-MM-DD` day, two currency codes and a rate above zero, by number.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut store = Self::default();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = || {
                format!(
                    "line {}: expected day,from,to,rate, got {line:?}",
                    index + 1
                )
            };
            let mut cells = line.split(',').map(str::trim);
            let (Some(day), Some(from), Some(to), Some(rate), None) = (
                cells.next(),
                cells.next(),
                cells.next(),
                cells.next(),
                cells.next(),
            ) else {
                return Err(bad());
            };
            let day = parse_iso_date(day).ok_or_else(bad)?;
            let (from, to) = (
                Currency::new(from).ok_or_else(bad)?,
                Currency::new(to).ok_or_else(bad)?,
            );
            let rate = Decimal::from_str_exact(rate).map_err(|_| bad())?;
            if !store.insert(from, to, day, rate) {
                return Err(bad());
            }
        }
        Ok(store)
    }

    /// Record that one `from` was worth `rate` of `to` on `day`, replacing
    /// what the day held. A rate at or below zero, or a pair of one
    /// currency, is no rate and is refused.
    pub fn insert(&mut self, from: Currency, to: Currency, day: CivilDate, rate: Decimal) -> bool {
        if rate <= Decimal::ZERO || from == to {
            return false;
        }
        self.series.entry((from, to)).or_default().insert(day, rate);
        true
    }

    /// Record a venue's daily candle for `pair`: its close is the rate of
    /// the UTC day the candle opened on, as the venue cuts its days.
    pub fn record_daily_close(
        &mut self,
        pair: &RatePair,
        open_time_ms: i64,
        close: Decimal,
    ) -> bool {
        let day = CivilDate::from_ms(open_time_ms, TzOffset::new(0));
        self.insert(pair.from.clone(), pair.to.clone(), day, close)
    }

    /// The stored rate from `from` to `to` on `day`, exactly as written:
    /// the reverse pair is the reader's to invert, said out loud.
    #[must_use]
    pub fn get(&self, from: &Currency, to: &Currency, day: CivilDate) -> Option<Decimal> {
        self.series
            .get(&(from.clone(), to.clone()))
            .and_then(|days| days.get(&day))
            .copied()
    }

    /// The pairs the store holds a rate for.
    pub fn pairs(&self) -> impl Iterator<Item = &(Currency, Currency)> {
        self.series.keys()
    }

    /// The store as a rates file: the header, then pairs and days in order,
    /// so two writes of the same rates are the same bytes.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = format!("{RATES_HEADER}\n");
        for ((from, to), days) in &self.series {
            for (day, rate) in days {
                text.push_str(&format!(
                    "{},{},{},{}\n",
                    day.iso(),
                    from.code(),
                    to.code(),
                    rate.normalize()
                ));
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(text: &str) -> Currency {
        Currency::new(text).expect("test currency")
    }

    #[test]
    fn a_store_round_trips_through_its_text_in_order() {
        let mut store = RateStore::default();
        let day = parse_iso_date("2026-11-20").expect("day");
        assert!(store.insert(
            code("USDT"),
            code("BRL"),
            day.offset_days(1),
            Decimal::new(58, 1)
        ));
        assert!(store.insert(code("USDT"), code("BRL"), day, Decimal::new(58_123, 4)));
        let text = store.to_text();
        assert_eq!(
            text,
            "# quantick-rates v1\n2026-11-20,USDT,BRL,5.8123\n2026-11-21,USDT,BRL,5.8\n"
        );
        assert_eq!(RateStore::parse(&text), Ok(store.clone()));
        assert_eq!(
            store.get(&code("USDT"), &code("BRL"), day),
            Some(Decimal::new(58_123, 4))
        );
        assert_eq!(
            store.get(&code("BRL"), &code("USDT"), day),
            None,
            "never inverted here"
        );
        assert_eq!(
            store.get(&code("USDT"), &code("BRL"), day.offset_days(2)),
            None
        );
    }

    #[test]
    fn a_line_that_is_not_a_rate_is_named() {
        for bad in [
            "2026-02-30,USDT,BRL,5",
            "2026-11-20,USDT,BRL",
            "2026-11-20,USDT,BRL,0",
            "2026-11-20,BRL,BRL,1",
            "2026-11-20,USDT,,5",
            "2026-11-20,USDT,BRL,five",
        ] {
            let error = RateStore::parse(&format!("{RATES_HEADER}\n\n{bad}\n")).unwrap_err();
            assert!(error.starts_with("line 3:"), "{bad}: {error}");
        }
    }
}
