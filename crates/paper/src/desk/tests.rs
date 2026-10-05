//! The desk's rules, tested without a window.
//!
//! Moved here with the code they cover: the cmd aim's kind table, its
//! tokens and its layout, the capture hook's parse, and the geometry the
//! paint and the press share. The host's own tests still drive the whole
//! gesture through its window-typed input; these pin the rules underneath.

use quantick_engine::{Side, Trade};
use quantick_sim::{BracketTarget, EntryKind, OrderId, Position};
use rust_decimal::Decimal;

use super::cmd::{
    CMD_LABEL_CURSOR_GAP_PX, CMD_LABEL_WIDTH_PX, CMD_LINE_MIN_PX, CmdEntryKind, CmdModifier,
    CmdPreviewForce, CmdTradingSettings, cmd_preview_layout, resolve_cmd_kind,
};
use super::geometry::{
    Bounds, CHIP_CLEAR_PX, Point, TAG_HEIGHT_PX, clamp_tag_center, dodged_chip_y,
};
use super::gesture::{ArmedPlacement, ChartCommand, ChartFrame, PaperDrag};
use super::leg_tag::{LegPaint, leg_tag_text};
use super::ruler::{RULER_MAX_NOTCHES, Ruler};
use super::ticket::{Ticket, entry_label, offset_price, parse_offset};
use super::{Desk, HeldKeys, PriceAxis, StrategyEditor};
use crate::PaperAccount;
use crate::account::Leg;
use crate::order_strategies::{NEW_RUNG_TICKS, OrderStrategy, StrategyRow};
use crate::scratch::ScratchDir;

/// The stated kind wins where both are conceivable to a trader but only
/// one can rest — which is every price except the mark.
///
/// The pairing to read here is the second and third assertion: at 95,
/// with the market at 100, `Auto` yields a limit. Ask for a stop at that
/// same price and the aim stands down instead of handing you the limit.
/// That is the whole feature: the click that lands is the order you came
/// to place, or no click at all.
#[test]
fn a_stated_entry_kind_is_honoured_or_the_aim_stands_down() {
    let mark = Decimal::from(100);
    let below = Decimal::from(95);
    let above = Decimal::from(105);

    // Auto reads the market, exactly as it always has.
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Auto, Side::Buy, below, mark),
        Some(EntryKind::Limit),
        "a buy below the market waits at a limit"
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Auto, Side::Buy, above, mark),
        Some(EntryKind::Stop),
        "and above it stops in"
    );

    // A stated kind takes the price where it is valid...
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Buy, below, mark),
        Some(EntryKind::Limit)
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Stop, Side::Buy, above, mark),
        Some(EntryKind::Stop)
    );

    // ...and stands the aim down where it is not, rather than silently
    // placing the other kind. A trader who came to buy a pullback must
    // never be handed a breakout stop.
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Stop, Side::Buy, below, mark),
        None,
        "a buy stop cannot arm below the market, so nothing is offered"
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Buy, above, mark),
        None,
        "and a buy limit above it would fill at once"
    );

    // A sell mirrors, on every choice.
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Sell, above, mark),
        Some(EntryKind::Limit)
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Stop, Side::Sell, below, mark),
        Some(EntryKind::Stop)
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Sell, below, mark),
        None
    );

    // On the mark nothing rests, whatever was asked for: a resting order
    // there fills on the next print, which is a market order wearing the
    // wrong name.
    for choice in CmdEntryKind::ALL {
        assert_eq!(
            resolve_cmd_kind(choice, Side::Buy, mark, mark),
            None,
            "{choice:?} rests nothing on the mark"
        );
    }
}

/// The choice survives a restart, and an unknown token in a
/// hand-edited sidecar falls back rather than refusing to open.
#[test]
fn the_entry_kind_choice_is_remembered_and_unknown_tokens_fall_back() {
    let state = crate::state::PaperState {
        cmd_entry_kind: Some("stop".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        CmdTradingSettings::from_state(&state).kind,
        CmdEntryKind::Stop
    );

    let state = crate::state::PaperState {
        cmd_entry_kind: Some("teleport".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        CmdTradingSettings::from_state(&state).kind,
        CmdEntryKind::Auto,
        "a token this build does not know is the default, not a crash"
    );
}

/// The aim label rides the pointer instead of parking at the right
/// edge — the whole point of the change — while the dashed line still
/// reaches the axis, so label and price chip stay one statement.
#[test]
fn the_cmd_label_follows_the_pointer_and_the_line_reaches_the_axis() {
    let band = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(800.0, 400.0));
    let mut previous: Option<f32> = None;
    for x in [200.0_f32, 400.0, 600.0] {
        let (start, end, label) = cmd_preview_layout(band, band.right(), Point::new(x, 250.0));
        assert_eq!(end.x, 800.0, "the line always reaches the axis");
        assert_eq!(start.x, x, "the line starts under the cursor");
        assert_eq!(
            label.right(),
            x - CMD_LABEL_CURSOR_GAP_PX,
            "the label rides a fixed gap off the pointer"
        );
        assert_eq!(label.width(), CMD_LABEL_WIDTH_PX);
        assert_eq!(label.center().y, 250.0);
        assert!(
            !label.contains(Point::new(x, 250.0)),
            "never under the cursor it belongs to"
        );
        if let Some(previous) = previous {
            assert!(label.left() > previous, "moving right moves the label");
        }
        previous = Some(label.left());
    }
}

/// The tape lane is not a wall. Its divider ends the *band* — where a
/// press can still land — and the label stops there with it, but the
/// line carries on to the axis, because a gap across the widest lane on
/// the chart is exactly where a trader loses the order.
#[test]
fn the_aim_line_crosses_the_live_lane_to_the_axis() {
    // A chart 1000 wide whose live tape lane opens at 700: the band the
    // aim lays out against stops at the divider, the gutter does not.
    let band = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(700.0, 400.0));
    let axis_x = 1000.0;
    let (start, end, label) = cmd_preview_layout(band, axis_x, Point::new(400.0, 250.0));
    assert_eq!(
        end.x, axis_x,
        "the line spans the lane instead of stopping at its divider"
    );
    assert!(
        end.x > band.right(),
        "and it is the lane it crosses, not the plot it started in"
    );
    assert_eq!(start.x, 400.0, "it still starts under the cursor");
    assert!(
        label.right() <= band.right(),
        "the label stays inside the band a press can reach: {label:?}"
    );
}

/// A pane with no live lane hands the same x twice, and the line must
/// not double back on itself.
#[test]
fn the_aim_line_ends_at_the_axis_with_no_lane_open() {
    let band = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(800.0, 400.0));
    let (_, end, _) = cmd_preview_layout(band, 800.0, Point::new(400.0, 250.0));
    assert_eq!(end.x, 800.0, "band right and axis coincide");
    // A gutter reported left of the plot (a pane mid-resize) must never
    // shorten the line to a stub pointing the wrong way.
    let (start, end, _) = cmd_preview_layout(band, 10.0, Point::new(400.0, 250.0));
    assert!(end.x >= start.x, "never a line running backwards");
}

/// The two edges: near the left one the label flips to the pointer's
/// right rather than leaving the band, and near the right one the line
/// starts further left so there is still a line to read.
#[test]
fn the_cmd_layout_clamps_at_both_edges_of_the_band() {
    let band = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(800.0, 400.0));

    let pointer = Point::new(20.0, 250.0);
    let (_, _, label) = cmd_preview_layout(band, band.right(), pointer);
    assert!(label.left() >= band.left(), "never off the left edge");
    assert_eq!(
        label.left(),
        pointer.x + CMD_LABEL_CURSOR_GAP_PX,
        "no room on the left, so it flips right"
    );
    assert!(!label.contains(pointer), "still clear of the cursor");

    let pointer = Point::new(780.0, 250.0);
    let (start, end, label) = cmd_preview_layout(band, band.right(), pointer);
    assert!(label.right() <= band.right(), "never off the right edge");
    assert_eq!(
        end.x - start.x,
        CMD_LINE_MIN_PX,
        "close to the axis the line starts further left"
    );
    assert!(!label.contains(pointer), "still clear of the cursor");

    // A band narrower than the label plus its gap cannot hold both; it
    // parks at the left edge rather than running off-plot to the left.
    let sliver = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(100.0, 400.0));
    let (start, _, label) = cmd_preview_layout(sliver, sliver.right(), Point::new(50.0, 250.0));
    assert_eq!(label.left(), sliver.left(), "a sliver parks at its edge");
    assert_eq!(start.x, sliver.left(), "and the line spans what there is");
}

/// The capture hook: a side, and optionally where along the band to
/// park the hand the run does not have.
#[test]
fn the_cmd_preview_hook_parses_a_side_and_an_optional_x() {
    assert_eq!(
        CmdPreviewForce::parse("buy"),
        Some(CmdPreviewForce {
            side: Side::Buy,
            x_fraction: None
        })
    );
    assert_eq!(
        CmdPreviewForce::parse("SELL@0.15"),
        Some(CmdPreviewForce {
            side: Side::Sell,
            x_fraction: Some(0.15)
        })
    );
    assert_eq!(
        CmdPreviewForce::parse("buy@9"),
        Some(CmdPreviewForce {
            side: Side::Buy,
            x_fraction: Some(1.0)
        }),
        "out of range clamps into the band"
    );
    for bad in [
        "buy@left", "buy@nan", "buy@NaN", "buy@inf", "buy@", "buy@0,15",
    ] {
        assert_eq!(
            CmdPreviewForce::parse(bad),
            Some(CmdPreviewForce {
                side: Side::Buy,
                x_fraction: None
            }),
            "a bad fraction still paints, mid-band: {bad}"
        );
    }
    assert_eq!(CmdPreviewForce::parse("hold"), None);
}

#[test]
fn cmd_modifier_tokens_round_trip_and_state_defaults_fill_gaps() {
    for modifier in CmdModifier::ALL {
        assert_eq!(CmdModifier::parse(modifier.as_str()), Some(modifier));
    }
    assert_eq!(CmdModifier::parse("hyper"), None);
    let state = crate::state::PaperState {
        cmd_trading_enabled: Some(false),
        cmd_buy_modifier: Some("alt".to_owned()),
        cmd_sell_modifier: Some("hyper".to_owned()),
        ..Default::default()
    };
    let settings = CmdTradingSettings::from_state(&state);
    assert!(!settings.enabled);
    assert_eq!(settings.buy, CmdModifier::Alt);
    assert_eq!(
        settings.sell,
        CmdModifier::Ctrl,
        "an unknown token falls back to the default"
    );
}

/// The chip dodge: lines keep their price, chips clear the last-price
/// row by the minimum, and the fill-moment tie steps down.
#[test]
fn paper_chips_dodge_the_last_price_chip_never_the_line() {
    // No reservation, or far enough away: the chip stays at its line.
    assert_eq!(dodged_chip_y(100.0, None, 0.0, 400.0), 100.0);
    assert_eq!(dodged_chip_y(100.0, Some(200.0), 0.0, 400.0), 100.0);
    // Inside the band: pushed just clear, towards its own side.
    assert_eq!(
        dodged_chip_y(210.0, Some(200.0), 0.0, 400.0),
        200.0 + CHIP_CLEAR_PX
    );
    assert_eq!(
        dodged_chip_y(190.0, Some(200.0), 0.0, 400.0),
        200.0 - CHIP_CLEAR_PX
    );
    // The fill moment: entry == last price, and the chip steps down.
    assert_eq!(
        dodged_chip_y(200.0, Some(200.0), 0.0, 400.0),
        200.0 + CHIP_CLEAR_PX
    );
    // Never dodged out of the pane.
    assert_eq!(dodged_chip_y(398.0, Some(399.0), 0.0, 400.0), 383.0);
}

/// A band too short to hold a tag is reachable — `split_panes` carves
/// the indicator strips out of the plot with no floor of its own — and
/// `f32::clamp` panics rather than saturating once its bounds cross.
/// Every tag, every ✕ hit-test and the aim's own layout run through
/// this, so the panic would take a live session down.
#[test]
fn a_band_too_short_for_a_tag_centres_it_instead_of_panicking() {
    // Shorter than a tag, and flat: the two cases that cross the bounds.
    assert_eq!(clamp_tag_center(5.0, 0.0, 10.0), 5.0);
    assert_eq!(clamp_tag_center(99.0, 40.0, 40.0), 40.0);
    assert_eq!(clamp_tag_center(-99.0, 0.0, TAG_HEIGHT_PX), 10.0);
    // And with room, it still clamps exactly as before.
    assert_eq!(clamp_tag_center(0.0, 0.0, 400.0), 10.0);
    assert_eq!(clamp_tag_center(400.0, 0.0, 400.0), 390.0);
    assert_eq!(clamp_tag_center(200.0, 0.0, 400.0), 200.0);
    // And the aim's layout lays out in it rather than panicking.
    let sliver = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(800.0, 12.0));
    let _ = cmd_preview_layout(sliver, sliver.right(), Point::new(400.0, 6.0));
}

// ----------------------------------------------------------------------
// The desk as a state machine, driven through plain values: no window,
// no toolkit, a straight-line price axis and a scratch account.
// ----------------------------------------------------------------------

/// Prices 80..120 over pixels 0..400, high at the top: the chart the host's
/// own tests draw, so y 250 is price 95 here as it is there.
struct LinearAxis;

impl PriceAxis for LinearAxis {
    fn y(&self, price: f64) -> f32 {
        ((120.0 - price) / 40.0 * 400.0) as f32
    }

    fn price_at(&self, y: f32) -> f64 {
        120.0 - f64::from(y) / 400.0 * 40.0
    }

    fn is_inverted(&self) -> bool {
        false
    }
}

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// An account marked at 100, in a folder of its own.
fn marked_account(dir: &ScratchDir) -> PaperAccount {
    let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
    account.set_symbol("DESKX");
    account.seed(&print(0, 100));
    account
}

fn chart() -> Bounds {
    Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(800.0, 400.0))
}

/// One frame with the pointer at `pointer`, Shift held when `aiming`.
fn frame(pointer: Point, aiming: bool, pressed: bool) -> ChartFrame {
    ChartFrame {
        chart: chart(),
        pointer: Some(pointer),
        primary_pressed: pressed,
        primary_down: false,
        primary_released: false,
        keys: HeldKeys {
            shift: aiming,
            ..HeldKeys::default()
        },
        canvas_claimed: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    }
}

/// A held key aims at the pointer's price, and the press asks for exactly
/// what the aim showed: the decision and the command come out of the desk
/// as values, before any window or venue is involved.
#[test]
fn a_held_key_aims_and_the_press_asks_to_rest_there() {
    let dir = ScratchDir::new("desk-aim");
    let account = marked_account(&dir);
    let mut desk = Desk::default();
    let at = Point::new(400.0, 250.0);

    let idle = desk.handle_input(&account, &frame(at, true, false), Some(&LinearAxis));
    assert!(!idle.owned, "an aim alone owns no gesture");
    assert_eq!(idle.command, None);
    let preview = desk.gesture.cmd_preview.expect("Shift over the chart aims");
    assert_eq!(
        (preview.side, preview.kind, preview.price),
        (Side::Buy, EntryKind::Limit, Decimal::from(95))
    );
    assert_eq!(preview.pointer, at, "the aim is the hand, whole");

    let pressed = desk.handle_input(&account, &frame(at, true, true), Some(&LinearAxis));
    assert!(pressed.owned);
    assert_eq!(
        pressed.command,
        Some(ChartCommand::PlaceResting {
            side: Side::Buy,
            kind: EntryKind::Limit,
            raw_price: 95.0,
        })
    );
}

/// The capture hook's aim paints and never places, and a hidden layer
/// takes nothing at all.
#[test]
fn a_forced_aim_never_places_and_a_hidden_layer_owns_nothing() {
    let dir = ScratchDir::new("desk-forced");
    let account = marked_account(&dir);
    let mut desk = Desk::default();
    desk.gesture.cmd_preview_force = CmdPreviewForce::parse("buy");
    let at = Point::new(400.0, 250.0);

    let outcome = desk.handle_input(&account, &frame(at, false, true), Some(&LinearAxis));
    assert!(
        desk.gesture
            .cmd_preview
            .is_some_and(|preview| preview.forced)
    );
    assert_eq!(outcome.command, None, "a forced aim never places");

    let mut hidden = frame(at, true, true);
    hidden.layer_visible = false;
    let outcome = desk.handle_input(&account, &hidden, Some(&LinearAxis));
    assert!(!outcome.owned);
    assert_eq!(outcome.command, None);
    assert_eq!(
        desk.gesture.cmd_preview, None,
        "nothing aims on a hidden layer"
    );
}

/// The wheel walks the ruler only under a real aim, and the chart is told
/// the travel was spent.
#[test]
fn the_wheel_winds_the_ruler_only_under_a_real_aim() {
    let dir = ScratchDir::new("desk-ruler");
    let account = marked_account(&dir);
    let mut desk = Desk::default();
    let at = Point::new(400.0, 250.0);

    let mut rolled = frame(at, false, false);
    rolled.scroll_y = 40.0;
    desk.handle_input(&account, &rolled, Some(&LinearAxis));
    assert_eq!(desk.ruler.notches, 0, "no aim, no ruler");
    assert!(!desk.gesture.scroll_consumed, "the chart keeps its wheel");

    let mut aimed = frame(at, true, false);
    aimed.scroll_y = 40.0;
    desk.handle_input(&account, &aimed, Some(&LinearAxis));
    assert_eq!(desk.ruler.notches, 1);
    assert!(desk.gesture.scroll_consumed);
    assert!(desk.ruler.rolled);
    let preview = desk.gesture.cmd_preview.expect("still aiming");
    assert!(
        !preview.bracket.is_empty(),
        "the recomputed aim carries the ruler's pair"
    );

    let mut put_away = frame(at, true, false);
    put_away.middle_pressed = true;
    desk.handle_input(&account, &put_away, Some(&LinearAxis));
    assert_eq!(desk.ruler.notches, 0, "pressing the wheel puts it away");
}

/// An armed click asks to place at the clicked price, and Escape's first
/// layer is the armed entry, then the line in the hand.
#[test]
fn an_armed_click_asks_to_place_and_escape_peels_one_layer() {
    let dir = ScratchDir::new("desk-armed");
    let account = marked_account(&dir);
    let mut desk = Desk::default();
    let armed = ArmedPlacement {
        side: Side::Sell,
        kind: EntryKind::Limit,
    };
    desk.armed = Some(armed);
    let at = Point::new(400.0, 50.0);

    let outcome = desk.handle_input(&account, &frame(at, false, true), Some(&LinearAxis));
    assert_eq!(
        outcome.command,
        Some(ChartCommand::PlaceArmed {
            armed,
            raw_price: 115.0,
        })
    );
    assert!(desk.armed.is_some(), "disarming waits for the venue's yes");

    desk.gesture.drag = PaperDrag::Order(OrderId(7));
    desk.gesture.drag_price = Some(101.0);
    assert!(desk.cancel_gesture(), "the armed entry goes first");
    assert!(desk.armed.is_none());
    assert_eq!(desk.gesture.drag, PaperDrag::Order(OrderId(7)));
    assert!(desk.cancel_gesture(), "then the line in the hand");
    assert_eq!(desk.gesture.drag, PaperDrag::None);
    assert_eq!(desk.gesture.drag_price, None);
    assert!(!desk.cancel_gesture(), "and then nothing is left");
}

/// The armed click disarms only when the venue took the order: a refused
/// price and an unreadable offset both leave it armed for the next click,
/// and only the offset's complaint comes back for the host to show.
#[test]
fn an_armed_click_disarms_only_when_the_venue_takes_it() {
    let dir = ScratchDir::new("desk-armed-carry");
    let mut account = marked_account(&dir);
    let mut desk = Desk::default();
    let armed = ArmedPlacement {
        side: Side::Sell,
        kind: EntryKind::Limit,
    };
    desk.armed = Some(armed);

    // A sell limit under the market is not a resting order.
    let refused = desk.carry_out(
        &mut account,
        ChartCommand::PlaceArmed {
            armed,
            raw_price: 85.0,
        },
    );
    assert_eq!(refused, Ok(()), "a venue refusal is the account's toast");
    assert!(account.take_toast().is_some(), "the refusal is explained");
    assert!(account.working_orders().is_empty());
    assert_eq!(desk.armed, Some(armed), "refused, so still armed");

    // An offset that does not parse never reaches the venue.
    desk.ticket.stop_offset_text = "abc".to_owned();
    let unreadable = desk.carry_out(
        &mut account,
        ChartCommand::PlaceArmed {
            armed,
            raw_price: 115.0,
        },
    );
    assert_eq!(
        unreadable,
        Err("SIM: the stop offset must be a positive number of points - got `abc`".to_owned())
    );
    assert!(account.working_orders().is_empty());
    assert_eq!(desk.armed, Some(armed), "unreadable, so still armed");

    // Taken: one order rests, and the click is spent.
    desk.ticket.stop_offset_text.clear();
    let taken = desk.carry_out(
        &mut account,
        ChartCommand::PlaceArmed {
            armed,
            raw_price: 115.0,
        },
    );
    assert_eq!(taken, Ok(()));
    assert_eq!(account.working_orders().len(), 1);
    assert_eq!(desk.armed, None, "taken, so disarmed");
}

/// The notch is the device's: learned from the smallest roll, never
/// assumed, and bounded at both ends.
#[test]
fn the_ruler_learns_its_notch_and_stays_in_range() {
    for notch in [13.0_f32, 40.0, 120.0] {
        let mut ruler = Ruler::default();
        ruler.step(notch);
        assert_eq!(ruler.notches, 1, "one roll of {notch} px is one notch");
        ruler.step(notch * 3.0);
        assert_eq!(ruler.notches, 4);
        ruler.step(-notch * 9.0);
        assert_eq!(ruler.notches, 0, "never below zero");
    }
    let mut ruler = Ruler::default();
    ruler.step(0.5);
    assert_eq!(ruler.notches, 0, "jitter under the floor is not a notch");
    assert!(!ruler.notch_px.is_finite(), "and teaches nothing");
    assert_eq!(ruler.set_ticks(RULER_MAX_NOTCHES + 50), RULER_MAX_NOTCHES);
    assert!(ruler.clear(), "a standing ruler clears");
    assert!(!ruler.clear(), "and a cleared one says so");
}

/// The pair sits the same distance either side, mirrors for a sell, and
/// stands down rather than pricing a stop through zero.
#[test]
fn the_ruler_projects_a_symmetric_pair() {
    let mut ruler = Ruler::default();
    let price = Decimal::from(100);
    let step = Decimal::from(2);
    assert_eq!(
        ruler.levels(Side::Buy, price, step),
        (None, None),
        "off at zero"
    );
    ruler.set_ticks(3);
    assert_eq!(
        ruler.levels(Side::Buy, price, step),
        (Some(Decimal::from(94)), Some(Decimal::from(106)))
    );
    assert_eq!(
        ruler.levels(Side::Sell, price, step),
        (Some(Decimal::from(106)), Some(Decimal::from(94)))
    );
    assert_eq!(
        ruler.levels(Side::Buy, Decimal::from(5), step),
        (None, None),
        "a stop through zero is no stop"
    );
}

/// A typed step is the trader's, per symbol; a switch forgets the
/// distance but not the first symbol's arrival.
#[test]
fn a_typed_step_is_kept_per_symbol_and_a_switch_resets_the_distance() {
    let mut ruler = Ruler::default();
    let derived = Decimal::new(25, 2);
    ruler.set_step("WIN", Some(Decimal::from(5)));
    assert_eq!(ruler.step_for("WIN", derived), Decimal::from(5));
    assert_eq!(
        ruler.step_for("BTC", derived),
        derived,
        "unnamed follows the instrument"
    );
    ruler.set_step("WIN", Some(Decimal::ZERO));
    assert_eq!(ruler.step_for("WIN", derived), derived, "zero clears it");

    ruler.set_step("WIN", Some(Decimal::from(5)));
    ruler.set_ticks(4);
    ruler.follow_symbol("WIN", true);
    assert_eq!(ruler.notches, 4, "arriving is not a switch");
    assert_eq!(ruler.step_text, "5");
    ruler.follow_symbol("BTC", false);
    assert_eq!(ruler.notches, 0, "a switch puts the ruler away");
    assert_eq!(ruler.step_text, "", "and shows the new symbol's own step");
}

/// The ticket's boxes, read: offsets are positive or empty, a bad one is
/// named in the complaint, and the stepper never goes under the floor.
#[test]
fn the_ticket_reads_its_boxes() {
    assert_eq!(parse_offset("  "), Ok(None));
    assert_eq!(parse_offset(" 2.5 "), Ok(Some(Decimal::new(25, 1))));
    assert_eq!(parse_offset("-1"), Err("-1".to_owned()));

    let mut ticket = Ticket {
        stop_offset_text: "x".to_owned(),
        ..Ticket::default()
    };
    assert_eq!(
        ticket.parse_bracket(Side::Buy, Decimal::from(100)),
        Err("SIM: the stop offset must be a positive number of points - got `x`".to_owned())
    );
    ticket.stop_offset_text = "2".to_owned();
    ticket.profit_offset_text = "4".to_owned();
    let bracket = ticket
        .parse_bracket(Side::Buy, Decimal::from(100))
        .expect("both offsets read");
    assert_eq!(bracket.stop_loss(), Some(Decimal::from(98)));
    assert_eq!(bracket.take_profit(), Some(Decimal::from(104)));

    ticket.qty_text = "0".to_owned();
    assert!(ticket.form().quantity.is_err(), "zero is not a size");
    assert_eq!(ticket.quantity_preview(), None);
    ticket.step_quantity(Decimal::ONE, Decimal::ONE, Decimal::ONE);
    assert_eq!(
        ticket.qty_text, "2",
        "an unreadable size steps from the floor"
    );
    ticket.step_quantity(-Decimal::TEN, Decimal::ONE, Decimal::ONE);
    assert_eq!(ticket.qty_text, "2", "and never under it");
}

/// The entry button says what the press does to the open position.
#[test]
fn the_entry_label_discloses_close_add_and_reverse() {
    let long = Position {
        side: Side::Buy,
        quantity: Decimal::from(2),
        avg_price: Decimal::from(100),
        opened_ms: 0,
        opened_agg_id: 0,
        low_price: Decimal::from(100),
        high_price: Decimal::from(100),
        stop_loss: None,
        take_profit: None,
    };
    assert_eq!(entry_label(Side::Sell, None, Some(&long)), "SELL");
    assert_eq!(entry_label(Side::Buy, Some(Decimal::ONE), None), "BUY 1");
    assert_eq!(
        entry_label(Side::Buy, Some(Decimal::ONE), Some(&long)),
        "BUY 1 (adds to 3)"
    );
    assert_eq!(
        entry_label(Side::Sell, Some(Decimal::ONE), Some(&long)),
        "SELL 1 (closes 1 of 2)"
    );
    assert_eq!(
        entry_label(Side::Sell, Some(Decimal::from(2)), Some(&long)),
        "SELL 2 (closes)"
    );
    assert_eq!(
        entry_label(Side::Sell, Some(Decimal::from(5)), Some(&long)),
        "SELL 5 (reverses to short 3)"
    );
    assert_eq!(
        offset_price(&long, Decimal::from(3), true),
        Decimal::from(97)
    );
    assert_eq!(
        offset_price(&long, Decimal::from(3), false),
        Decimal::from(103)
    );
}

/// The editor opens on the armed strategy, follows the list through an
/// add and a delete, and saves once, on close.
#[test]
fn the_strategy_editor_follows_the_list_and_saves_on_close() {
    let mut editor = StrategyEditor::default();
    editor.open_on(None, 0);
    assert!(editor.open);
    assert_eq!(editor.editing, None, "an empty list opens on nothing");
    editor.added(1);
    assert_eq!(editor.editing, Some(0));
    editor.open_on(Some(2), 3);
    assert_eq!(editor.editing, Some(2), "it opens on the armed one");
    editor.removed(2, 2);
    assert_eq!(editor.editing, Some(1), "a delete lands on the last left");
    editor.removed(0, 0);
    assert_eq!(editor.editing, None);
    editor.settle_editing(Some(1), 2);
    assert_eq!(
        editor.editing,
        Some(1),
        "a blank pane opens on the armed one"
    );
    assert!(editor.close(), "the edits reach disk once");
    assert!(!editor.close(), "and only once");
    assert!(!editor.open);
}

/// A new strategy is one whole rung; a new row takes what is left, or
/// halves the last rather than arriving broken.
#[test]
fn a_new_row_takes_what_is_left_or_halves_the_last() {
    let mut strategy = OrderStrategy::starter(2);
    assert_eq!(strategy.name, "Strategy 3");
    assert_eq!(
        strategy.summary(),
        format!("100% +{NEW_RUNG_TICKS}/-{NEW_RUNG_TICKS}")
    );
    strategy.push_row();
    assert_eq!(strategy.rows[0].share_percent, Decimal::from(50));
    assert_eq!(strategy.rows[1].share_percent, Decimal::from(50));
    assert!(strategy.validate().is_ok(), "still a whole position");

    strategy.rows = vec![StrategyRow {
        share_percent: Decimal::from(60),
        gain_ticks: None,
        loss_ticks: Some(5),
    }];
    strategy.push_row();
    assert_eq!(
        strategy.rows[1].share_percent,
        Decimal::from(40),
        "what is left"
    );
    assert_eq!(strategy.summary(), "60% runs/-5 · 40% +20/-20");
}

/// A leg's tag: the pill at rest, the whole statement open, the R:R while
/// dragging, and the order's id while it has not filled.
#[test]
fn a_leg_tag_says_what_it_is_and_what_it_is_worth() {
    let mut paint = LegPaint {
        owner: BracketTarget::Position,
        leg: Leg::StopLoss,
        side: Side::Buy,
        reference: Decimal::from(100),
        quantity: Decimal::ONE,
        level: Some(Decimal::from(95)),
        other_level: Some(Decimal::from(110)),
        pending: false,
    };
    let stop = Decimal::from(95);
    assert_eq!(leg_tag_text(&paint, stop, false, false), "SL -5");
    assert_eq!(leg_tag_text(&paint, stop, true, false), "SL 95 -5 pts");
    assert_eq!(
        leg_tag_text(&paint, stop, true, true),
        "SL 95 -5 pts · R:R 2"
    );
    paint.owner = BracketTarget::Order(OrderId(3));
    paint.pending = true;
    assert_eq!(leg_tag_text(&paint, stop, false, false), "#3 SL -5");
    assert_eq!(
        leg_tag_text(&paint, stop, true, false),
        "#3 SL 95 -5 pts · on fill"
    );
}
