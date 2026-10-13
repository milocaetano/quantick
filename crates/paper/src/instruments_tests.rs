// The instrument catalog: the shipped file parses, families match like the
// feeds' symbol families, and a bad entry is refused by name.

use quantick_sim::MoneySource;
use rust_decimal::Decimal;

use super::*;

/// The file `crates/app` compiles in as the default.
const SHIPPED: &str = include_str!("../../app/config/instruments.toml");

fn d(text: &str) -> Decimal {
    Decimal::from_str_exact(text).expect("test decimal literal")
}

#[test]
fn the_shipped_catalog_prices_the_b3_minis_and_the_offered_crypto() {
    let catalog = InstrumentCatalog::parse(SHIPPED).expect("shipped catalog parses");
    for (symbol, point_value, currency) in [
        ("WINV26", "0.20", "BRL"),
        ("WIN$N", "0.20", "BRL"),
        ("WDO$N", "10", "BRL"),
        ("BTCUSDT", "1", "USDT"),
        ("ETHUSDT", "1", "USDT"),
    ] {
        let feed = if symbol.starts_with('W') {
            "metatrader-b3"
        } else {
            "binance"
        };
        let spec = catalog
            .lookup(feed, symbol)
            .unwrap_or_else(|| panic!("{symbol}"));
        assert_eq!(spec.money.point_value, d(point_value), "{symbol}");
        assert_eq!(spec.money.currency.code(), currency, "{symbol}");
        assert_eq!(spec.money.source, MoneySource::Config, "{symbol}");
    }
    assert_eq!(
        catalog
            .lookup("binance", "BTCUSDT")
            .map(|spec| spec.price_unit),
        Some(PriceUnit::Quote)
    );
    assert_eq!(
        catalog
            .lookup("metatrader-b3", "WINV26")
            .map(|spec| spec.price_unit),
        Some(PriceUnit::Points)
    );
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
fn a_family_prices_only_the_feeds_it_names() {
    let catalog = InstrumentCatalog::parse(SHIPPED).expect("shipped catalog parses");
    // `WIN*` and `DOL*` are prefixes, and Binance lists pairs that share
    // them: no B3 point value may read onto a crypto pair.
    assert!(catalog.lookup("binance", "WINUSDT").is_none());
    assert!(catalog.lookup("binance", "DOLOUSDT").is_none());
    let win = catalog
        .lookup("metatrader-b3", "WINZ25")
        .expect("the mini index on its own feed");
    assert_eq!(win.money.point_value, d("0.20"));
    assert_eq!(win.money.currency.code(), "BRL");
    assert!(
        catalog.lookup("hyperliquid", "BTCUSDT").is_none(),
        "a Binance pair is priced on Binance only"
    );
}

#[test]
fn a_trader_overlay_switches_a_shipped_key_off() {
    let (catalog, warning) = InstrumentCatalog::layered(
        SHIPPED,
        Some("version = 1\n[instruments.\"WIN*\"]\ndisabled = true\n"),
    );
    assert_eq!(warning, None);
    assert!(catalog.lookup("metatrader-b3", "WINZ25").is_none());
    assert!(
        catalog.lookup("metatrader-b3", "WDOX25").is_some(),
        "only the key named goes"
    );
}
