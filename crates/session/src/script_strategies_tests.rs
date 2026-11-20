//! Script strategies on a tab's panes, armed from their source and run
//! through the same sweep as the drawn instances.

use quantick_engine::{Bar, Side};
use quantick_strategy::Region;
use rust_decimal::Decimal;

use crate::script_strategies::{ScriptArm, ScriptPlacement, ScriptReading};
use crate::strategies::{DRAWING_REMOVED, LiveClose, Print, TabStrategies};
use crate::strategies_tests::{DrawingId, RecordingPort};

const PANE: u64 = 4;

const RENKO_POC_FLIP: &str = include_str!("../../app/scripts/renko_poc_flip.pine");

const BREAKS_ON_THE_THIRD_BAR: &str = "//@version=5\n\
    indicator(\"breaks\")\n\
    alertcondition(close > open, title=\"Buy now\")\n\
    x = 0.0\n\
    if bar_index == 2\n\
    \x20   while x >= 0\n\
    \x20       x := x + 1\n\
    plot(x)\n";

const ALWAYS_RINGS: &str = "//@version=5\n\
    indicator(\"steady\")\n\
    alertcondition(close > open, title=\"Long up\")\n\
    plot(close)\n";

fn dec(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn close(index: i64, open: &str, close: &str, poc: Option<&str>) -> LiveClose {
    let (open, close) = (dec(open), dec(close));
    LiveClose {
        bar: Bar {
            open_time: index * 1_000,
            close_time: index * 1_000 + 999,
            open,
            high: open.max(close),
            low: open.min(close),
            close,
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        },
        poc: poc.map(dec),
        slot: usize::try_from(index).expect("small"),
    }
}

fn arm_side(
    strategies: &mut TabStrategies<DrawingId>,
    (script, source): (&str, &str),
    side: Side,
    place: ScriptPlacement<DrawingId>,
    warmup: Vec<(Bar, Option<Decimal>)>,
) -> Result<ScriptReading<DrawingId>, String> {
    let request = ScriptArm {
        pane_id: PANE,
        script,
        source,
        side,
        place,
    };
    strategies.arm_script(request, |_| warmup)
}

fn arm(
    strategies: &mut TabStrategies<DrawingId>,
    script: (&str, &str),
    place: ScriptPlacement<DrawingId>,
) -> Result<ScriptReading<DrawingId>, String> {
    arm_side(strategies, script, Side::Buy, place, Vec::new())
}

const FLIP: (&str, &str) = ("renko_poc_flip.pine", RENKO_POC_FLIP);

/// Queue `closes` on the pane and sweep, the region of drawing 3 sitting
/// around the flip's close; answers how many alarms rang.
fn sweep(
    strategies: &mut TabStrategies<DrawingId>,
    closes: &[LiveClose],
    drawing_lives: bool,
) -> usize {
    for close in closes {
        strategies.queue_closed(PANE, &close.bar, close.poc, close.slot, true);
    }
    let region = Region::new(dec("100.9"), dec("101.2"));
    strategies
        .evaluate(
            &[PANE],
            &mut RecordingPort::default(),
            Print {
                events: &[],
                now_ms: 0,
            },
            |_, drawing| drawing_lives && drawing == DrawingId(3),
            |_, drawing, _| (drawing == DrawingId(3)).then_some((region, true)),
            |_| None,
        )
        .alarms
        .len()
}

#[test]
fn the_bundled_flip_rings_through_the_sweep_on_a_region_and_anywhere() {
    let mut strategies = TabStrategies::default();
    let region = arm(&mut strategies, FLIP, ScriptPlacement::Region(DrawingId(3)));
    let anywhere = arm(&mut strategies, FLIP, ScriptPlacement::Anywhere);
    assert_eq!(region.map(|reading| reading.id), Ok(1));
    assert_eq!(anywhere.map(|reading| reading.id), Ok(2));
    assert!(strategies.any_alarm(), "a watching script carries an alarm");

    let down = close(0, "101", "100", Some("100.5"));
    assert_eq!(sweep(&mut strategies, &[down], true), 0, "no flip yet");
    let up = close(1, "100", "101", Some("100.8"));
    assert_eq!(
        sweep(&mut strategies, &[up], true),
        2,
        "both placements ring the flip"
    );
}

#[test]
fn a_footprint_warmup_rings_a_flip_on_the_first_live_brick() {
    let mut strategies = TabStrategies::default();
    let down = close(0, "101", "100", Some("100.5"));
    let warmup = vec![(down.bar, down.poc)];
    arm_side(
        &mut strategies,
        FLIP,
        Side::Buy,
        ScriptPlacement::Anywhere,
        warmup,
    )
    .expect("arms");
    let up = close(1, "100", "101", Some("100.8"));
    assert_eq!(
        sweep(&mut strategies, &[up], true),
        1,
        "the warm POC is history"
    );
}

#[test]
fn a_failing_script_disarms_alone_and_its_sibling_keeps_ringing() {
    let mut strategies = TabStrategies::default();
    let breaks = ("breaks.pine", BREAKS_ON_THE_THIRD_BAR);
    let steady = ("steady.pine", ALWAYS_RINGS);
    arm(&mut strategies, breaks, ScriptPlacement::Anywhere).expect("arms");
    arm(&mut strategies, steady, ScriptPlacement::Anywhere).expect("arms");

    let up = |index| close(index, "100", "101", None);
    assert_eq!(
        sweep(&mut strategies, &[up(0), up(1)], true),
        4,
        "both ring twice"
    );
    assert_eq!(
        sweep(&mut strategies, &[up(2)], true),
        1,
        "only the sibling rings"
    );
    let readings = strategies.script_readings();
    let reason = readings[0]
        .disarm_reason
        .as_deref()
        .expect("the failure disarmed it");
    assert!(reason.starts_with("line 6: PINE_LOOP_BUDGET "), "{reason}");
    assert_eq!(readings[1].disarm_reason, None, "{:?}", readings[1]);
    assert_eq!(
        sweep(&mut strategies, &[up(3)], true),
        1,
        "the sibling rings on"
    );
}

#[test]
fn arming_refuses_a_script_with_no_alert_and_a_side_no_title_names() {
    let mut strategies = TabStrategies::default();
    let rsi = ("rsi.pine", include_str!("../../app/scripts/rsi.pine"));
    let silent = arm(&mut strategies, rsi, ScriptPlacement::Anywhere);
    assert!(
        silent.is_err_and(|refusal| refusal.contains("`rsi.pine` cannot be armed")),
        "an indicator with no alertcondition never rings"
    );
    let steady = ("steady.pine", ALWAYS_RINGS);
    let sell = arm_side(
        &mut strategies,
        steady,
        Side::Sell,
        ScriptPlacement::Anywhere,
        Vec::new(),
    );
    assert_eq!(
        sell,
        Err(
            "`steady.pine` cannot be armed to sell: none of its alert titles (`Long up`) \
             names that side"
                .to_owned()
        )
    );
    assert!(strategies.script_readings().is_empty(), "nothing armed");
    assert!(!strategies.any_armed());
}

#[test]
fn a_disarm_removes_one_instance_by_id_and_answers_it_disarmed() {
    let mut strategies = TabStrategies::default();
    let range = ScriptPlacement::Range {
        a: dec("102"),
        b: dec("100"),
    };
    let line = ScriptPlacement::Line {
        level: dec("100.5"),
        rising: true,
    };
    arm(&mut strategies, FLIP, range).expect("arms");
    arm(&mut strategies, FLIP, line).expect("arms");
    let gone = strategies.disarm_script(1).expect("instance 1 was armed");
    assert_eq!(gone.place, range);
    assert_eq!(gone.disarm_reason.as_deref(), Some("disarmed"));
    assert_eq!(strategies.disarm_script(1), None, "already gone");
    let left: Vec<_> = (strategies.script_readings().into_iter())
        .map(|reading| reading.id)
        .collect();
    assert_eq!(left, [2]);
    assert_eq!(strategies.script_readings()[0].place, line);
}

#[test]
fn a_region_script_stops_with_its_drawing_and_says_why() {
    let mut strategies = TabStrategies::default();
    arm(&mut strategies, FLIP, ScriptPlacement::Region(DrawingId(3))).expect("arms");
    arm(&mut strategies, FLIP, ScriptPlacement::Anywhere).expect("arms");
    sweep(&mut strategies, &[close(0, "101", "100", None)], false);
    let reasons: Vec<_> = (strategies.script_readings().into_iter())
        .map(|reading| reading.disarm_reason)
        .collect();
    assert_eq!(reasons, [Some(DRAWING_REMOVED.to_owned()), None]);

    let mut strategies = TabStrategies::default();
    arm(&mut strategies, FLIP, ScriptPlacement::Region(DrawingId(3))).expect("arms");
    strategies.pane_mut(PANE).remove_for_drawing(DrawingId(3));
    let reading = &strategies.script_readings()[0];
    assert_eq!(reading.disarm_reason.as_deref(), Some(DRAWING_REMOVED));
    assert!(
        !strategies.any_armed(),
        "a stopped script keeps nothing listening"
    );
    assert!(!strategies.any_alarm());
}

#[test]
fn a_refilled_close_warms_the_script_so_its_next_verdict_matches_a_full_run() {
    let down = close(0, "101", "100", Some("100.5"));
    let up = close(1, "100", "101", Some("100.8"));
    let mut heard = TabStrategies::default();
    arm(&mut heard, FLIP, ScriptPlacement::Anywhere).expect("arms");
    let mut refilled = TabStrategies::default();
    arm(&mut refilled, FLIP, ScriptPlacement::Anywhere).expect("arms");

    assert_eq!(sweep(&mut heard, std::slice::from_ref(&down), true), 0);
    refilled.queue_closed(PANE, &down.bar, down.poc, down.slot, false);
    assert_eq!(
        sweep(&mut refilled, &[], true),
        0,
        "a refilled close rings nothing"
    );

    let rang = |strategies: &mut TabStrategies<DrawingId>| {
        sweep(strategies, std::slice::from_ref(&up), true)
    };
    assert_eq!(
        rang(&mut heard),
        1,
        "a run that saw every bar rings the flip"
    );
    assert_eq!(
        rang(&mut refilled),
        1,
        "so does one that saw the hole refilled"
    );
    assert_eq!(
        heard.script_readings()[0].status,
        refilled.script_readings()[0].status
    );
}

#[test]
fn a_refilled_close_reaches_no_drawn_instance_and_is_not_queued_without_a_script() {
    let mut strategies = TabStrategies::default();
    strategies
        .pane_mut(PANE)
        .anchors
        .arm(crate::strategies_tests::instance(DrawingId(3)));
    let down = close(0, "101", "100", None);
    strategies.queue_closed(PANE, &down.bar, down.poc, down.slot, false);
    let pending = strategies.pane(PANE).map(|pane| pane.pending().len());
    assert_eq!(pending, Some(0), "only a watching script hears a refill");
}
