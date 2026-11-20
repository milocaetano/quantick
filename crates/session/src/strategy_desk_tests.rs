//! The strategy desk over a tab's drawn and scripted instances, with no
//! pane: drawings are plain ids, the chart's facts closures.

use quantick_engine::{Bar, Side};
use quantick_strategy::{ArmedState, Region};
use rust_decimal::Decimal;

use crate::script_strategies::{ScriptArm, ScriptPlacement};
use crate::strategies::{Print, TabStrategies};
use crate::strategies_tests::{DrawingId, RecordingPort, instance};
use crate::strategy_desk::{
    Asset, DAY_MS, DeskAction, DeskLink, DeskOutcome, DeskState, DeskTab, InstanceRef,
    StrategyDeskCommand, StrategyDeskView,
};

const TICK: u64 = 1;
const TIME: u64 = 2;

const ALWAYS_RINGS: &str = "//@version=5\n\
    indicator(\"Steady rise\")\n\
    len = input.int(14, title=\"Length\")\n\
    alertcondition(close > open, title=\"Long up\")\n\
    plot(ta.sma(close, len))\n";

const RENKO_POC_FLIP: &str = include_str!("../../app/scripts/renko_poc_flip.pine");

fn panes() -> Vec<(u64, String)> {
    vec![(TICK, "tick(4)".to_owned()), (TIME, "time(1m)".to_owned())]
}

fn tab(panes: &[(u64, String)]) -> DeskTab<'_> {
    DeskTab {
        tab_id: 9,
        symbol: "WINZ26",
        panes,
    }
}

fn asset(bar_type: &str) -> Asset {
    Asset {
        symbol: "WINZ26".to_owned(),
        bar_type: bar_type.to_owned(),
    }
}

fn candle(index: i64, open: &str, close: &str, poc: Option<&str>) -> (Bar, Option<Decimal>) {
    let dec = |text: &str| text.parse::<Decimal>().expect("fixture decimal");
    let (open, close) = (dec(open), dec(close));
    let bar = Bar {
        open_time: index * 1_000,
        close_time: index * 1_000 + 999,
        open,
        high: open.max(close),
        low: open.min(close),
        close,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    };
    (bar, poc.map(dec))
}

fn up(index: i64) -> (Bar, Option<Decimal>) {
    candle(index, "100", "101", None)
}

fn arm_script(
    strategies: &mut TabStrategies<DrawingId>,
    pane_id: u64,
    source: &str,
    place: ScriptPlacement<DrawingId>,
) -> u64 {
    let request = ScriptArm {
        pane_id,
        script: "steady.pine",
        source,
        side: Side::Buy,
        place,
    };
    let armed = strategies.arm_script(request, |_| Vec::new());
    armed.expect("the fixture arms").id
}

/// Close `bars` on `pane_id` one by one at `now_ms`; the slots that rang.
fn sweep(
    strategies: &mut TabStrategies<DrawingId>,
    pane_id: u64,
    bars: &[(Bar, Option<Decimal>)],
    first_slot: usize,
    now_ms: u64,
) -> Vec<usize> {
    let mut rang = Vec::new();
    for (offset, (bar, poc)) in bars.iter().enumerate() {
        let slot = first_slot + offset;
        strategies.queue_closed(pane_id, bar, *poc, slot, true);
        let region = Region::new(Decimal::from(100), Decimal::from(110));
        let answer = strategies.evaluate(
            &[pane_id],
            &mut RecordingPort::default(),
            Print {
                events: &[],
                now_ms,
            },
            |_, _| true,
            |_, _, _| Some((region, true)),
            |_| None,
        );
        if !answer.alarms.is_empty() {
            rang.push(slot);
        }
    }
    rang
}

fn command(
    strategies: &mut TabStrategies<DrawingId>,
    command: StrategyDeskCommand<DrawingId>,
) -> Result<DeskOutcome<DrawingId>, String> {
    let panes = panes();
    strategies.execute_desk(&tab(&panes), command, true, |_, _| Vec::new())
}

fn view(strategies: &TabStrategies<DrawingId>) -> Vec<crate::strategy_desk::DeskGroup<DrawingId>> {
    let panes = panes();
    let mut view = StrategyDeskView::default();
    view.read(&tab(&panes), strategies, |_, drawing| {
        Some(format!("zone-{}", drawing.0))
    });
    view.groups()
}

#[test]
fn groups_instances_by_asset() {
    let mut strategies = TabStrategies::default();
    strategies
        .pane_mut(TICK)
        .anchors
        .arm(instance(DrawingId(3)));
    arm_script(
        &mut strategies,
        TIME,
        ALWAYS_RINGS,
        ScriptPlacement::Anywhere,
    );

    let groups = view(&strategies);

    let assets: Vec<_> = groups.iter().map(|group| &group.asset).collect();
    assert_eq!(assets, [&asset("tick(4)"), &asset("time(1m)")]);
    let counts: Vec<_> = groups.iter().map(|group| group.rows.len()).collect();
    assert_eq!(counts, [1, 1]);
    let drawn = &groups[0].rows[0];
    assert_eq!(drawn.name, "test");
    assert_eq!(drawn.state, DeskState::Watching);
    assert_eq!(
        drawn.link,
        DeskLink::Drawing {
            drawing: DrawingId(3),
            name: Some("zone-3".to_owned()),
        }
    );
    assert_eq!(drawn.action, DeskAction::Order);
    assert_eq!(
        drawn.id,
        InstanceRef::Drawing {
            pane_id: TICK,
            drawing: DrawingId(3),
            nth: 0,
        }
    );
    let script = &groups[1].rows[0];
    assert_eq!(script.name, "steady.pine");
    assert_eq!(script.link, DeskLink::Chart, "unlinked reads as the chart");
    assert_eq!(script.action, DeskAction::Alarm);
    assert_eq!(script.id, InstanceRef::Script(1));
    assert!(groups.iter().all(|group| !group.paused));
}

#[test]
fn paused_instance_does_not_fire() {
    let mut strategies = TabStrategies::default();
    let script = InstanceRef::Script(arm_script(
        &mut strategies,
        TICK,
        ALWAYS_RINGS,
        ScriptPlacement::Anywhere,
    ));
    strategies
        .pane_mut(TICK)
        .anchors
        .arm(instance(DrawingId(3)));
    let drawn = InstanceRef::Drawing {
        pane_id: TICK,
        drawing: DrawingId(3),
        nth: 0,
    };

    assert_eq!(
        command(&mut strategies, StrategyDeskCommand::Pause(script)),
        Ok(DeskOutcome::Changed(1))
    );
    assert_eq!(
        command(&mut strategies, StrategyDeskCommand::Pause(drawn)),
        Ok(DeskOutcome::Changed(1))
    );
    assert_eq!(
        command(&mut strategies, StrategyDeskCommand::Pause(script)),
        Ok(DeskOutcome::Changed(0)),
        "pausing twice changes nothing"
    );
    // The drawn fixture's ruler fires on this third bar when it judges it.
    let force = [
        candle(0, "100", "101", None),
        candle(1, "101", "102", None),
        candle(2, "102", "106", None),
    ];
    assert_eq!(
        sweep(&mut strategies, TICK, &force, 0, 0),
        Vec::<usize>::new()
    );
    let drawn_state = strategies.anchors(TICK).expect("armed").instances[0]
        .armed
        .state()
        .clone();
    assert_eq!(
        drawn_state,
        ArmedState::Armed,
        "a paused instance placed nothing"
    );

    assert_eq!(
        command(&mut strategies, StrategyDeskCommand::Resume(script)),
        Ok(DeskOutcome::Changed(1))
    );
    assert_eq!(
        command(&mut strategies, StrategyDeskCommand::Resume(drawn)),
        Ok(DeskOutcome::Changed(1))
    );
    assert_eq!(
        sweep(&mut strategies, TICK, &[up(3)], 3, 0),
        [3],
        "resumed, the next firing bar fires"
    );
    assert!(
        view(&strategies)[0]
            .rows
            .iter()
            .all(|row| row.state == DeskState::Watching)
    );
}

#[test]
fn asset_pause_spans_its_instances_only() {
    let mut strategies = TabStrategies::default();
    strategies
        .pane_mut(TICK)
        .anchors
        .arm(instance(DrawingId(3)));
    arm_script(
        &mut strategies,
        TICK,
        ALWAYS_RINGS,
        ScriptPlacement::Anywhere,
    );
    arm_script(
        &mut strategies,
        TIME,
        ALWAYS_RINGS,
        ScriptPlacement::Anywhere,
    );

    let paused = command(
        &mut strategies,
        StrategyDeskCommand::PauseAsset(asset("tick(4)")),
    );

    assert_eq!(paused, Ok(DeskOutcome::Changed(2)));
    let groups = view(&strategies);
    assert!(groups[0].paused);
    assert!(
        groups[0]
            .rows
            .iter()
            .all(|row| row.state == DeskState::Paused)
    );
    assert!(!groups[1].paused);
    assert_eq!(groups[1].rows[0].state, DeskState::Watching);
    assert_eq!(
        sweep(&mut strategies, TIME, &[up(0)], 0, 0),
        [0],
        "the other asset still rings"
    );
    assert_eq!(
        sweep(&mut strategies, TICK, &[up(0)], 0, 0),
        Vec::<usize>::new()
    );

    let resumed = command(
        &mut strategies,
        StrategyDeskCommand::ResumeAsset(asset("tick(4)")),
    );
    assert_eq!(resumed, Ok(DeskOutcome::Changed(2)));
    assert!(!view(&strategies)[0].paused);
}

#[test]
fn remove_drops_only_that_instance() {
    let mut strategies = TabStrategies::default();
    let mut second = instance(DrawingId(3));
    second.preset = "second".to_owned();
    let pane = strategies.pane_mut(TICK);
    pane.anchors.arm(instance(DrawingId(3)));
    pane.anchors.arm(second);
    let first = InstanceRef::Drawing {
        pane_id: TICK,
        drawing: DrawingId(3),
        nth: 0,
    };

    assert_eq!(
        command(&mut strategies, StrategyDeskCommand::Remove(first)),
        Ok(DeskOutcome::Changed(1))
    );

    let rows = &view(&strategies)[0].rows;
    assert_eq!(rows.len(), 1, "the drawing keeps its other instance");
    assert_eq!(rows[0].name, "second");
    assert_eq!(rows[0].id, first, "the survivor takes the first place");
    let script = arm_script(
        &mut strategies,
        TICK,
        ALWAYS_RINGS,
        ScriptPlacement::Anywhere,
    );
    let gone = InstanceRef::Script(script);
    assert_eq!(
        command(&mut strategies, StrategyDeskCommand::Remove(gone)),
        Ok(DeskOutcome::Changed(1))
    );
    assert!(
        command(&mut strategies, StrategyDeskCommand::Remove(gone)).is_err(),
        "gone is gone"
    );
    assert_eq!(view(&strategies)[0].rows.len(), 1);
}

#[test]
fn duplicate_and_save_as() {
    let mut strategies = TabStrategies::default();
    let line = ScriptPlacement::Line {
        level: Decimal::from(100),
        rising: true,
    };
    let original = InstanceRef::Script(arm_script(&mut strategies, TICK, ALWAYS_RINGS, line));

    let copy = command(&mut strategies, StrategyDeskCommand::Duplicate(original));

    assert_eq!(copy, Ok(DeskOutcome::Duplicated(InstanceRef::Script(2))));
    let rows = &view(&strategies)[0].rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].name, "steady.pine");
    assert_eq!(rows[1].state, DeskState::Paused, "a copy is armed paused");
    let readings = strategies.script_readings();
    assert_eq!(readings[1].place, line, "the copy keeps the placement");

    let saved = command(
        &mut strategies,
        StrategyDeskCommand::SaveAs {
            instance: original,
            name: "steady_copy.pine".to_owned(),
        },
    );
    assert_eq!(
        saved,
        Ok(DeskOutcome::Save {
            name: "steady_copy.pine".to_owned(),
            source: ALWAYS_RINGS.to_owned(),
        })
    );
    strategies
        .pane_mut(TICK)
        .anchors
        .arm(instance(DrawingId(3)));
    let drawn = InstanceRef::Drawing {
        pane_id: TICK,
        drawing: DrawingId(3),
        nth: 0,
    };
    let refused = StrategyDeskCommand::SaveAs {
        instance: drawn,
        name: "x.pine".to_owned(),
    };
    assert!(
        command(&mut strategies, refused).is_err(),
        "a drawn instance has no script"
    );
    let drawn_copy = command(&mut strategies, StrategyDeskCommand::Duplicate(drawn));
    assert_eq!(
        drawn_copy,
        Ok(DeskOutcome::Duplicated(InstanceRef::Drawing {
            pane_id: TICK,
            drawing: DrawingId(3),
            nth: 1,
        }))
    );
}

#[test]
fn detail_reports_inputs_and_fires_today() {
    let mut strategies = TabStrategies::default();
    let id = InstanceRef::Script(arm_script(
        &mut strategies,
        TICK,
        ALWAYS_RINGS,
        ScriptPlacement::Anywhere,
    ));
    let yesterday = 2 * DAY_MS + 5_000;
    let today = 3 * DAY_MS + 5_000;
    sweep(&mut strategies, TICK, &[up(0)], 0, yesterday);
    sweep(&mut strategies, TICK, &[up(1), up(2)], 1, today);

    let panes = panes();
    let detail = strategies
        .detail(
            &tab(&panes),
            id,
            |_, _| None,
            |_| Vec::new(),
            |_, _, _| None,
        )
        .expect("the instance exists");

    assert_eq!(detail.description, "Steady rise");
    assert_eq!(detail.placement, ScriptPlacement::Anywhere);
    assert_eq!(detail.row.action, DeskAction::Alarm);
    assert_eq!(detail.inputs, [("Length".to_owned(), "14".to_owned())]);
    assert_eq!(detail.fires_today, 2, "yesterday's ring is not today's");
    assert_eq!(detail.code.as_deref(), Some(ALWAYS_RINGS));
}

#[test]
fn history_markers_match_live_fires() {
    let bars = [
        candle(0, "101", "100", Some("100.5")),
        candle(1, "100", "101", Some("100.8")),
        candle(2, "101", "100", Some("100.2")),
        candle(3, "100", "101", Some("100.1")),
        candle(4, "101", "100", Some("100.6")),
        candle(5, "100", "101", Some("100.9")),
    ];
    let mut live = TabStrategies::default();
    let id = InstanceRef::Script(arm_script(
        &mut live,
        TICK,
        RENKO_POC_FLIP,
        ScriptPlacement::Anywhere,
    ));
    let fired = sweep(&mut live, TICK, &bars, 0, 0);
    assert_eq!(fired, [1, 5], "the fixture flips twice");

    let markers = live.history_markers(id, &bars, |_, _| None);
    assert_eq!(markers.as_deref(), Some(fired.as_slice()));
    assert_eq!(
        live.history_markers(id, &bars, |_, _| None),
        markers,
        "deterministic"
    );
}

#[test]
fn a_caller_without_the_trade_tier_cannot_restart_an_order_strategy() {
    let mut strategies = TabStrategies::default();
    strategies
        .pane_mut(TICK)
        .anchors
        .arm(instance(DrawingId(3)));
    arm_script(
        &mut strategies,
        TICK,
        ALWAYS_RINGS,
        ScriptPlacement::Anywhere,
    );
    let drawn = InstanceRef::Drawing {
        pane_id: TICK,
        drawing: DrawingId(3),
        nth: 0,
    };
    let panes = panes();
    let mut agent =
        |command| strategies.execute_desk(&tab(&panes), command, false, |_, _| Vec::new());

    assert_eq!(
        agent(StrategyDeskCommand::PauseAsset(asset("tick(4)"))),
        Ok(DeskOutcome::Changed(2)),
        "pausing is always allowed"
    );
    for command in [
        StrategyDeskCommand::Resume(drawn),
        StrategyDeskCommand::Arm(drawn),
        StrategyDeskCommand::Duplicate(drawn),
    ] {
        assert!(agent(command.clone()).is_err(), "{command:?}");
    }
    assert_eq!(
        agent(StrategyDeskCommand::ResumeAsset(asset("tick(4)"))),
        Ok(DeskOutcome::Changed(1)),
        "the script resumes, the order strategy stays paused"
    );
}
