//! Each quiet candle dot sums its factual native levels at their weighted price.
//! Zoomed out, neighbouring candles fold into one mark by absolute slot: the
//! sums stay exact, panning never regroups, and area follows volume.

use super::*;
use crate::projection::{
    CANDLE_GROUP_HOLD_BAND, CANDLE_GROUP_MIN_WIDTH_PX, CANDLE_MARK_MAX_RADIUS_PX, CandleDot,
    CandleDotFrame, CandleDotGrid, CandleDotView, CandleFootprint, CandleFootprintSource,
    CandleGroupMemory, project_candle_dots,
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
        visible: (0, 64),
        candles_per_mark: 1,
    }
}

fn grouped(
    candles_per_mark: usize,
    candle_width_px: f32,
    visible: (usize, usize),
) -> CandleDotView {
    CandleDotView {
        candle_width_px,
        visible,
        candles_per_mark,
        ..view()
    }
}

/// A mark without its radius: the facts grouping and panning must keep.
fn facts(dot: &CandleDot) -> (usize, usize, Decimal, Decimal, Decimal, u64) {
    (
        dot.slot,
        dot.last_slot,
        dot.price,
        dot.buy_quantity,
        dot.sell_quantity,
        dot.trade_count,
    )
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
    assert_eq!(frame.marks[0].radius_px, CANDLE_MARK_MAX_RADIUS_PX / 2.0);
    assert_eq!(frame.marks[1].radius_px, CANDLE_MARK_MAX_RADIUS_PX);
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
    assert_eq!(frame.marks[0].radius_px, CANDLE_MARK_MAX_RADIUS_PX);
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
    assert_eq!(
        broad_price_range.marks[0].radius_px,
        CANDLE_MARK_MAX_RADIUS_PX
    );
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
        CandleDotView {
            candles_per_mark: 0,
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

#[test]
fn a_group_sums_its_members_exactly_at_their_quantity_weighted_price() {
    let members = [
        ladder(
            "5",
            &[
                print(1, "100", "1", Side::Buy),
                print(2, "105", "2", Side::Sell),
            ],
        ),
        ladder("5", &[print(3, "110", "3", Side::Buy)]),
        ladder("5", &[print(4, "100", "0.5", Side::Sell)]),
        ladder(
            "5",
            &[
                print(5, "115", "1.5", Side::Buy),
                print(6, "115", "0.5", Side::Sell),
            ],
        ),
        ladder("5", &[print(7, "95", "7", Side::Sell)]),
        // Its own weighted price is off the window; the group's is not.
        ladder("5", &[print(8, "150", "1", Side::Buy)]),
    ];
    let inputs = || {
        members
            .iter()
            .enumerate()
            .map(|(index, member)| factual(8 + index, member))
    };
    let view = grouped(4, 2.0, (8, 14));
    let frame = project_candle_dots(inputs(), grid(), view);
    assert_eq!(
        frame.marks.iter().map(facts).collect::<Vec<_>>(),
        [
            (8, 11, dec("920") / dec("8.5"), dec("5.5"), dec("3"), 6),
            (12, 13, dec("101.875"), dec("1"), dec("7"), 2),
        ],
        "slots 8..=11 and 12..=13 fold by slot / 4: exact Decimal sums, one \
         division of the summed price moment"
    );
    assert_eq!(frame.full_quantity, dec("8.5"));
    let alone = project_candle_dots(inputs().take(4), grid(), grouped(1, 32.0, (8, 12)));
    assert_eq!(alone.marks.len(), 4);
    assert_eq!(
        totals(&alone),
        (dec("5.5"), dec("3"), 6),
        "the group's sums are its candles' sums"
    );
    let reversed: Vec<_> = inputs().collect::<Vec<_>>().into_iter().rev().collect();
    assert_eq!(
        project_candle_dots(reversed, grid(), view).marks,
        frame.marks,
        "input order is not a group's identity"
    );
}

#[test]
fn panning_never_regroups_and_an_edge_group_sums_its_hidden_members() {
    // Twelve closed candles after a two-candle venue prefix, so closed ladder
    // `i` is slot `2 + i`, then the forming candle at slot 14.
    let closed: Vec<BarFootprint> = (0..12_u64)
        .map(|index| {
            let price = (100 + 5 * (index % 4)).to_string();
            let quantity = (index + 1).to_string();
            let side = if index % 3 == 0 {
                Side::Sell
            } else {
                Side::Buy
            };
            ladder("5", &[print(index + 1, &price, &quantity, side)])
        })
        .collect();
    let forming = ladder("5", &[print(99, "110", "2", Side::Sell)]);
    let project = |candles_per_mark: usize, visible: (usize, usize)| {
        let view = grouped(candles_per_mark, 2.0, visible);
        let inputs = view.trade_built(&closed, 2, Some((14, &forming)));
        project_candle_dots(inputs, grid(), view)
    };
    assert_eq!(grouped(4, 2.0, (5, 11)).input_slots(), 4..12);
    assert_eq!(grouped(4, 2.0, (8, 12)).input_slots(), 8..12);
    let left = project(4, (5, 11));
    let right = project(4, (7, 15));
    let far_left = project(4, (1, 6));
    let spans = |frame: &CandleDotFrame| {
        frame
            .marks
            .iter()
            .map(|dot| (dot.slot, dot.last_slot))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        spans(&left),
        [(4, 7), (8, 11)],
        "slot 4 is off screen and still in its group"
    );
    assert_eq!(
        spans(&right),
        [(4, 7), (8, 11), (12, 14)],
        "the forming candle joins the group its slot names"
    );
    assert_eq!(
        spans(&far_left),
        [(2, 3), (4, 7)],
        "venue prefix slots own no ladder"
    );
    for (panned, reference) in [(&left, &right), (&far_left, &left)] {
        for mark in &panned.marks {
            let same = reference.marks.iter().find(|other| other.slot == mark.slot);
            if let Some(same) = same {
                assert_eq!(facts(mark), facts(same), "a pan changed a group's sums");
            }
        }
    }
    let per_candle = project(1, (4, 8));
    assert_eq!(per_candle.marks.len(), 4);
    let edge = &left.marks[0];
    assert_eq!(
        (edge.buy_quantity, edge.sell_quantity, edge.trade_count),
        totals(&per_candle),
        "the edge group sums every member, hidden or not"
    );
}

#[test]
fn at_one_candle_per_mark_each_candle_keeps_todays_mark() {
    let candles = [("100", "1"), ("105", "4"), ("110", "2"), ("95", "8")]
        .map(|(price, quantity)| ladder("5", &[print(1, price, quantity, Side::Buy)]));
    // The chart's default zoom, 8 px per bar.
    let view = grouped(1, 8.0, (0, 4));
    let inputs = || {
        candles
            .iter()
            .enumerate()
            .map(|(slot, candle)| factual(slot, candle))
    };
    let frame = project_candle_dots(inputs(), grid(), view);
    assert_eq!(frame.marks.len(), candles.len());
    for (mark, input) in frame.marks.iter().zip(inputs()) {
        let alone = project_candle_dots([input], grid(), view);
        assert_eq!(facts(mark), facts(&alone.marks[0]));
        assert_eq!((mark.slot, mark.last_slot), (input.slot, input.slot));
        assert_eq!(
            mark.radius_px,
            4.0 * normalized_area_size(mark.buy_quantity + mark.sell_quantity, dec("8")),
            "today's rule at the default zoom: half the 8 px column, area by volume"
        );
    }
    assert_eq!(frame.marks[3].radius_px, 4.0);
}

#[test]
fn mark_area_follows_exact_volume_against_the_largest_visible_mark() {
    let candles = ["16", "4", "1", "0.01"]
        .map(|quantity| ladder("5", &[print(1, "100", quantity, Side::Buy)]));
    let inputs = || {
        candles
            .iter()
            .enumerate()
            .map(|(slot, candle)| factual(slot, candle))
    };
    let frame = project_candle_dots(inputs(), grid(), view());
    let radius = |index: usize| frame.marks[index].radius_px;
    assert_eq!(radius(0), CANDLE_MARK_MAX_RADIUS_PX);
    assert_eq!(radius(0).powi(2) / radius(1).powi(2), 4.0);
    assert_eq!(radius(0).powi(2) / radius(2).powi(2), 16.0);
    assert_eq!(
        radius(3),
        MIN_DOT_RADIUS_PX,
        "a speck is lifted to the floor, never hidden, and above it no ratio moves"
    );
    let one_group = project_candle_dots(inputs(), grid(), grouped(4, 2.0, (0, 4)));
    assert_eq!(one_group.marks.len(), 1);
    assert_eq!(
        one_group.marks[0].radius_px, 4.0,
        "four 2 px candles are an 8 px column: radius 4, not one candle's 1"
    );
    let wide = project_candle_dots(inputs(), grid(), grouped(8, 2.0, (0, 4)));
    assert_eq!(wide.marks[0].radius_px, CANDLE_MARK_MAX_RADIUS_PX);
    let pairs =
        ["12", "4", "3", "1"].map(|quantity| ladder("5", &[print(1, "100", quantity, Side::Sell)]));
    let merged = project_candle_dots(
        pairs
            .iter()
            .enumerate()
            .map(|(slot, candle)| factual(slot, candle)),
        grid(),
        grouped(2, 6.0, (0, 4)),
    );
    assert_eq!(
        merged
            .marks
            .iter()
            .map(|dot| dot.sell_quantity)
            .collect::<Vec<_>>(),
        [dec("16"), dec("4")]
    );
    assert_eq!(merged.marks[0].radius_px, CANDLE_MARK_MAX_RADIUS_PX);
    assert_eq!(
        merged.marks[0].radius_px.powi(2) / merged.marks[1].radius_px.powi(2),
        4.0,
        "merged marks keep area proportional to their summed volume"
    );
}

#[test]
fn the_group_ladder_holds_inside_its_band_and_moves_past_it() {
    let least = CANDLE_GROUP_MIN_WIDTH_PX;
    let halve_at = least * CANDLE_GROUP_HOLD_BAND;
    let mut memory = CandleGroupMemory::default();
    assert_eq!(
        memory.choose(8.0),
        1,
        "the chart's default 8 px per bar draws one mark per candle"
    );
    for _ in 0..3 {
        assert_eq!(memory.choose(least), 1, "a steady zoom never flickers");
    }
    assert_eq!(
        memory.choose(least * 0.99),
        2,
        "under the least width, pairs"
    );
    assert_eq!(
        memory.choose(halve_at * 0.99),
        2,
        "widening inside the band keeps the pairs"
    );
    assert_eq!(memory.choose(halve_at), 1, "past the band, one per candle");
    let mut memory = CandleGroupMemory::default();
    assert_eq!(memory.choose(1.0), 8);
    assert_eq!(
        memory.choose(1.8),
        8,
        "an 8-candle group is held while its 4-candle half is inside the band"
    );
    assert_eq!(
        memory.choose(8.0),
        1,
        "the default zoom is one mark per candle whatever came before"
    );
    for width in [7.0, 5.0, 3.1, 2.9, 1.4, 1.0, 1.6, 3.8, 5.9, 7.6] {
        let candles = memory.choose(width);
        assert!(candles.is_power_of_two());
        assert!(
            candles as f32 * width >= least,
            "a held group is never narrower than {least} px: {candles} x {width}"
        );
    }
}
