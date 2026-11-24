use quantick_anchored_studies::{AnchoredAverage, AverageInputs, AverageRequest, AvwapBand};
use quantick_engine::{Bar, Side, Trade};
use quantick_indicators::SourceId;
use rust_decimal::Decimal;

fn bar(time: i64, price: i64, volume: i64) -> Bar {
    Bar::opened_by(&Trade {
        agg_id: 1,
        timestamp_ms: time,
        price: Decimal::from(price),
        quantity: Decimal::from(volume),
        side: Side::Buy,
    })
}

fn request(anchor_bar: f32) -> AverageRequest {
    AverageRequest {
        anchor_bar,
        source: SourceId::Close,
        bands: [AvwapBand {
            on: false,
            mult: 1.0,
        }; 3],
    }
}

fn inputs<'a>(closed: &'a [Bar], partial: Option<&'a Bar>) -> AverageInputs<'a> {
    AverageInputs {
        closed,
        partial,
        prefix: &[],
        timeline_revision: 1,
    }
}

fn values(cache: &Option<AnchoredAverage>) -> Vec<f64> {
    cache
        .as_ref()
        .unwrap()
        .output()
        .rows
        .iter()
        .map(|row| row[0])
        .collect()
}

#[test]
fn non_finite_anchor_removes_a_previously_valid_result() {
    let closed = [bar(1, 100, 1)];
    for anchor in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut cache = None;
        AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, None));
        assert!(cache.is_some());
        AnchoredAverage::refresh(&mut cache, request(anchor), &inputs(&closed, None));
        assert!(
            cache.is_none(),
            "invalid coordinates must not retain a stale study"
        );
    }
}

#[test]
fn anchor_rounding_changes_the_first_contributing_bar_at_half_a_slot() {
    let closed = [bar(1, 100, 1), bar(2, 200, 1), bar(3, 300, 1)];
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.49), &inputs(&closed, None));
    assert_eq!(values(&cache), [100.0, 150.0, 200.0]);
    AnchoredAverage::refresh(&mut cache, request(0.5), &inputs(&closed, None));
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.first_slot, 1);
    assert_eq!(values(&cache), [200.0, 250.0]);
}

#[test]
fn a_negative_anchor_includes_the_first_bar_without_wrapping() {
    let closed = [bar(1, 100, 1), bar(2, 160, 2)];
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(-100.0), &inputs(&closed, None));
    assert_eq!(cache.as_ref().unwrap().output().first_slot, 0);
    assert_eq!(values(&cache), [100.0, 140.0]);
}

#[test]
fn partial_replacement_uses_the_closed_accumulator_without_double_counting() {
    let closed = [bar(1, 100, 2)];
    let first = bar(2, 160, 1);
    let second = bar(3, 200, 2);
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&first)));
    assert_eq!(values(&cache), [100.0, 120.0]);
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&second)));
    assert_eq!(values(&cache), [100.0, 150.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 1);
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&second)));
    assert_eq!(values(&cache), [100.0, 150.0]);
}

#[test]
fn trade_count_refreshes_a_partial_even_when_its_timestamp_is_unchanged() {
    let closed = [bar(1, 100, 1)];
    let mut partial = bar(2, 200, 1);
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&partial)));
    partial.extend(&Trade {
        agg_id: 2,
        timestamp_ms: 2,
        price: Decimal::from(250),
        quantity: Decimal::ONE,
        side: Side::Sell,
    });
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&partial)));
    assert_eq!(values(&cache), [100.0, 200.0]);
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.partial.unwrap().close_time, 2);
    assert_eq!(output.partial.unwrap().trades, 2);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 1);
}

#[test]
fn removing_a_partial_drops_only_the_preview_row() {
    let closed = [bar(1, 100, 1), bar(2, 200, 1)];
    let partial = bar(3, 300, 1);
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&partial)));
    assert_eq!(values(&cache), [100.0, 150.0, 200.0]);
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, None));
    assert_eq!(values(&cache), [100.0, 150.0]);
    assert!(cache.as_ref().unwrap().output().partial.is_none());
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 2);
}

#[test]
fn a_partial_can_return_after_a_gap_without_changing_committed_history() {
    let closed = [bar(1, 100, 1)];
    let partial = bar(2, 200, 1);
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&partial)));
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, None));
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&partial)));
    assert_eq!(values(&cache), [100.0, 150.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 1);
}

#[test]
fn closing_multiple_bars_replaces_the_preview_then_appends_each_close_once() {
    let closed = [bar(1, 100, 1), bar(2, 200, 1), bar(3, 300, 1)];
    let next = bar(4, 400, 1);
    let mut cache = None;
    AnchoredAverage::refresh(
        &mut cache,
        request(0.0),
        &inputs(&closed[..1], Some(&closed[1])),
    );
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, Some(&next)));
    assert_eq!(values(&cache), [100.0, 150.0, 200.0, 250.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 3);
    assert_eq!(cache.as_ref().unwrap().output().key.closed_len, 3);
}

#[test]
fn appending_after_an_anchor_inside_history_does_not_revisit_excluded_bars() {
    let closed = [
        bar(1, 900, 10),
        bar(2, 100, 1),
        bar(3, 200, 1),
        bar(4, 300, 1),
    ];
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(1.0), &inputs(&closed[..2], None));
    AnchoredAverage::refresh(&mut cache, request(1.0), &inputs(&closed, None));
    assert_eq!(values(&cache), [100.0, 150.0, 200.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 3);
    assert_eq!(cache.as_ref().unwrap().output().first_slot, 1);
}

#[test]
fn anchoring_on_a_partial_commits_it_only_when_it_closes() {
    let closed = [bar(1, 900, 10), bar(2, 100, 1)];
    let next = bar(3, 200, 1);
    let mut cache = None;
    AnchoredAverage::refresh(
        &mut cache,
        request(1.0),
        &inputs(&closed[..1], Some(&closed[1])),
    );
    assert_eq!(values(&cache), [100.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 0);
    AnchoredAverage::refresh(&mut cache, request(1.0), &inputs(&closed, Some(&next)));
    assert_eq!(values(&cache), [100.0, 150.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 1);
}

#[test]
fn a_revision_change_replaces_corrected_closed_values_even_at_equal_length() {
    let original = [bar(1, 100, 1), bar(2, 200, 1)];
    let corrected = [bar(1, 200, 1), bar(2, 400, 1)];
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&original, None));
    let corrected_inputs = AverageInputs {
        timeline_revision: 2,
        ..inputs(&corrected, None)
    };
    AnchoredAverage::refresh(&mut cache, request(0.0), &corrected_inputs);
    assert_eq!(values(&cache), [200.0, 300.0]);
    assert_eq!(cache.as_ref().unwrap().output().key.timeline_revision, 2);
}

#[test]
fn shrinking_history_discards_committed_rows_beyond_the_new_end() {
    let closed = [bar(1, 100, 1), bar(2, 200, 1), bar(3, 300, 1)];
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, None));
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed[..1], None));
    assert_eq!(values(&cache), [100.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 1);
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed[..2], None));
    assert_eq!(values(&cache), [100.0, 150.0]);
}

#[test]
fn backfilling_the_prefix_rebuilds_the_slot_to_bar_mapping() {
    let prefix = [bar(1, 100, 1), bar(2, 200, 1)];
    let closed = [bar(3, 300, 1)];
    let mut cache = None;
    let before = AverageInputs {
        prefix: &prefix[1..],
        ..inputs(&closed, None)
    };
    AnchoredAverage::refresh(&mut cache, request(1.0), &before);
    assert_eq!(values(&cache), [300.0]);
    let after = AverageInputs {
        prefix: &prefix,
        ..inputs(&closed, None)
    };
    AnchoredAverage::refresh(&mut cache, request(1.0), &after);
    assert_eq!(values(&cache), [200.0, 250.0]);
    assert_eq!(cache.as_ref().unwrap().output().key.prefix_len, 2);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 2);
}

#[test]
fn a_source_edit_changes_values_without_changing_the_anchor() {
    let mut candle = bar(1, 100, 1);
    candle.high = Decimal::from(160);
    candle.low = Decimal::from(80);
    candle.close = Decimal::from(120);
    let closed = [candle];
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, None));
    assert_eq!(values(&cache), [120.0]);
    let open_request = AverageRequest {
        source: SourceId::Open,
        ..request(0.0)
    };
    AnchoredAverage::refresh(&mut cache, open_request, &inputs(&closed, None));
    assert_eq!(values(&cache), [100.0]);
    assert_eq!(cache.as_ref().unwrap().output().key.source, SourceId::Open);
}

#[test]
fn enabling_and_disabling_bands_rebuilds_only_the_requested_plot_values() {
    let closed = [bar(1, 100, 1), bar(2, 200, 1)];
    let mut cache = None;
    let mut config = request(0.0);
    config.bands[1] = AvwapBand {
        on: true,
        mult: 2.0,
    };
    AnchoredAverage::refresh(&mut cache, config, &inputs(&closed, None));
    let output = cache.as_ref().unwrap().output();
    assert_eq!(output.rows[1][0], 150.0);
    assert_eq!(output.rows[1][3], 250.0);
    assert_eq!(output.rows[1][4], 50.0);
    assert!(output.rows[1][1].is_nan());
    assert!(output.rows[1][5].is_nan());
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&closed, None));
    assert_eq!(values(&cache), [100.0, 150.0]);
    assert!(
        cache.as_ref().unwrap().output().rows[1][1..]
            .iter()
            .all(|v| v.is_nan())
    );
}

#[test]
fn a_partial_only_series_does_not_commit_preview_volume() {
    let first = bar(1, 100, 1);
    let second = bar(2, 200, 2);
    let mut cache = None;
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&[], Some(&first)));
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&[], Some(&second)));
    assert_eq!(values(&cache), [200.0]);
    assert_eq!(cache.as_ref().unwrap().committed_rows(), 0);
    AnchoredAverage::refresh(&mut cache, request(0.0), &inputs(&[], None));
    assert!(cache.is_none());
}
