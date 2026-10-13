// The instrument catalog: the shipped file parses, a B3 series matches its
// contract codes and nothing else, and a bad entry is refused by name.

use quantick_sim::MoneySource;
use rust_decimal::Decimal;

use super::*;

/// The file `crates/app` compiles in as the default.
const SHIPPED: &str = include_str!("../../app/config/instruments.toml");

fn d(text: &str) -> Decimal {
    Decimal::from_str_exact(text).expect("test decimal literal")
}

/// Every feed id a tab could be on, and none: the shipped catalog must
/// answer the same for all of them.
const FEEDS: [&str; 4] = ["metatrader-b3", "binance", "hyperliquid", ""];

#[test]
fn the_shipped_catalog_prices_each_instrument_whatever_the_feed() {
    let catalog = InstrumentCatalog::parse(SHIPPED).expect("shipped catalog parses");
    for (symbol, point_value, currency, unit) in [
        ("WINV26", "0.20", "BRL", PriceUnit::Points),
        ("WIN$N", "0.20", "BRL", PriceUnit::Points),
        ("WDO$N", "10", "BRL", PriceUnit::Points),
        ("BTCUSDT", "1", "USDT", PriceUnit::Quote),
        ("ETHUSDT", "1", "USDT", PriceUnit::Quote),
        ("BTC", "1", "USDC", PriceUnit::Quote),
    ] {
        for feed in FEEDS {
            let spec = catalog
                .lookup(feed, symbol)
                .unwrap_or_else(|| panic!("{symbol} on {feed:?}"));
            assert_eq!(spec.money.point_value, d(point_value), "{symbol}");
            assert_eq!(spec.money.currency.code(), currency, "{symbol}");
            assert_eq!(spec.money.source, MoneySource::Config, "{symbol}");
            assert_eq!(spec.price_unit, unit, "{symbol}");
        }
    }
    // Forex ships as commented examples only: nothing here knows a broker's
    // contract size.
    assert!(catalog.lookup("metatrader-b3", "EURUSD").is_none());
}

#[test]
fn an_exact_symbol_beats_the_longest_family_which_beats_a_shorter_one() {
    let catalog = InstrumentCatalog::parse(
        r#"
version = 1
[instruments."W*"]
currency = "BRL"
point_value = "1"
size_step = "1"
[instruments."WIN*"]
currency = "BRL"
point_value = "0.20"
size_step = "1"
[instruments.WINV26]
currency = "BRL"
point_value = "0.25"
size_step = "1"
"#,
    )
    .expect("parses");
    let value = |symbol: &str| {
        catalog
            .lookup("any", symbol)
            .map(|spec| spec.money.point_value)
    };
    assert_eq!(value("WINV26"), Some(d("0.25")));
    assert_eq!(value("WINZ26"), Some(d("0.20")));
    assert_eq!(value("WDOX26"), Some(d("1")));
    assert_eq!(value("BTCUSDT"), None);
}

#[test]
fn a_missing_min_size_reads_as_one_step_and_the_unit_defaults_to_points() {
    let catalog = InstrumentCatalog::parse(
        r#"
version = 1
[instruments.ES]
currency = "USD"
point_value = "50"
size_step = "1"
"#,
    )
    .expect("parses");
    let spec = catalog.lookup("any", "ES").expect("ES");
    assert_eq!(spec.money.min_size, Decimal::ONE);
    assert_eq!(spec.price_unit, PriceUnit::Points);
    assert_eq!(spec.pip_size, None);
}

#[test]
fn the_display_default_is_read_from_the_file() {
    let catalog =
        InstrumentCatalog::parse("version = 1\n[display]\nprimary = \"points_times_qty\"\n")
            .expect("parses");
    assert_eq!(catalog.default_primary(), Some(PnlPrimary::PointsTimesQty));
}

#[test]
fn a_bad_entry_is_refused_by_name() {
    for (text, needle) in [
        (
            "version = 1\n[instruments.X]\ncurrency = \"BRL\"\npoint_value = \"abc\"\nsize_step = \"1\"\n",
            "X: point_value",
        ),
        (
            "version = 1\n[instruments.X]\ncurrency = \"BRL\"\npoint_value = \"0\"\nsize_step = \"1\"\n",
            "X: point_value",
        ),
        (
            "version = 1\n[instruments.X]\ncurrency = \" \"\npoint_value = \"1\"\nsize_step = \"1\"\n",
            "X: currency",
        ),
        (
            "version = 1\n[instruments.X]\ncurrency = \"USD\"\npoint_value = \"1\"\nsize_step = \"1\"\nprice_unit = \"pips\"\n",
            "X: pip_size",
        ),
        (
            "version = 1\n[instruments.\"*\"]\ncurrency = \"USD\"\npoint_value = \"1\"\nsize_step = \"1\"\n",
            "*: ",
        ),
        ("version = 2\n", "version"),
        (
            "version = 1\n[display]\nprimary = \"dollars\"\n",
            "display.primary",
        ),
    ] {
        let error = InstrumentCatalog::parse(text).expect_err(needle);
        assert!(error.contains(needle), "{error:?} should name {needle:?}");
    }
}

#[test]
fn a_b3_series_matches_contract_codes_and_never_a_crypto_pair() {
    let catalog = InstrumentCatalog::parse(SHIPPED).expect("shipped catalog parses");
    for (symbol, family, point_value) in [
        ("WINV26", "WIN", "0.20"),
        ("WINZ25", "WIN", "0.20"),
        ("WIN$", "WIN", "0.20"),
        ("WIN$N", "WIN", "0.20"),
        ("INDZ26", "IND", "1"),
        ("WDOX25", "WDO", "10"),
        ("WDO$", "WDO", "10"),
        ("DOLF27", "DOL", "50"),
    ] {
        for feed in FEEDS {
            let spec = catalog
                .lookup(feed, symbol)
                .unwrap_or_else(|| panic!("{symbol} on {feed:?}"));
            assert_eq!(spec.money.point_value, d(point_value), "{symbol}");
            assert_eq!(spec.money.currency.code(), "BRL", "{symbol}");
            assert_eq!(spec.family, family, "{symbol}");
        }
    }
    // Binance lists pairs that share the B3 roots, and malformed codes are
    // no contract: no B3 point value may read onto any of them, whatever
    // tab asks.
    for symbol in [
        "WINUSDT", "DOLOUSDT", "WIN", "WINV", "WINV2", "WINV261", "WINA26", "WIN$D", "WINDOWS",
    ] {
        for feed in FEEDS {
            assert!(
                catalog.lookup(feed, symbol).is_none(),
                "{symbol} on {feed:?} must read unpriced"
            );
        }
    }
}

#[test]
fn a_trader_entry_naming_feeds_prices_only_those() {
    let catalog = InstrumentCatalog::parse(
        r#"
version = 1
[instruments.ES]
feeds = [" cme "]
currency = "USD"
point_value = "50"
size_step = "1"
"#,
    )
    .expect("parses");
    assert!(catalog.lookup("cme", "ES").is_some());
    assert!(
        catalog.lookup(" cme", "ES").is_some(),
        "padding is not an id"
    );
    assert!(catalog.lookup("binance", "ES").is_none());
}

#[test]
fn a_series_key_is_a_bare_root_of_a_known_exchange() {
    for (text, needle) in [
        (
            "version = 1\n[instruments.\"WIN*\"]\nseries = \"b3\"\ncurrency = \"BRL\"\npoint_value = \"1\"\nsize_step = \"1\"\n",
            "WIN*: a series key",
        ),
        (
            "version = 1\n[instruments.WIN]\nseries = \"cme\"\ncurrency = \"BRL\"\npoint_value = \"1\"\nsize_step = \"1\"\n",
            "WIN: series must be b3",
        ),
    ] {
        let error = InstrumentCatalog::parse(text).expect_err(needle);
        assert!(error.contains(needle), "{error:?} should name {needle:?}");
    }
}

#[test]
fn a_trader_overlay_switches_a_shipped_key_off() {
    let (catalog, warning) = InstrumentCatalog::layered(
        SHIPPED,
        Some("version = 1\n[instruments.WIN]\ndisabled = true\n"),
    );
    assert_eq!(warning, None);
    assert!(catalog.lookup("metatrader-b3", "WINZ25").is_none());
    assert!(
        catalog.lookup("metatrader-b3", "WDOX25").is_some(),
        "only the key named goes"
    );
}
