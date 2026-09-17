use super::*;
use quantick_engine::bar_registry::{BarDefinition, BarRegistry, definitions::TICK};
use quantick_engine::{Bar, BarBuilder, TickBarBuilder};
use quantick_sim::VenueEvent;
use rust_decimal::prelude::ToPrimitive;

thread_local! {
    static FOLDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// Count the actual production runner's calls through the existing factory
// port: on_bar alone cannot detect a fold moved ahead of event reactions.
static TRACED_TICK: BarDefinition = BarDefinition {
    id: "traced_tick",
    factory: |value, _| Box::new(TracedTick(TickBarBuilder::new(value.to_u64().unwrap()))),
    ..TICK
};

struct TracedTick(TickBarBuilder);
impl BarBuilder for TracedTick {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        FOLDS.set(FOLDS.get() + 1);
        self.0.push(trade)
    }

    fn partial(&self) -> Option<&Bar> {
        self.0.partial()
    }
}

#[derive(Default)]
struct ProtectOnDroppedBracket {
    trace: Vec<String>,
}

impl Strategy for ProtectOnDroppedBracket {
    fn name(&self) -> &str {
        "ordering-characterization"
    }

    fn on_events(&mut self, events: &[VenueEvent]) -> Vec<Command> {
        let names: Vec<_> = events
            .iter()
            .map(|event| match event {
                VenueEvent::Placed(_) => "placed",
                VenueEvent::Filled(_) => "filled",
                VenueEvent::Closed(_) => "closed",
                VenueEvent::BracketDropped { .. } => "bracket_dropped",
                _ => "other",
            })
            .collect();
        self.trace.push(format!("events@{}:{names:?}", FOLDS.get()));
        if events
            .iter()
            .any(|event| matches!(event, VenueEvent::BracketDropped { .. }))
        {
            vec![Command::ClosePosition]
        } else {
            Vec::new()
        }
    }

    fn on_bar(&mut self, view: &BarView<'_>) -> Vec<Command> {
        assert_eq!(
            view.signals.rows(),
            view.index + 1,
            "indicators committed before on_bar"
        );
        assert_eq!(view.account.mark_price, Some(view.bar.close));
        self.trace.push(format!(
            "bar:{}:position={}:closed={}",
            view.index,
            view.account.position.is_some(),
            view.account.closed_trades
        ));
        if view.index == 0 {
            vec![Command::PlaceMarket {
                side: Side::Buy,
                quantity: Decimal::ONE,
                bracket: quantick_sim::Bracket::whole(None, Some(Decimal::from(101))),
            }]
        } else {
            Vec::new()
        }
    }
}

#[test]
fn fill_events_and_self_protection_precede_the_bar_and_fill_only_on_the_next_print() {
    // Print 1 queues the entry. Print 2 gaps beyond its target: the
    // dropped-bracket reaction queues a close before bar 2 is delivered.
    // Only print 3 may execute that close; each bar sees the updated account.
    let session = synthetic(&tape_of(&["100", "102", "103"]));
    let mut strategy = ProtectOnDroppedBracket::default();
    let registry = BarRegistry::new([&TRACED_TICK]).unwrap();
    FOLDS.set(0);
    let run = run_session(
        &session,
        registry.parse("traced_tick:1").unwrap(),
        &mut strategy,
    );
    assert_eq!(
        strategy.trace,
        [
            "events@0:[]",
            "bar:0:position=false:closed=0",
            "events@1:[\"placed\"]",
            "events@1:[\"filled\", \"bracket_dropped\"]",
            "events@1:[]",
            "bar:1:position=true:closed=0",
            "events@2:[\"filled\", \"closed\"]",
            "bar:2:position=false:closed=1",
        ]
    );
    assert_eq!(run.bars, 3);
    assert_eq!(run.anomalies.dropped_brackets(), 1);
    assert_eq!(run.trades.len(), 1);
    let trade = &run.trades[0];
    assert_eq!((trade.entry_agg_id, trade.exit_agg_id), (Some(2), Some(3)));
    assert_eq!(
        (trade.entry_price, trade.exit_price, trade.pnl_points),
        (Decimal::from(102), Decimal::from(103), Decimal::ONE)
    );
    assert_eq!(
        (trade.opened_ms, trade.closed_ms),
        (1_786_233_601_000, 1_786_233_602_000)
    );
    assert_eq!(run.open_at_end, None);
}
