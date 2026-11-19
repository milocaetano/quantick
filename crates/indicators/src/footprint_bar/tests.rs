use super::*;
use quantick_engine::{DEFAULT_LEVEL_CAP, FootprintBuilder, Side, Trade};
use std::str::FromStr as _;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn trade(id: u64, price: &str, qty: &str) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: 1_700_000_000_000 + id as i64,
        price: dec(price),
        quantity: dec(qty),
        side: Side::Buy,
    }
}

fn bar_of(trades: &[Trade]) -> Bar {
    let mut bar = Bar::opened_by(&trades[0]);
    for t in &trades[1..] {
        bar.extend(t);
    }
    bar
}

fn ladder(trades: &[Trade], group: &str) -> BarFootprint {
    let mut builder = FootprintBuilder::new(dec(group), DEFAULT_LEVEL_CAP);
    for t in trades {
        builder.push(t);
    }
    builder.close_or_empty()
}

#[test]
fn poc_is_the_heaviest_row_priced_at_its_row_width() {
    let trades = [
        trade(1, "100.0", "1"),
        trade(2, "100.5", "3"),
        trade(3, "101.0", "1"),
    ];
    let footprint = ladder(&trades, "0.5");
    let bar = FootprintBar::new(bar_of(&trades), Some(&footprint));
    assert_eq!(bar.poc, Some(dec("100.5")));
    assert_eq!(IndicatorBar::from(&bar).poc, Some(100.5));
}

#[test]
fn no_footprint_means_no_poc() {
    let trades = [trade(1, "100.0", "1")];
    let bar = FootprintBar::new(bar_of(&trades), None);
    assert_eq!(bar.poc, None);
    assert_eq!(IndicatorBar::from(&bar).poc, None);
    assert_eq!(FootprintBar::from(bar_of(&trades)), bar);
}

#[test]
fn an_empty_ladder_has_no_poc() {
    let trades = [trade(1, "100.0", "1")];
    let empty = ladder(&[], "0.5");
    let bar = FootprintBar::new(bar_of(&trades), Some(&empty));
    assert_eq!(bar.poc, None);
}

#[test]
fn a_bare_engine_bar_projects_poc_as_na() {
    let trades = [trade(1, "100.0", "1")];
    assert_eq!(IndicatorBar::from(&bar_of(&trades)).poc, None);
}
