//! A Pine script armed as an alarm-only strategy: its `alertcondition`s are
//! the trigger, the title's first word is the side, and any placement the
//! kernel knows decides where the alarm may ring.

use quantick_engine::{Bar, Side};
use quantick_script_trigger::{
    ArmRefusal, ResolvedPlacement, Script, ScriptInstance, ScriptPlacement, ScriptState,
};
use quantick_strategy::{DisarmReason, Region};
use rust_decimal::Decimal;

const RENKO_POC_FLIP: &str = include_str!("../../app/scripts/renko_poc_flip.pine");

/// The drawing key a region rides; a test needs only to tell two apart.
type Drawing = u32;

fn dec(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn brick(index: i64, open: &str, close: &str) -> Bar {
    let (open, close) = (dec(open), dec(close));
    let t = 1_700_000_000_000 + index * 1_000;
    Bar {
        open_time: t,
        close_time: t + 999,
        open,
        high: open.max(close),
        low: open.min(close),
        close,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

/// A down brick, then an up brick whose POC sits above the first one's: the
/// fixture's "Bull flip" on the second close.
fn bull_flip() -> [(Bar, Option<Decimal>); 2] {
    [
        (brick(0, "101", "100"), Some(dec("100.5"))),
        (brick(1, "100", "101"), Some(dec("100.8"))),
    ]
}

fn arm(source: &str, side: Side, placement: ScriptPlacement<Drawing>) -> ScriptInstance<Drawing> {
    let script = Script::load("test.pine", source).expect("the script arms");
    ScriptInstance::arm(1, &script, side, placement, &[])
}

/// Feed one closed bar, resolving a region placement to `region`; answers
/// whether the alarm rang.
fn ring(
    instance: &mut ScriptInstance<Drawing>,
    (bar, poc): &(Bar, Option<Decimal>),
    region: Region,
) -> bool {
    let resolved = instance.placement.resolve(|_| Some((region, true)));
    instance
        .on_closed_bar(bar, *poc, resolved.as_ref(), 0)
        .is_some()
}

#[test]
fn the_fixture_rings_on_a_region_and_anywhere_on_its_flip() {
    let around_the_close = Region::new(dec("100.9"), dec("101.2"));
    let mut on_region = arm(RENKO_POC_FLIP, Side::Buy, ScriptPlacement::Region(7));
    let mut anywhere = arm(RENKO_POC_FLIP, Side::Buy, ScriptPlacement::Anywhere);
    let [down, up] = bull_flip();

    assert!(
        !ring(&mut on_region, &down, around_the_close),
        "no flip yet"
    );
    assert!(!ring(&mut anywhere, &down, around_the_close), "no flip yet");
    assert!(
        ring(&mut on_region, &up, around_the_close),
        "the flip closed inside the region"
    );
    assert!(
        ring(&mut anywhere, &up, around_the_close),
        "anywhere admits it"
    );
}

#[test]
fn a_region_the_flip_closed_outside_holds_the_alarm() {
    let far_below = Region::new(dec("90"), dec("95"));
    let mut instance = arm(RENKO_POC_FLIP, Side::Buy, ScriptPlacement::Region(7));
    let [down, up] = bull_flip();
    assert!(!ring(&mut instance, &down, far_below));
    assert!(
        !ring(&mut instance, &up, far_below),
        "the placement held it"
    );
    assert_eq!(instance.state(), ScriptState::Watching);
}

#[test]
fn a_flip_without_footprints_never_rings() {
    let mut instance = arm(RENKO_POC_FLIP, Side::Buy, ScriptPlacement::Anywhere);
    let anywhere = Region::new(Decimal::ZERO, Decimal::ONE);
    for (bar, _) in bull_flip() {
        assert!(
            !ring(&mut instance, &(bar, None), anywhere),
            "no POC, no flip"
        );
    }
}

#[test]
fn an_instance_hunting_the_other_side_stays_quiet() {
    let mut sell = arm(RENKO_POC_FLIP, Side::Sell, ScriptPlacement::Anywhere);
    let anywhere = Region::new(Decimal::ZERO, Decimal::ONE);
    let [down, up] = bull_flip();
    assert!(!ring(&mut sell, &down, anywhere));
    assert!(!ring(&mut sell, &up, anywhere), "a bull flip is not a sell");
}

#[test]
fn a_fixed_range_and_a_line_cross_gate_the_same_flip() {
    let unused = Region::new(Decimal::ZERO, Decimal::ONE);
    let [down, up] = bull_flip();
    let mut in_range = arm(
        RENKO_POC_FLIP,
        Side::Buy,
        ScriptPlacement::Range {
            a: dec("100.5"),
            b: dec("102"),
        },
    );
    let mut crossing = arm(
        RENKO_POC_FLIP,
        Side::Buy,
        ScriptPlacement::Line {
            level: dec("100.5"),
            rising: true,
        },
    );
    let mut wrong_way = arm(
        RENKO_POC_FLIP,
        Side::Buy,
        ScriptPlacement::Line {
            level: dec("100.5"),
            rising: false,
        },
    );
    for instance in [&mut in_range, &mut crossing, &mut wrong_way] {
        assert!(!ring(instance, &down, unused));
    }
    assert!(ring(&mut in_range, &up, unused), "closed inside the range");
    assert!(ring(&mut crossing, &up, unused), "crossed the line upward");
    assert!(!ring(&mut wrong_way, &up, unused), "the line wanted a fall");
}

#[test]
fn a_title_without_a_side_refuses_arming_and_names_it() {
    let source = "//@version=5\n\
        indicator(\"t\")\n\
        alertcondition(close > open, title=\"Bull flip\")\n\
        alertcondition(close < open, title=\"Momentum fade\")\n\
        plot(close)\n";
    let refusal = Script::load("t.pine", source).expect_err("one title has no side");
    assert_eq!(
        refusal,
        ArmRefusal::UnsidedTitle("Momentum fade".to_owned())
    );
    assert!(refusal.to_string().contains("`Momentum fade`"), "{refusal}");
}

#[test]
fn a_script_with_no_alertcondition_or_a_computed_title_refuses_arming() {
    let silent = "//@version=5\nindicator(\"t\")\nplot(close)\n";
    assert_eq!(
        Script::load("t.pine", silent).expect_err("nothing to ring on"),
        ArmRefusal::NoAlerts
    );
    let untitled = "//@version=5\nindicator(\"t\")\nalertcondition(close > open)\nplot(close)\n";
    assert_eq!(
        Script::load("t.pine", untitled).expect_err("no title, no side"),
        ArmRefusal::DefaultTitle
    );
    let computed = "//@version=5\n\
        indicator(\"t\")\n\
        name = close > open ? \"buy\" : \"sell\"\n\
        alertcondition(close != open, name)\n\
        plot(close)\n";
    assert_eq!(
        Script::load("t.pine", computed).expect_err("a title known only per bar"),
        ArmRefusal::ComputedTitle { line: 4 }
    );
}

#[test]
fn a_script_that_does_not_compile_refuses_with_line_and_code() {
    let source = "//@version=5\nindicator(\"t\")\nplot(nonsense_name)\n";
    let refusal = Script::load("t.pine", source).expect_err("an unknown name");
    let ArmRefusal::Compile(fault) = &refusal else {
        panic!("a compile refusal, got {refusal:?}");
    };
    assert_eq!(fault.line, 3);
    assert_eq!(fault.code, "PINE_UNKNOWN_NAME");
    assert!(
        fault.to_string().starts_with("line 3: PINE_UNKNOWN_NAME "),
        "{fault}"
    );
}

/// Rings `Buy now` on every up bar, and spends the loop budget on the third
/// bar it closes.
const BREAKS_ON_THE_THIRD_BAR: &str = "//@version=5\n\
    indicator(\"breaks\")\n\
    alertcondition(close > open, title=\"Buy now\")\n\
    x = 0.0\n\
    if bar_index == 2\n\
    \x20   while x >= 0\n\
    \x20       x := x + 1\n\
    plot(x)\n";

/// Rings `Long up` on every up bar, forever.
const ALWAYS_RINGS: &str = "//@version=5\n\
    indicator(\"steady\")\n\
    alertcondition(close > open, title=\"Long up\")\n\
    plot(close)\n";

#[test]
fn a_runtime_error_disarms_that_instance_only_with_line_and_code() {
    let unused = Region::new(Decimal::ZERO, Decimal::ONE);
    let mut broken = arm(
        BREAKS_ON_THE_THIRD_BAR,
        Side::Buy,
        ScriptPlacement::Anywhere,
    );
    let mut sibling = arm(ALWAYS_RINGS, Side::Buy, ScriptPlacement::Anywhere);
    let up = |index| (brick(index, "100", "101"), None);

    for index in 0..2 {
        assert!(
            ring(&mut broken, &up(index), unused),
            "rings before it breaks"
        );
        assert!(ring(&mut sibling, &up(index), unused));
    }
    assert!(
        !ring(&mut broken, &up(2), unused),
        "the failing bar rings nothing"
    );
    assert!(
        ring(&mut sibling, &up(2), unused),
        "the sibling never noticed"
    );

    let ScriptState::Disarmed { reason } = broken.state() else {
        panic!("the error disarms: {:?}", broken.state());
    };
    assert!(
        reason.starts_with("line 6: PINE_LOOP_BUDGET "),
        "the reason names line and code: {reason}"
    );
    for index in 3..5 {
        assert!(
            !ring(&mut broken, &up(index), unused),
            "disarmed stays quiet"
        );
        assert!(
            ring(&mut sibling, &up(index), unused),
            "the sibling rings on"
        );
    }
    assert_eq!(sibling.state(), ScriptState::Watching);
}

#[test]
fn an_error_during_warmup_disarms_at_arm_time() {
    let script = Script::load("breaks.pine", BREAKS_ON_THE_THIRD_BAR).expect("arms");
    let history: Vec<_> = (0..4)
        .map(|index| (brick(index, "100", "101"), None))
        .collect();
    let instance: ScriptInstance<Drawing> =
        ScriptInstance::arm(1, &script, Side::Buy, ScriptPlacement::Anywhere, &history);
    assert!(
        matches!(instance.state(), ScriptState::Disarmed { reason } if reason.starts_with("line 6:")),
        "{:?}",
        instance.state()
    );
}

#[test]
fn warmup_feeds_the_script_without_ringing_for_history() {
    let script = Script::load("steady.pine", ALWAYS_RINGS).expect("arms");
    let history: Vec<_> = (0..3)
        .map(|index| (brick(index, "100", "101"), None))
        .collect();
    let mut instance: ScriptInstance<Drawing> =
        ScriptInstance::arm(1, &script, Side::Buy, ScriptPlacement::Anywhere, &history);
    let unused = Region::new(Decimal::ZERO, Decimal::ONE);
    assert!(ring(&mut instance, &(brick(3, "100", "101"), None), unused));
}

#[test]
fn a_user_disarm_silences_the_instance_and_says_so() {
    let mut instance = arm(ALWAYS_RINGS, Side::Buy, ScriptPlacement::Anywhere);
    instance.disarm(DisarmReason::User);
    let unused = Region::new(Decimal::ZERO, Decimal::ONE);
    assert!(!ring(
        &mut instance,
        &(brick(0, "100", "101"), None),
        unused
    ));
    assert_eq!(
        instance.state(),
        ScriptState::Disarmed {
            reason: "disarmed".to_owned()
        }
    );
}

#[test]
fn a_region_placement_names_its_drawing_and_the_others_resolve_alone() {
    assert_eq!(ScriptPlacement::<Drawing>::Region(7).drawing(), Some(7));
    assert_eq!(ScriptPlacement::<Drawing>::Anywhere.drawing(), None);
    let unresolved = ScriptPlacement::<Drawing>::Region(7).resolve(|_| None);
    assert!(unresolved.is_none(), "a gone drawing resolves to nothing");
    let anywhere = ScriptPlacement::<Drawing>::Anywhere.resolve(|_| None);
    assert!(matches!(
        anywhere,
        Some((ResolvedPlacement::Anywhere, true))
    ));
}

#[test]
fn a_warmup_with_footprints_lets_the_first_live_brick_flip() {
    let script = Script::load("renko_poc_flip.pine", RENKO_POC_FLIP).expect("arms");
    let [down, up] = bull_flip();
    let unused = Region::new(Decimal::ZERO, Decimal::ONE);
    let mut warmed: ScriptInstance<Drawing> = ScriptInstance::arm(
        1,
        &script,
        Side::Buy,
        ScriptPlacement::Anywhere,
        std::slice::from_ref(&down),
    );
    assert!(
        !warmed.armed().status_line().contains("Bull flip"),
        "a fresh arm narrates no bar: {}",
        warmed.armed().status_line()
    );
    assert!(
        ring(&mut warmed, &up, unused),
        "the warm brick's POC is in its history"
    );

    let mut blind: ScriptInstance<Drawing> = ScriptInstance::arm(
        2,
        &script,
        Side::Buy,
        ScriptPlacement::Anywhere,
        &[(down.0.clone(), None)],
    );
    assert!(
        !ring(&mut blind, &up, unused),
        "a warmup without POC hides the flip"
    );
}

#[test]
fn a_flip_during_warmup_is_not_the_status() {
    let script = Script::load("renko_poc_flip.pine", RENKO_POC_FLIP).expect("arms");
    let instance: ScriptInstance<Drawing> = ScriptInstance::arm(
        1,
        &script,
        Side::Buy,
        ScriptPlacement::Anywhere,
        &bull_flip(),
    );
    let status = instance.armed().status_line();
    assert!(status.contains("waiting for bars"), "{status}");
}

#[test]
fn a_halted_instance_names_its_reason_and_stays_quiet() {
    let mut instance = arm(ALWAYS_RINGS, Side::Buy, ScriptPlacement::Anywhere);
    instance.halt("its region drawing was removed");
    instance.halt("a later reason");
    let unused = Region::new(Decimal::ZERO, Decimal::ONE);
    assert!(!ring(
        &mut instance,
        &(brick(0, "100", "101"), None),
        unused
    ));
    assert_eq!(
        instance.state(),
        ScriptState::Disarmed {
            reason: "its region drawing was removed".to_owned()
        }
    );
}
