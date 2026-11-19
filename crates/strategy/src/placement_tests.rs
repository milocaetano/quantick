//! The placement port: each placement's verdict on a trigger bar's body,
//! and the kernel reading it through the one gate the alarm and the order
//! share.

use core::str::FromStr;

use quantick_engine::{Bar, Side};
use quantick_sim::{Bracket, Command};
use rust_decimal::Decimal;

use crate::{
    Anywhere, ArmedStrategy, BreakPolicy, CrossDirection, Execution, ForceParams, ForceTrigger,
    LineCross, Opportunity, Placement, PriceRange, Reach, Rearm, Region, StrategyParams,
};

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).expect("fixture decimal")
}

/// A bar whose shadows run one point past its body, so only the body can
/// decide a verdict.
fn bar(open: &str, close: &str) -> Bar {
    let (open, close) = (dec(open), dec(close));
    Bar {
        open_time: 0,
        close_time: 0,
        open,
        high: open.max(close) + Decimal::ONE,
        low: open.min(close) - Decimal::ONE,
        close,
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

fn up(level: &str) -> LineCross {
    LineCross {
        level: dec(level),
        direction: CrossDirection::Up,
    }
}

fn down(level: &str) -> LineCross {
    LineCross {
        level: dec(level),
        direction: CrossDirection::Down,
    }
}

#[test]
fn the_rectangle_keeps_its_verdicts_and_its_words() {
    let region = Region::new(dec("100"), dec("110"));
    assert_eq!(region.reach(Side::Buy, &bar("95", "105")), Reach::Market);
    assert_eq!(
        region.reach(Side::Buy, &bar("105", "115")),
        Reach::Cut {
            edge: dec("110"),
            retest_off: "cut the region, retest option off",
        }
    );
    assert_eq!(
        region.reach(Side::Buy, &bar("111", "115")),
        Reach::Missed("the body never cut the region")
    );
    assert_eq!(
        region.reach(Side::Buy, &bar("105", "95")),
        Reach::Missed("closed outside region")
    );
}

#[test]
fn a_price_range_tests_like_a_band_and_names_itself() {
    let range = PriceRange::new(dec("110"), dec("100"));
    assert_eq!(range.band(), Region::new(dec("100"), dec("110")));
    assert_eq!(range.reach(Side::Sell, &bar("115", "105")), Reach::Market);
    assert_eq!(
        range.reach(Side::Sell, &bar("105", "95")),
        Reach::Cut {
            edge: dec("100"),
            retest_off: "cut the range, retest option off",
        }
    );
    assert_eq!(
        range.reach(Side::Sell, &bar("99", "95")),
        Reach::Missed("the body never cut the range")
    );
    assert_eq!(
        range.reach(Side::Sell, &bar("105", "115")),
        Reach::Missed("closed outside range")
    );
}

#[test]
fn an_upward_cross_needs_the_body_to_finish_above_the_line() {
    let line = up("100");
    assert_eq!(line.reach(Side::Buy, &bar("98", "102")), Reach::Market);
    // An open resting on the line counts as below it.
    assert_eq!(line.reach(Side::Buy, &bar("100", "102")), Reach::Market);
    // Opened above: travelled beyond a line it never crossed.
    assert_eq!(
        line.reach(Side::Buy, &bar("101", "104")),
        Reach::Missed("the body never crossed the line")
    );
    // A close on the line has not crossed it.
    assert_eq!(
        line.reach(Side::Buy, &bar("98", "100")),
        Reach::Missed("did not close beyond the line")
    );
    // The wrong direction: a body falling through the line.
    assert_eq!(
        line.reach(Side::Buy, &bar("102", "98")),
        Reach::Missed("did not close beyond the line")
    );
}

#[test]
fn a_downward_cross_mirrors_the_upward_one() {
    let line = down("100");
    assert_eq!(line.reach(Side::Sell, &bar("102", "98")), Reach::Market);
    assert_eq!(line.reach(Side::Sell, &bar("100", "98")), Reach::Market);
    assert_eq!(
        line.reach(Side::Sell, &bar("99", "96")),
        Reach::Missed("the body never crossed the line")
    );
    assert_eq!(
        line.reach(Side::Sell, &bar("102", "100")),
        Reach::Missed("did not close beyond the line")
    );
    assert_eq!(
        line.reach(Side::Sell, &bar("98", "102")),
        Reach::Missed("did not close beyond the line")
    );
}

/// The direction is the line's, not the side's: the same upward cross
/// answers a buy and a sell alike.
#[test]
fn a_cross_ignores_the_side_it_is_asked_for() {
    let line = up("100");
    let crossing = bar("98", "102");
    assert_eq!(
        line.reach(Side::Sell, &crossing),
        line.reach(Side::Buy, &crossing)
    );
}

#[test]
fn anywhere_enters_at_market_on_any_body() {
    for (open, close) in [("95", "105"), ("300", "200"), ("1", "1")] {
        assert_eq!(Anywhere.reach(Side::Buy, &bar(open, close)), Reach::Market);
        assert_eq!(Anywhere.reach(Side::Sell, &bar(open, close)), Reach::Market);
    }
}

fn instance(on_break: BreakPolicy) -> ArmedStrategy {
    let mut armed = ArmedStrategy::new(
        StrategyParams {
            side: Side::Buy,
            quantity: Decimal::ONE,
            tp_mult: Decimal::ONE,
            sl_mult: Decimal::ONE,
            rearm: Rearm::OneShot,
            on_break,
            execution: Execution::Paper,
        },
        Box::new(ForceTrigger::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: Decimal::ZERO,
        })),
    );
    armed.warm(&[bar("100", "101"), bar("101", "102")]);
    armed
}

/// Judge the body-4 buy force bar (102 to 106) through both readers: the
/// alarm's preview, then the order path. Answers what each said.
fn judge(
    placement: &dyn Placement,
    on_break: BreakPolicy,
) -> (
    Option<Opportunity>,
    Option<Opportunity>,
    Vec<Command>,
    ArmedStrategy,
) {
    let mut armed = instance(on_break);
    let force = bar("102", "106");
    let previewed = armed.preview_opportunity(&force, placement, true);
    let commands = armed.on_closed_bar(&force, placement, true, true);
    (previewed, armed.last_close_opportunity(), commands, armed)
}

/// Close 106, range 6 (high 107, low 101): TP 112, SL 100.
fn market_order() -> Vec<Command> {
    vec![Command::PlaceMarket {
        side: Side::Buy,
        quantity: Decimal::ONE,
        bracket: Bracket::whole(Some(dec("100")), Some(dec("112"))),
    }]
}

#[test]
fn every_placement_that_admits_the_bar_fires_where_its_alarm_rings() {
    let admitting: [&dyn Placement; 3] = [
        &PriceRange::new(dec("100"), dec("110")),
        &up("104"),
        &Anywhere,
    ];
    for placement in admitting {
        let (previewed, closed, commands, _) = judge(placement, BreakPolicy::Ignore);
        assert_eq!(previewed, Some(Opportunity::Market));
        assert_eq!(closed, previewed, "the alarm and the order read one gate");
        assert_eq!(commands, market_order());
    }
}

#[test]
fn every_placement_that_refuses_the_bar_is_silent_on_both_paths_and_says_why() {
    let refusing: [(&dyn Placement, &str); 4] = [
        (
            &PriceRange::new(dec("90"), dec("100")),
            "the body never cut the range",
        ),
        (
            &PriceRange::new(dec("110"), dec("120")),
            "closed outside range",
        ),
        (&down("104"), "did not close beyond the line"),
        // Opened at 102, already above the line: it never crossed.
        (&up("101"), "the body never crossed the line"),
    ];
    for (placement, reason) in refusing {
        let (previewed, closed, commands, armed) = judge(placement, BreakPolicy::Ignore);
        assert_eq!(previewed, None, "{reason}");
        assert_eq!(closed, None, "{reason}");
        assert!(commands.is_empty(), "{reason}");
        assert_eq!(armed.hold_reason().map(|hold| hold.reason), Some(reason));
    }
}

#[test]
fn a_cut_range_follows_the_break_policy_like_a_region() {
    let range = PriceRange::new(dec("100"), dec("104"));
    let (previewed, closed, commands, armed) = judge(&range, BreakPolicy::Ignore);
    assert_eq!((previewed, closed), (None, None));
    assert!(commands.is_empty());
    assert_eq!(
        armed.hold_reason().map(|hold| hold.reason),
        Some("cut the range, retest option off")
    );
    let (previewed, closed, _, _) = judge(&range, BreakPolicy::RetestLimit);
    let retest = Some(Opportunity::Retest { edge: dec("104") });
    assert_eq!((previewed, closed), (retest, retest));
}

/// The cross is the entry: no edge is left to retest, whatever the policy.
#[test]
fn a_line_cross_enters_at_market_even_with_the_retest_option_on() {
    let (previewed, closed, commands, _) = judge(&up("104"), BreakPolicy::RetestLimit);
    assert_eq!(
        (previewed, closed),
        (Some(Opportunity::Market), Some(Opportunity::Market))
    );
    assert_eq!(commands, market_order());
}

/// The time gate still runs first: an inactive placement holds fire on a
/// bar every placement would admit.
#[test]
fn an_inactive_placement_holds_fire_whatever_its_geometry() {
    let mut armed = instance(BreakPolicy::Ignore);
    let force = bar("102", "106");
    assert_eq!(armed.preview_opportunity(&force, &Anywhere, false), None);
    assert!(
        armed
            .on_closed_bar(&force, &Anywhere, false, true)
            .is_empty()
    );
    assert_eq!(
        armed.hold_reason().map(|hold| hold.reason),
        Some("region not active on this bar")
    );
}
