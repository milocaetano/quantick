//! Exact instrument price steps from exchangeInfo, never display precision.

use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr as _;

#[derive(Deserialize)]
struct ExchangeInfo {
    symbols: Vec<Instrument>,
}

#[derive(Deserialize)]
struct Instrument {
    symbol: String,
    filters: Vec<Filter>,
}

#[derive(Deserialize)]
struct Filter {
    #[serde(rename = "filterType")]
    kind: String,
    #[serde(rename = "tickSize")]
    tick_size: Option<String>,
}

pub(super) fn price_step(body: &str, symbol: &str) -> Result<Decimal, String> {
    let info: ExchangeInfo = serde_json::from_str(body).map_err(|error| error.to_string())?;
    let instrument = info
        .symbols
        .into_iter()
        .find(|entry| entry.symbol == symbol)
        .ok_or_else(|| format!("exchangeInfo has no instrument {symbol}"))?;
    let filter = instrument
        .filters
        .into_iter()
        .find(|filter| filter.kind == "PRICE_FILTER")
        .ok_or_else(|| format!("exchangeInfo has no PRICE_FILTER for {symbol}"))?;
    let value = filter
        .tick_size
        .ok_or_else(|| "PRICE_FILTER has no tickSize".to_string())?;
    let step = Decimal::from_str(&value).map_err(|error| error.to_string())?;
    if step <= Decimal::ZERO {
        return Err(format!("PRICE_FILTER tickSize must be positive: {value}"));
    }
    Ok(step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_wins_over_precision_and_lot_size_for_the_exact_instrument() {
        let body = r#"{"symbols":[{"symbol":"ETHUSDT","filters":[{"filterType":"PRICE_FILTER","tickSize":"1"}]},{"symbol":"BTCUSDT","pricePrecision":2,"filters":[{"filterType":"LOT_SIZE","stepSize":"0.001"},{"filterType":"PRICE_FILTER","tickSize":"0.10"}]}]}"#;
        assert_eq!(price_step(body, "BTCUSDT"), Ok(Decimal::new(1, 1)));
        assert!(price_step(body, "BTCUSDT.p").is_err());
    }

    #[test]
    fn absent_invalid_or_disabled_filters_are_errors_not_inferred_steps() {
        for filter in [
            r#"{"filterType":"LOT_SIZE"}"#,
            r#"{"filterType":"PRICE_FILTER"}"#,
            r#"{"filterType":"PRICE_FILTER","tickSize":"0"}"#,
            r#"{"filterType":"PRICE_FILTER","tickSize":"-1"}"#,
            r#"{"filterType":"PRICE_FILTER","tickSize":"bad"}"#,
        ] {
            let body = format!(r#"{{"symbols":[{{"symbol":"BTCUSDT","filters":[{filter}]}}]}}"#);
            assert!(price_step(&body, "BTCUSDT").is_err());
        }
    }
}
