//! The tab's armed strategies, judged without a pane: drawings are plain
//! ids, their existence and regions closures.

use quantick_engine::{Bar, Side};
use quantick_sim::{Bracket, Command, EntryKind, Order, OrderId, OrderRole, VenueEvent};
use quantick_strategy::anchors::{AlarmMark, AnchoredInstance};
use quantick_strategy::presets::StoredPreset;
use quantick_strategy::runner::StrategyPort;
use quantick_strategy::sound::Cue;
use quantick_strategy::{
    ArmedStrategy, BreakPolicy, DisarmReason, Execution, ForceParams, ForceTrigger, Rearm, Region,
    StrategyParams,
};
use rust_decimal::Decimal;

use crate::strategies::{Print, TabStrategies};

/// The window anchors to its drawing id; any copyable key will do here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawingId(pub(crate) u64);

const PANE: u64 = 7;

/// An account that records every command it is handed and is always flat.
#[derive(Default)]
struct RecordingPort {
    applied: Vec<Command>,
}

impl StrategyPort for RecordingPort {
    fn apply(&mut self, command: Command) -> Vec<VenueEvent> {
        self.applied.push(command);
        Vec::new()
    }

    fn is_flat(&self) -> bool {
        true
    }
}

pub(crate) fn bar(open: i64, close: i64) -> Bar {
    Bar {
        open_time: 0,
        close_time: 0,
        open: Decimal::from(open),
        high: Decimal::from(open.max(close)) + Decimal::ONE,
        low: Decimal::from(open.min(close)) - Decimal::ONE,
        close: Decimal::from(close),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

pub(crate) fn instance(drawing: DrawingId) -> AnchoredInstance<DrawingId> {
    AnchoredInstance {
        drawing,
        preset: "test".to_owned(),
        spec: StoredPreset::starting_point(Side::Buy),
        armed: ArmedStrategy::new(
            StrategyParams {
                side: Side::Buy,
                quantity: Decimal::ONE,
                tp_mult: Decimal::ONE,
                sl_mult: Decimal::ONE,
                rearm: Rearm::OneShot,
                on_break: BreakPolicy::Ignore,
                execution: Execution::Paper,
            },
            Box::new(ForceTrigger::new(ForceParams {
                window: 3,
                min_factor: "1.5".parse().expect("fixture"),
                max_factor: "2.5".parse().expect("fixture"),
                min_range: Decimal::ZERO,
            })),
        ),
        alarm: None,
        cue: Cue::default(),
        mark: AlarmMark::Quiet,
    }
}

/// An instance whose 3-bar ruler fired on its third bar and whose entry,
/// order 11, is resting.
fn resting(drawing: DrawingId) -> AnchoredInstance<DrawingId> {
    let mut riding = instance(drawing);
    let region = Region::new(Decimal::from(100), Decimal::from(110));
    let _ = riding
        .armed
        .on_closed_bar(&bar(100, 101), &region, true, true);
    let _ = riding
        .armed
        .on_closed_bar(&bar(101, 102), &region, true, true);
    let commands = riding
        .armed
        .on_closed_bar(&bar(102, 106), &region, true, true);
    assert_eq!(commands.len(), 1, "the fixture fires: {commands:?}");
    let _ = riding.armed.on_sim_events(&[VenueEvent::Placed(Order {
        id: OrderId(11),
        side: Side::Buy,
        kind: EntryKind::Market,
        price: None,
        quantity: Decimal::ONE,
        bracket: Bracket::none(),
        cancel_at: None,
        flat_only: false,
        placed_ms: 0,
        role: OrderRole::Entry,
        oco: None,
        reduce_only: false,
    })]);
    riding
}

#[test]
fn orphaned_instance_returns_its_cleanup() {
    let mut strategies = TabStrategies::default();
    strategies.pane_mut(PANE).anchors.arm(resting(DrawingId(3)));
    strategies.queue_closed(PANE, &bar(106, 107), 4);
    let mut port = RecordingPort::default();
    let answer = strategies.evaluate(
        &[PANE],
        &mut port,
        Print {
            events: &[],
            now_ms: 0,
        },
        |_, _| false,
        |_, _, _| None,
        |_| None,
    );
    assert_eq!(
        port.applied,
        vec![Command::CancelOrder { id: OrderId(11) }],
        "the dead drawing's resting entry is cancelled through the port"
    );
    assert!(strategies.anchors(PANE).expect("the pane").is_empty());
    assert_eq!(answer.watching, 0);
}

#[test]
fn an_idle_pane_queues_nothing_and_a_disarm_forgets_the_queue() {
    let mut strategies: TabStrategies<DrawingId> = TabStrategies::default();
    strategies.queue_closed(PANE, &bar(1, 2), 0);
    assert!(
        strategies.pane(PANE).is_none(),
        "the per-trade path opens no entry"
    );
    let pane = strategies.pane_mut(PANE);
    pane.queue_closed(&bar(1, 2), 0);
    assert!(pane.pending().is_empty(), "nothing armed, nothing cloned");
    pane.anchors.arm(instance(DrawingId(1)));
    pane.queue_closed(&bar(1, 2), 5);
    assert_eq!(pane.pending().len(), 1);
    let cleanup = pane.disarm_all(DisarmReason::TimelineReset);
    assert!(cleanup.is_empty(), "nothing was resting");
    assert!(pane.take_bars().is_empty(), "the bars of a dead series go");
}

#[test]
fn removal_and_disarm_queue_their_cancels_for_the_drain_in_pane_order() {
    let mut strategies = TabStrategies::default();
    strategies.pane_mut(9).anchors.arm(resting(DrawingId(1)));
    strategies.pane_mut(PANE).anchors.arm(resting(DrawingId(2)));
    strategies.pane_mut(9).remove_for_drawing(DrawingId(1));
    strategies.pane_mut(PANE).disarm(DrawingId(2));
    assert_eq!(
        strategies.take_cleanup(&[9, PANE]),
        vec![
            Command::CancelOrder { id: OrderId(11) },
            Command::CancelOrder { id: OrderId(11) },
        ]
    );
    assert!(
        strategies.take_cleanup(&[9, PANE]).is_empty(),
        "drained once"
    );
}

/// The drawing's Disarm and Re-arm speak for every instance riding it;
/// another drawing's bot is left as it is.
#[test]
fn disarm_and_rearm_reach_every_instance_on_the_drawing() {
    let mut strategies = TabStrategies::default();
    let pane = strategies.pane_mut(PANE);
    pane.anchors.arm(resting(DrawingId(1)));
    pane.anchors.arm(instance(DrawingId(1)));
    pane.anchors.arm(instance(DrawingId(2)));
    pane.disarm(DrawingId(1));
    let watching: Vec<_> = (pane.anchors.instances.iter())
        .map(AnchoredInstance::watching)
        .collect();
    assert_eq!(watching, [false, false, true], "only drawing 1's bots stop");
    assert_eq!(
        strategies.take_cleanup(&[PANE]),
        vec![Command::CancelOrder { id: OrderId(11) }]
    );

    let pane = strategies.pane_mut(PANE);
    let _ = pane.anchors.instances[2].armed.disarm(DisarmReason::User);
    pane.rearm(DrawingId(1), |_| Vec::new());
    let watching: Vec<_> = (pane.anchors.instances.iter())
        .map(AnchoredInstance::watching)
        .collect();
    assert_eq!(
        watching,
        [true, true, false],
        "both re-armed, not the other"
    );
}

#[test]
fn a_refused_check_arms_nothing_and_a_passed_one_warms_on_the_series() {
    let mut strategies: TabStrategies<DrawingId> = TabStrategies::default();
    let pane = strategies.pane_mut(PANE);
    let form = StoredPreset::starting_point(Side::Buy);
    let refused = pane.arm(
        DrawingId(1),
        &form,
        "BF".to_owned(),
        || Err("the drawing is gone".to_owned()),
        |_| unreachable!("a refused arm warms nothing"),
    );
    assert_eq!(refused.err().as_deref(), Some("the drawing is gone"));
    assert!(pane.anchors.is_empty());
    let mut depth = None;
    let armed = pane.arm(
        DrawingId(1),
        &form,
        "BF".to_owned(),
        || Ok(()),
        |asked| {
            depth = Some(asked);
            Vec::new()
        },
    );
    assert!(armed.is_ok(), "{armed:?}");
    assert!(depth.is_some(), "the ruler asked for its warmup");
    assert_eq!(pane.anchors.watching(), 1);
}

#[test]
fn a_sweep_walks_only_the_panes_it_is_given() {
    let mut strategies = TabStrategies::default();
    strategies.pane_mut(PANE).anchors.arm(resting(DrawingId(3)));
    strategies.queue_closed(PANE, &bar(106, 107), 4);
    let mut port = RecordingPort::default();
    let answer = strategies.evaluate(
        &[1],
        &mut port,
        Print {
            events: &[],
            now_ms: 0,
        },
        |_, _| false,
        |_, _, _| None,
        |_| None,
    );
    assert!(port.applied.is_empty());
    assert_eq!(answer.watching, 0);
    assert_eq!(strategies.pane(PANE).expect("kept").pending().len(), 1);
}
