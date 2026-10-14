// The report worded per figure: ledger totals per currency, the equity
// walk in money, points per contract and grouped points, and the honest
// fallback where money cannot be plotted.

use std::collections::BTreeMap;
use std::sync::Arc;

use quantick_civil::TzOffset;
use quantick_engine::Side;
use quantick_sim::{ExitReason, history};

use super::*;
use crate::instruments::InstrumentCatalog;
use crate::pnl::{PnlBook, PnlSettings, UNSET_MONEY};
use crate::report::{LoadedHistory, ReportPeriod, SourceFilter};
use crate::risk_sizing::InstrumentBook;

const CATALOG: &str = r#"
version = 1

[instruments."WIN*"]
currency = "BRL"
point_value = "0.20"
size_step = "1"
min_size = "1"
price_unit = "points"

[instruments.BTCUSDT]
currency = "USDT"
point_value = "1"
size_step = "0.00001"
min_size = "0.00001"
price_unit = "quote"
"#;

fn d(text: &str) -> Decimal {
    Decimal::from_str_exact(text).expect("test decimal literal")
}

fn row(symbol: &str, closed_ms: i64, pnl_points: &str, quantity: &str) -> HistoryRow {
    let trade = ClosedTrade {
        side: Side::Buy,
        quantity: d(quantity),
        entry_price: d("100"),
        exit_price: d("100"),
        opened_ms: closed_ms - 1,
        closed_ms,
        pnl_points: d(pnl_points),
        exit_reason: ExitReason::Manual,
        entry_agg_id: None,
        exit_agg_id: None,
        mae_points: None,
        mfe_points: None,
    };
    HistoryRow {
        symbol: symbol.to_owned(),
        source: None,
        trade_ref: history::TradeRefs::default().next(&trade),
        trade,
        session: None,
        tags: Vec::new(),
    }
}

/// One fixture world: the catalog above, nothing declared, and a choice.
struct World {
    declared: InstrumentBook,
    catalog: Arc<InstrumentCatalog>,
    settings: PnlSettings,
}

impl World {
    fn new(chosen: Option<PnlPrimary>) -> Self {
        let catalog = Arc::new(InstrumentCatalog::parse(CATALOG).expect("fixture catalog"));
        Self {
            declared: InstrumentBook::new(),
            settings: PnlSettings {
                catalog: Arc::clone(&catalog),
                chosen,
            },
            catalog,
        }
    }

    fn reader(&self) -> PnlReader<'_> {
        PnlReader {
            book: PnlBook {
                declared: &self.declared,
                catalog: &self.catalog,
                feed: "",
            },
            settings: &self.settings,
        }
    }
}

/// Two winners and a loser on the mini index: +200 pts on 2, -100 on 1,
/// +50 on 1.
fn win_rows() -> Vec<HistoryRow> {
    vec![
        row("WINZ26", 1_000, "400", "2"),
        row("WINZ26", 2_000, "-100", "1"),
        row("WINZ26", 3_000, "50", "1"),
    ]
}

#[test]
fn the_ledger_totals_keep_each_currency_apart() {
    let world = World::new(None);
    let rows = vec![
        row("WINZ26", 1_000, "400", "2"),
        row("BTCUSDT", 2_000, "60", "0.5"),
    ];
    let totals = LedgerTotals::of_rows(world.reader(), &rows);
    assert_eq!((totals.trades, totals.wins), (2, 2));
    assert_eq!(totals.win_rate(), Some(100));
    let money: BTreeMap<String, Decimal> = totals
        .net
        .money()
        .expect("both are priced")
        .iter()
        .map(|(currency, amount)| (currency.code().to_owned(), *amount))
        .collect();
    assert_eq!(
        money,
        BTreeMap::from([("BRL".to_owned(), d("80")), ("USDT".to_owned(), d("60"))]),
        "reais and tether are never added together"
    );
    assert_eq!(
        world.reader().word(&totals.net),
        "+80.00 BRL / +60.00 USDT · mixed units"
    );
    assert_eq!(
        LedgerTotals::default().plus(&totals),
        totals,
        "an empty half must be the identity"
    );
    let session = LedgerTotals::of_trades(world.reader(), "WINZ26", [&rows[0].trade]);
    assert_eq!(
        world.reader().word(&totals.plus(&session).net),
        "+160.00 BRL / +60.00 USDT · mixed units",
        "the session's half folds into the saved one per currency"
    );
    assert_eq!(LedgerTotals::default().win_rate(), None, "0/0 is not 0%");
}

#[test]
fn a_ledger_total_with_an_unpriced_trade_is_never_a_partial_money_sum() {
    let world = World::new(Some(PnlPrimary::Money));
    let rows = vec![
        row("WINZ26", 1_000, "400", "2"),
        row("XYZ", 2_000, "5", "1"),
    ];
    let totals = LedgerTotals::of_rows(world.reader(), &rows);
    assert_eq!(totals.net.money(), None);
    assert_eq!(
        world.reader().word(&totals.net),
        format!("{UNSET_MONEY} · mixed units")
    );
    let world = World::new(None);
    assert_eq!(
        world.reader().word(&totals.net),
        format!("{UNSET_MONEY} · mixed units"),
        "points of two instruments never add up, choice or not"
    );
}

#[test]
fn the_equity_walk_follows_the_chosen_figure() {
    let rows = win_rows();
    let cases = [
        (
            PnlPrimary::Money,
            ["0", "80", "60", "70"],
            "BRL",
            "+80.00 BRL",
            "-20.00 BRL",
        ),
        (
            PnlPrimary::PointsPerContract,
            ["0", "200", "100", "150"],
            "pts/ct",
            "+200 pts/ct",
            "-100 pts/ct",
        ),
        (
            PnlPrimary::PointsTimesQty,
            ["0", "400", "300", "350"],
            "pts",
            "+400 pts",
            "-100 pts",
        ),
    ];
    for (primary, points, unit, first, drawdown) in cases {
        let world = World::new(Some(primary));
        let reader = world.reader();
        let total = reader.sum(rows.iter().map(|row| (row.symbol.as_str(), &row.trade)));
        let walk = EquityWalk::read(&rows, reader, &total);
        let expected: Vec<Decimal> = points.iter().map(|text| d(text)).collect();
        assert_eq!(walk.points, expected, "{primary:?}");
        assert_eq!(walk.primary, primary);
        assert_eq!(walk.unit, unit);
        assert_eq!(walk.fallback, None);
        assert_eq!(walk.text(1), first);
        assert_eq!(walk.caption(), format!("REALIZED EQUITY · {unit}"));
        assert_eq!(
            walk.drawdown.map(|(peak, trough, _)| (peak, trough)),
            Some((1, 2))
        );
        assert_eq!(walk.drawdown_text(), drawdown);
    }
}

#[test]
fn the_equity_walk_falls_back_to_grouped_points_and_says_why() {
    let world = World::new(Some(PnlPrimary::Money));
    let reader = world.reader();
    let rows = vec![row("XYZ", 1_000, "5", "1"), row("XYZ", 2_000, "-3", "1")];
    let total = reader.sum(rows.iter().map(|row| (row.symbol.as_str(), &row.trade)));
    let walk = EquityWalk::read(&rows, reader, &total);
    assert_eq!(walk.primary, PnlPrimary::PointsTimesQty);
    assert_eq!(walk.fallback, Some("pt value unset"));
    assert_eq!(walk.points, vec![d("0"), d("5"), d("2")]);
    assert!(
        walk.caption().ends_with("(pt value unset)"),
        "the curve says why it is not in money: {}",
        walk.caption()
    );
}

/// No money to plot and points worth different things: no curve, no
/// drawdown and no equity cells — never a sum of index points and tether.
#[test]
fn a_walk_over_mixed_units_without_money_plots_nothing() {
    let world = World::new(Some(PnlPrimary::Money));
    let reader = world.reader();
    for rows in [
        vec![
            row("WINZ26", 1_000, "400", "2"),
            row("BTCUSDT", 2_000, "60", "0.5"),
        ],
        vec![
            row("WINZ26", 1_000, "400", "2"),
            row("XYZ", 2_000, "-500", "1"),
        ],
        vec![row("ABC", 1_000, "5", "1"), row("XYZ", 2_000, "-9", "1")],
    ] {
        let total = reader.sum(rows.iter().map(|row| (row.symbol.as_str(), &row.trade)));
        let walk = EquityWalk::read(&rows, reader, &total);
        assert!(!walk.plotted());
        assert!(walk.points.is_empty() && walk.plot.is_empty());
        assert_eq!(walk.drawdown, None, "no chip on a curve not drawn");
        assert_eq!(walk.text(0), "");
        assert_eq!(walk.text(rows.len()), "", "no EQUITY cell value");
        assert_eq!(walk.caption(), "REALIZED EQUITY (mixed units — no curve)");
    }
}

/// Points chosen outright, not fallen back to, still never sum across
/// instruments: index points and tether moves are worth different things.
#[test]
fn a_walk_in_chosen_points_over_mixed_units_plots_nothing() {
    for chosen in [PnlPrimary::PointsTimesQty, PnlPrimary::PointsPerContract] {
        let world = World::new(Some(chosen));
        let reader = world.reader();
        let rows = vec![
            row("WINZ26", 1_000, "400", "2"),
            row("BTCUSDT", 2_000, "60", "0.5"),
        ];
        let total = reader.sum(rows.iter().map(|row| (row.symbol.as_str(), &row.trade)));
        let walk = EquityWalk::read(&rows, reader, &total);
        assert!(!walk.plotted(), "{chosen:?} plotted a sum across units");
        assert_eq!(walk.drawdown, None, "no chip on a curve not drawn");
        assert_eq!(walk.text(rows.len()), "", "no EQUITY cell value");
    }
}

#[test]
fn the_bare_walk_stays_in_stored_points() {
    let walk = EquityWalk::of(&win_rows());
    assert_eq!(walk.points, vec![d("0"), d("400"), d("300"), d("350")]);
    assert_eq!(walk.unit, "pts");
    assert_eq!(walk.drawdown, Some((1, 2, d("100"))));
    let flat = EquityWalk::of(&[]);
    assert_eq!(flat.drawdown, None, "no trades, no drawdown");
    assert_eq!(flat.text(0), "0 pts");
}

#[test]
fn a_worded_view_leads_each_row_and_the_net_with_the_chosen_figure() {
    let history = LoadedHistory {
        rows: win_rows(),
        files: 1,
        unreadable_files: 0,
        problem_rows: 0,
        sessions: BTreeMap::new(),
    };
    let utc = TzOffset::new(0);
    let cut = || ReportView::cut(&history, ReportPeriod::All, SourceFilter::All, None, utc);
    assert_eq!(cut().trade_text(0), "", "a bare cut words nothing");

    let world = World::new(None);
    let view = cut().with_pnl(world.reader());
    assert_eq!(view.trade_text(0), "+80.00 BRL · +200 pts × 2");
    assert_eq!(
        view.trade_text(9),
        "",
        "past the end reads empty, never panics"
    );
    assert_eq!(
        view.net_text,
        ("+70.00 BRL".to_owned(), "+150 pts/ct".to_owned())
    );
    assert_eq!(view.equity.unit, "BRL");
    assert!(view.worded_by(world.reader()));

    let grouped = World::new(Some(PnlPrimary::PointsTimesQty));
    assert!(
        !view.worded_by(grouped.reader()),
        "a mode switch makes the cached wording stale"
    );
    let mut declared = World::new(None);
    declared.declared.insert(
        "WINZ26".to_owned(),
        quantick_trading::InstrumentMoney {
            point_value: d("1"),
            size_step: d("1"),
            min_size: d("1"),
            max_size: None,
            currency: quantick_trading::Currency::new("BRL").expect("BRL"),
            source: quantick_trading::MoneySource::Declared,
        },
    );
    assert!(
        !view.worded_by(declared.reader()),
        "so does a point value the trader declares"
    );
    let view = cut().with_pnl(grouped.reader());
    assert_eq!(view.trade_text(0), "+400 pts · +80.00 BRL");
    assert_eq!(view.equity.text(3), "+350 pts");
}

#[test]
fn saved_totals_are_summed_again_when_the_money_changes() {
    let rows = vec![row("WINZ26", 1_000, "400", "2")];
    let world = World::new(None);
    let mut totals = LedgerTotals::of_rows(world.reader(), &rows);
    let eighty = BTreeMap::from([(
        quantick_trading::Currency::new("BRL").expect("BRL"),
        d("80"),
    )]);
    assert_eq!(totals.net.money(), Some(&eighty));
    totals.reworded(world.reader(), Some(&rows));
    assert_eq!(totals.net.money(), Some(&eighty), "same reader, same sum");

    let mut declared = World::new(None);
    declared.declared.insert(
        "WINZ26".to_owned(),
        quantick_trading::InstrumentMoney {
            point_value: d("1"),
            size_step: d("1"),
            min_size: d("1"),
            max_size: None,
            currency: quantick_trading::Currency::new("BRL").expect("BRL"),
            source: quantick_trading::MoneySource::Declared,
        },
    );
    totals.reworded(declared.reader(), Some(&rows));
    let four_hundred = BTreeMap::from([(
        quantick_trading::Currency::new("BRL").expect("BRL"),
        d("400"),
    )]);
    assert_eq!(
        totals.net.money(),
        Some(&four_hundred),
        "a declared point value re-sums the saved half"
    );
}
