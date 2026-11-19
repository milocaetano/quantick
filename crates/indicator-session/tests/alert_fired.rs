//! A script's `alertcondition` reaches the session's consumer once per bar
//! that closes live, and never for a forming bar or replayed history.
use quantick_engine::{Bar, BarBuilder as _, Side, TickBarBuilder, Trade, golden};
use quantick_indicator_session::{
    IndicatorCommand as Command, IndicatorEvent as Event, IndicatorSession,
    IndicatorSource as Source, InputsRebound, SessionEffects, SlotId,
};
use rust_decimal::Decimal;

const SCRIPT: &str = "//@version=5\n\
    indicator(\"every close\")\n\
    alertcondition(close > 0, title=\"closed\", message=\"a bar closed\")\n\
    plot(close)\n";

#[derive(Default)]
struct Recorder(Vec<Event>);
impl SessionEffects for Recorder {
    fn event(&mut self, event: Event) {
        self.0.push(event);
    }
    fn inputs_rebound(&mut self, _: InputsRebound<'_>) {}
}
impl Recorder {
    /// `(slot, bar close)` of every alarm recorded, taking them.
    fn fired(&mut self) -> Vec<(u64, i64)> {
        self.0
            .drain(..)
            .filter_map(|event| match event {
                Event::AlertFired {
                    slot,
                    bar_close_ms,
                    title,
                    message,
                } => {
                    assert_eq!((title.as_str(), message.as_str()), ("closed", "a bar closed"));
                    Some((slot.0, bar_close_ms))
                }
                _ => None,
            })
            .collect()
    }
}

fn bars() -> (Vec<Bar>, Bar) {
    let trades: Vec<Trade> = (1..=11)
        .map(|i| Trade {
            agg_id: i,
            timestamp_ms: 1_000 + i as i64 * 100,
            price: Decimal::from(100 + (i % 3) as i64),
            quantity: Decimal::ONE,
            side: Side::Buy,
        })
        .collect();
    let mut builder = TickBarBuilder::new(2);
    let bars = golden::replay(&mut builder, &trades);
    (bars, builder.partial().cloned().expect("one print stays forming"))
}

fn add(slot: u64) -> Command {
    Command::Add {
        slot: SlotId(slot),
        source: Source::Script {
            name: "every_close.pine".into(),
            text: SCRIPT.into(),
        },
    }
}

fn partial(bar: &Bar) -> Command {
    Command::PartialUpdated {
        partial: Some(bar.clone().into()),
        run: Vec::new(),
        rungs: 0,
    }
}

fn run(session: &mut IndicatorSession, commands: Vec<Command>, recorder: &mut Recorder) {
    let mut applying = session.begin_batch(commands.iter().enumerate());
    let mut partials = 0;
    for (index, command) in commands.into_iter().enumerate() {
        applying.apply(index, command, &mut partials, recorder);
    }
    applying.finish_applying().publish(recorder);
}

#[test]
fn only_a_live_close_fires_and_only_once() {
    let (bars, forming) = bars();
    let (history, live) = bars.split_at(3);
    let mut session = IndicatorSession::new();
    let mut recorder = Recorder::default();

    let backfill = history.iter().cloned().map(Into::into).collect();
    run(&mut session, vec![add(1), Command::Backfilled(backfill)], &mut recorder);
    run(&mut session, vec![add(2)], &mut recorder);
    run(&mut session, vec![partial(&live[0])], &mut recorder);
    assert_eq!(recorder.fired(), vec![], "history, add and preview fire nothing");

    run(&mut session, vec![Command::BarClosed(live[0].clone().into())], &mut recorder);
    let close = live[0].close_time;
    assert_eq!(recorder.fired(), vec![(1, close), (2, close)], "one per slot");

    run(&mut session, vec![partial(&forming)], &mut recorder);
    run(
        &mut session,
        vec![Command::SetInputs {
            slot: SlotId(1),
            values: Vec::new(),
        }],
        &mut recorder,
    );
    let rebuild = bars.iter().cloned().map(Into::into).collect();
    run(
        &mut session,
        vec![Command::Rebuild(rebuild, Some(forming.clone().into()))],
        &mut recorder,
    );
    assert_eq!(
        recorder.fired(),
        vec![],
        "a preview, a rebind and a rebuild never ring the closed bar again"
    );

    run(&mut session, vec![Command::BarClosed(live[1].clone().into())], &mut recorder);
    let close = live[1].close_time;
    assert_eq!(recorder.fired(), vec![(1, close), (2, close)]);
}
