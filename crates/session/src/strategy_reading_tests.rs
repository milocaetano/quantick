use quantick_engine::Side;
use quantick_strategy::DisarmReason;
use rust_decimal::Decimal;

use crate::strategies::TabStrategies;
use crate::strategies_tests::{DrawingId, instance};

#[test]
fn a_pane_reads_its_instances_in_arming_order_with_their_state() {
    let mut strategies = TabStrategies::default();
    let pane = strategies.pane_mut(7);
    pane.anchors.arm(instance(DrawingId(1)));
    pane.anchors.arm(instance(DrawingId(2)));
    let _ = pane.anchors.instances[1]
        .armed
        .disarm(DisarmReason::MarketChanged);
    let read = strategies.readings(7);
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].drawing, DrawingId(1));
    assert_eq!(read[0].preset, "test");
    assert_eq!(read[0].side, Side::Buy);
    assert_eq!(read[0].quantity, Decimal::ONE);
    assert_eq!(
        (read[0].tp_mult, read[0].sl_mult),
        (Decimal::ONE, Decimal::ONE)
    );
    assert_eq!(read[0].on_break, "ignore");
    assert_eq!(
        (read[0].execution, read[0].rearm, read[0].state),
        ("paper", "one_shot", "armed")
    );
    assert_eq!(read[0].disarm_reason, None);
    assert_eq!(read[1].state, "disarmed");
    assert_eq!(read[1].disarm_reason, Some("market changed"));
    assert!(
        read[1].badge.contains("market changed"),
        "{}",
        read[1].badge
    );
}

/// Instances stacked on one drawing each read their own state, and all
/// read the one badge the chart prints beside it.
#[test]
fn stacked_instances_read_the_drawing_s_one_badge() {
    let mut strategies = TabStrategies::default();
    let pane = strategies.pane_mut(7);
    pane.anchors.arm(instance(DrawingId(1)));
    pane.anchors.arm(instance(DrawingId(1)));
    let _ = pane.anchors.instances[0].armed.disarm(DisarmReason::User);
    let read = strategies.readings(7);
    let states: Vec<_> = read.iter().map(|reading| reading.state).collect();
    assert_eq!(states, ["disarmed", "armed"]);
    let badges: Vec<_> = read.iter().map(|reading| reading.badge.as_str()).collect();
    assert_eq!(badges, ["⚡×2 test", "⚡×2 test"]);
}

#[test]
fn a_pane_that_never_held_an_instance_reads_none() {
    let strategies = TabStrategies::<DrawingId>::default();
    assert!(strategies.readings(7).is_empty());
}
