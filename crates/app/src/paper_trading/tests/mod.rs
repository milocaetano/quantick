// What the tab's paper decides alone - the reversal, the ticket's refusal,
// the ruler following the symbol - is tested in `quantick_session::paper_tests`.
//
// The `paper_trading.rs` unit tests, moved out of the file so a session
// opening the ticket to change one control no longer reads 3,651 lines of
// tests it did not ask for.
//
// They stay a child module of `crate::paper_trading` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.
//
// The file's second test module, `risk_tests`, sits beside this one in
// `risk_tests.rs`. It reaches this module's scope through its own
// `use super::*`, which carries `paper_trading`'s imports down the same
// way -- the shape `app/tests/` already uses across twelve files.

mod layout_tests;
mod light_plate_tests;
mod risk_tests;

// The items the split gave owners; `super::*` still carries the ticket
// itself, the module's constants and the account's vocabulary.
use super::paint_ctx::{PaintCtx, cmd_preview_layout};
use super::*;
// The desk's vocabulary and geometry, which the tests below arrange and
// measure against; the rules themselves are tested in `quantick_paper`.
use crate::paper_report::{load_history, report_from_history};
use crate::timezone::TzOffset;
use quantick_paper::desk::PaperDrag;
use quantick_paper::desk::geometry::{TAG_BUTTON_PX, TAG_GAP_PX};
use quantick_sim::EntryKind;

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

#[test]
fn utc_dates_format_from_the_same_civil_math() {
    assert_eq!(fmt_utc_date(1_773_666_068_000), "2026-03-16");
}

/// Esc puts the ruler away - but only once the gestures it was already
/// for have nothing to cancel. A standing distance must not shadow
/// disarming an order.
#[test]
fn escape_clears_the_ruler_but_only_after_an_armed_placement() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 40.0));
    assert_eq!(paper.desk.ruler.notches, 1, "the ruler stands");
    paper.desk.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });

    assert!(paper.cancel_interaction(), "the first press has work to do");
    assert!(paper.desk.armed.is_none(), "and it disarmed the placement");
    assert_eq!(
        paper.desk.ruler.notches, 1,
        "the distance survives - it was not what the trader was cancelling"
    );

    assert!(
        paper.cancel_interaction(),
        "the second press reaches the ruler"
    );
    assert_eq!(paper.desk.ruler.notches, 0);
    assert!(
        !paper.cancel_interaction(),
        "and then there is nothing left"
    );
}

/// A frame with the aim's modifier held and wheel travel to spend.
fn ruler_frame<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    pointer: egui::Pos2,
    scroll_y: f32,
) -> ChartInput<'a> {
    ChartInput {
        chart,
        aim_area: chart,
        scale: Some(scale),
        pointer: Some(pointer),
        primary_pressed: false,
        primary_down: false,
        primary_released: false,
        modifiers: egui::Modifiers {
            shift: true,
            ..Default::default()
        },
        canvas_claimed: false,
        on_chrome: false,
        scroll_y,
        middle_pressed: false,
        layer_visible: true,
    }
}

fn chart_and_scale(lo: f64, hi: f64) -> (egui::Rect, PriceScale) {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    (chart, PriceScale::from_range(lo, hi, 0.0, 400.0))
}

/// One pointer frame mid-chart, where nothing right-anchored lives.
fn frame<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    y: f32,
    pressed: bool,
    down: bool,
    released: bool,
) -> ChartInput<'a> {
    frame_at(chart, scale, egui::pos2(400.0, y), pressed, down, released)
}

/// One pointer frame at an exact position — for the controls that live
/// against the plot's right edge, which `frame`'s mid-chart x misses.
fn frame_at<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    pointer: egui::Pos2,
    pressed: bool,
    down: bool,
    released: bool,
) -> ChartInput<'a> {
    ChartInput {
        chart,
        aim_area: chart,
        scale: Some(scale),
        pointer: Some(pointer),
        primary_pressed: pressed,
        primary_down: down,
        primary_released: released,
        modifiers: egui::Modifiers::default(),
        canvas_claimed: false,
        on_chrome: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    }
}

/// A `frame` with held modifiers and a free pointer — the cmd-trading
/// gesture's shape of input.
fn cmd_frame<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    pressed: bool,
) -> ChartInput<'a> {
    ChartInput {
        chart,
        aim_area: chart,
        scale: Some(scale),
        pointer: Some(pointer),
        primary_pressed: pressed,
        primary_down: pressed,
        primary_released: false,
        modifiers,
        canvas_claimed: false,
        on_chrome: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    }
}

#[test]
fn an_armed_click_places_the_order_at_the_clicked_price_and_disarms() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.desk.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    let (chart, scale) = chart_and_scale(90.0, 110.0);
    // y = 300 sits at price 95 on this scale.
    let consumed = paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false));
    assert!(consumed, "the armed click never reaches the chart pan");
    assert!(paper.desk.armed.is_none(), "a successful placement disarms");
    assert_eq!(paper.account.venue().working_orders().len(), 1);
    assert_eq!(
        paper.account.venue().working_orders()[0].price,
        Some(Decimal::from(95))
    );
}

/// The lines say what a press would do before it happens (audit M3/M4):
/// draggable levels wear the resize cursor, an entry line with a
/// missing leg offers the create-drag, and empty tape asks nothing.
#[test]
fn hover_cursors_announce_draggable_and_creatable_lines() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.desk.ticket.stop_offset_text = "10".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 300.0), chart, &scale),
        Some(egui::CursorIcon::ResizeVertical),
        "the stop at 90 sits at y 300 and drags"
    );
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 200.0), chart, &scale),
        Some(egui::CursorIcon::PointingHand),
        "the entry at 100 is a click that flips money and points; a pull still grows the take profit"
    );
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 40.0), chart, &scale),
        None,
        "empty tape belongs to the chart"
    );
}

/// Off-screen render proof (for environments where no window can
/// present): the preview paints a dashed line, a label carrying
/// side+kind+qty, and the gutter chip with the snapped price.
#[test]
fn the_cmd_preview_paints_line_label_and_price_chip() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 300.0),
        shift,
        false,
    ));
    assert!(
        paper.desk.gesture.cmd_preview.is_some(),
        "the held key builds a preview"
    );

    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let paint = PaintCtx {
            palette: crate::theme::get(&egui::Context::default()),
            painter: &painter,
            chart_rect: chart,
            tag_right: chart.right(),
            axis_x: chart.right(),
            scale: &scale,
            reserved_chip_y: None,
            pointer: Some(egui::pos2(400.0, 300.0)),
        };
        paper.draw_cmd_preview(&paint, true);
    });
    let shapes = format!("{:?}", output.shapes);
    assert!(shapes.contains("BUY"), "the label names the side: {shapes}");
    assert!(
        shapes.contains("90"),
        "the gutter chip carries the snapped price"
    );
    let segments = shapes.matches("LineSegment").count();
    assert!(
        segments >= 8,
        "a dashed line paints as many short segments, got {segments}"
    );
}

/// A pointer from another pane's band paints nothing here: the label
/// rides an x, so laying it out against a band that does not hold that
/// x would put a click target off the end of the plot.
#[test]
fn the_aim_paints_only_in_the_band_it_was_aimed_in() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(700.0, 300.0),
        shift,
        false,
    ));
    assert!(
        paper.desk.gesture.cmd_preview.is_some(),
        "aimed on this band"
    );

    // The same simulator drawn against a narrower band — the other
    // pane of a split, whose right edge stops short of that x.
    let other = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 400.0));
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        paper.draw_cmd_preview(
            &PaintCtx {
                palette: crate::theme::get(&egui::Context::default()),
                painter: &painter,
                chart_rect: other,
                tag_right: other.right(),
                axis_x: other.right(),
                scale: &scale,
                reserved_chip_y: None,
                pointer: Some(egui::pos2(700.0, 300.0)),
            },
            true,
        );
    });
    let shapes = format!("{:?}", output.shapes);
    assert!(
        !shapes.contains("BUY"),
        "a foreign pointer paints no label: {shapes}"
    );
}

/// Paint one frame of the paper layer and return its shape dump — the
/// off-screen render proof the tag tests read.
fn layer_shapes(
    paper: &PaperTrading,
    chart: egui::Rect,
    scale: &PriceScale,
    pointer: Option<egui::Pos2>,
) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        paper.draw_layer(
            &painter,
            chart,
            chart.right(),
            chart.right(),
            scale,
            None,
            pointer,
            pointer.is_some(),
        );
    });
    format!("{:?}", output.shapes)
}

/// A resting order rests as a pill and opens under the pointer: the
/// full tag used to sit over the candles at the live price all
/// session. The pill still names side, kind and size — an order line
/// is accent-coloured whatever its side, so dropping the word would
/// leave the chart unable to say what waits there. Off-screen render
/// proof.
#[test]
fn a_resting_order_tag_is_a_pill_until_the_pointer_reaches_it() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // A buy limit at 90 — y 300 on this scale.
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id.0;

    // The frame the pointer arrives in is the frame that decides; the
    // paint reads that decision rather than re-asking its own pointer.
    let frame_at = |paper: &mut PaperTrading, y: f32| {
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, y),
            egui::Modifiers::default(),
            false,
        ));
        layer_shapes(paper, chart, &scale, Some(egui::pos2(400.0, y)))
    };

    let resting = frame_at(&mut paper, 60.0);
    assert!(
        resting.contains("BUY LMT 1"),
        "the pill still names side, kind and size: {resting}"
    );
    assert!(
        !resting.contains(&format!("#{id}")),
        "the id waits until you mean to act on it: {resting}"
    );
    assert!(
        !resting.contains("@ 90"),
        "the price is the gutter chip's job: {resting}"
    );
    assert!(!resting.contains('×'), "and no ✕ over the candles");

    let opened = frame_at(&mut paper, 300.0);
    assert!(
        opened.contains(&format!("#{id} BUY LMT 1 @ 90")),
        "reaching for it states the order whole: {opened}"
    );
    assert!(opened.contains('×'), "…and offers the cancel: {opened}");
}

/// The ✕ and its press are one thing, checked the only way that means
/// anything: sweep a pointer down the ✕ column, and for every stop ask
/// **the painter** whether a ✕ came out and **`control_at`** whether a
/// cancel is offered. Both sides run off one `handle_chart_input`, so
/// this fails the moment they are handed different pointers, different
/// rects, or different `dragged` terms again.
#[test]
fn a_tag_offers_its_cancel_exactly_while_it_paints_one() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // Two orders: one near the top edge, where `clamp_tag_center`
    // pushes the tag off its own line and the two rows part company,
    // and one mid-plot where they coincide. Above the mark a buy rests
    // as a stop, below it as a limit.
    assert!(paper.place_resting(Side::Buy, EntryKind::Stop, 119.5));
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let x = chart.right() - TAG_GAP_PX - TAG_BUTTON_PX / 2.0;
    let mut painted_anywhere = false;
    for step in -4_i16..=84 {
        let pointer = egui::pos2(x, f32::from(step) * 5.0);
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            pointer,
            egui::Modifiers::default(),
            false,
        ));
        let painted = layer_shapes(&paper, chart, &scale, Some(pointer)).contains('×');
        let pressable = matches!(
            paper.control_at(pointer, chart, &scale),
            Some(PaperControl::CancelOrder(_))
        );
        assert_eq!(painted, pressable, "at y {}", pointer.y);
        painted_anywhere |= painted;
    }
    assert!(painted_anywhere, "the sweep crossed a ✕ at all");
}

/// The press side is fed a pointer the paint side never sees — the
/// pane nulls `hover_pos` over its own chrome while `latest_pos`
/// survives — so the ✕'s offer must come from the frame's *input*, not
/// from whatever pointer each side happens to hold.
#[test]
fn a_cancel_offered_this_frame_survives_a_paint_with_no_pointer() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let on_row = egui::pos2(chart.right() - TAG_GAP_PX - TAG_BUTTON_PX / 2.0, 300.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        on_row,
        egui::Modifiers::default(),
        false,
    ));
    assert!(
        layer_shapes(&paper, chart, &scale, None).contains('×'),
        "the paint follows the frame's decision, not its own pointer"
    );
    assert!(matches!(
        paper.control_at(on_row, chart, &scale),
        Some(PaperControl::CancelOrder(_))
    ));

    // Pointer off the row: neither side offers anything.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(on_row.x, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    assert!(!layer_shapes(&paper, chart, &scale, Some(on_row)).contains('×'));
    assert!(paper.control_at(on_row, chart, &scale).is_none());
}

/// A bracket leg's tag rests as a pill, in the order tag's grammar, and
/// opens under the pointer: the whole statement on every leg used to bury
/// the candles beside the axis. At rest an unfilled order's leg keeps its
/// id — two orders' legs are otherwise identical — and its signed points;
/// the price, `on fill` and the ✕ wait for the pointer.
#[test]
fn a_bracket_leg_tag_is_a_pill_until_the_pointer_reaches_it() {
    let mut paper = PaperTrading::new();
    // The capture hook forces every tag open; this test is about the resting pill.
    paper.desk.gesture.order_hover_force = false;
    paper.seed(&print(0, 100));
    paper.desk.ticket.stop_offset_text = "5".to_owned();
    paper.desk.ticket.profit_offset_text = "15".to_owned();
    // A buy limit at 95 (y 250): its stop at 90 (y 300), target at 110 (y 100).
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
    let id = paper.working_orders()[0].id.0;
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let frame_at = |paper: &mut PaperTrading, y: f32| {
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, y),
            egui::Modifiers::default(),
            false,
        ));
        layer_shapes(paper, chart, &scale, Some(egui::pos2(400.0, y)))
    };

    let resting = frame_at(&mut paper, 175.0);
    assert!(
        resting.contains(&format!("\"#{id} SL -5 pts × 1\"")),
        "the pill names the order, the leg and what it risks: {resting}"
    );
    assert!(
        resting.contains(&format!("\"#{id} TP +15 pts × 1\"")),
        "{resting}"
    );
    assert!(!resting.contains("on fill"), "the words wait: {resting}");
    // The clear is its own glyph; the `×` inside `pts × 1` is the quantity.
    assert!(!resting.contains("\"×\""), "and no ✕ beside the axis");

    let opened = frame_at(&mut paper, 300.0);
    assert!(
        opened.contains(&format!("#{id} SL -5 pts × 1 · on fill")),
        "reaching for it states the leg whole: {opened}"
    );
    assert!(opened.contains("\"×\""), "…and offers the clear: {opened}");
    assert!(
        opened.contains(&format!("\"#{id} TP +15 pts × 1\"")),
        "only the row under the pointer opens: {opened}"
    );
}

/// A live position's leg needs no id — there is one position — and rests
/// just as small.
#[test]
fn a_position_leg_rests_as_the_leg_and_its_points() {
    let mut paper = PaperTrading::new();
    paper.desk.gesture.order_hover_force = false;
    paper.seed(&print(0, 100));
    paper.desk.ticket.stop_offset_text = "10".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 175.0),
        egui::Modifiers::default(),
        false,
    ));
    let shapes = layer_shapes(&paper, chart, &scale, Some(egui::pos2(400.0, 175.0)));
    assert!(shapes.contains("\"SL -10 pts × 1\""), "{shapes}");
    assert!(
        !shapes.contains("SL 90"),
        "the price is the chip's: {shapes}"
    );
}

/// The leg's ✕ obeys the order's law: sweep the ✕ column, and at every
/// stop ask the painter whether a ✕ came out and `control_at` whether a
/// press is offered. The target sits against the top edge, where its tag
/// is clamped off its own line and the two rows part company.
#[test]
fn a_leg_offers_its_clear_exactly_while_it_paints_one() {
    let mut paper = PaperTrading::new();
    paper.desk.gesture.order_hover_force = false;
    paper.seed(&print(0, 100));
    paper.desk.ticket.stop_offset_text = "5".to_owned();
    paper.desk.ticket.profit_offset_text = "24.5".to_owned();
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let x = chart.right() - TAG_GAP_PX - TAG_BUTTON_PX / 2.0;
    let mut cleared = Vec::new();
    for step in -4_i16..=84 {
        let pointer = egui::pos2(x, f32::from(step) * 5.0);
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            pointer,
            egui::Modifiers::default(),
            false,
        ));
        let painted = layer_shapes(&paper, chart, &scale, Some(pointer)).contains("\"×\"");
        let control = paper.control_at(pointer, chart, &scale);
        let pressable = matches!(
            control,
            Some(PaperControl::CancelOrder(_) | PaperControl::ClearLeg { .. })
        );
        assert_eq!(painted, pressable, "at y {}", pointer.y);
        if let Some(PaperControl::ClearLeg { leg, .. }) = control {
            cleared.push(leg);
        }
    }
    assert!(
        cleared.contains(&Leg::StopLoss),
        "the sweep cleared the stop"
    );
    assert!(
        cleared.contains(&Leg::TakeProfit),
        "and the edge-clamped target"
    );
}

/// A dragged order keeps every field: a trader repricing one is
/// reading the number they are moving, and the pointer is on the line
/// they grabbed, not on the tag.
#[test]
fn a_dragged_order_keeps_its_full_statement() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id;
    paper.desk.gesture.drag = PaperDrag::Order(id);
    paper.desk.gesture.drag_price = Some(88.0);
    // The frame decides; the paint reads. A button-free frame leaves
    // the drag exactly where it was.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    let shapes = layer_shapes(&paper, chart, &scale, None);
    assert!(
        shapes.contains(&format!("#{} BUY LMT 1 @ 88", id.0)),
        "the drag reads the price it is moving: {shapes}"
    );
    assert!(
        !shapes.contains('×'),
        "a moving order offers no cancel, as before: {shapes}"
    );
}

/// The capture hook's orders reach the account on the first print, once.
/// Where they rest is `quantick_paper::account::capture_orders`'s, tested
/// there.
#[test]
fn the_orders_hook_rests_through_the_account_once() {
    let mut paper = PaperTrading::new();
    let one = quantick_paper::account::CaptureOrders {
        rungs: 1,
        bracket: false,
    };
    paper.capture_orders = Some(one);
    paper.on_trade(&print(1, 100_000));
    assert!(paper.capture_orders.is_none(), "placed once, never again");
    assert_eq!(paper.working_orders().len(), 2);
    paper.on_trade(&print(2, 100_000));
    assert_eq!(paper.working_orders().len(), 2, "a second print adds none");
}

/// The capture hook opens every tag with nobody at the mouse — the
/// pill's open form is otherwise unreachable from a scripted run.
#[test]
fn the_order_hover_hook_opens_the_tag_with_no_pointer() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id.0;
    assert!(
        !layer_shapes(&paper, chart, &scale, None).contains(&format!("#{id}")),
        "no hand, no open tag"
    );
    paper.desk.gesture.order_hover_force = true;
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    assert!(
        layer_shapes(&paper, chart, &scale, None).contains(&format!("#{id} BUY LMT 1 @ 90")),
        "the hook supplies the hand"
    );
}

/// One hover, two surfaces: the dock row already lifted the chart
/// line, and now it opens the tag too.
#[test]
fn hovering_the_dock_row_opens_the_chart_tag() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id;
    paper.desk.gesture.hovered_order = Some(id);
    // The dock draws before the canvas, so the frame that carries the
    // row's hover is the frame the chart reads.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    assert!(
        layer_shapes(&paper, chart, &scale, None).contains(&format!("#{} BUY LMT 1 @ 90", id.0)),
        "the row's hover reaches the chart"
    );
}

/// A band too short to hold a tag is reachable — `split_panes` carves
/// the indicator strips out of the plot with no floor of its own — and
/// `f32::clamp` panics rather than saturating once its bounds cross.
/// Every tag, every ✕ hit-test and the aim's own layout run through
/// this, so the panic would take a live session down.
#[test]
fn a_band_too_short_for_a_tag_survives_the_paint_and_the_press() {
    // The clamp's own table is `quantick_paper::desk`'s; the paint and the
    // press both survive it end to end.
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let sliver = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 12.0));
    let scale = PriceScale::from_range(80.0, 120.0, 0.0, 12.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let pointer = egui::pos2(400.0, 6.0);
    paper.handle_chart_input(&cmd_frame(
        sliver,
        &scale,
        pointer,
        egui::Modifiers::default(),
        false,
    ));
    let _ = layer_shapes(&paper, sliver, &scale, Some(pointer));
    let _ = paper.control_at(pointer, sliver, &scale);
    let _ = cmd_preview_layout(sliver, sliver.right(), pointer);
}

#[test]
fn a_timeline_reset_journals_the_flatten_and_clears_the_form_state() {
    let dir = crate::scratch::ScratchDir::new("paper-reset-test");
    let mut paper = PaperTrading::new();
    paper.account.redirect_history_dir(dir.path().to_path_buf());
    paper.set_symbol("RESETX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    paper.desk.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    paper.on_timeline_reset();
    assert!(paper.account.venue().position().is_none());
    assert!(
        paper.desk.armed.is_none(),
        "an armed click dies with the timeline"
    );
    assert!(
        paper.account.peek_toast().is_some(),
        "the flatten is never silent"
    );
    let history = load_history(&dir, Some("RESETX"), &[]);
    assert_eq!(history.rows.len(), 1);
    assert_eq!(
        report_from_history(&history).trades,
        1,
        "the reset exit is a real, journaled trade"
    );
}

// The stored-pick-vs-configured-base precedence now lives in
// `paper_home::chosen`, tested there beside the documents default.

/// The panel's folder picker retargets everything downstream: the next
/// close opens a new session file under the new home, and the ledger
/// and report re-read from it. Files already written stay put.
#[test]
fn switching_the_trades_dir_retargets_journal_ledger_and_report() {
    let dir_a = crate::scratch::ScratchDir::new("paper-dir-a");
    let dir_b = crate::scratch::ScratchDir::new("paper-dir-b");
    let mut paper = PaperTrading::new();
    paper
        .account
        .redirect_history_dir(dir_a.path().to_path_buf());
    paper.set_symbol("SWITCHX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 103));
    assert!(
        paper.account.journal_path().is_some(),
        "the close journaled under A"
    );
    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    assert!(paper.account.report_state().saved_rows_loaded().is_some());

    paper.account.set_trades_dir(dir_b.path().to_path_buf());
    assert_eq!(paper.account.trades_dir(), dir_b.path());
    assert!(
        paper.account.journal_path().is_none(),
        "the next close opens a new session file under B"
    );
    assert!(
        paper.account.report_state().saved_rows_loaded().is_none(),
        "the ledger re-reads from the new home"
    );
    assert!(
        paper.account.peek_toast().is_some(),
        "the switch is never silent"
    );
    assert!(
        dir_a.join("SWITCHX").exists(),
        "files already written stay where they are"
    );
}

/// The ledger's cache reads every saved file except the live session's
/// own (its trades are already in the simulator), and remembers which
/// symbol each row came from.
#[test]
fn the_ledger_cache_excludes_the_live_session_file() {
    let dir = crate::scratch::ScratchDir::new("paper-ledger-test");
    let mut paper = PaperTrading::new();
    paper.account.redirect_history_dir(dir.path().to_path_buf());
    paper.set_symbol("LEDGX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));
    assert!(
        paper.account.journal_path().is_some(),
        "the close journaled"
    );

    // An earlier session's file, written by hand beside the live one.
    let trade = ClosedTrade {
        side: Side::Sell,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(200),
        exit_price: Decimal::from(195),
        opened_ms: 10,
        closed_ms: 20,
        pnl_points: Decimal::from(5),
        exit_reason: quantick_sim::ExitReason::Manual,
        entry_agg_id: Some(1),
        exit_agg_id: Some(2),
        mae_points: Some(Decimal::ZERO),
        mfe_points: Some(Decimal::from(5)),
    };
    let mut text = history::write_header("LEDGX", history::SessionSource::Live);
    text.push_str(&history::write_trade(&trade));
    std::fs::write(dir.join("LEDGX").join("20200101-000000.csv"), text)
        .expect("the earlier session file writes");

    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    let cache = paper
        .account
        .report_state()
        .saved_rows_loaded()
        .expect("loaded");
    assert_eq!(
        cache.len(),
        1,
        "the live session's file is excluded, the earlier one loads"
    );
    assert_eq!(cache[0].symbol, "LEDGX");
    assert_eq!(cache[0].trade, trade);
    assert_eq!(cache[0].source, Some(history::SessionSource::Live));
}

#[test]
fn the_ledger_never_lists_this_sessions_trades_twice_after_a_retarget() {
    // Hunt-confirmed: close live, flip to replay (what a same-symbol
    // replay open does), reload the ledger — the live session's file
    // must stay excluded, or every trade counts twice in the totals
    // and the export.
    let mut paper = PaperTrading::new();
    paper.set_symbol("DUPX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));
    assert_eq!(paper.account.venue().closed_trades().len(), 1);

    paper
        .account_mut()
        .set_session_source(history::SessionSource::Replay);
    paper.on_timeline_reset();
    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    let cache = paper
        .account
        .report_state()
        .saved_rows_loaded()
        .expect("loaded");
    assert_eq!(
        cache.len(),
        0,
        "the session's own files stay excluded across the retarget"
    );
}

/// A revealed page must survive the ledger's own lazy first load.
/// `QUANTICK_LEDGER_PAGES` sets the page count during construction,
/// long before the Trades tab is first drawn; the load that tab
/// triggers used to reset it, so the hook reached page one and the
/// state it exists to photograph was unreachable.
#[test]
fn a_revealed_page_survives_the_ledgers_lazy_first_load() {
    let dir = crate::scratch::ScratchDir::new("paper-pages-test");
    let mut paper = PaperTrading::new();
    paper.account.redirect_history_dir(dir.path().to_path_buf());
    paper.set_symbol("PAGEX");

    paper.account.report_state_mut().autostart_ledger_pages(3);
    assert_eq!(paper.account.report_state().revealed_pages(), 3);
    // What the first `draw_trades_tab` does before painting a row.
    assert!(paper.account.report_state().saved_rows_loaded().is_none());
    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    assert_eq!(
        paper.account.report_state().revealed_pages(),
        3,
        "the lazy load must not retire the hook's page count"
    );
    // And what every tab does on every drain: sync the journal to its
    // symbol. This runs on the frame, so it must not retire the page
    // either — the hook set it before the feed had a symbol at all.
    paper.set_symbol("PAGEY");
    assert_eq!(
        paper.account.report_state().revealed_pages(),
        3,
        "the per-frame symbol sync must not retire the page"
    );

    // A *scope* change is the one thing that does reset it: a deep
    // page cannot survive a list it was never counted against.
    {
        let (state, env) = paper.report_parts();
        state.rescope_ledger(&env)
    };
    assert_eq!(paper.account.report_state().revealed_pages(), 1);
}

#[test]
fn a_close_refreshes_an_open_report_by_itself() {
    let utc = TzOffset::new(0);
    let mut paper = PaperTrading::new();
    paper.set_symbol("FRESH");
    {
        let (state, env) = paper.report_parts();
        state.open(&env)
    };
    paper
        .account
        .report_state_mut()
        .ensure_report_view(utc, quantick_paper::pnl::PnlReader::plain());
    assert!(
        paper
            .account
            .report_state()
            .view_rows()
            .expect("view")
            .is_empty(),
        "nothing saved yet"
    );

    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));

    paper
        .account
        .report_state_mut()
        .ensure_report_view(utc, quantick_paper::pnl::PnlReader::plain());
    let view = paper.account.report_state().view_rows().expect("refreshed");
    assert_eq!(
        view.len(),
        1,
        "the close re-read the journal without a manual refresh"
    );
}

/// C5: a live print reaches the account through the tab's paper, and the
/// host re-reads the open report when that print closes a journaled trade.
#[test]
fn live_print_fills_a_resting_limit_through_tab_paper() {
    let utc = TzOffset::new(0);
    let dir = crate::scratch::ScratchDir::new("paper-tab-limit");
    let mut paper = PaperTrading::new();
    paper.account.redirect_history_dir(dir.path().to_path_buf());
    paper.set_symbol("LIMITX");
    {
        let (state, env) = paper.report_parts();
        state.open(&env)
    };
    paper.seed(&print(0, 100));
    let events = paper.account.venue_mut().submit(
        quantick_sim::OrderIntent::limit(Side::Buy, Decimal::ONE, Decimal::from(95))
            .with_bracket(Bracket::whole(None, Some(Decimal::from(105)))),
    );
    paper.account.handle_events(events);
    assert_eq!(paper.working_orders().len(), 1, "the limit rests");

    paper.on_trade(&print(1, 95));
    assert!(
        !paper.is_flat(),
        "the print crossed the limit and it filled"
    );

    paper.on_trade(&print(2, 105));
    assert_eq!(paper.session_trades().len(), 1, "the target closed it");
    paper
        .account
        .report_state_mut()
        .ensure_report_view(utc, quantick_paper::pnl::PnlReader::plain());
    let view = paper.account.report_state().view_rows().expect("re-read");
    assert_eq!(view.len(), 1, "the open report re-read the journal");
}

#[test]
fn default_cmd_trading_does_not_arm_a_sell_with_ctrl() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 100.0),
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        false,
    ));
    assert!(paper.desk.gesture.cmd_preview.is_none());
}

impl PaperTrading {
    /// Arm the ticket's click as its Buy/Sell button does, for the window
    /// tests that press the chart with a ticket armed.
    pub(crate) fn arm_for_tests(&mut self, side: Side, kind: EntryKind) {
        self.desk.armed = Some(ArmedPlacement { side, kind });
    }
}
