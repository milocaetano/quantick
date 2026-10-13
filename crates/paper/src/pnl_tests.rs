// The per-contract, grouped and money figures one closed trade reads as,
// over fixed fixtures: B3 futures in points, a linear crypto pair in its
// quote asset, a forex pair in pips, and a symbol nobody priced.

use quantick_sim::{Currency, InstrumentMoney, MoneySource};
use rust_decimal::Decimal;

use super::*;
use crate::instruments::{InstrumentCatalog, PriceUnit};
use crate::risk_sizing::InstrumentBook;

fn d(text: &str) -> Decimal {
    Decimal::from_str_exact(text).expect("test decimal literal")
}

/// The catalog these fixtures read, written the way the shipped file is.
const CATALOG: &str = r#"
version = 1

[instruments."WIN*"]
currency = "BRL"
point_value = "0.20"
size_step = "1"
min_size = "1"
price_unit = "points"

[instruments."WDO*"]
currency = "BRL"
point_value = "10"
size_step = "1"
min_size = "1"
price_unit = "points"

[instruments.BTCUSDT]
currency = "USDT"
point_value = "1"
size_step = "0.00001"
min_size = "0.00001"
price_unit = "quote"

[instruments.EURUSD]
currency = "USD"
point_value = "100000"
size_step = "0.01"
min_size = "0.01"
price_unit = "pips"
pip_size = "0.0001"
"#;

fn catalog() -> InstrumentCatalog {
    InstrumentCatalog::parse(CATALOG).expect("fixture catalog parses")
}

fn trade(symbol: &str, pnl_points: &str, quantity: &str) -> (TradePnl, String) {
    let catalog = catalog();
    let declared = InstrumentBook::new();
    let book = PnlBook {
        declared: &declared,
        catalog: &catalog,
    };
    let units = book.units(symbol);
    let pnl = TradePnl::of(d(pnl_points), d(quantity), &units);
    let row = fmt_trade(&pnl, &units, PnlPrimary::PointsPerContract);
    (pnl, row)
}

/// The trader's own example: two mini-index contracts that each made 200
/// points. Stored as 400 (move x quantity); read as 200 per contract, 400
/// grouped and 80.00 BRL.
#[test]
fn win_two_contracts_read_per_contract_grouped_and_in_reais() {
    let catalog = catalog();
    let declared = InstrumentBook::new();
    let book = PnlBook {
        declared: &declared,
        catalog: &catalog,
    };
    let units = book.units("WINV26");
    let pnl = TradePnl::of(d("400"), d("2"), &units);
    assert_eq!(pnl.per_unit, d("200"));
    assert_eq!(pnl.grouped, d("400"));
    assert_eq!(pnl.quantity, d("2"));
    let money = pnl.money.clone().expect("WIN has a point value");
    assert_eq!(money.amount, d("80"));
    assert_eq!(money.currency.code(), "BRL");

    assert_eq!(fmt_per_unit(&pnl, &units), "+200 pts × 2");
    assert_eq!(fmt_grouped(&pnl, &units), "+400 pts");
    assert_eq!(fmt_money(&money), "+80.00 BRL");
    assert_eq!(
        fmt_trade(&pnl, &units, PnlPrimary::Money),
        "+80.00 BRL · +200 pts × 2"
    );
    assert_eq!(
        fmt_trade(&pnl, &units, PnlPrimary::PointsPerContract),
        "+200 pts × 2 · +80.00 BRL"
    );
    assert_eq!(
        fmt_trade(&pnl, &units, PnlPrimary::PointsTimesQty),
        "+400 pts · +80.00 BRL"
    );
}

#[test]
fn wdo_losses_read_in_reais_at_ten_per_point() {
    let (pnl, row) = trade("WDOX26", "-5.5", "1");
    assert_eq!(pnl.per_unit, d("-5.5"));
    assert_eq!(pnl.money.expect("WDO money").amount, d("-55"));
    assert_eq!(row, "-5.5 pts × 1 · -55.00 BRL");
}

/// A linear crypto pair never says "pts": the per-unit figure is the price
/// move in the quote asset, and the quantity is a fraction of a coin.
#[test]
fn btcusdt_half_a_coin_reads_as_a_quote_move_and_usdt() {
    let (pnl, row) = trade("BTCUSDT", "60", "0.5");
    assert_eq!(pnl.per_unit, d("120"));
    assert_eq!(pnl.grouped, d("60"));
    assert_eq!(pnl.money.expect("BTCUSDT money").amount, d("60"));
    assert_eq!(row, "+120 USDT move × 0.5 · +60.00 USDT");
    assert!(!row.contains("pts"), "crypto must never say pts: {row}");
}

/// Forex reads in pips: a 0.0010 move is 10 pips per lot; half a lot of
/// 100,000 units makes 50 USD.
#[test]
fn eurusd_half_a_lot_reads_in_pips_and_dollars() {
    let (pnl, row) = trade("EURUSD", "0.0005", "0.5");
    assert_eq!(pnl.per_unit, d("10"));
    assert_eq!(pnl.grouped, d("5"));
    assert_eq!(pnl.money.expect("EURUSD money").amount, d("50"));
    assert_eq!(row, "+10 pips × 0.5 · +50.00 USD");
}

/// No point value anywhere: money is unset, said so, never guessed - and
/// the default primary figure falls back to points per contract.
#[test]
fn a_symbol_with_no_point_value_shows_money_as_unset() {
    let (pnl, row) = trade("XAUUSD", "30", "3");
    assert_eq!(pnl.per_unit, d("10"));
    assert!(pnl.money.is_none());
    assert_eq!(row, format!("+10 pts × 3 · {UNSET_MONEY}"));
    assert_eq!(
        effective_primary(None, false),
        PnlPrimary::PointsPerContract
    );
    assert_eq!(effective_primary(None, true), PnlPrimary::Money);
    assert_eq!(
        effective_primary(Some(PnlPrimary::PointsTimesQty), true),
        PnlPrimary::PointsTimesQty
    );
}

/// A trader-declared record outranks the shipped config, and says where its
/// number came from.
#[test]
fn a_declared_point_value_wins_over_the_config() {
    let catalog = catalog();
    let declared: InstrumentBook = [(
        "WINV26".to_owned(),
        InstrumentMoney {
            point_value: d("1"),
            size_step: Decimal::ONE,
            min_size: Decimal::ONE,
            max_size: None,
            currency: Currency::new("BRL").expect("BRL"),
            source: MoneySource::Declared,
        },
    )]
    .into_iter()
    .collect();
    let book = PnlBook {
        declared: &declared,
        catalog: &catalog,
    };
    let units = book.units("WINV26");
    let money = units.money.expect("declared money");
    assert_eq!(money.point_value, d("1"));
    assert_eq!(money.source, MoneySource::Declared);
    assert_eq!(units.price_unit, PriceUnit::Points);

    let from_config = book.units("WINZ26").money.expect("config money");
    assert_eq!(from_config.source, MoneySource::Config);
    assert_eq!(from_config.point_value, d("0.20"));
}

#[test]
fn totals_sum_per_currency_and_never_partially() {
    let catalog = catalog();
    let declared = InstrumentBook::new();
    let book = PnlBook {
        declared: &declared,
        catalog: &catalog,
    };
    let mut reais = PnlTotal::default();
    for (pnl_points, quantity) in [("400", "2"), ("-100", "1")] {
        let units = book.units("WINV26");
        reais.add(&TradePnl::of(d(pnl_points), d(quantity), &units), &units);
    }
    assert_eq!(reais.fmt(PnlPrimary::Money), "+60.00 BRL");
    assert_eq!(reais.fmt(PnlPrimary::PointsPerContract), "+100 pts/ct");
    assert_eq!(reais.fmt(PnlPrimary::PointsTimesQty), "+300 pts");

    let mut mixed = reais.clone();
    let btc = book.units("BTCUSDT");
    mixed.add(&TradePnl::of(d("60"), d("0.5"), &btc), &btc);
    assert_eq!(mixed.fmt(PnlPrimary::Money), "+60.00 BRL / +60.00 USDT");

    let mut unknown = reais;
    let gold = book.units("XAUUSD");
    unknown.add(&TradePnl::of(d("30"), d("3"), &gold), &gold);
    assert_eq!(unknown.fmt(PnlPrimary::Money), UNSET_MONEY);
}

#[test]
fn the_display_preference_round_trips_its_token() {
    for primary in PnlPrimary::ALL {
        assert_eq!(PnlPrimary::from_token(primary.token()), Some(primary));
    }
    assert_eq!(PnlPrimary::from_token("dollars"), None);
}
