//! The desk's rules, tested without a window.
//!
//! Moved here with the code they cover: the cmd aim's kind table, its
//! tokens and its layout, the capture hook's parse, and the geometry the
//! paint and the press share. The host's own tests still drive the whole
//! gesture through its window-typed input; these pin the rules underneath.

use quantick_engine::Side;
use quantick_sim::EntryKind;
use rust_decimal::Decimal;

use super::cmd::{
    CMD_LABEL_CURSOR_GAP_PX, CMD_LABEL_WIDTH_PX, CMD_LINE_MIN_PX, CmdEntryKind, CmdModifier,
    CmdPreviewForce, CmdTradingSettings, cmd_preview_layout, resolve_cmd_kind,
};
use super::geometry::{
    Bounds, CHIP_CLEAR_PX, Point, TAG_HEIGHT_PX, clamp_tag_center, dodged_chip_y,
};

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
