//! Public projection and bounded wire pages retain full execution facts.
use super::FlowExecutionSnapshot;
use quantick_engine::{Side, Trade};
use quantick_orderflow::projection::{
    PriceWindow,
    flow_tape::{
        FlowExecution, FlowProgress, FlowReference, FlowTapeFrame, FlowTapeView, project_flow_tape,
    },
};
use rust_decimal::Decimal;
use serde_json::Value;

fn source_trade(ordinal: usize, price: i64, quantity: i64) -> Trade {
    Trade {
        // Restarted IDs are intentional. Retained ordinal is the identity.
        agg_id: 7 + u64::try_from(ordinal % 3).unwrap(),
        timestamp_ms: 1_000 + i64::try_from(ordinal).unwrap() * 100,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side: if ordinal.is_multiple_of(2) {
            Side::Buy
        } else {
            Side::Sell
        },
    }
}

fn projected(trades: &[Trade], ticks: usize, width: f32, height: f32) -> FlowTapeFrame {
    let slots = trades.len().div_ceil(ticks);
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / ticks,
                accepted_ordinal: ordinal % ticks,
                ticks_per_bar: Decimal::from(ticks),
                trade,
                opening: false,
            }),
        42,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: slots,
            clip_left: Decimal::ZERO,
            clip_right: Decimal::from(slots),
            width_px: width,
            height_px: height,
            prices: PriceWindow::new(90.into(), 130.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 7.0,
            merge_support_radius: 12.0,
            exclude_opening: false,
        },
    )
}

fn settled(frame: &FlowTapeFrame) -> FlowProgress {
    FlowProgress {
        pending: false,
        submitted_executions: frame.requested_ordinals.len(),
        requested_executions: frame.requested_ordinals.len(),
        layout_revision: frame.layout_revision,
        current_source_count: frame.source_count,
        requested_first_ordinal: frame.requested_ordinals.start,
        requested_end_ordinal: frame.requested_ordinals.end,
    }
}

fn wire(frame: &FlowTapeFrame, progress: FlowProgress) -> Value {
    let snapshot = FlowExecutionSnapshot::from((frame, progress));
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(
        serde_json::from_value::<FlowExecutionSnapshot>(value.clone()).unwrap(),
        snapshot,
        "bounded wire output must deserialize without losing facts or flags"
    );
    value
}

#[test]
fn paired_radii_read_back_each_actual_side_and_the_equivalent_gross_area() {
    for (buy, sell) in [(51, 49), (100, 0), (0, 100)] {
        let trades = [source_trade(0, 100, buy), source_trade(1, 100, sell)];
        let mut frame = projected(&trades, 2, 1.0, 1.0);
        frame.dots[0].mark.buy_share = 0.0;
        let value = wire(&frame, settled(&frame));
        let mark = &value["marks"][0];
        let radius = |field: &str| mark[field].as_str().unwrap().parse::<f64>().unwrap();
        assert_eq!(radius("radius_px"), 7.0);
        assert!((radius("buy_radius_px").powi(2) - 49.0 * buy as f64 / 100.0).abs() < 0.00001);
        assert!((radius("sell_radius_px").powi(2) - 49.0 * sell as f64 / 100.0).abs() < 0.00001);
        assert_eq!(radius("buy_radius_px") == 0.0, buy == 0);
        assert_eq!(radius("sell_radius_px") == 0.0, sell == 0);
    }
}

#[test]
fn mark_page_boundary_keeps_full_frame_totals_and_reports_only_omitted_rows() {
    for (count, buy, sell, reference) in [(256, "128", "128", "1"), (257, "135", "128", "7")] {
        let trades = (0..count)
            .map(|n| {
                // The omitted 257th mark carries seven buys at another price.
                source_trade(
                    n,
                    if n == 256 { 120 } else { 100 },
                    if n == 256 { 7 } else { 1 },
                )
            })
            .collect::<Vec<_>>();
        // Each slot is hundreds of screen units apart, so this fixture has
        // independently separated marks rather than a copied merge algorithm.
        let frame = projected(&trades, 1, 100_000.0, 1_000.0);
        assert_eq!(
            frame.dots.len(),
            count,
            "fixture must contain separate marks"
        );
        let value = wire(&frame, settled(&frame));
        let marks = value["marks"].as_array().unwrap();

        assert_eq!(marks.len(), 256);
        assert_eq!(value["mark_count"], count.to_string());
        assert_eq!(value["marks_truncated"], count == 257);
        assert_eq!(value["buy_quantity"], buy);
        assert_eq!(value["sell_quantity"], sell);
        assert_eq!(value["trade_count"], count.to_string());
        assert_eq!(value["effective_reference"], reference);
        assert_eq!(
            value["omitted_executions"], "0",
            "readback row truncation is not missing domain source coverage"
        );
        assert_eq!(value["worker"]["loaded_executions"], count.to_string());
        assert_eq!(
            value["worker"]["computed_through_ordinal"],
            count.to_string()
        );

        for (ordinal, mark) in marks.iter().enumerate() {
            let members = mark["members"].as_array().unwrap();
            assert_eq!(members.len(), 1);
            assert_eq!(mark["members_truncated"], false);
            assert_eq!(members[0]["ordinal"], ordinal.to_string());
            assert_eq!(members[0]["source_id"], (7 + ordinal % 3).to_string());
            assert_eq!(members[0]["candle_slot"], ordinal.to_string());
            assert_eq!(members[0]["accepted_ordinal"], "0");
        }
        assert_eq!(marks[255]["first_slot"], "255");
        assert_eq!(marks[255]["end_slot"], "256");
        assert_eq!(marks[255]["first_timestamp_ms"], 26_500);
        assert_eq!(marks[255]["last_timestamp_ms"], 26_500);
    }
}

fn merged_frame(count: usize) -> FlowTapeFrame {
    let trades = (0..count)
        .map(|n| {
            // Only the 129th member extends both price and candle span and adds
            // two buys. A DTO that recomputes from its first 128 members loses it.
            source_trade(
                n,
                if n == 128 { 120 } else { 100 },
                if n == 128 { 2 } else { 1 },
            )
        })
        .collect::<Vec<_>>();
    // All native cells lie in a 1x1 screen-unit box under a 12-unit support.
    // The public projection must therefore produce the one marked region.
    let frame = projected(&trades, 64, 1.0, 1.0);
    assert_eq!(
        frame.dots.len(),
        1,
        "fixture must contain one merged region"
    );
    frame
}

#[test]
fn member_page_boundary_keeps_unlisted_member_volume_and_actual_spans() {
    for (count, buy, end_slot, high_price, last_time) in [
        (128, "64", "2", "100", 13_700),
        (129, "66", "3", "120", 13_800),
    ] {
        let frame = merged_frame(count);
        let value = wire(&frame, settled(&frame));
        let mark = &value["marks"][0];
        let members = mark["members"].as_array().unwrap();

        assert_eq!(value["mark_count"], "1");
        assert_eq!(value["marks_truncated"], false);
        assert_eq!(members.len(), 128);
        assert_eq!(mark["members_truncated"], count == 129);
        assert_eq!(value["buy_quantity"], buy);
        assert_eq!(value["sell_quantity"], "64");
        assert_eq!(value["trade_count"], count.to_string());
        assert_eq!(mark["buy_quantity"], buy);
        assert_eq!(mark["sell_quantity"], "64");
        assert_eq!(mark["trade_count"], count.to_string());
        assert_eq!(mark["native_cells"], count.to_string());
        assert_eq!(mark["first_slot"], "0");
        assert_eq!(mark["end_slot"], end_slot);
        assert_eq!(mark["price_low"], "100");
        assert_eq!(mark["price_high"], high_price);
        assert_eq!(mark["first_timestamp_ms"], 1_000);
        assert_eq!(mark["last_timestamp_ms"], last_time);

        for (ordinal, member) in members.iter().enumerate() {
            assert_eq!(member["ordinal"], ordinal.to_string());
            assert_eq!(member["source_id"], (7 + ordinal % 3).to_string());
            assert_eq!(member["candle_slot"], (ordinal / 64).to_string());
            assert_eq!(member["accepted_ordinal"], (ordinal % 64).to_string());
        }
        assert_eq!(members[63]["candle_slot"], "0");
        assert_eq!(members[63]["accepted_ordinal"], "63");
        assert_eq!(members[64]["candle_slot"], "1");
        assert_eq!(members[64]["accepted_ordinal"], "0");
    }
}

#[test]
fn pending_new_request_does_not_relabel_the_completed_painted_frame() {
    let frame = merged_frame(129);
    let progress = FlowProgress {
        pending: true,
        submitted_executions: 64,
        requested_executions: 120,
        layout_revision: 7,
        current_source_count: 256,
        requested_first_ordinal: 80,
        requested_end_ordinal: 200,
    };
    let value = wire(&frame, progress);
    let worker = &value["worker"];

    // Old completed frame: original 129-record projection, including member 129
    // whose identity is outside the 128-member wire page.
    assert_eq!(value["source_revision"], "42");
    assert_eq!(value["trade_count"], "129");
    assert_eq!(value["buy_quantity"], "66");
    assert_eq!(value["sell_quantity"], "64");
    assert_eq!(value["first_slot"], "0");
    assert_eq!(value["end_slot"], "3");
    assert_eq!(value["clip_left"], "0");
    assert_eq!(value["clip_right"], "3");
    assert_eq!(value["omitted_executions"], "0");
    assert_eq!(worker["layout_revision"], "0");
    assert_eq!(worker["projected_source_count"], "129");
    assert_eq!(worker["requested_first_ordinal"], "0");
    assert_eq!(worker["requested_end_ordinal"], "129");
    assert_eq!(worker["loaded_executions"], "129");
    assert_eq!(worker["computed_through_ordinal"], "129");

    // New request/current source: must coexist without masquerading as painted.
    assert_eq!(value["retained_source_count"], "256");
    assert_eq!(worker["pending"], true);
    assert_eq!(worker["requested_layout_revision"], "7");
    assert_eq!(worker["current_requested_first_ordinal"], "80");
    assert_eq!(worker["current_requested_end_ordinal"], "200");
    assert_eq!(worker["requested_executions"], "120");
    assert_eq!(worker["submitted_executions"], "64");

    // When the caller reports the old request settled, facts stay the same and
    // only truthful progress/current metadata changes.
    let complete = wire(&frame, settled(&frame));
    assert_eq!(complete["worker"]["pending"], false);
    assert_eq!(complete["retained_source_count"], "129");
    assert_eq!(complete["marks"], value["marks"]);
    assert_eq!(complete["buy_quantity"], value["buy_quantity"]);
    assert_eq!(complete["sell_quantity"], value["sell_quantity"]);
}
