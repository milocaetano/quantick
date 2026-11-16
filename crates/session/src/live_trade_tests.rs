//! One live print carried through a fake owner set: the stage order, the
//! closes routed by pane id, and the clock read only behind an alarm.

use quantick_chart_interaction::live_trade_plan::LiveTradePlan;
use quantick_engine::{Side, Trade};
use quantick_strategy::{AlarmParams, SignalAlarm};
use rust_decimal::Decimal;

use crate::live_trade::{LiveClose, LiveTradeOwners, PaneFold, execute_live_trade};
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
    strategies: TabStrategies<DrawingId>,
    /// What each pane's fold reports, flow first.
    folds: Vec<PaneFold>,
    seen: Vec<Seen>,
}

impl Owners {
    /// A flow pane and a time pane each closing one bar, and a strategy
    /// armed on the time pane only.
    fn new(alarm: bool) -> Self {
        let mut armed = instance(DrawingId(1));
        armed.alarm = alarm.then(|| SignalAlarm::new(AlarmParams::default()));
        let mut strategies = TabStrategies::default();
        let _ = strategies.pane_mut(TIME).anchors.arm(armed);
        let fold = |pane_id, slot| PaneFold {
            pane_id,
            closes: vec![LiveClose {
                bar: bar(100, 101),
                slot,
            }],
        };
        Self {
            strategies,
            folds: vec![fold(FLOW, 40), fold(TIME, 12)],
            seen: Vec::new(),
        }
    }
}

impl LiveTradeOwners for Owners {
    type Drawing = DrawingId;
    fn strategies(&mut self) -> &mut TabStrategies<DrawingId> {
        &mut self.strategies
    }
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
    fn evaluate_strategies(&mut self, now_ms: u64) {
        let queued = [FLOW, TIME]
            .into_iter()
            .flat_map(|id| {
                let pending = self.strategies.pane(id).map(|pane| pane.pending().to_vec());
                pending
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |(_, slot)| (id, slot))
            })
            .collect();
        self.seen.push(Seen::Evaluate { now_ms, queued });
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

/// Run one print through the validated plan; the owners' log and how many
/// times the clock was read.
fn run(owners: &mut Owners) -> usize {
    let mut reads = 0;
    execute_live_trade(&print(), LiveTradePlan::stages(), owners, || {
        reads += 1;
        5_000
    });
    reads
}

#[test]
fn live_trade_plan_runs_without_app() {
    let mut owners = Owners::new(true);
    let reads = run(&mut owners);
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
         only the armed pane's close is queued, then the sweep runs"
    );
    assert_eq!(reads, 1, "an armed alarm reads the clock once per print");

    let mut quiet = Owners::new(false);
    assert_eq!(
        run(&mut quiet),
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
}
