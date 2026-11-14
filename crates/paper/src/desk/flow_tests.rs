//! The desk's gestures carried end to end: a frame in, the command a press
//! asked for carried out through the account, and the order, leg or position
//! it produced read back.
//!
//! Moved here from `app`'s window tests, which drove the same desk through
//! window-typed input. The geometry is the one `tests` uses: an 800×400
//! chart over 80..120, so price 100 sits at y 200 and every 10 px is a point.

use quantick_engine::Side;
use quantick_trading::{Bracket, BracketTarget, Currency, EntryKind, InstrumentMoney, MoneySource};
use rust_decimal::Decimal;

use super::cmd::{CmdEntryKind, CmdPreviewForce, CmdTradingSettings, HeldKeys};
use super::geometry::{
    Point, PriceAxis, bracket_handle_rect, clamp_tag_center, close_button_rect, handles_visible,
};
use super::gesture::{
    ArmedPlacement, ChartCommand, ChartFrame, CursorHint, PaperControl, PaperDrag,
};
use super::tests::{LinearAxis, chart, frame, held, marked_account, print};
use super::{Desk, line_at};
use crate::PaperAccount;
use crate::account::Leg;
use crate::order_strategies::{OrderStrategy, StrategyRow};
use crate::risk_sizing::{InstrumentBook, RiskBasis, RiskSettings};
use crate::scratch::ScratchDir;

/// One frame the way the host runs it: the desk decides, the command a
/// press asked for goes through the account's funnel, and the ticket's
/// complaint lands where the host would toast it. Answers whether the desk
/// owned the gesture.
fn drive(desk: &mut Desk, account: &mut PaperAccount, frame: &ChartFrame) -> bool {
    let outcome = desk.handle_input(account, frame, Some(&LinearAxis));
    if let Some(command) = outcome.command
        && let Err(message) = desk.carry_out(account, command)
    {
        account.set_toast(message);
    }
    outcome.owned
}

/// Mid-chart at `y`, where nothing right-anchored lives.
fn at(y: f32) -> Point {
    Point::new(400.0, y)
}

/// The aim's key held at `pointer`, with `scroll_y` of wheel travel.
fn rolled(pointer: Point, scroll_y: f32) -> ChartFrame {
    ChartFrame {
        scroll_y,
        ..frame(pointer, true, false)
    }
}

/// `keys` held at `pointer`, pressing or not.
fn keyed(pointer: Point, keys: HeldKeys, pressed: bool) -> ChartFrame {
    ChartFrame {
        keys,
        ..frame(pointer, false, pressed)
    }
}

const ALT: HeldKeys = HeldKeys {
    shift: false,
    command: false,
    alt: true,
};

/// Press at `from`, hold to `to`, let go there.
fn drag(desk: &mut Desk, account: &mut PaperAccount, from: f32, to: f32) {
    assert!(
        drive(desk, account, &frame(at(from), false, true)),
        "the press at y {from} is paper's"
    );
    assert!(drive(desk, account, &held(at(to), true, false)));
    assert!(drive(desk, account, &held(at(to), false, true)));
}

/// A market entry the way the ticket's button takes one: the offsets read
/// around the mark, and the desk's ruler and form handed to the account.
fn market(desk: &Desk, account: &mut PaperAccount, side: Side) -> bool {
    let reference = account.mark_price().unwrap_or_default();
    let ticket = desk
        .ticket
        .parse_bracket(side, reference)
        .expect("the offsets read");
    let env = desk.account_env(account, side, reference);
    account.market(side, reference, ticket, &env)
}

/// A long opened at 100 by the next print, protected by the ticket's
/// offsets.
fn long_at_100(desk: &Desk, account: &mut PaperAccount) {
    assert!(market(desk, account, Side::Buy));
    account.on_trade(&print(1, 100));
    assert!(account.venue().position().is_some(), "the long filled");
}

fn rest(desk: &mut Desk, account: &mut PaperAccount, kind: EntryKind, raw_price: f64) {
    desk.carry_out(
        account,
        ChartCommand::PlaceResting {
            side: Side::Buy,
            kind,
            raw_price,
        },
    )
    .expect("the offsets read");
}

/// The trader's own two-rung ladder: half at +8/-4, half at +2/-5.
fn halves() -> OrderStrategy {
    OrderStrategy {
        name: "halves".to_owned(),
        rows: vec![
            StrategyRow {
                share_percent: Decimal::from(50),
                gain_ticks: Some(8),
                loss_ticks: Some(4),
            },
            StrategyRow {
                share_percent: Decimal::from(50),
                gain_ticks: Some(2),
                loss_ticks: Some(5),
            },
        ],
    }
}

fn aimed_bracket(desk: &Desk) -> Bracket {
    desk.gesture.cmd_preview.expect("the aim is up").bracket
}

/// The headline gesture: a working order offers labelled handles, pressing
/// one and dragging sets that leg on the order, and the fill opens the
/// position already protected. The handles draw only on the pane with the
/// hand.
#[test]
fn a_working_orders_handle_sets_a_stop_that_arms_on_the_fill() {
    let dir = ScratchDir::new("flow-order-handle");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    rest(&mut desk, &mut account, EntryKind::Limit, 95.0);
    let id = account.working_orders()[0].id;

    let center = clamp_tag_center(LinearAxis.y(95.0), 0.0, 400.0);
    let handle = bracket_handle_rect(chart().right(), center, false).center();
    assert_eq!(
        desk.gesture
            .control_at(&account, handle, chart(), &LinearAxis),
        Some(PaperControl::Handle {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        }),
        "a buy's stop handle sits below its line"
    );
    assert!(!handles_visible(None, true, false), "no hand, no handles");
    assert!(handles_visible(Some(handle), true, false));
    assert!(handles_visible(Some(handle), false, true));

    drive(&mut desk, &mut account, &frame(handle, false, true));
    assert_eq!(
        desk.gesture.drag,
        PaperDrag::CreateLeg {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        },
        "the press pulls a leg out of the order, not the position"
    );
    drive(&mut desk, &mut account, &held(at(300.0), true, false));
    drive(&mut desk, &mut account, &held(at(300.0), false, true));
    assert_eq!(
        account.working_orders()[0].bracket.stop_loss(),
        Some(Decimal::from(90))
    );
    assert!(account.venue().position().is_none(), "and opened nothing");

    account.on_trade(&print(1, 95));
    assert_eq!(
        account
            .venue()
            .position()
            .expect("the limit filled")
            .stop_loss,
        Some(Decimal::from(90)),
        "the leg armed itself on the fill"
    );
}

/// The position's own handle births its stop the same way.
#[test]
fn a_positions_handle_births_its_stop() {
    let dir = ScratchDir::new("flow-position-handle");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    long_at_100(&desk, &mut account);
    let center = clamp_tag_center(LinearAxis.y(100.0), 0.0, 400.0);
    let handle = bracket_handle_rect(chart().right(), center, false).center();

    assert!(drive(&mut desk, &mut account, &frame(handle, false, true)));
    assert!(drive(
        &mut desk,
        &mut account,
        &held(at(300.0), true, false)
    ));
    assert!(drive(
        &mut desk,
        &mut account,
        &held(at(300.0), false, true)
    ));
    assert_eq!(
        account.venue().position().expect("long").stop_loss,
        Some(Decimal::from(90))
    );
}

/// A leg that exists is its own handle: its line is grabbable, and the
/// cross its opened tag offers clears that leg and leaves the other.
#[test]
fn a_working_orders_leg_is_grabbed_and_its_cross_clears_only_that_leg() {
    let dir = ScratchDir::new("flow-order-legs");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    desk.ticket.stop_offset_text = "5".to_owned();
    desk.ticket.profit_offset_text = "15".to_owned();
    rest(&mut desk, &mut account, EntryKind::Limit, 95.0);
    let id = account.working_orders()[0].id;
    assert_eq!(
        account.working_orders()[0].bracket,
        Bracket::whole(Some(Decimal::from(90)), Some(Decimal::from(110))),
        "the ticket's offsets rode along"
    );
    assert_eq!(
        line_at(&account, at(300.0), &LinearAxis),
        Some(PaperDrag::Leg {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        })
    );

    let cross = close_button_rect(chart().right(), clamp_tag_center(300.0, 0.0, 400.0)).center();
    assert_eq!(
        desk.gesture
            .control_at(&account, cross, chart(), &LinearAxis),
        None,
        "a resting pill has no cross"
    );
    drive(&mut desk, &mut account, &frame(cross, false, false));
    assert_eq!(
        desk.gesture
            .control_at(&account, cross, chart(), &LinearAxis),
        Some(PaperControl::ClearLeg {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        }),
        "reaching the row opens it"
    );
    assert!(drive(&mut desk, &mut account, &frame(cross, false, true)));
    assert_eq!(
        account.working_orders()[0].bracket,
        Bracket::whole(None, Some(Decimal::from(110))),
        "clearing one leg never drops the other"
    );
}

/// Ask for a stop where only a limit can rest and the aim stands down, and
/// its press places nothing.
#[test]
fn a_stated_kind_that_cannot_rest_there_aims_and_places_nothing() {
    let dir = ScratchDir::new("flow-stated-kind");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();

    drive(&mut desk, &mut account, &frame(at(250.0), true, false));
    assert_eq!(
        desk.gesture.cmd_preview.map(|preview| preview.kind),
        Some(EntryKind::Limit),
        "auto offers the kind that rests at 95"
    );
    desk.cmd_trading.kind = CmdEntryKind::Stop;
    drive(&mut desk, &mut account, &frame(at(250.0), true, false));
    assert_eq!(desk.gesture.cmd_preview, None);
    assert!(!drive(
        &mut desk,
        &mut account,
        &frame(at(250.0), true, true)
    ));
    assert!(account.working_orders().is_empty());
}

/// The held keys pick the side, two at once or none aim nothing, Ctrl
/// arms no sell by default, the press rests what the aim showed, and the
/// toggle hides the gesture.
#[test]
fn the_held_keys_pick_the_side_and_the_toggle_hides_the_aim() {
    let dir = ScratchDir::new("flow-keys");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    let above = Point::new(400.0, 100.0);
    let side_kind = |desk: &Desk| {
        desk.gesture
            .cmd_preview
            .map(|preview| (preview.side, preview.kind))
    };

    drive(&mut desk, &mut account, &frame(above, true, false));
    let preview = desk.gesture.cmd_preview.expect("a buy aims above");
    assert_eq!((preview.side, preview.kind), (Side::Buy, EntryKind::Stop));
    assert_eq!(preview.price, Decimal::from(110));
    drive(&mut desk, &mut account, &frame(at(300.0), true, false));
    assert_eq!(side_kind(&desk), Some((Side::Buy, EntryKind::Limit)));
    drive(&mut desk, &mut account, &keyed(above, ALT, false));
    assert_eq!(side_kind(&desk), Some((Side::Sell, EntryKind::Limit)));
    let both = HeldKeys { shift: true, ..ALT };
    drive(&mut desk, &mut account, &keyed(above, both, false));
    assert_eq!(side_kind(&desk), None, "both keys are ambiguous");
    drive(&mut desk, &mut account, &frame(above, false, false));
    assert_eq!(side_kind(&desk), None, "no key, no aim");
    let ctrl = HeldKeys {
        command: true,
        ..HeldKeys::default()
    };
    drive(&mut desk, &mut account, &keyed(above, ctrl, false));
    assert_eq!(side_kind(&desk), None, "Ctrl is bound to nothing");

    assert!(drive(
        &mut desk,
        &mut account,
        &frame(Point::new(120.0, 300.0), true, true)
    ));
    let orders = account.working_orders();
    assert_eq!(orders.len(), 1);
    assert_eq!(
        (orders[0].side, orders[0].price),
        (Side::Buy, Some(Decimal::from(90)))
    );
    assert!(
        !drive(&mut desk, &mut account, &frame(above, false, true)),
        "a bare click is the chart's"
    );
    assert_eq!(account.working_orders().len(), 1);

    desk.set_cmd_trading(CmdTradingSettings {
        enabled: false,
        ..CmdTradingSettings::default()
    });
    drive(&mut desk, &mut account, &frame(above, true, false));
    assert_eq!(side_kind(&desk), None, "the toggle hides the gesture");
}

/// The capture hook's stated x wins over the real pointer, whose y still
/// sets the price; with no x the pointer is left alone.
#[test]
fn a_forced_aim_parks_its_x_and_reads_its_price_from_the_hand() {
    let dir = ScratchDir::new("flow-forced-x");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    let hand = Point::new(700.0, 100.0);
    desk.gesture.cmd_preview_force = Some(CmdPreviewForce {
        side: Side::Sell,
        x_fraction: Some(0.25),
    });
    drive(&mut desk, &mut account, &frame(hand, false, false));
    let preview = desk.gesture.cmd_preview.expect("the hook aims");
    assert_eq!(preview.side, Side::Sell);
    assert_eq!(preview.pointer, Point::new(200.0, 100.0));

    desk.gesture.cmd_preview_force = Some(CmdPreviewForce {
        side: Side::Sell,
        x_fraction: None,
    });
    drive(&mut desk, &mut account, &frame(hand, false, false));
    assert_eq!(desk.gesture.cmd_preview.expect("forced").pointer, hand);
}

/// The ladder the aim projects is the bracket the press rests - at the
/// ticket's default size of one too.
#[test]
fn a_ladder_is_projected_and_placed_as_one_bracket() {
    let dir = ScratchDir::new("flow-ladder");
    let mut account = marked_account(&dir);
    account.set_order_strategies(vec![halves()], Some("halves"));
    let mut desk = Desk::default();
    let aim = at(250.0);

    assert_eq!(desk.ticket.qty_text, "1");
    drive(&mut desk, &mut account, &frame(aim, true, false));
    assert!(
        !aimed_bracket(&desk).is_empty(),
        "one contract still projects the ladder"
    );

    desk.ticket.qty_text = "2".to_owned();
    drive(&mut desk, &mut account, &frame(aim, true, false));
    let projected = aimed_bracket(&desk);
    let parts: Vec<_> = projected.parts().copied().collect();
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert_eq!(parts[0].quantity, Some(Decimal::ONE));
    assert_eq!(parts[0].take_profit, Some(Decimal::from(103)));
    assert_eq!(parts[0].stop_loss, Some(Decimal::from(91)));
    assert_eq!(parts[1].take_profit, Some(Decimal::from(97)));
    assert_eq!(parts[1].stop_loss, Some(Decimal::from(90)));

    assert!(drive(&mut desk, &mut account, &frame(aim, true, true)));
    assert_eq!(account.working_orders()[0].bracket, projected);
}

/// Selecting a ladder puts it on the aim and `<None>` takes it away; the
/// wheel takes over the aim while it is wound and hands it back at zero.
#[test]
fn the_ruler_and_the_ladder_take_turns_on_the_aim() {
    let dir = ScratchDir::new("flow-ladder-ruler");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    desk.ticket.qty_text = "2".to_owned();
    let aim = at(250.0);

    account.set_order_strategies(vec![halves()], None);
    drive(&mut desk, &mut account, &frame(aim, true, false));
    assert!(aimed_bracket(&desk).is_empty(), "no strategy, a bare aim");
    account.set_order_strategies(vec![halves()], Some("halves"));
    drive(&mut desk, &mut account, &frame(aim, true, false));
    assert_eq!(aimed_bracket(&desk).parts().count(), 2);

    for _ in 0..3 {
        drive(&mut desk, &mut account, &rolled(aim, 40.0));
    }
    assert!(desk.gesture.scroll_consumed);
    assert_eq!(desk.ruler.notches, 3, "the wheel works with a ladder armed");
    assert_eq!(aimed_bracket(&desk).stop_loss(), Some(Decimal::from(92)));
    assert_eq!(aimed_bracket(&desk).take_profit(), Some(Decimal::from(98)));
    for _ in 0..3 {
        drive(&mut desk, &mut account, &rolled(aim, -40.0));
    }
    assert_eq!(
        aimed_bracket(&desk).parts().count(),
        2,
        "the ladder is back"
    );

    account.set_order_strategies(vec![halves()], None);
    drive(&mut desk, &mut account, &frame(aim, true, false));
    assert!(
        aimed_bracket(&desk).is_empty(),
        "and `<None>` takes it away"
    );
}

/// A ladder that cannot resolve projects nothing, so the ticket must name
/// why.
#[test]
fn an_unusable_ladder_projects_nothing_and_has_a_reason() {
    let mut broken = halves();
    broken.rows.push(StrategyRow {
        share_percent: Decimal::ZERO,
        gain_ticks: Some(20),
        loss_ticks: Some(20),
    });
    let error = broken.validate().expect_err("a zero share is not a share");
    assert!(!error.advice().is_empty());

    let dir = ScratchDir::new("flow-ladder-broken");
    let mut account = marked_account(&dir);
    account.set_order_strategies(vec![broken], Some("halves"));
    let mut desk = Desk::default();
    drive(&mut desk, &mut account, &frame(at(250.0), true, false));
    assert!(aimed_bracket(&desk).is_empty());
}

/// The ticket's market button honours the armed ladder, and the position
/// it opens reads as laddered: no single stop, and no create-handle a drag
/// would replace every rung through.
#[test]
fn a_market_entry_under_a_ladder_arms_every_rung_and_offers_no_handle() {
    let dir = ScratchDir::new("flow-ladder-market");
    let mut account = marked_account(&dir);
    account.set_order_strategies(vec![halves()], Some("halves"));
    let mut desk = Desk::default();
    desk.ticket.qty_text = "2".to_owned();
    long_at_100(&desk, &mut account);

    let legs = account
        .working_orders()
        .iter()
        .filter(|order| order.is_protective())
        .count();
    assert_eq!(legs, 4, "both rungs armed, two legs each");
    let position = account.venue().position().expect("long").clone();
    let bracket = account.position_bracket(&position);
    assert!(bracket.is_laddered(), "{bracket:?}");
    assert_eq!(bracket.parts().count(), 2);
    assert_eq!(bracket.stop_loss(), None, "no one stop for two rungs");
    drive(&mut desk, &mut account, &frame(at(200.0), false, false));
    for above in [true, false] {
        let handle = bracket_handle_rect(chart().right(), 200.0, above).center();
        assert_eq!(
            desk.gesture
                .control_at(&account, handle, chart(), &LinearAxis),
            None,
            "no create-handle (above: {above})"
        );
    }
}

/// A rung of a resting order is the order's: hauling it moves that rung's
/// leg alone and never the saved ladder.
#[test]
fn a_rung_of_a_resting_order_moves_alone_and_the_ladder_stays() {
    let dir = ScratchDir::new("flow-rung");
    let mut account = marked_account(&dir);
    account.set_order_strategies(vec![halves()], Some("halves"));
    let mut desk = Desk::default();
    desk.ticket.qty_text = "2".to_owned();
    assert!(drive(
        &mut desk,
        &mut account,
        &frame(at(250.0), true, true)
    ));
    let id = account.working_orders()[0].id;
    let before: Vec<_> = account.working_orders()[0]
        .bracket
        .parts()
        .copied()
        .collect();
    assert_eq!(before[0].stop_loss, Some(Decimal::from(91)));

    let stop_y = LinearAxis.y(91.0);
    assert_eq!(
        line_at(&account, at(stop_y), &LinearAxis),
        Some(PaperDrag::Rung {
            order: id,
            index: 0,
            leg: Leg::StopLoss,
        })
    );
    drag(&mut desk, &mut account, stop_y, 320.0);

    let after: Vec<_> = account.working_orders()[0]
        .bracket
        .parts()
        .copied()
        .collect();
    assert_eq!(after[0].stop_loss, Some(Decimal::from(88)));
    assert_eq!(after[0].take_profit, before[0].take_profit);
    assert_eq!(after[1], before[1], "the other rung never moved");
    let strategy = account.selected_order_strategy().expect("still armed");
    assert_eq!(strategy.rows[0].loss_ticks, Some(4));
    assert_eq!(strategy.rows[0].gain_ticks, Some(8));
}

/// The wheel walks the aim's pair out a tick a notch, back to bare at
/// zero, and the click rests exactly the pair it showed.
#[test]
fn the_wheel_walks_the_aims_pair_and_the_click_carries_it() {
    let dir = ScratchDir::new("flow-ruler");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    let aim = at(250.0);

    drive(&mut desk, &mut account, &rolled(aim, 0.0));
    let preview = desk.gesture.cmd_preview.expect("the aim is up");
    assert_eq!(preview.kind, EntryKind::Limit);
    assert_eq!(preview.ruler_ticks, 0);
    assert!(preview.bracket.is_empty(), "the ruler starts off");

    for _ in 0..3 {
        drive(&mut desk, &mut account, &rolled(aim, 50.0));
    }
    assert_eq!(desk.ruler.notches, 3);
    assert_eq!(aimed_bracket(&desk).stop_loss(), Some(Decimal::from(92)));
    assert_eq!(aimed_bracket(&desk).take_profit(), Some(Decimal::from(98)));
    drive(&mut desk, &mut account, &rolled(aim, -50.0));
    assert_eq!(aimed_bracket(&desk).stop_loss(), Some(Decimal::from(93)));
    assert_eq!(aimed_bracket(&desk).take_profit(), Some(Decimal::from(97)));
    for _ in 0..5 {
        drive(&mut desk, &mut account, &rolled(aim, -50.0));
    }
    assert_eq!(desk.ruler.notches, 0, "it stops at zero");
    assert!(
        aimed_bracket(&desk).is_empty(),
        "and zero is the bare order"
    );

    for _ in 0..5 {
        drive(&mut desk, &mut account, &rolled(aim, 50.0));
    }
    assert!(drive(&mut desk, &mut account, &frame(aim, true, true)));
    let order = &account.working_orders()[0];
    assert_eq!(order.price, Some(Decimal::from(95)));
    assert_eq!(order.bracket.stop_loss(), Some(Decimal::from(90)));
    assert_eq!(order.bracket.take_profit(), Some(Decimal::from(100)));
}

/// A short's pair mirrors, and with no aim up the wheel's press is not the
/// ruler's, even over a ruler set by name.
#[test]
fn a_shorts_ruler_mirrors_and_no_aim_leaves_the_wheel_press_alone() {
    let dir = ScratchDir::new("flow-ruler-sell");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    let sell = ChartFrame {
        scroll_y: 50.0,
        ..keyed(at(250.0), ALT, false)
    };
    for _ in 0..4 {
        drive(&mut desk, &mut account, &sell);
    }
    let preview = desk.gesture.cmd_preview.expect("the sell aim is up");
    assert_eq!(preview.side, Side::Sell);
    assert_eq!(preview.bracket.stop_loss(), Some(Decimal::from(99)));
    assert_eq!(preview.bracket.take_profit(), Some(Decimal::from(91)));

    let mut idle = Desk::default();
    idle.ruler.set_ticks(2);
    let pressed = ChartFrame {
        middle_pressed: true,
        ..frame(at(250.0), false, false)
    };
    drive(&mut idle, &mut account, &pressed);
    assert_eq!(idle.ruler.notches, 2, "no aim, no claim on the press");
}

/// Escape drops the line in the hand, and the release that follows
/// submits nothing.
#[test]
fn a_dropped_line_submits_nothing_on_release() {
    let dir = ScratchDir::new("flow-escape");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    desk.ticket.stop_offset_text = "10".to_owned();
    long_at_100(&desk, &mut account);

    assert!(drive(
        &mut desk,
        &mut account,
        &frame(at(300.0), false, true)
    ));
    assert!(desk.cancel_gesture(), "the grabbed stop is let go");
    assert!(!drive(
        &mut desk,
        &mut account,
        &held(at(250.0), false, true)
    ));
    assert_eq!(
        account.venue().position().expect("long").stop_loss,
        Some(Decimal::from(90)),
        "a cancelled drag never moves the stop"
    );
}

/// Pulling away from a bare entry line births the missing leg on the side
/// it was pulled to, and a leg's own line reprices it on release.
#[test]
fn the_entry_line_births_legs_and_a_legs_line_reprices_it() {
    let dir = ScratchDir::new("flow-position-drags");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    long_at_100(&desk, &mut account);
    let position = |account: &PaperAccount| account.venue().position().expect("long").clone();

    drag(&mut desk, &mut account, 200.0, 150.0);
    assert_eq!(position(&account).take_profit, Some(Decimal::from(105)));
    assert_eq!(position(&account).avg_price, Decimal::from(100));
    drag(&mut desk, &mut account, 200.0, 300.0);
    assert_eq!(position(&account).stop_loss, Some(Decimal::from(90)));
    assert_eq!(position(&account).take_profit, Some(Decimal::from(105)));

    drag(&mut desk, &mut account, 300.0, 250.0);
    assert_eq!(position(&account).stop_loss, Some(Decimal::from(95)));
}

/// A fully bracketed entry line holds the gesture but moves nothing, and a
/// press away from every line is the chart's.
#[test]
fn a_fully_bracketed_entry_line_holds_the_press_and_never_moves() {
    let dir = ScratchDir::new("flow-blocked");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    desk.ticket.stop_offset_text = "10".to_owned();
    desk.ticket.profit_offset_text = "10".to_owned();
    long_at_100(&desk, &mut account);
    assert_eq!(
        desk.gesture
            .hover_cursor(&account, at(200.0), chart(), &LinearAxis),
        Some(CursorHint::PointingHand)
    );

    drag(&mut desk, &mut account, 200.0, 150.0);
    let position = account.venue().position().expect("long");
    assert_eq!(position.avg_price, Decimal::from(100));
    assert_eq!(position.stop_loss, Some(Decimal::from(90)));
    assert_eq!(position.take_profit, Some(Decimal::from(110)));
    assert!(!drive(
        &mut desk,
        &mut account,
        &frame(at(40.0), false, true)
    ));
}

/// A working order's cross wins over the armed click, and starts no drag.
#[test]
fn the_order_tags_cross_beats_the_armed_click() {
    let dir = ScratchDir::new("flow-cross-armed");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    rest(&mut desk, &mut account, EntryKind::Limit, 95.0);
    desk.armed = Some(ArmedPlacement {
        side: Side::Sell,
        kind: EntryKind::Stop,
    });
    let cross = close_button_rect(
        chart().right(),
        clamp_tag_center(LinearAxis.y(95.0), 0.0, 400.0),
    )
    .center();

    assert!(drive(&mut desk, &mut account, &frame(cross, false, true)));
    assert!(account.working_orders().is_empty(), "the order is gone");
    assert!(
        desk.armed.is_some(),
        "the armed click neither fired nor died"
    );
    assert_eq!(desk.gesture.drag, PaperDrag::None);
}

/// The aim yields wherever something already holds the pixel: a drawing's
/// handle, a hidden layer, and a capture hook's aim that never places.
#[test]
fn the_aim_yields_to_a_claimed_canvas_a_hidden_layer_and_a_forced_aim() {
    let dir = ScratchDir::new("flow-yield");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    let claimed = ChartFrame {
        canvas_claimed: true,
        ..frame(at(300.0), true, true)
    };
    assert!(!drive(&mut desk, &mut account, &claimed));
    assert_eq!(desk.gesture.cmd_preview, None);
    assert!(account.working_orders().is_empty());
    assert_eq!(
        desk.gesture
            .hover_cursor(&account, at(300.0), chart(), &LinearAxis),
        None,
        "no hand promising a click that will not happen"
    );
    assert!(drive(
        &mut desk,
        &mut account,
        &frame(at(300.0), true, true)
    ));
    assert_eq!(account.working_orders().len(), 1, "clear canvas, it rests");

    let hidden = ChartFrame {
        layer_visible: false,
        ..frame(at(200.0), true, true)
    };
    assert!(!drive(&mut desk, &mut account, &hidden));
    assert_eq!(account.working_orders().len(), 1);
    assert_eq!(
        desk.gesture
            .hover_cursor(&account, at(300.0), chart(), &LinearAxis),
        None,
        "an unpainted line offers no cursor"
    );

    let mut forced = Desk::default();
    forced.gesture.cmd_preview_force = CmdPreviewForce::parse("buy@0.5");
    assert!(!drive(
        &mut forced,
        &mut account,
        &frame(at(250.0), false, true)
    ));
    assert!(forced.gesture.cmd_preview.is_some(), "it paints");
    assert_eq!(account.working_orders().len(), 1, "and never places");
    assert_eq!(
        forced
            .gesture
            .hover_cursor(&account, at(250.0), chart(), &LinearAxis),
        None
    );
}

/// A paper line keeps its pixel from the aim, and an armed ticket keeps
/// the click it was armed for.
#[test]
fn the_aim_stands_down_over_a_paper_line_and_an_armed_ticket() {
    let dir = ScratchDir::new("flow-stand-down");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    desk.ticket.stop_offset_text = "10".to_owned();
    long_at_100(&desk, &mut account);
    let on_stop = at(300.0);

    drive(&mut desk, &mut account, &frame(on_stop, true, false));
    assert_eq!(desk.gesture.cmd_preview, None, "the stop keeps its pixel");
    assert_eq!(
        desk.gesture
            .hover_cursor(&account, on_stop, chart(), &LinearAxis),
        Some(CursorHint::ResizeVertical)
    );
    assert!(drive(&mut desk, &mut account, &frame(on_stop, true, true)));
    assert_eq!(
        desk.gesture.drag,
        PaperDrag::Leg {
            owner: BracketTarget::Position,
            leg: Leg::StopLoss,
        }
    );
    assert!(account.working_orders().is_empty());
    desk.cancel_gesture();

    desk.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    drive(&mut desk, &mut account, &frame(at(100.0), true, false));
    assert_eq!(desk.gesture.cmd_preview, None, "the armed ticket keeps it");
    assert!(drive(
        &mut desk,
        &mut account,
        &frame(at(320.0), true, true)
    ));
    assert_eq!(desk.armed, None);
    let orders = account.working_orders();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].kind, EntryKind::Limit, "the kind it armed");
}

/// WIN$N marked at 140,000 with its money declared and 100 BRL a trade.
fn win_account(dir: &ScratchDir) -> PaperAccount {
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
    account.set_symbol("WIN$N");
    account.seed(&print(0, 140_000));
    let book: InstrumentBook = [(
        "WIN$N".to_owned(),
        InstrumentMoney {
            point_value: Decimal::new(20, 2),
            size_step: Decimal::ONE,
            min_size: Decimal::ONE,
            max_size: None,
            currency: Currency::new("BRL").expect("BRL"),
            source: MoneySource::Declared,
        },
    )]
    .into_iter()
    .collect();
    account.set_instrument_money(book);
    account.set_risk_settings(RiskSettings {
        basis: RiskBasis::Amount,
        amount: Decimal::from(100),
        amount_currency: None,
        percent: Decimal::ZERO,
        lock: true,
    });
    account
}

/// The wheel and a saved ladder are two ways of saying where the stop is;
/// the size must not depend on which one the trader used.
#[test]
fn the_wheel_and_a_saved_ladder_size_one_entry_alike() {
    let dir = ScratchDir::new("flow-risk-alike");
    let mark = Decimal::from(140_000);
    let mut account = win_account(&dir);
    let mut wheel = Desk::default();
    wheel.ruler.set_step("WIN$N", Some(Decimal::ONE));
    wheel.ruler.set_ticks(10);
    let by_wheel = account.risk_state(
        Side::Buy,
        mark,
        &wheel.account_env(&account, Side::Buy, mark),
    );

    account.set_order_strategies(
        vec![OrderStrategy {
            name: "one rung".to_owned(),
            rows: vec![StrategyRow {
                share_percent: Decimal::ONE_HUNDRED,
                gain_ticks: Some(10),
                loss_ticks: Some(10),
            }],
        }],
        Some("one rung"),
    );
    let ladder = Desk::default();
    let by_ladder = account.risk_state(
        Side::Buy,
        mark,
        &ladder.account_env(&account, Side::Buy, mark),
    );

    assert_eq!(by_wheel.code(), by_ladder.code());
    // 10 points x 0.20 = 2.00 a contract; 100 / 2 = 50.
    assert_eq!(by_wheel.derived_quantity(), Some(Decimal::from(50)));
    assert_eq!(by_ladder.derived_quantity(), Some(Decimal::from(50)));
}

/// A stop wider than the budget blocks the entry and names the number;
/// taking the lock down lets that entry, and a named one, through.
#[test]
fn a_stop_past_the_budget_blocks_until_the_lock_comes_down() {
    let dir = ScratchDir::new("flow-risk-lock");
    let mut account = win_account(&dir);
    let mut desk = Desk::default();
    desk.ruler.set_step("WIN$N", Some(Decimal::from(20)));
    desk.ruler.set_ticks(200);

    let (state, blocks) = desk.risk_report(&account);
    assert_eq!(state.code(), "clamped_at_minimum", "{state:?}");
    assert!(blocks);
    let sentence = state.sentence();
    assert!(sentence.contains("800 BRL"), "{sentence}");
    assert!(sentence.contains("raise the risk"), "{sentence}");
    assert!(!market(&desk, &mut account, Side::Buy), "refused");
    account.on_trade(&print(1, 140_000));
    assert!(account.is_flat(), "the lock refused the placement itself");

    let mut risk = account.risk_settings().clone();
    risk.lock = false;
    account.set_risk_settings(risk);
    let (state, blocks) = desk.risk_report(&account);
    assert_eq!(state.code(), "clamped_at_minimum", "still over budget");
    assert!(!blocks, "but no longer refused");
    let named = quantick_trading::OrderIntent::market(Side::Buy, Decimal::from(10))
        .with_bracket(Bracket::whole(Some(Decimal::from(136_000)), None));
    assert_eq!(account.risk_refusal_for(&named), None);
}
