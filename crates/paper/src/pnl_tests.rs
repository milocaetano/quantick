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
        feed: "",
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
        feed: "",
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
        feed: "",
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
        feed: "",
    };
    let mut reais = PnlTotal::default();
    for (pnl_points, quantity) in [("400", "2"), ("-100", "1")] {
        let units = book.units("WINV26");
        reais.add(
            "WINV26",
            &TradePnl::of(d(pnl_points), d(quantity), &units),
            &units,
        );
    }
    assert_eq!(reais.fmt(PnlPrimary::Money), "+60.00 BRL");
    assert_eq!(reais.fmt(PnlPrimary::PointsPerContract), "+100 pts/ct");
    assert_eq!(reais.fmt(PnlPrimary::PointsTimesQty), "+300 pts");

    let mut mixed = reais.clone();
    let btc = book.units("BTCUSDT");
    mixed.add("BTCUSDT", &TradePnl::of(d("60"), d("0.5"), &btc), &btc);
    assert_eq!(mixed.fmt(PnlPrimary::Money), "+60.00 BRL / +60.00 USDT");

    let mut unknown = reais;
    let gold = book.units("XAUUSD");
    unknown.add("XAUUSD", &TradePnl::of(d("30"), d("3"), &gold), &gold);
    assert_eq!(unknown.fmt(PnlPrimary::Money), UNSET_MONEY);
}

#[test]
fn the_display_preference_round_trips_its_token() {
    for primary in PnlPrimary::ALL {
        assert_eq!(PnlPrimary::from_token(primary.token()), Some(primary));
    }
    assert_eq!(PnlPrimary::from_token("dollars"), None);
}

fn closed(pnl_points: &str, quantity: &str) -> quantick_sim::ClosedTrade {
    quantick_sim::ClosedTrade {
        side: quantick_engine::Side::Buy,
        quantity: d(quantity),
        entry_price: d("100"),
        exit_price: d("100"),
        opened_ms: 0,
        closed_ms: 1,
        pnl_points: d(pnl_points),
        exit_reason: quantick_sim::ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    }
}

#[test]
fn the_reader_leads_with_the_chosen_figure_and_keeps_the_other_beside_it() {
    let declared = InstrumentBook::new();
    let catalog = Arc::new(catalog());
    let mut settings = PnlSettings {
        catalog: Arc::clone(&catalog),
        chosen: None,
    };
    let book = PnlBook {
        declared: &declared,
        catalog: &catalog,
        feed: "",
    };
    let reader = PnlReader {
        book,
        settings: &settings,
    };
    assert_eq!(
        reader.trade("WINZ26", d("400"), d("2")),
        "+80.00 BRL · +200 pts × 2",
        "money leads by default where a point value is known"
    );
    assert_eq!(
        reader.trade("XYZ", d("10"), d("1")),
        "+10 pts × 1 · — set pt value",
        "points lead where none is"
    );
    let win = closed("400", "2");
    assert_eq!(
        reader.total([("WINZ26", &win), ("WINZ26", &win)]),
        "+160.00 BRL · +400 pts/ct"
    );
    settings.chosen = Some(PnlPrimary::PointsTimesQty);
    let reader = PnlReader {
        book,
        settings: &settings,
    };
    assert_eq!(
        reader.trade("WINZ26", d("400"), d("2")),
        "+400 pts · +80.00 BRL"
    );
    assert_eq!(
        reader.total([("WINZ26", &win), ("XYZ", &closed("5", "1"))]),
        "— set pt value · mixed units",
        "a money total with an unpriced trade is never a partial sum, and WIN          points never add to another instrument's"
    );
    assert_eq!(
        reader.sum([("WINZ26", &win)]).unit_label(PnlPrimary::Money),
        "BRL"
    );
}

#[test]
fn a_missing_trader_catalog_loads_the_shipped_one_silently() {
    let settings = PnlSettings::load(
        CATALOG,
        Some(std::path::Path::new("no-such-instruments.toml")),
        Some("points_per_contract"),
    );
    assert!(settings.catalog.lookup("metatrader-b3", "WINZ26").is_some());
    assert_eq!(settings.chosen, Some(PnlPrimary::PointsPerContract));
    let settings = PnlSettings::load(CATALOG, None, Some("dollars"));
    assert_eq!(
        settings.chosen, None,
        "an unknown stored token is no choice"
    );
}

const SHIPPED: &str = include_str!("../../app/config/instruments.toml");

fn shipped() -> InstrumentCatalog {
    InstrumentCatalog::parse(SHIPPED).expect("shipped catalog parses")
}

fn money(amount: &str, code: &str) -> Money {
    Money::new(d(amount), Currency::new(code).expect("currency"))
}

#[test]
fn one_lookup_answers_sizing_the_ticket_step_and_the_readouts() {
    let catalog = shipped();
    let mut declared = InstrumentBook::new();
    let book = PnlBook {
        declared: &declared,
        catalog: &catalog,
        feed: "binance",
    };
    let btc = book.money("BTCUSDT").expect("the catalog knows BTCUSDT");
    assert_eq!(
        btc.size_step,
        d("0.00001"),
        "never a step of one whole coin"
    );
    assert_eq!(btc.currency.code(), "USDT");
    assert_eq!(btc.source, MoneySource::Config);
    let settings = crate::risk_sizing::RiskSettings::default();
    let capital = crate::risk_sizing::Capital::new();
    let context = crate::risk_sizing::RiskContext {
        settings: &settings,
        capital: &capital,
        book,
        symbol: "BTCUSDT",
    };
    assert_eq!(context.money(), Some(btc), "sizing reads the same record");
    assert_eq!(book.units("BTCUSDT").money, Some(btc), "so do the readouts");

    let mut mine = btc.clone();
    mine.size_step = d("0.001");
    mine.source = MoneySource::Declared;
    declared.insert("BTCUSDT".to_owned(), mine);
    let book = PnlBook {
        declared: &declared,
        catalog: &catalog,
        feed: "binance",
    };
    assert_eq!(
        book.money("BTCUSDT").map(|money| money.size_step),
        Some(d("0.001")),
        "a declared record outranks the catalog"
    );
}

#[test]
fn small_money_never_reads_zero_and_zero_never_reads_negative() {
    assert_eq!(fmt_money(&money("0.004", "USDT")), "+0.004 USDT");
    assert_eq!(fmt_money(&money("-0.0012", "USDT")), "-0.0012 USDT");
    assert_eq!(fmt_money(&money("-0.00", "BRL")), "0.00 BRL");
    assert_eq!(fmt_money(&money("0", "BRL")), "0.00 BRL");
    assert_eq!(fmt_money(&money("80", "BRL")), "+80.00 BRL");
    assert_eq!(fmt_money(&money("-1234.567", "BRL")), "-1234.57 BRL");
}

fn b3_reader<'a>(
    declared: &'a InstrumentBook,
    catalog: &'a InstrumentCatalog,
    settings: &'a PnlSettings,
) -> PnlReader<'a> {
    PnlReader {
        book: PnlBook {
            declared,
            catalog,
            feed: "metatrader-b3",
        },
        settings,
    }
}

#[test]
fn win_and_wdo_points_never_sum_into_one_total() {
    let catalog = shipped();
    let declared = InstrumentBook::new();
    let settings = PnlSettings {
        chosen: Some(PnlPrimary::PointsPerContract),
        ..PnlSettings::default()
    };
    let reader = b3_reader(&declared, &catalog, &settings);
    let win = closed("400", "2");
    let wdo = closed("10", "1");
    let mixed = reader.sum([("WINZ25", &win), ("WDOX25", &wdo)]);
    assert!(mixed.mixed_units());
    assert_eq!(mixed.fmt(PnlPrimary::PointsPerContract), "mixed units");
    assert_eq!(mixed.fmt(PnlPrimary::PointsTimesQty), "mixed units");
    assert_eq!(
        reader.word(&mixed),
        "+180.00 BRL · mixed units",
        "falls back to money per currency, labelled"
    );
    let rolled = reader.sum([("WINV25", &win), ("WINZ25", &win)]);
    assert!(!rolled.mixed_units(), "two WIN contracts are one unit");
    assert_eq!(rolled.fmt(PnlPrimary::PointsPerContract), "+400 pts/ct");
}

/// A unit is the instrument, not what a point of it is worth: a BTCUSDT
/// and an ETHUSDT move are both worth 1 USDT and still never add up.
#[test]
fn btcusdt_and_ethusdt_moves_never_sum_into_one_total() {
    let catalog = shipped();
    let declared = InstrumentBook::new();
    let settings = PnlSettings {
        chosen: Some(PnlPrimary::PointsPerContract),
        ..PnlSettings::default()
    };
    let reader = b3_reader(&declared, &catalog, &settings);
    let btc = closed("60", "0.5");
    let eth = closed("10", "1");
    let mixed = reader.sum([("BTCUSDT", &btc), ("ETHUSDT", &eth)]);
    assert!(mixed.mixed_units());
    assert_eq!(mixed.fmt(PnlPrimary::PointsPerContract), "mixed units");
    assert_eq!(mixed.fmt(PnlPrimary::PointsTimesQty), "mixed units");
    assert_eq!(reader.word(&mixed), "+70.00 USDT · mixed units");
    let one = reader.sum([("BTCUSDT", &btc), ("BTCUSDT", &btc)]);
    assert!(!one.mixed_units(), "one pair is one unit");
    assert_eq!(one.fmt(PnlPrimary::PointsPerContract), "+240 USDT move/ct");
}

#[test]
fn a_total_is_tinted_by_the_figure_it_shows() {
    let catalog = shipped();
    let declared = InstrumentBook::new();
    // +10 points on one contract, then -3 points on five: per contract
    // +7, but the account lost 5 points, which is -1.00 BRL.
    let trades = [closed("10", "1"), closed("-15", "5")];
    let rows = trades.iter().map(|trade| ("WINZ25", trade));
    let mut settings = PnlSettings {
        chosen: Some(PnlPrimary::PointsPerContract),
        ..PnlSettings::default()
    };
    let total = b3_reader(&declared, &catalog, &settings).sum(rows.clone());
    assert!(b3_reader(&declared, &catalog, &settings).tint(&total) > Decimal::ZERO);
    settings.chosen = Some(PnlPrimary::Money);
    let reader = b3_reader(&declared, &catalog, &settings);
    assert_eq!(reader.word(&total), "-1.00 BRL · +7 pts/ct");
    assert!(
        reader.tint(&total) < Decimal::ZERO,
        "the money shown is a loss"
    );
}

#[test]
fn the_chart_label_is_the_ticket_lead_figure() {
    let catalog = shipped();
    let declared = InstrumentBook::new();
    for (primary, compact) in [
        (PnlPrimary::Money, "+80.00 BRL"),
        (PnlPrimary::PointsPerContract, "+200 pts × 2"),
        (PnlPrimary::PointsTimesQty, "+400 pts"),
    ] {
        let settings = PnlSettings {
            chosen: Some(primary),
            ..PnlSettings::default()
        };
        let reader = b3_reader(&declared, &catalog, &settings);
        assert_eq!(reader.compact("WINZ25", d("400"), d("2")), compact);
        assert!(
            reader
                .trade("WINZ25", d("400"), d("2"))
                .starts_with(&format!("{compact} · ")),
            "the ticket leads with the same figure"
        );
    }
}
