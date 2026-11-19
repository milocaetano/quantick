//! Script strategies on a tab's panes, armed from a library folder and run
//! through the same sweep as the drawn instances.

use quantick_engine::{Bar, Side};
use quantick_strategy::Region;
use rust_decimal::Decimal;

use crate::script_strategies::{ScriptArm, ScriptPlace, ScriptReading, library_scripts};
use crate::strategies::{LiveClose, Print, TabStrategies};
use crate::strategies_tests::{DrawingId, RecordingPort};

#[path = "../../stores/src/scratch.rs"]
#[allow(dead_code)]
mod scratch;

const PANE: u64 = 4;

/// The bundled scripts, `renko_poc_flip.pine` among them.
const BUNDLED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../app/scripts");

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

fn arm(
    strategies: &mut TabStrategies<DrawingId>,
    library: &str,
    script: &str,
    place: ScriptPlace<DrawingId>,
) -> Result<ScriptReading<DrawingId>, String> {
    let request = ScriptArm {
        pane_id: PANE,
        library: Some(library),
        script,
        side: Side::Buy,
        place,
    };
    strategies.arm_script(request, |_| Vec::new())
}

/// Queue `closes` on the pane and sweep, the region of drawing 3 sitting
/// around the flip's close; answers how many alarms rang.
fn sweep(
    strategies: &mut TabStrategies<DrawingId>,
    closes: &[LiveClose],
    drawing_lives: bool,
) -> usize {
    for close in closes {
        strategies.queue_closed(PANE, &close.bar, close.poc, close.slot);
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
    let region = arm(
        &mut strategies,
        BUNDLED,
        "renko_poc_flip.pine",
        ScriptPlace::Region(DrawingId(3)),
    );
    let anywhere = arm(
        &mut strategies,
        BUNDLED,
        "renko_poc_flip.pine",
        ScriptPlace::Anywhere,
    );
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
fn a_failing_script_disarms_alone_and_its_sibling_keeps_ringing() {
    let library = scratch::ScratchDir::new("session-script-strategies");
    std::fs::write(library.path().join("breaks.pine"), BREAKS_ON_THE_THIRD_BAR).expect("writable");
    std::fs::write(library.path().join("steady.pine"), ALWAYS_RINGS).expect("writable");
    let folder = library.path().to_str().expect("a unicode scratch path");
    let mut strategies = TabStrategies::default();
    arm(
        &mut strategies,
        folder,
        "breaks.pine",
        ScriptPlace::Anywhere,
    )
    .expect("arms");
    arm(
        &mut strategies,
        folder,
        "steady.pine",
        ScriptPlace::Anywhere,
    )
    .expect("arms");
    assert_eq!(
        library_scripts(Some(folder)),
        Ok(vec!["breaks.pine".to_owned(), "steady.pine".to_owned()])
    );

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
    assert!(!readings[0].watching);
    assert!(readings[1].watching, "{:?}", readings[1]);
    assert_eq!(
        sweep(&mut strategies, &[up(3)], true),
        1,
        "the sibling rings on"
    );
}

#[test]
fn arming_refuses_an_unreadable_name_and_an_unsided_title() {
    let mut strategies = TabStrategies::default();
    let missing = arm(
        &mut strategies,
        BUNDLED,
        "absent.pine",
        ScriptPlace::Anywhere,
    );
    assert!(missing.is_err_and(|refusal| refusal.contains("absent.pine")));
    let escape = arm(
        &mut strategies,
        BUNDLED,
        "../scripts/rsi.pine",
        ScriptPlace::Anywhere,
    );
    assert!(escape.is_err_and(|refusal| refusal.contains("bare file name")));
    let silent = arm(&mut strategies, BUNDLED, "rsi.pine", ScriptPlace::Anywhere);
    assert!(
        silent.is_err_and(|refusal| refusal.contains("`rsi.pine` cannot be armed")),
        "an indicator with no alertcondition never rings"
    );
    assert!(strategies.script_readings().is_empty(), "nothing armed");
    assert!(!strategies.any_armed());
}

#[test]
fn a_disarm_removes_one_instance_by_id() {
    let mut strategies = TabStrategies::default();
    let range = ScriptPlace::Range {
        a: dec("102"),
        b: dec("100"),
    };
    let line = ScriptPlace::Line {
        level: dec("100.5"),
        rising: true,
    };
    arm(&mut strategies, BUNDLED, "renko_poc_flip.pine", range).expect("arms");
    arm(&mut strategies, BUNDLED, "renko_poc_flip.pine", line).expect("arms");
    let gone = strategies.disarm_script(1).expect("instance 1 was armed");
    assert_eq!(
        gone.place,
        ScriptPlace::Range {
            a: dec("100"),
            b: dec("102")
        },
        "a band reads back low to high"
    );
    assert_eq!(strategies.disarm_script(1), None, "already gone");
    let left: Vec<_> = strategies
        .script_readings()
        .into_iter()
        .map(|reading| reading.id)
        .collect();
    assert_eq!(left, [2]);
    assert_eq!(strategies.script_readings()[0].place, line);
}

#[test]
fn a_region_script_dies_with_its_drawing() {
    let mut strategies = TabStrategies::default();
    arm(
        &mut strategies,
        BUNDLED,
        "renko_poc_flip.pine",
        ScriptPlace::Region(DrawingId(3)),
    )
    .expect("arms");
    arm(
        &mut strategies,
        BUNDLED,
        "renko_poc_flip.pine",
        ScriptPlace::Anywhere,
    )
    .expect("arms");
    sweep(&mut strategies, &[close(0, "101", "100", None)], false);
    let left: Vec<_> = strategies
        .script_readings()
        .into_iter()
        .map(|reading| reading.place)
        .collect();
    assert_eq!(
        left,
        [ScriptPlace::Anywhere],
        "the region's drawing is gone"
    );
}
