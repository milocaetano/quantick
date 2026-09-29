//! Each quiet candle dot sums its factual native levels at their weighted price.

use super::*;
use crate::projection::{
    CandleDotFrame, CandleDotGrid, CandleDotView, CandleFootprint, CandleFootprintSource,
    project_candle_dots,
};
use quantick_engine::{BarFootprint, FootprintBuilder};

fn print(id: u64, price: &str, quantity: &str, side: Side) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: 1_000,
        price: dec(price),
        quantity: dec(quantity),
        side,
    }
}

fn ladder(group: &str, prints: &[Trade]) -> BarFootprint {
    let mut builder = FootprintBuilder::new(dec(group), 2_000);
    for trade in prints {
        builder.push(trade);
    }
    builder.close().unwrap()
}

fn factual(slot: usize, ladder: &BarFootprint) -> CandleFootprint<'_> {
    CandleFootprint {
        slot,
        ladder,
        source: CandleFootprintSource::TradeBuilt,
    }
}

fn grid() -> Option<CandleDotGrid> {
    Some(CandleDotGrid {
        step: dec("5"),
        reference_price: dec("100"),
    })
}

fn view() -> CandleDotView {
    CandleDotView {
        prices: PriceWindow::new(dec("90"), dec("120")).unwrap(),
        candle_width_px: 32.0,
    }
}

fn totals(frame: &CandleDotFrame) -> (Decimal, Decimal, u64) {
    frame.marks.iter().fold(
        (Decimal::ZERO, Decimal::ZERO, 0),
        |(buy, sell, count), dot| {
            (
                buy + dot.buy_quantity,
                sell + dot.sell_quantity,
                count + dot.trade_count,
            )
        },
    )
}

#[test]
fn one_candle_dot_preserves_exact_sides_count_and_quantity_weighted_price() {
    let first = ladder(
        "5",
        &[
            print(1, "100", "0.1", Side::Buy),
            print(2, "100", "0.2", Side::Sell),
            print(3, "105", "1.2", Side::Buy),
        ],
    );
    let next = ladder("5", &[print(4, "100", "0.4", Side::Sell)]);
    let frame = project_candle_dots([factual(17, &first), factual(18, &next)], grid(), view());
    assert_eq!(frame.full_quantity, dec("1.5"));
    assert_eq!(totals(&frame), (dec("1.3"), dec("0.6"), 4));
    assert_eq!(
        frame
            .marks
            .iter()
            .map(|dot| (dot.slot, dot.price))
            .collect::<Vec<_>>(),
        [(17, dec("104")), (18, dec("100"))]
    );
    assert_eq!(frame.marks[0].buy_quantity, dec("1.3"));
    assert_eq!(frame.marks[0].sell_quantity, dec("0.2"));
    assert_eq!(frame.marks[0].trade_count, 3);
    assert_eq!(
        frame.marks[0].price,
        (dec("100") * dec("0.3") + dec("105") * dec("1.2")) / dec("1.5"),
        "price is the exact Decimal moment divided once by the whole candle quantity"
    );
    let reversed = project_candle_dots([factual(18, &next), factual(17, &first)], grid(), view());
    assert_eq!(
        frame.marks, reversed.marks,
        "input iteration order is not display identity"
    );
}

#[test]
fn visible_candle_totals_supply_the_exact_decimal_area_reference() {
    let small = ladder("5", &[print(1, "100", "1", Side::Buy)]);
    let large = ladder(
        "5",
        &[
            print(2, "100", "1", Side::Sell),
            print(3, "110", "3", Side::Sell),
        ],
    );
    let hidden = ladder("5", &[print(4, "150", "100000", Side::Buy)]);
    let frame = project_candle_dots(
        [factual(7, &small), factual(8, &large), factual(9, &hidden)],
        grid(),
        view(),
    );
    assert_eq!(frame.full_quantity, dec("4"));
    assert_eq!(frame.marks.len(), 2);
    assert_eq!(frame.marks[0].radius_px, 2.0);
    assert_eq!(frame.marks[1].radius_px, 4.0);
    assert_eq!(
        frame.marks[1].radius_px.powi(2),
        4.0 * frame.marks[0].radius_px.powi(2)
    );
    assert_eq!(frame.marks[1].price, dec("107.5"));
    assert_eq!(totals(&frame), (dec("1"), dec("4"), 3));
}

#[test]
fn candle_visibility_filters_the_weighted_center_after_summing_every_native_row() {
    let crossing = ladder(
        "5",
        &[
            print(1, "80", "1", Side::Buy),
            print(2, "130", "1", Side::Sell),
        ],
    );
    let mostly_outside = ladder(
        "5",
        &[
            print(3, "100", "1", Side::Buy),
            print(4, "150", "9", Side::Sell),
        ],
    );
    let frame = project_candle_dots(
        [factual(7, &crossing), factual(8, &mostly_outside)],
        grid(),
        view(),
    );
    assert_eq!(frame.marks.len(), 1);
    assert_eq!(frame.marks[0].slot, 7);
    assert_eq!(frame.marks[0].price, dec("105"));
    assert_eq!(totals(&frame), (Decimal::ONE, Decimal::ONE, 2));
    assert_eq!(frame.full_quantity, dec("2"));
    assert_eq!(frame.marks[0].radius_px, 4.0);
}

#[test]
fn a_current_partial_has_one_dot_and_updates_without_closing_the_candle() {
    let mut builder = FootprintBuilder::new(dec("5"), 2_000);
    builder.push(&print(1, "100", "9", Side::Buy));
    let before = project_candle_dots([factual(18, builder.partial().unwrap())], grid(), view());
    assert_eq!(before.marks.len(), 1);
    assert_eq!(before.marks[0].price, dec("100"));
    builder.push(&print(2, "105", "16", Side::Sell));
    let after = project_candle_dots([factual(18, builder.partial().unwrap())], grid(), view());
    assert_eq!(after.marks.len(), 1);
    assert_eq!(after.marks[0].slot, 18);
    assert_eq!(after.marks[0].price, dec("103.2"));
    assert_eq!(totals(&after), (dec("9"), dec("16"), 2));
    assert_eq!(after.full_quantity, dec("25"));
}

#[test]
fn candle_radius_stays_inside_its_column_without_a_native_price_row_cap() {
    let current = ladder("5", &[print(1, "100", "10", Side::Buy)]);
    let narrow = project_candle_dots(
        [factual(0, &current)],
        grid(),
        CandleDotView {
            candle_width_px: 4.0,
            ..view()
        },
    );
    assert_eq!(narrow.marks[0].radius_px, 2.0);
    let broad_price_range = project_candle_dots(
        [factual(0, &current)],
        grid(),
        CandleDotView {
            prices: PriceWindow::new(Decimal::ZERO, dec("10000")).unwrap(),
            ..view()
        },
    );
    assert_eq!(broad_price_range.marks[0].radius_px, 4.0);
}

#[test]
fn fine_aligned_rows_are_exact_but_offset_or_incompatible_grids_are_not() {
    let prints = [
        print(1, "100", "1", Side::Buy),
        print(2, "105", "3", Side::Sell),
    ];
    let fine = ladder("1", &prints);
    let frame = project_candle_dots([factual(0, &fine)], grid(), view());
    assert_eq!(frame.marks.len(), 1);
    assert_eq!(frame.marks[0].price, dec("103.75"));
    let native = ladder("5", &prints);
    assert_eq!(
        frame,
        project_candle_dots([factual(0, &native)], grid(), view()),
        "empty sub-tick capture buckets cannot shrink native-price dots"
    );
    let incompatible = ladder("2", &[print(1, "105", "3", Side::Buy)]);
    assert!(
        project_candle_dots([factual(0, &incompatible)], grid(), view())
            .marks
            .is_empty()
    );
    let offset = ladder("5", &[print(1, "102", "3", Side::Buy)]);
    let offset_grid = Some(CandleDotGrid {
        step: dec("5"),
        reference_price: dec("102"),
    });
    assert!(
        project_candle_dots([factual(0, &offset)], offset_grid, view())
            .marks
            .is_empty()
    );
    assert!(
        project_candle_dots([factual(0, &fine)], None, view())
            .marks
            .is_empty()
    );
}

#[test]
fn a_coarse_base_or_cap_fold_cannot_claim_an_exact_execution_price() {
    let prints = [
        print(1, "100", "2", Side::Buy),
        print(2, "105", "3", Side::Sell),
    ];
    let coarse = ladder("10", &prints);
    assert!(
        !coarse.is_aggregated(),
        "a configured coarse base is distinct from a cap fold"
    );
    assert!(
        project_candle_dots([factual(0, &coarse)], grid(), view())
            .marks
            .is_empty()
    );
    let mut capped = FootprintBuilder::new(dec("5"), 1);
    for trade in &prints {
        capped.push(trade);
    }
    let capped = capped.close().unwrap();
    assert!(capped.is_aggregated());
    assert!(
        project_candle_dots([factual(0, &capped)], grid(), view())
            .marks
            .is_empty()
    );
}

#[test]
fn an_approximated_venue_ladder_never_becomes_execution_dots() {
    let first = print(1, "100", "2", Side::Buy);
    let mut bar = Bar::opened_by(&first);
    bar.extend(&print(2, "110", "3", Side::Sell));
    let guessed = BarFootprint::approximated(&bar, dec("5"), 2_000).unwrap();
    assert!(!guessed.is_aggregated());
    let frame = project_candle_dots(
        [CandleFootprint {
            slot: 0,
            ladder: &guessed,
            source: CandleFootprintSource::Approximate,
        }],
        grid(),
        view(),
    );
    assert!(frame.marks.is_empty());
    assert_eq!(frame.full_quantity, Decimal::ONE);
}

#[test]
fn invalid_geometry_or_native_step_cannot_emit_nonfinite_dots() {
    let current = ladder("5", &[print(1, "100", "10", Side::Buy)]);
    for invalid in [
        CandleDotView {
            candle_width_px: 0.0,
            ..view()
        },
        CandleDotView {
            candle_width_px: f32::NAN,
            ..view()
        },
        CandleDotView {
            candle_width_px: f32::INFINITY,
            ..view()
        },
        CandleDotView {
            candle_width_px: -1.0,
            ..view()
        },
    ] {
        assert!(
            project_candle_dots([factual(0, &current)], grid(), invalid)
                .marks
                .is_empty()
        );
    }
    let invalid_grid = Some(CandleDotGrid {
        step: Decimal::ZERO,
        reference_price: dec("100"),
    });
    assert!(
        project_candle_dots([factual(0, &current)], invalid_grid, view())
            .marks
            .is_empty()
    );
}
