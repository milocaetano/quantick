// A merge reads money only, converts each trade at its own day's rate,
// lists what it could not convert and never guesses.

use std::collections::BTreeMap;
use std::sync::Arc;

use quantick_civil::{TzOffset, parse_iso_date};
use quantick_engine::Side;
use quantick_sim::{ExitReason, history};
use quantick_trading::ClosedTrade;
use quantick_trading::rates::RateStore;

use super::*;
use crate::instruments::InstrumentCatalog;
use crate::pnl::{PnlBook, PnlSettings};
use crate::rates::StoredRates;
use crate::report::{HistoryRow, LoadedHistory, ReportPeriod, SourceFilter};
use crate::risk_sizing::InstrumentBook;

const CATALOG: &str = r#"
version = 1

[currencies]
same = { USDT = "USD", USDC = "USD" }

[[rates]]
from = "USDT"
to = "BRL"
venue = "binance"
symbol = "USDTBRL"

[instruments.WIN]
series = "b3"
currency = "BRL"
point_value = "0.20"
size_step = "1"
price_unit = "points"

[instruments.BTCUSDT]
currency = "USDT"
point_value = "1"
size_step = "0.00001"
price_unit = "quote"
"#;

const UTC: TzOffset = TzOffset::new(0);

fn d(text: &str) -> Decimal {
    Decimal::from_str_exact(text).expect("test decimal literal")
}

fn code(text: &str) -> Currency {
    Currency::new(text).expect("test currency")
}

fn day(text: &str) -> CivilDate {
    parse_iso_date(text).expect("test day")
}

/// A trade of `symbol` closed at noon UTC on `on`, its stored figure
/// `pnl_points` (move × quantity) on one contract.
fn row(symbol: &str, on: &str, pnl_points: &str, broker: bool) -> HistoryRow {
    let closed_ms = day(on).start_ms(UTC) + 12 * 3_600_000;
    let trade = ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
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
        fees_points: Decimal::ZERO,
    };
    HistoryRow {
        symbol: symbol.to_owned(),
        source: broker.then_some(history::SessionSource::Broker(
            quantick_trading::TradeMode::Demo,
        )),
        trade_ref: history::TradeRefs::default().next(&trade),
        trade,
        session: None,
        tags: Vec::new(),
    }
}

struct World {
    declared: InstrumentBook,
    catalog: Arc<InstrumentCatalog>,
    settings: PnlSettings,
    store: RateStore,
}

impl World {
    fn new(primary: PnlPrimary) -> Self {
        let catalog = Arc::new(InstrumentCatalog::parse(CATALOG).expect("fixture catalog"));
        let mut store = RateStore::default();
        assert!(store.insert(code("USDT"), code("BRL"), day("2026-11-20"), d("5")));
        Self {
            declared: InstrumentBook::new(),
            settings: PnlSettings {
                catalog: Arc::clone(&catalog),
                chosen: Some(primary),
                ..PnlSettings::default()
            },
            catalog,
            store,
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

    /// `rows` cut over all time, worded, then merged into `target`.
    fn view(&self, rows: Vec<HistoryRow>, target: Option<&str>) -> ReportView {
        let history = LoadedHistory {
            rows,
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
            sessions: BTreeMap::new(),
        };
        let rates = StoredRates {
            rules: self.catalog.currencies(),
            store: &self.store,
        };
        ReportView::cut(&history, ReportPeriod::All, SourceFilter::All, &Default::default(), UTC)
            .with_pnl(self.reader())
            .merged(target.map(code), self.reader(), &rates)
    }
}

/// +400 WIN points (80 BRL), then +60 USDT, both on the 20th.
fn brl_and_usdt() -> Vec<HistoryRow> {
    vec![
        row("WINZ26", "2026-11-20", "400", false),
        row("BTCUSDT", "2026-11-20", "60", false),
    ]
}

fn plot(view: &ReportView) -> Vec<Decimal> {
    view.equity.walks()[0].points.clone()
}

#[test]
fn brl_and_usdt_merge_in_brl_at_the_days_rate() {
    let world = World::new(PnlPrimary::Money);
    let per_asset = world.view(brl_and_usdt(), None);
    assert_eq!(per_asset.equity.walks().len(), 2, "per asset: a curve per currency");
    assert_eq!(per_asset.merge.per_asset_walks, 2);
    assert_eq!(per_asset.merge.currencies, vec![code("BRL"), code("USD")]);
    assert!(per_asset.merge.readout.is_none());

    let view = world.view(brl_and_usdt(), Some("BRL"));
    assert_eq!(view.equity.walks().len(), 1, "one merged curve");
    assert_eq!(plot(&view), vec![d("0"), d("80"), d("380")]);
    assert_eq!(view.equity.walks()[0].unit, "BRL");
    assert_eq!(view.net_text.0, "+380.00 BRL");
    assert_eq!(view.net_text.1, "+80.00 BRL / +60.00 USDT", "the native sums behind it");
    let readout = view.merge.readout.as_ref().expect("merged");
    assert_eq!(readout.net, d("380"));
    assert_eq!(
        readout.rates,
        vec![RateUsed {
            day: day("2026-11-20"),
            from: code("USDT"),
            to: code("BRL"),
            rate: d("5"),
        }]
    );
    assert!(readout.unconverted.is_empty());
    assert_eq!(
        readout.caption(),
        "Merged in BRL · BTCUSDT USDT + WINZ26 BRL · USDT→BRL 5 on 2026-11-20"
    );
    assert_eq!(view.merge.per_asset_walks, 2, "the switch stays on offer");
}

#[test]
fn brl_and_usdt_merge_in_usd_through_the_reverse_rate_and_the_peg() {
    let world = World::new(PnlPrimary::Money);
    let view = world.view(brl_and_usdt(), Some("USD"));
    assert_eq!(plot(&view), vec![d("0"), d("16"), d("76")]);
    assert_eq!(view.net_text.0, "+76.00 USD");
    let readout = view.merge.readout.as_ref().expect("merged");
    assert_eq!(
        readout.rates,
        vec![RateUsed {
            day: day("2026-11-20"),
            from: code("BRL"),
            to: code("USD"),
            rate: d("0.2"),
        }],
        "BRL→USD is the stored USDT→BRL inverted; USDT needs no rate to be USD"
    );
}

#[test]
fn a_trade_without_a_rate_or_a_point_value_is_left_out_and_listed() {
    let world = World::new(PnlPrimary::Money);
    let mut rows = brl_and_usdt();
    rows.push(row("BTCUSDT", "2026-11-21", "100", false));
    rows.push(row("XYZ", "2026-11-21", "7", false));
    let view = world.view(rows, Some("BRL"));
    assert_eq!(
        plot(&view),
        vec![d("0"), d("80"), d("380"), d("380"), d("380")],
        "flat where a trade could not be converted"
    );
    let readout = view.merge.readout.as_ref().expect("merged");
    assert_eq!(readout.net, d("380"), "the net leaves them out too");
    assert_eq!(
        readout.unconverted,
        vec![
            Unconverted {
                symbol: "BTCUSDT".to_owned(),
                day: day("2026-11-21"),
                why: Unconvertible::NoRate(code("USDT")),
                trades: 1,
            },
            Unconverted {
                symbol: "XYZ".to_owned(),
                day: day("2026-11-21"),
                why: Unconvertible::PointValueUnset,
                trades: 1,
            },
        ]
    );
    assert!(
        readout.caption().ends_with(
            "not converted: 2 trade(s) - BTCUSDT 2026-11-21 no USDT→BRL rate, \
             XYZ 2026-11-21 pt value unset"
        ),
        "{}",
        readout.caption()
    );
}

#[test]
fn win_alone_reads_in_usd_when_asked() {
    let world = World::new(PnlPrimary::Money);
    let rows = vec![row("WINZ26", "2026-11-20", "400", false)];
    let own = world.view(rows.clone(), None);
    assert_eq!(own.merge.per_asset_walks, 1, "one walk: no Per asset | Merge all switch");
    assert_eq!(own.merge.own, Some(code("BRL")));
    assert_eq!(own.merge.currencies, vec![code("BRL"), code("USD")]);
    let usd = world.view(rows, Some("USD"));
    assert_eq!(plot(&usd), vec![d("0"), d("16")]);
    assert_eq!(usd.net_text.0, "+16.00 USD");
}

#[test]
fn points_merge_one_instrument_across_sources_and_never_two_instruments() {
    let world = World::new(PnlPrimary::PointsTimesQty);
    let win = vec![
        row("WINZ26", "2026-11-20", "400", false),
        row("WIN$N", "2026-11-20", "-100", true),
    ];
    let view = world.view(win.clone(), None);
    assert_eq!(view.equity.walks().len(), 1, "sim and broker WIN: one points curve");
    assert_eq!(plot(&view), vec![d("0"), d("400"), d("300")]);

    let mut mixed = win;
    mixed.push(row("BTCUSDT", "2026-11-20", "60", false));
    let view = world.view(mixed, Some("BRL"));
    assert_eq!(view.equity.walks().len(), 2, "WIN and BTC points stay apart");
    assert!(view.merge.readout.is_none(), "Merge all is money only");
    assert_eq!(view.merge.note, Some(POINTS_APART));
}
