//! The local rates store kept filled: each configured pair's daily closes
//! fetched from its venue and written where a merged report reads them.
//!
//! The report never fetches. It reads `quantick_trading::rates::RateStore`
//! from disk and converts only with what it finds; this module is the other
//! half, run once at startup on a thread of its own. A pair whose venue this
//! build cannot fetch from is logged and skipped, a fetch that fails leaves
//! the store as it was, and a store that does not parse is never written
//! over: the trader's file is not ours to discard.

use std::path::{Path, PathBuf};

use quantick_engine::Bar;
use quantick_feed_binance::{BinanceKlineHttp, BinanceMarket, KLINE_INTERVAL_1D, fetch_history};
use quantick_trading::rates::{RatePair, RateStore};

/// How far back the daily closes are fetched: a year and a margin, inside
/// one page of Binance's klines.
pub const RATE_DAYS: i64 = 400;

/// The venue token a pair names for Binance's spot market.
pub const BINANCE: &str = "binance";

const DAY_MS: i64 = 86_400_000;

/// Fold a venue's daily candles for `pair` into `store`: each close is the
/// rate of its candle's UTC day. Answers how many days were recorded.
pub fn record(store: &mut RateStore, pair: &RatePair, bars: &[Bar]) -> usize {
    bars.iter()
        .filter(|bar| store.record_daily_close(pair, bar.open_time, bar.close))
        .count()
}

/// Fetch every pair's daily closes once, on a background thread, and write
/// them into the store at `path`. Nothing to fetch spawns nothing.
pub fn refresh_in_background(path: PathBuf, pairs: Vec<RatePair>) {
    if pairs.is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("quantick-rates".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::warn!(target: "quantick::feed", event_code = "RATES_FETCH", %error, "no runtime for the rates fetch");
                    return;
                }
            };
            let to_ms = crate::clock::wall_clock_ms();
            let from_ms = to_ms.saturating_sub(RATE_DAYS * DAY_MS);
            let mut fetched = Vec::new();
            for pair in &pairs {
                if !pair.venue.eq_ignore_ascii_case(BINANCE) {
                    tracing::warn!(target: "quantick::feed", event_code = "RATES_FETCH", venue = %pair.venue, symbol = %pair.symbol, "no rates fetch for this venue");
                    continue;
                }
                let source = BinanceKlineHttp::for_market(BinanceMarket::Spot);
                let history = runtime.block_on(fetch_history(
                    &source,
                    &pair.symbol,
                    KLINE_INTERVAL_1D,
                    DAY_MS,
                    from_ms,
                    to_ms,
                ));
                fetched.push((pair.clone(), history.bars));
            }
            write(&path, &fetched);
        });
    if let Err(error) = spawned {
        tracing::warn!(target: "quantick::feed", event_code = "RATES_FETCH", %error, "rates fetch thread not started");
    }
}

/// Merge `fetched` into the store at `path`, written whole through a
/// temporary file so a crash never leaves half a store.
fn write(path: &Path, fetched: &[(RatePair, Vec<Bar>)]) {
    let mut store = match std::fs::read_to_string(path) {
        Ok(text) => match RateStore::parse(&text) {
            Ok(store) => store,
            Err(warning) => {
                tracing::warn!(target: "quantick::feed", event_code = "RATES_FETCH", path = %path.display(), %warning, "rates store left as it is");
                return;
            }
        },
        Err(_) => RateStore::default(),
    };
    let days: usize = fetched
        .iter()
        .map(|(pair, bars)| record(&mut store, pair, bars))
        .sum();
    if days == 0 {
        return;
    }
    let temp = path.with_extension("csv.tmp");
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temp, store.to_text()))
        .and_then(|()| std::fs::rename(&temp, path));
    match written {
        Ok(()) => {
            tracing::info!(target: "quantick::feed", event_code = "RATES_FETCH", path = %path.display(), days, "rates store refreshed")
        }
        Err(error) => {
            tracing::warn!(target: "quantick::feed", event_code = "RATES_FETCH", path = %path.display(), %error, "rates store not written")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_trading::Currency;
    use rust_decimal::Decimal;

    fn pair() -> RatePair {
        RatePair {
            from: Currency::new("USDT").expect("code"),
            to: Currency::new("BRL").expect("code"),
            venue: BINANCE.to_owned(),
            symbol: "USDTBRL".to_owned(),
        }
    }

    fn bar(open_time: i64, close: i64) -> Bar {
        Bar {
            open_time,
            close_time: open_time + DAY_MS - 1,
            open: Decimal::ONE,
            high: Decimal::ONE,
            low: Decimal::ONE,
            close: Decimal::new(close, 2),
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    #[test]
    fn each_daily_close_is_the_rate_of_its_utc_day_and_a_fetch_adds_to_the_store() {
        let scratch = crate::scratch::ScratchDir::new("rates-store");
        let path = scratch.path().join("rates.csv");
        // 2026-11-20 and 2026-11-21 at 00:00 UTC.
        let day = 20_777 * DAY_MS;
        write(
            &path,
            &[(pair(), vec![bar(day, 581), bar(day + DAY_MS, 579)])],
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# quantick-rates v1\n2026-11-20,USDT,BRL,5.81\n2026-11-21,USDT,BRL,5.79\n"
        );
        write(&path, &[(pair(), vec![bar(day + 2 * DAY_MS, 580)])]);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .ends_with("2026-11-21,USDT,BRL,5.79\n2026-11-22,USDT,BRL,5.8\n"),
            "a later fetch adds its days"
        );
        std::fs::write(&path, "not a store").unwrap();
        write(&path, &[(pair(), vec![bar(day, 581)])]);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "not a store",
            "a store that does not parse is never written over"
        );
    }
}
