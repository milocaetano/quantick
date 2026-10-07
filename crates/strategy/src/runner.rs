//! The step every consumer runs its armed instances through.
//!
//! The chart's paper trading and the backtest harness used to each carry
//! their own copy of the same loop — hand a print's venue events to every
//! instance, apply what it answers, then judge each closed bar and apply
//! that. Two copies of an ordering are two chances for it to drift, and the
//! ordering is the contract:
//!
//! 1. The account consumes the print first (the caller's half: the chart's
//!    paper account, the harness's simulator). Resting orders and brackets
//!    meet the print before any instance looks at it.
//! 2. [`deliver_print_events`] hands the events that print produced to
//!    **every** instance, in creation order. Whatever an instance answers —
//!    the self-protection close after a dropped bracket — is applied
//!    through the port at once and its events are echoed back to that same
//!    instance, once: applying such a command emits no events of its own
//!    (kernel contract), so one echo settles it.
//! 3. Only then does [`step_closed_bar`] judge each bar the print closed,
//!    instance by instance, applying each instance's commands through the
//!    port and echoing their events to it before the next instance reads
//!    the account.
//!
//! What stays with the caller is everything that is not this ordering: when
//! the account consumes the print, sweeping instances whose anchor died,
//! the forming-bar alarm, and whether the paper host keeps listening. The
//! runner touches the account only through [`StrategyPort`], so the chart
//! lends it the paper account and the harness its simulator.

use quantick_engine::Bar;
use quantick_sim::{Command, VenueEvent};
use rust_decimal::Decimal;

use crate::anchors::AnchoredInstance;
use crate::sound::Cue;
use crate::{ArmedStrategy, Region};

/// The account as the runner reaches it: the one way a command gets out,
/// and the one question the flat gate asks.
pub trait StrategyPort {
    /// Apply `command` and answer with the venue events it produced, in
    /// order. They are echoed to the instance that issued the command.
    fn apply(&mut self, command: Command) -> Vec<VenueEvent>;

    /// Whether the account holds no position and nothing working — the
    /// kernel's flat gate, read fresh before each instance judges a bar.
    fn is_flat(&self) -> bool;
}

/// One armed instance as the runner steps it: the anchor its region is
/// resolved by, the kernel's state machine, and the alarm that judges the
/// same close.
pub trait RunnerInstance {
    /// What the caller resolves a region from — the chart's drawing id;
    /// `()` for an instance whose region is fixed.
    type Anchor: Copy;

    /// The anchor this instance rides.
    fn anchor(&self) -> Self::Anchor;

    /// The kernel state machine.
    fn armed_mut(&mut self) -> &mut ArmedStrategy;

    /// Judge the bar that just closed for the alarm, after the kernel has.
    fn alarm_on_closed_bar(&mut self, now_ms: u64) -> Option<Cue>;
}

impl<K: Copy> RunnerInstance for AnchoredInstance<K> {
    type Anchor = K;

    fn anchor(&self) -> K {
        self.drawing
    }

    fn armed_mut(&mut self) -> &mut ArmedStrategy {
        &mut self.armed
    }

    fn alarm_on_closed_bar(&mut self, now_ms: u64) -> Option<Cue> {
        AnchoredInstance::alarm_on_closed_bar(self, now_ms)
    }
}

/// A bare kernel: no drawing to resolve, no alarm to sound — the backtest's
/// fixed-region instance.
impl RunnerInstance for ArmedStrategy {
    type Anchor = ();

    fn anchor(&self) {}

    fn armed_mut(&mut self) -> &mut ArmedStrategy {
        self
    }

    fn alarm_on_closed_bar(&mut self, _now_ms: u64) -> Option<Cue> {
        None
    }
}

/// The region a bar is judged against when none can be resolved: zero
/// width and inactive.
///
/// A region that cannot honestly be tested (hidden, off its series, another
/// market) holds fire but never starves the ruler: the trigger's contract
/// is every closed bar, so the gates shut instead of the feed.
#[must_use]
pub fn region_or_hold(resolved: Option<(Region, bool)>) -> (Region, bool) {
    resolved.unwrap_or((Region::new(Decimal::ZERO, Decimal::ZERO), false))
}

/// Hand the events the account produced consuming one print to every
/// instance, applying each instance's answer through `port` and echoing
/// its events back to that instance once. Step 2 of the module contract.
pub fn deliver_print_events<I, P>(instances: &mut [I], events: &[VenueEvent], port: &mut P)
where
    I: RunnerInstance,
    P: StrategyPort + ?Sized,
{
    if events.is_empty() {
        return;
    }
    for instance in instances {
        let responses = instance.armed_mut().on_sim_events(events);
        for command in responses {
            let echoed = port.apply(command);
            let _ = instance.armed_mut().on_sim_events(&echoed);
        }
    }
}

/// Judge one closed bar with every instance, in creation order. Step 3 of
/// the module contract.
///
/// `region_of` resolves an instance's anchor into the kernel's terms for
/// the bar that closed at `slot`; `None` falls back to [`region_or_hold`].
/// Each instance reads the flat gate after the previous one's commands
/// were applied, and hears its own commands' events before the next
/// instance judges. `now_ms` governs only the alarms; the answer is the
/// cues they raised.
#[must_use = "play the cues the closed bar raised"]
pub fn step_closed_bar<I, P>(
    instances: &mut [I],
    bar: &Bar,
    slot: usize,
    mut region_of: impl FnMut(I::Anchor, usize) -> Option<(Region, bool)>,
    port: &mut P,
    now_ms: u64,
) -> Vec<Cue>
where
    I: RunnerInstance,
    P: StrategyPort + ?Sized,
{
    let mut cues = Vec::new();
    for instance in instances {
        let (region, active) = region_or_hold(region_of(instance.anchor(), slot));
        let flat = port.is_flat();
        let commands = instance
            .armed_mut()
            .on_closed_bar(bar, &region, active, flat);
        // The alarm judges the same bar, from the kernel's own reading of
        // it rather than from whether an order went out — a busy account
        // and a spent one shot silence the order, never the signal. Every
        // closed bar is offered, qualifying or not: a preview that failed
        // to hold is reported here, and this bar's repeat budget resets.
        cues.extend(instance.alarm_on_closed_bar(now_ms));
        for command in commands {
            let echoed = port.apply(command);
            let _ = instance.armed_mut().on_sim_events(&echoed);
        }
    }
    cues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArmedState, DisarmReason, Execution, ForceParams, ForceTrigger, Rearm};
    use crate::{BreakPolicy, StrategyParams};
    use quantick_engine::Side;
    use quantick_sim::RejectReason;
    use quantick_sim::{Bracket, EntryKind, Fill, FillRole, Order, OrderId, OrderRole};
    use std::cell::RefCell;

    /// What the fake account saw, in the order it saw it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Seen {
        Print,
        Flat,
        Apply(&'static str),
    }

    /// The commands these tests issue, by name.
    fn named(command: &Command) -> &'static str {
        match command {
            Command::PlaceMarket {
                side: Side::Buy, ..
            } => "buy market",
            Command::ClosePosition => "close position",
            _ => "another command",
        }
    }

    /// An account that records every call — the flat read through a
    /// shared borrow, hence the cell — and acknowledges each market entry
    /// with a placement under the next order id.
    #[derive(Default)]
    struct RecordingPort {
        log: RefCell<Vec<Seen>>,
        next_id: u64,
    }

    impl RecordingPort {
        /// The account consuming a print: recorded, and its events handed
        /// back for the instances to hear.
        fn consume(&mut self, events: Vec<VenueEvent>) -> Vec<VenueEvent> {
            self.log.get_mut().push(Seen::Print);
            events
        }

        fn take_log(&mut self) -> Vec<Seen> {
            std::mem::take(self.log.get_mut())
        }
    }

    impl StrategyPort for RecordingPort {
        fn apply(&mut self, command: Command) -> Vec<VenueEvent> {
            self.log.get_mut().push(Seen::Apply(named(&command)));
            match command {
                Command::PlaceMarket { side, quantity, .. } => {
                    self.next_id += 1;
                    vec![VenueEvent::Placed(order(self.next_id, side, quantity))]
                }
                _ => Vec::new(),
            }
        }

        fn is_flat(&self) -> bool {
            self.log.borrow_mut().push(Seen::Flat);
            true
        }
    }

    fn order(id: u64, side: Side, quantity: Decimal) -> Order {
        Order {
            id: OrderId(id),
            side,
            kind: EntryKind::Market,
            price: None,
            quantity,
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
            placed_ms: 0,
            role: OrderRole::Entry,
            oco: None,
            reduce_only: false,
        }
    }

    fn entry_fill(id: u64) -> VenueEvent {
        VenueEvent::Filled(Fill {
            timestamp_ms: 1,
            agg_id: 1,
            side: Side::Buy,
            price: Decimal::from(106),
            quantity: Decimal::ONE,
            role: FillRole::Entry(OrderId(id)),
        })
    }

    fn bar(open: i64, close: i64) -> Bar {
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

    /// A buy instance whose 3-bar ruler fires on a body-4 bar.
    fn warmed() -> ArmedStrategy {
        let mut armed = ArmedStrategy::new(
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
                min_factor: "1.5".parse().expect("fixture factor"),
                max_factor: "2.5".parse().expect("fixture factor"),
                min_range: Decimal::ZERO,
            })),
        );
        armed.warm(&[bar(100, 101), bar(101, 102)]);
        armed
    }

    fn region() -> Option<(Region, bool)> {
        Some((Region::new(Decimal::from(100), Decimal::from(110)), true))
    }

    /// The order the module contract names, end to end on two instances:
    /// each closed-bar command is applied and echoed to the instance that
    /// issued it before the next one judges; the account consumes the next
    /// print before anyone hears it; its events reach every instance, the
    /// self-protection answer is applied once and echoed; and only then
    /// does the next closed bar step.
    #[test]
    fn events_reach_every_instance_and_echo_to_their_issuer_before_bars_step() {
        let mut instances = [warmed(), warmed()];
        let mut port = RecordingPort::default();

        let cues = step_closed_bar(
            &mut instances,
            &bar(102, 106),
            3,
            |(), _| region(),
            &mut port,
            0,
        );
        assert!(cues.is_empty(), "no alarm rides a bare kernel");
        assert_eq!(
            port.take_log(),
            [
                Seen::Flat,
                Seen::Apply("buy market"),
                Seen::Flat,
                Seen::Apply("buy market"),
            ],
            "each instance read the account, then its entry went out"
        );
        // Each placement went back to the instance that asked: the first
        // order id to the first instance, the second to the second.
        for (instance, id) in instances.iter().zip([1, 2]) {
            assert_eq!(
                instance.state(),
                &ArmedState::Fired {
                    order_id: Some(OrderId(id)),
                    retest: false
                }
            );
        }

        // The next print fills both entries, and the first one's bracket
        // could not be priced. Only that instance owns the dropped leg —
        // its fill precedes the drop in the batch — so only it answers.
        let events = port.consume(vec![
            entry_fill(1),
            VenueEvent::BracketDropped {
                reason: RejectReason::StopLossOnWrongSide(Side::Buy),
            },
            entry_fill(2),
        ]);
        deliver_print_events(&mut instances, &events, &mut port);
        let _ = step_closed_bar(
            &mut instances,
            &bar(106, 106),
            4,
            |(), _| region(),
            &mut port,
            0,
        );

        assert_eq!(
            port.take_log(),
            [
                Seen::Print,
                Seen::Apply("close position"),
                Seen::Flat,
                Seen::Flat,
            ],
            "the account consumed the print first, the close was applied \
             once, and only then did the next bar step"
        );
        assert_eq!(
            instances[0].state(),
            &ArmedState::Disarmed {
                reason: DisarmReason::ProtectionDropped
            }
        );
        assert_eq!(
            instances[1].state(),
            &ArmedState::Done,
            "the second instance heard the same batch — its entry filled,              so the flat account the next bar saw completed its one shot"
        );
    }

    /// An unresolvable region holds fire on a bar that would have fired,
    /// and the instance is still asked — the ruler is fed every close.
    #[test]
    fn an_unresolved_region_holds_fire_on_the_inactive_zero_region() {
        let mut instances = [warmed()];
        let mut port = RecordingPort::default();
        let mut asked = Vec::new();
        let _ = step_closed_bar(
            &mut instances,
            &bar(102, 106),
            7,
            |(), slot| {
                asked.push(slot);
                None
            },
            &mut port,
            0,
        );
        assert_eq!(asked, [7], "the region is resolved for the bar's own slot");
        assert_eq!(port.take_log(), [Seen::Flat], "nothing fired");
        assert_eq!(instances[0].state(), &ArmedState::Armed);
        assert_eq!(
            region_or_hold(None),
            (Region::new(Decimal::ZERO, Decimal::ZERO), false)
        );
    }
}
