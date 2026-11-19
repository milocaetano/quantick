//! One live print carried through a tab's market and a fake owner set: the
//! stage order, the closes routed by pane id, the clock read only behind an
//! alarm, and the sweep's sounds parked until the window drains them.

use quantick_chart_interaction::live_trade_plan::LiveTradePlan;
use quantick_engine::{Side, Trade};
use quantick_strategy::sound::Cue;
use quantick_strategy::{AlarmParams, SignalAlarm};
use rust_decimal::Decimal;

use crate::live_trade::{LiveClose, LiveTradeOwners, PaneFold};
use crate::market::TabMarket;
use crate::strategies::TabStrategies;
use crate::strategies_tests::{DrawingId, bar, instance};

const FLOW: u64 = 3;
const TIME: u64 = 7;

/// What the owners were asked to do, in the order they were asked.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Paper(u64),
    Fold(usize),
    Alert(usize, Option<usize>),
    /// The sweep, with the closes queued for each pane id at that moment.
    Evaluate {
        now_ms: u64,
        queued: Vec<(u64, usize)>,
    },
}

struct Owners {
    /// What each pane's fold reports, flow first.
    folds: Vec<PaneFold>,
    /// What the sweep answers with.
    sounds: Vec<Cue>,
    seen: Vec<Seen>,
}

/// A market with a strategy armed on the time pane only, with or without
/// an alarm, and a flow pane and a time pane each closing one bar.
fn setup(alarm: bool) -> (TabMarket<DrawingId>, Owners) {
    let mut armed = instance(DrawingId(1));
    armed.alarm = alarm.then(|| SignalAlarm::new(AlarmParams::default()));
    let mut market = TabMarket::default();
    let _ = market.strategies.pane_mut(TIME).anchors.arm(armed);
    let fold = |pane_id, slot| PaneFold {
        pane_id,
        closes: vec![LiveClose {
            bar: bar(100, 101),
            poc: None,
            slot,
        }],
    };
    let owners = Owners {
        folds: vec![fold(FLOW, 40), fold(TIME, 12)],
        sounds: alarm.then(Cue::default).into_iter().collect(),
        seen: Vec::new(),
    };
    (market, owners)
}

impl LiveTradeOwners for Owners {
    type Drawing = DrawingId;
    fn paper_trade(&mut self, trade: &Trade) {
        self.seen.push(Seen::Paper(trade.agg_id));
    }
    fn pane_count(&self) -> usize {
        self.folds.len()
    }
    fn fold_pane(&mut self, pane: usize, _: &Trade) -> PaneFold {
        self.seen.push(Seen::Fold(pane));
        self.folds[pane].clone()
    }
    fn observe_alert(&mut self, pane: usize, _: &Trade, close: Option<&LiveClose>) {
        self.seen
            .push(Seen::Alert(pane, close.map(|close| close.slot)));
    }
    fn evaluate_strategies(
        &mut self,
        strategies: &mut TabStrategies<DrawingId>,
        now_ms: u64,
    ) -> Vec<Cue> {
        let queued = [FLOW, TIME]
            .into_iter()
            .flat_map(|id| {
                let pending = strategies.pane(id).map(|pane| pane.pending().to_vec());
                pending
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |close| (id, close.slot))
            })
            .collect();
        self.seen.push(Seen::Evaluate { now_ms, queued });
        self.sounds.clone()
    }
}

fn print() -> Trade {
    Trade {
        agg_id: 9,
        timestamp_ms: 5_000,
        price: Decimal::from(101),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// Run one print through the validated plan with an injected clock; how
/// many times the clock was read.
fn run(market: &mut TabMarket<DrawingId>, owners: &mut Owners) -> usize {
    let mut reads = 0;
    market.execute_live_trade(&print(), LiveTradePlan::stages(), owners, || {
        reads += 1;
        5_000
    });
    reads
}

#[test]
fn a_print_walks_the_market_and_parks_its_alarms() {
    let (mut market, mut owners) = setup(true);
    let reads = run(&mut market, &mut owners);
    assert_eq!(
        owners.seen,
        vec![
            Seen::Paper(9),
            Seen::Fold(0),
            Seen::Alert(0, Some(40)),
            Seen::Alert(0, None),
            Seen::Fold(1),
            Seen::Alert(1, Some(12)),
            Seen::Alert(1, None),
            Seen::Evaluate {
                now_ms: 5_000,
                queued: vec![(TIME, 12)],
            },
        ],
        "paper fills, every pane folds and shows its closes to the alerts, \
         only the armed pane's close is queued, then the sweep runs at the \
         injected time"
    );
    assert_eq!(reads, 1, "an armed alarm reads the clock once per print");
    assert_eq!(
        market.pending_alarms(),
        &[Cue::default()],
        "the sweep's sound waits in the market"
    );
    assert_eq!(
        market.drain_alarms().collect::<Vec<_>>(),
        vec![Cue::default()],
        "the window drains it"
    );
    assert!(market.pending_alarms().is_empty(), "and it is played once");
}

#[test]
fn a_print_without_an_alarm_never_reads_the_clock() {
    let (mut market, mut quiet) = setup(false);
    assert_eq!(
        run(&mut market, &mut quiet),
        0,
        "without an alarm the clock is never read"
    );
    assert_eq!(
        quiet.seen.last(),
        Some(&Seen::Evaluate {
            now_ms: 0,
            queued: vec![(TIME, 12)],
        }),
        "the sweep still runs, judged at zero"
    );
    assert!(market.pending_alarms().is_empty(), "no sound is queued");
}
