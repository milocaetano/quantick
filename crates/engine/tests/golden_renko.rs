//! Renko bricks as ProfitChart draws them, pinned twice: a hand-computed
//! golden over ten prints, and the trader's ProfitChart 50R chart of WIN over
//! a trimmed MetaTrader tape of the same two sessions.

use quantick_engine::bar_registry::{BUILTIN_BARS, InstrumentFacts};
use quantick_engine::{Bar, BarBuilder, RenkoBarBuilder, Trade, fixture, golden};
use rust_decimal::Decimal;

const TRADES: &str = include_str!("fixtures/renko_trades.csv");
const EXPECTED: &str = include_str!("fixtures/renko_n3_expected.csv");
const WIN_TAPE: &str = include_str!("fixtures/renko_win_50r_trades.csv");

#[test]
fn renko_bricks_match_golden() {
    golden::assert_golden(
        || RenkoBarBuilder::new(3, Some(Decimal::ONE)),
        TRADES,
        EXPECTED,
    );
}

/// Every closed brick of `trades`, through the call the chart, the backtest
/// and the bot make.
fn closed_bricks(builder: &mut dyn BarBuilder, trades: &[Trade]) -> Vec<Bar> {
    let mut closed = Vec::new();
    for trade in trades {
        builder.push_into(trade, &mut closed);
    }
    closed
}

#[test]
fn a_print_clearing_two_levels_closes_two_bricks_and_the_second_holds_no_print() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let mut builder = RenkoBarBuilder::new(3, Some(Decimal::ONE));
    // Trades 1-7 close two bricks; trade 8 alone closes the next two.
    let before = closed_bricks(&mut builder, &trades[..7]);
    assert_eq!(before.len(), 2);
    let mut closed = Vec::new();
    builder.push_into(&trades[7], &mut closed);
    assert_eq!(
        closed.len(),
        2,
        "7.0 clears the reversal level and one more"
    );
    let passed = &closed[1];
    assert_eq!(
        passed.trade_count, 0,
        "a brick a print cleared on its way past"
    );
    assert_eq!(
        passed.volume(),
        Decimal::ZERO,
        "holds no volume it never traded"
    );
    assert_eq!(
        (passed.open_time, passed.close_time),
        (trades[7].timestamp_ms, trades[7].timestamp_ms),
        "stamped with the print that cleared it"
    );
    let forming = builder
        .partial()
        .expect("the clearing print opens the next brick");
    assert_eq!(forming.trade_count, 1);
    assert_eq!(forming.open, passed.close, "it opens on the last close");
}

#[test]
fn without_a_price_step_no_brick_is_cut() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let mut builder = RenkoBarBuilder::new(3, None);
    assert!(closed_bricks(&mut builder, &trades).is_empty());
    let forming = builder.partial().expect("every print is held");
    assert_eq!(forming.trade_count, trades.len() as u64);
}

/// One brick of the reference: direction (`'U'` up, `'D'` down), open,
/// close, high, low, and its open time in UTC milliseconds.
type Brick = (char, i64, i64, i64, i64, Option<i64>);

/// ProfitChart's 50R chart of WINV26 on 2026-10-01 and 2026-10-02, brick by
/// brick. The open times are the brick's first print; the comments read them
/// on B3's clock, UTC-03:00, the clock the chart is drawn on. The first
/// brick's open time is the trimmed tape's, not the chart's, so it is left out.
const REFERENCE: [Brick; 17] = [
    ('U', 187425, 187670, 187670, 187115, None),
    ('U', 187670, 187915, 187915, 187255, Some(1_790_879_696_315)), // 10-01 15:34:56.315
    ('U', 187915, 188160, 188160, 187775, Some(1_790_883_185_942)), // 10-01 16:33:05.942
    ('D', 187915, 187670, 188185, 187670, Some(1_790_885_478_406)), // 10-01 17:11:18.406
    ('U', 187915, 188160, 188160, 187625, Some(1_790_942_562_523)), // 10-02 09:02:42.523
    ('U', 188160, 188405, 188405, 188160, Some(1_790_942_563_510)), // 10-02 09:02:43.510
    ('U', 188405, 188650, 188650, 188200, Some(1_790_942_563_686)), // 10-02 09:02:43.686
    ('D', 188405, 188160, 188755, 188160, Some(1_790_942_566_698)), // 10-02 09:02:46.698
    ('U', 188405, 188650, 188650, 188155, Some(1_790_942_585_671)), // 10-02 09:03:05.671
    ('U', 188650, 188895, 188895, 188515, Some(1_790_942_602_520)), // 10-02 09:03:22.520
    ('U', 188895, 189140, 189140, 188895, Some(1_790_942_680_911)), // 10-02 09:04:40.911
    ('U', 189140, 189385, 189385, 188980, Some(1_790_942_688_611)), // 10-02 09:04:48.611
    ('U', 189385, 189630, 189630, 189325, Some(1_790_942_767_594)), // 10-02 09:06:07.594
    ('U', 189630, 189875, 189875, 189575, Some(1_790_942_851_321)), // 10-02 09:07:31.321
    ('U', 189875, 190120, 190120, 189670, Some(1_790_942_906_629)), // 10-02 09:08:26.629
    ('D', 189875, 189630, 190140, 189630, Some(1_790_943_063_488)), // 10-02 09:11:03.488
    ('U', 189875, 190120, 190120, 189410, Some(1_790_943_160_677)), // 10-02 09:12:40.677
];

fn direction(bar: &Bar) -> char {
    if bar.close > bar.open { 'U' } else { 'D' }
}

/// `50R` on WIN, typed as the trader types it, on WIN's five-point step.
fn win_fifty_r() -> (Vec<Bar>, Option<Bar>) {
    let trades = fixture::parse_trades(WIN_TAPE).unwrap();
    let mut builder = BUILTIN_BARS
        .parse("renko:50")
        .unwrap()
        .build_for(InstrumentFacts {
            price_step: Some(Decimal::from(5)),
        });
    let closed = closed_bricks(&mut *builder, &trades);
    (closed, builder.partial().cloned())
}

#[test]
fn fifty_r_on_win_cuts_profitcharts_seventeen_bricks() {
    let (closed, _) = win_fifty_r();
    // A fresh builder cuts one brick before the reference starts, then the
    // reference's seventeen in a row and nothing after.
    assert_eq!(closed.len(), 1 + REFERENCE.len());
    for (index, (bar, expected)) in closed[1..].iter().zip(REFERENCE).enumerate() {
        let (dir, open, close, high, low, open_time) = expected;
        let got = (direction(bar), bar.open, bar.close, bar.high, bar.low);
        let want = (
            dir,
            Decimal::from(open),
            Decimal::from(close),
            Decimal::from(high),
            Decimal::from(low),
        );
        assert_eq!(got, want, "reference brick {}", index + 1);
        if let Some(open_time) = open_time {
            assert_eq!(
                bar.open_time,
                open_time,
                "reference brick {} opens",
                index + 1
            );
        }
    }
}

#[test]
fn a_fresh_builder_starts_in_the_brick_cell_holding_the_first_print() {
    // The first print, 187420, sits in the 245-point cell [187180, 187425];
    // the tape breaks its top first, so the first brick is that cell, up.
    let (closed, _) = win_fifty_r();
    let first = &closed[0];
    assert_eq!(
        (first.open, first.close, first.high, first.low),
        (
            Decimal::from(187_180),
            Decimal::from(187_425),
            Decimal::from(187_425),
            Decimal::from(187_180)
        )
    );
    assert_eq!(first.open_time, 1_790_877_252_981, "the tape's first print");
}

#[test]
fn the_forming_brick_opens_on_the_last_close() {
    // The print that closed the seventeenth brick, 190125, opens the next.
    let (_, forming) = win_fifty_r();
    let forming = forming.expect("a brick is forming");
    assert_eq!(
        (forming.open, forming.high, forming.low, forming.close),
        (
            Decimal::from(190_120),
            Decimal::from(190_125),
            Decimal::from(190_120),
            Decimal::from(190_125)
        )
    );
    assert_eq!(forming.trade_count, 1);
}
