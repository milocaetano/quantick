use quantick_anchored_studies::{
    AnchoredAverage, AverageInputs, AverageRequest, AvwapBand, ProfileInputs, ProfileRequest,
    RangeProfile,
};
use quantick_engine::{BarSpec, Trade, bar_registry::BarConfiguration};
use quantick_indicators::SourceId;
use rust_decimal::Decimal;
fn trade(id: u64, ms: i64, price: i64, qty: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: ms,
        price: Decimal::from(price),
        quantity: Decimal::from(qty),
        side: quantick_engine::Side::Buy,
    }
}

#[test]
fn refresh_reproduces_the_kernel_over_the_pane_bars() {
    let trades = [
        trade(1, 1_000, 100, 1),
        trade(2, 1_100, 100, 1),
        trade(3, 1_200, 100, 1),
        trade(4, 2_000, 112, 2),
        trade(5, 2_100, 80, 2),
        trade(6, 2_200, 96, 4),
        trade(7, 3_000, 144, 2),
        trade(8, 3_100, 112, 2),
        trade(9, 3_200, 128, 4),
        trade(10, 4_000, 96, 4),
        trade(11, 4_100, 64, 4),
        trade(12, 4_200, 80, 8),
    ];
    let mut builder = BarConfiguration::from(BarSpec::Tick(3)).build();
    let closed: Vec<_> = trades
        .iter()
        .filter_map(|trade| builder.push(trade))
        .collect();
    assert_eq!(closed.len(), 4, "the tape cuts four closed bars");
    let mut owned = None;
    AnchoredAverage::refresh(
        &mut owned,
        AverageRequest {
            anchor_bar: 1.0,
            source: SourceId::Hlc3,
            bands: [
                AvwapBand {
                    on: true,
                    mult: 1.0,
                },
                AvwapBand {
                    on: false,
                    mult: 2.0,
                },
                AvwapBand {
                    on: false,
                    mult: 3.0,
                },
            ],
        },
        &AverageInputs {
            closed: &closed,
            partial: builder.partial(),
            prefix: &[],
            timeline_revision: 12,
        },
    );
    let cache = owned.as_ref().expect("refreshed");
    assert_eq!(cache.output().first_slot, 1);
    assert_eq!(
        cache.output().rows.len(),
        3,
        "anchor bar to the newest closed bar"
    );
    // The golden fixture's hand-computed values (see the indicators
    // crate's avwap_plots.csv): vwap 96 -> 112 -> 96, band1 = ±1σ.
    assert_eq!(cache.output().rows[0][0], 96.0);
    assert_eq!(cache.output().rows[1][0], 112.0);
    assert_eq!(cache.output().rows[2][0], 96.0);
    assert_eq!(cache.output().rows[1][1], 128.0, "+1σ with σ=16");
    assert_eq!(cache.output().rows[1][2], 96.0, "-1σ with σ=16");
    assert!(
        cache.output().rows[0][3].is_nan(),
        "band 2 is off by default"
    );
}

#[test]
fn profile_consumer_advances_real_candles_without_app() {
    let mut builder = BarConfiguration::from(BarSpec::Tick(1)).build();
    let bars: Vec<_> = [trade(1, 1_000, 100, 2), trade(2, 2_000, 104, 3)]
        .iter()
        .filter_map(|trade| builder.push(trade))
        .collect();
    let mut owned = None;
    let inputs = ProfileInputs {
        closed: &[],
        ladders: &[],
        prefix: &bars,
        partial_ladder: None,
        group: Decimal::ONE,
        series_revision: 0,
        partial_version: 0,
        blocked: false,
        side_inferred: false,
        partial_bucket_slot: None,
        budget: 1,
    };
    let request = || ProfileRequest {
        min_bar: 0.0,
        max_bar: 1.0,
        extend_right: false,
        approximate_history: true,
        value_area_pct: 70,
    };
    assert!(RangeProfile::refresh(&mut owned, request(), &inputs));
    let first = owned.as_ref().unwrap();
    assert_eq!(first.output().bars_folded, 1);
    assert_eq!(first.output().bars_approximated, 1);
    assert_eq!(
        first.output().profile.as_ref().unwrap().0.total_volume(),
        Decimal::from(2)
    );
    assert!(!RangeProfile::refresh(&mut owned, request(), &inputs));
    let complete = owned.as_ref().unwrap();
    assert_eq!(complete.output().bars_folded, 2);
    assert_eq!(complete.output().bars_approximated, 2);
    assert_eq!(
        complete.output().profile.as_ref().unwrap().0.total_volume(),
        Decimal::from(5)
    );
}
