//! Price distribution, exact conservation and stable axis detail.
use super::*;
use crate::projection::{
    CANDLE_MARK_MAX_RADIUS_PX, CandleDotFrame, CandleDotGrid, CandleDotView, CandleFootprint,
    CandleFootprintSource, CandleGroupMemory, CandlePriceMemory, CandleScaleMemory,
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
fn ladder(prints: &[Trade]) -> BarFootprint {
    ladder_on("5", prints)
}
fn ladder_on(step: &str, prints: &[Trade]) -> BarFootprint {
    let mut builder = FootprintBuilder::new(dec(step), 2_000);
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
        candle_width_px: 16.0,
        visible: (0, 64),
        candles_per_mark: 1,
        price_ticks_per_mark: 1,
        height_px: 120.0,
    }
}
fn totals(frame: &CandleDotFrame) -> (Decimal, Decimal, u64) {
    frame.marks.iter().fold(
        (Decimal::ZERO, Decimal::ZERO, 0),
        |(buy, sell, count), mark| {
            (
                buy + mark.buy_quantity,
                sell + mark.sell_quantity,
                count + mark.trade_count,
            )
        },
    )
}

#[test]
fn equal_total_candles_retain_opposite_price_distributions_and_sides() {
    // The execution sides are independent of candle direction.
    let low_strong = ladder(&[
        print(1, "100", "400", Side::Sell),
        print(2, "115", "100", Side::Buy),
    ]);
    let high_strong = ladder(&[
        print(3, "100", "100", Side::Buy),
        print(4, "115", "400", Side::Sell),
    ]);
    let frame = project_candle_dots(
        [factual(0, &low_strong), factual(1, &high_strong)],
        grid(),
        view(),
    );
    assert_eq!(frame.marks.len(), 4);
    assert_eq!(totals(&frame), (dec("200"), dec("800"), 4));
    let facts: Vec<_> = frame
        .marks
        .iter()
        .map(|m| (m.slot, m.price, m.buy_quantity, m.sell_quantity))
        .collect();
    assert_eq!(
        facts,
        [
            (0, dec("100"), dec("0"), dec("400")),
            (0, dec("115"), dec("100"), dec("0")),
            (1, dec("100"), dec("100"), dec("0")),
            (1, dec("115"), dec("0"), dec("400"))
        ]
    );
    for (strong, weak) in [(0, 1), (3, 2)] {
        assert_eq!(
            frame.marks[strong].radius_px,
            2.0 * frame.marks[weak].radius_px
        );
    }
    let reversed = project_candle_dots(
        [factual(1, &high_strong), factual(0, &low_strong)],
        grid(),
        view(),
    );
    assert_eq!(frame, reversed, "input order cannot change identity");
}

#[test]
fn common_scale_keeps_four_to_one_and_subpixel_area_ratios_without_floors() {
    let members = ["400", "100", "0.01"].map(|q| ladder(&[print(1, "100", q, Side::Buy)]));
    let frame = project_candle_dots(
        members.iter().enumerate().map(|(s, l)| factual(s, l)),
        grid(),
        view(),
    );
    assert_eq!(frame.full_quantity, dec("400"));
    assert_eq!(frame.marks[0].radius_px, CANDLE_MARK_MAX_RADIUS_PX);
    assert_eq!(
        frame.marks[0].radius_px.powi(2) / frame.marks[1].radius_px.powi(2),
        4.0
    );
    assert!(frame.marks[2].radius_px < 0.1);
    assert!(
        (frame.marks[2].radius_px.powi(2) / frame.marks[0].radius_px.powi(2) - 0.000025).abs()
            < 1e-10
    );
    assert_eq!(totals(&frame), (dec("500.01"), dec("0"), 3));
}

#[test]
fn horizontal_and_vertical_zoom_conserve_exact_sides_counts_and_price_spans() {
    let members = (0..8)
        .map(|id| {
            ladder(&[
                print(id * 2, "100", "0.1", Side::Buy),
                print(id * 2 + 1, "105", "0.2", Side::Sell),
            ])
        })
        .collect::<Vec<_>>();
    for candles in [1, 2, 4, 8] {
        for ticks in [1, 2, 4, 8] {
            let frame = project_candle_dots(
                members.iter().enumerate().map(|(s, l)| factual(s, l)),
                grid(),
                CandleDotView {
                    candles_per_mark: candles,
                    price_ticks_per_mark: ticks,
                    ..view()
                },
            );
            assert_eq!(totals(&frame), (dec("0.8"), dec("1.6"), 16));
            for mark in &frame.marks {
                assert_eq!(mark.last_slot - mark.slot + 1, candles);
                assert!(mark.price >= mark.price_low && mark.price <= mark.price_high);
                if ticks > 1 {
                    assert_eq!((mark.price_low, mark.price_high), (dec("100"), dec("105")));
                    assert_eq!(
                        mark.price,
                        (dec("100") * dec("0.1") + dec("105") * dec("0.2")) / dec("0.3")
                    );
                }
            }
        }
    }
}

#[test]
fn panning_owns_whole_absolute_groups_and_never_inflates_retained_marks() {
    let members = (0..12)
        .map(|id| {
            ladder(&[print(
                id,
                "100",
                if id < 4 { "400" } else { "100" },
                Side::Buy,
            )])
        })
        .collect::<Vec<_>>();
    let project = |visible| {
        let view = CandleDotView {
            visible,
            candles_per_mark: 4,
            candle_width_px: 2.0,
            ..view()
        };
        project_candle_dots(view.trade_built(&members, 2, None), grid(), view)
    };
    let mut memory = CandleScaleMemory::default();
    let mut left = project((3, 11));
    memory.apply(&mut left);
    let mut right = project((8, 13));
    memory.apply(&mut right);
    assert_eq!(right.full_quantity, left.full_quantity);
    assert_eq!(left.marks[0].slot, 2, "prefix slots have no ladder");
    assert_eq!((left.marks[1].slot, left.marks[1].last_slot), (4, 7));
    let shared = left.marks.iter().find(|m| m.slot == 8).unwrap();
    assert_eq!(
        &right.marks[0], shared,
        "panning retains sums, span, price and radius"
    );
    assert_eq!(totals(&left), (dec("2200"), dec("0"), 10));
    let mut changed_tier = project_candle_dots([factual(8, &members[8])], grid(), view());
    memory.apply(&mut changed_tier);
    assert_eq!(
        changed_tier.full_quantity,
        dec("100"),
        "a new tier starts its own reference"
    );
    let mut returned = project((8, 13));
    memory.apply(&mut returned);
    assert_eq!(
        returned.full_quantity, left.full_quantity,
        "returning to a tier retains its previously observed reference"
    );
}

#[test]
fn a_live_partial_updates_each_price_band_without_a_bar_close() {
    let mut builder = FootprintBuilder::new(dec("5"), 2000);
    builder.push(&print(1, "100", "9", Side::Buy));
    let before = project_candle_dots([factual(18, builder.partial().unwrap())], grid(), view());
    builder.push(&print(2, "105", "16", Side::Sell));
    let after = project_candle_dots([factual(18, builder.partial().unwrap())], grid(), view());
    assert_eq!(after.marks.len(), 2);
    assert_eq!(after.marks[0].buy_quantity, before.marks[0].buy_quantity);
    assert_eq!(after.marks[1].price, dec("105"));
    assert_eq!(totals(&after), (dec("9"), dec("16"), 2));
}

#[test]
fn price_visibility_never_pulls_outside_execution_rows_into_a_candle_centroid() {
    let crossing = ladder(&[
        print(1, "80", "1", Side::Buy),
        print(2, "130", "1", Side::Sell),
    ]);
    assert!(
        project_candle_dots([factual(0, &crossing)], grid(), view())
            .marks
            .is_empty()
    );
}

#[test]
fn price_bands_use_floor_for_negative_prices_and_aligned_offset_grids() {
    let native = ladder_on(
        "1",
        &[
            print(1, "-3", "1", Side::Buy),
            print(2, "-2", "1", Side::Sell),
            print(3, "1", "1", Side::Buy),
        ],
    );
    let grid = Some(CandleDotGrid {
        step: dec("1"),
        reference_price: dec("1"),
    });
    let view = CandleDotView {
        prices: PriceWindow::new(dec("-10"), dec("10")).unwrap(),
        price_ticks_per_mark: 4,
        ..view()
    };
    let frame = project_candle_dots([factual(0, &native)], grid, view);
    assert_eq!(frame.marks.len(), 2);
    assert_eq!(
        (frame.marks[0].price_low, frame.marks[0].price_high),
        (dec("-3"), dec("-2"))
    );
    assert_eq!(frame.marks[1].price, dec("1"));
}

#[test]
fn coarse_capped_approximate_and_incompatible_capture_grids_do_not_invent_prices() {
    let prints = [
        print(1, "100", "1", Side::Buy),
        print(2, "105", "3", Side::Sell),
    ];
    let native = ladder(&prints);
    let fine = ladder_on("1", &prints);
    assert_eq!(
        project_candle_dots([factual(0, &native)], grid(), view()),
        project_candle_dots([factual(0, &fine)], grid(), view())
    );
    for bad in [ladder_on("10", &prints), ladder_on("2", &prints)] {
        assert!(
            project_candle_dots([factual(0, &bad)], grid(), view())
                .marks
                .is_empty()
        );
    }
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
    let guessed = CandleFootprint {
        source: CandleFootprintSource::Approximate,
        ..factual(0, &native)
    };
    assert!(
        project_candle_dots([guessed], grid(), view())
            .marks
            .is_empty()
    );
    assert!(
        project_candle_dots([factual(0, &native)], None, view())
            .marks
            .is_empty()
    );
    let offset = Some(CandleDotGrid {
        step: dec("5"),
        reference_price: dec("102"),
    });
    assert!(
        project_candle_dots([factual(0, &native)], offset, view())
            .marks
            .is_empty()
    );
}

#[test]
fn independent_horizontal_and_vertical_ladders_hold_their_dead_bands() {
    let mut candles = CandleGroupMemory::default();
    let mut prices = CandlePriceMemory::default();
    assert_eq!(candles.choose(8.0), 1);
    assert_eq!(prices.choose(8.0), 1);
    for _ in 0..3 {
        assert_eq!(candles.choose(5.9), 2);
        assert_eq!(prices.choose(5.9), 2);
        assert_eq!(candles.choose(7.4), 2);
        assert_eq!(prices.choose(7.4), 2);
    }
    assert_eq!(candles.choose(7.5), 1);
    assert_eq!(prices.choose(7.5), 1);
    assert_eq!(prices.choose(0.1), 64);
    assert_eq!(candles.choose(8.0), 1);
    assert_eq!(candles.choose(1.0), 8);
    assert_eq!(prices.choose(8.0), 1);
}

#[test]
fn invalid_geometry_is_empty_and_compact_cap_is_shared_by_every_mark() {
    let native = ladder(&[
        print(1, "100", "400", Side::Buy),
        print(2, "105", "100", Side::Sell),
    ]);
    for bad in [
        CandleDotView {
            height_px: f32::NAN,
            ..view()
        },
        CandleDotView {
            height_px: 0.0,
            ..view()
        },
        CandleDotView {
            price_ticks_per_mark: 0,
            ..view()
        },
        CandleDotView {
            candle_width_px: f32::INFINITY,
            ..view()
        },
        CandleDotView {
            candles_per_mark: 0,
            ..view()
        },
    ] {
        assert!(
            project_candle_dots([factual(0, &native)], grid(), bad)
                .marks
                .is_empty()
        );
    }
    let frame = project_candle_dots(
        [factual(0, &native)],
        grid(),
        CandleDotView {
            height_px: 12.0,
            ..view()
        },
    );
    assert_eq!(frame.maximum_radius_px, 1.0);
    assert_eq!(frame.marks[0].radius_px, 2.0 * frame.marks[1].radius_px);
}
