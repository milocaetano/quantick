// The `pane.rs` unit tests, moved out of the file so a session opening
// `pane.rs` to change one gesture no longer reads 2,700 lines of tests it
// did not ask for. They stay a child module of `crate::pane` rather than
// moving to `crates/app/tests/`: a child sees its ancestor's private items,
// so the move costs no widened visibility in production code, and the one
// `use super::*` below is the same line the module had inline.

use super::*;
use crate::indicator_worker::IndicatorEvent;

/// A frame nobody builds is a surface nobody draws. The strip and the
/// lane's marks are the two surfaces that need the projection without
/// being the depth map or the bubbles, so each of them alone has to keep
/// it running — otherwise the lane sits reserved and unmarked (a band you
/// cannot tell from a dead feed) while its menu entry reads as on.
/// The two panes are configured apart, so the right-click has to say which
/// one it landed on — and it says so from the divider the draw published,
/// not from a second copy of the lane's geometry.
#[test]
fn a_right_click_is_the_tapes_only_on_the_tapes_side_of_the_divider() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());

    // No lane drawn yet: there is one pane, and every click is its.
    assert!(!pane.click_on_tape(0.0));
    assert!(!pane.click_on_tape(999.0));

    pane.frame.lane_divider_x = Some(700.0);
    assert!(!pane.click_on_tape(699.9), "the candles' last pixel");
    assert!(pane.click_on_tape(700.0), "the divider belongs to the tape");
    assert!(pane.click_on_tape(880.0), "and so does everything past it");

    // A lane that goes away takes its side of the question with it: no
    // click may configure a tape that is no longer drawn.
    pane.frame.lane_divider_x = None;
    assert!(!pane.click_on_tape(880.0));
}

#[test]
fn the_strip_and_the_lane_marks_each_keep_the_projection_alive() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let mut discarded = LayerActions::default();
    pane.set_layer_visible(ChartLayer::LiveStrip, false, &mut discarded);
    pane.set_layer_visible(ChartLayer::LaneMarks, false, &mut discarded);
    assert!(!pane.projection_demand(), "nobody is asking");

    pane.set_layer_visible(ChartLayer::LiveStrip, true, &mut discarded);
    assert!(pane.projection_demand(), "the strip alone asks");

    pane.set_layer_visible(ChartLayer::LiveStrip, false, &mut discarded);
    pane.set_layer_visible(ChartLayer::LaneMarks, true, &mut discarded);
    assert!(pane.projection_demand(), "the lane's marks alone ask");
}

/// A chart that has never been configured follows the window's setup —
/// which is what keeps one chart behaving like a global preference —
/// and a chart configured on its own ignores it, which is what a split
/// layout needs. Inverting this resolution would silently give both
/// charts one reading.
#[test]
fn a_chart_follows_the_window_until_it_is_configured_itself() {
    use crate::footprint_config::{FootprintConfig, FootprintStyle};
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let window = FootprintConfig::default();
    assert_eq!(pane.footprint_config(&window), &window, "virgin follows");

    let own = FootprintConfig {
        style: FootprintStyle::Ladder,
        show_numbers: false,
        ..FootprintConfig::default()
    };
    pane.set_footprint_override(Some(own.clone()));
    assert_eq!(
        pane.footprint_config(&window),
        &own,
        "configured keeps its own"
    );
    assert_ne!(pane.footprint_config(&window), &window);

    // "follow the default again".
    pane.set_footprint_override(None);
    assert_eq!(pane.footprint_config(&window), &window);
}

/// A fixed-range profile is the footprint ladders' second consumer: with
/// the layer itself hidden, a placed (or in-flight) profile object keeps
/// accumulation wanted, and deleting the last one releases it — the gate
/// `draw_chart` feeds into `set_footprint_enabled`.
#[test]
fn a_range_profile_drawing_wants_the_footprint_ladders() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    assert!(!pane.wants_range_profile(), "empty store wants nothing");

    let frvp = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .expect("frvp is registered");
    // The first anchor alone is an in-flight draft — accumulation must
    // already be on, or the preview would show an empty profile.
    assert!(
        !pane
            .drawings
            .place(frvp, crate::drawings::ChartPoint::at(1.0, 100.0))
    );
    assert!(pane.wants_range_profile(), "a draft already wants ladders");
    assert!(
        pane.drawings
            .place(frvp, crate::drawings::ChartPoint::at(5.0, 105.0))
    );
    assert!(pane.wants_range_profile());

    assert_eq!(
        pane.drawings.delete_selected(false),
        crate::drawings::DeleteOutcome::Deleted
    );
    assert!(
        !pane.wants_range_profile(),
        "deleting the last profile releases the ladders"
    );
}

/// …and wanting the ladders is *not* asking for the layer. The two were
/// one flag once, and the trader saw the result: a profile dropped on a
/// fresh chart restyled every candle — sidebar lane, faded body, spacing
/// that opened up as they zoomed — for a footprint that never painted.
/// The candle dressing reads `layer_visible`, the accumulation switch
/// reads this; they must be able to disagree.
#[test]
fn a_range_profile_wants_the_ladders_without_asking_for_the_layer() {
    let style = crate::style::ChartStyle::default();
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let frvp = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .expect("frvp is registered");
    assert!(
        !pane
            .drawings
            .place(frvp, crate::drawings::ChartPoint::at(1.0, 100.0))
    );
    assert!(
        pane.drawings
            .place(frvp, crate::drawings::ChartPoint::at(5.0, 105.0))
    );

    assert!(
        pane.wants_range_profile(),
        "the profile folds ladders, so accumulation is on"
    );
    assert!(
        !pane.layer_visible(ChartLayer::Footprint, &style),
        "and the layer is still hidden, so the candles are not dressed"
    );

    pane.set_layer_visible(
        ChartLayer::Footprint,
        true,
        &mut crate::chart_layers::LayerActions::default(),
    );
    assert!(
        pane.layer_visible(ChartLayer::Footprint, &style),
        "turning the layer on is the only thing that dresses a candle"
    );
    assert!(pane.wants_range_profile());
}

/// The tape switch reaches the canvas through one number: the band's
/// width. Zero is what every downstream reader already treats as "there is
/// no lane" — no divider, so no carve, no rungs, no tape time axis.
#[test]
fn a_tape_that_is_off_publishes_no_divider_for_anything_to_hang_off() {
    use quantick_orderflow::LiveLaneStyle;
    let chart = egui::Rect::from_min_max(egui::pos2(60.0, 80.0), egui::pos2(1_000.0, 700.0));

    let mut lane = LiveLaneStyle::default();
    let on = lane.resolved_width_px(chart.width());
    assert!(on > 0.0, "a tape that is on reserves a band");
    assert!(crate::orderflow_render::lane_divider_x(chart, on).is_some());
    assert!(lane_rungs(on) > 0, "and a ladder is walked for it");

    lane.enabled = false;
    let off = lane.resolved_width_px(chart.width());
    assert_eq!(off, 0.0, "a tape that is off reserves nothing");
    assert!(
        crate::orderflow_render::lane_divider_x(chart, off).is_none(),
        "so the canvas is not split and the candles take all of it"
    );
    assert_eq!(lane_rungs(off), 0, "and no ladder is walked on its account");
}

/// Everything one draw call put on the canvas, as text.
fn painted(draw: impl Fn(&egui::Painter)) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        draw(&ctx.layer_painter(egui::LayerId::background()));
    });
    format!("{:?}", output.shapes)
}

/// Width one of the axis labels takes, measured the way the axis measures
/// it. The collision cases below are derived from these rather than from
/// literals: a font bump would move every number, and a test that failed
/// for that reason would read as a geometry regression on a branch that
/// changed nothing.
fn axis_label_width(text: &str) -> f32 {
    let ctx = egui::Context::default();
    let mut width = 0.0;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        width = ctx
            .layer_painter(egui::LayerId::background())
            .layout_no_wrap(
                text.to_owned(),
                egui::FontId::monospace(LANE_AXIS_FONT_PX),
                theme::TEXT_MUTED,
            )
            .size()
            .x;
    });
    width
}

/// Clip rect of each shape the paint call emitted.
///
/// Read off the shapes rather than matched against egui's `Debug` string:
/// an upstream bump that renders a `Rect` differently would otherwise turn
/// this red on a branch that changed nothing, which is the failure this
/// test was already rewritten once to avoid.
fn painted_clips(painted: &str) -> Vec<egui::Rect> {
    painted
        .split("ClippedShape { clip_rect: [[")
        .skip(1)
        .filter_map(|rest| {
            let head = rest.split("]]").next()?;
            let numbers: Vec<f32> = head
                .replace("] - [", " ")
                .split_whitespace()
                .filter_map(|value| value.parse().ok())
                .collect();
            match numbers[..] {
                [left, top, right, bottom] => Some(egui::Rect::from_min_max(
                    egui::pos2(left, top),
                    egui::pos2(right, bottom),
                )),
                _ => None,
            }
        })
        .collect()
}

/// Left edge of each text shape, from the paint call's own output.
///
/// Anchored on `TextShape` rather than on `pos:` alone: a galley's debug
/// output carries a `pos` for every glyph inside it, so the looser split
/// returns dozens of offsets relative to the text instead of the handful
/// of placements on the canvas.
fn painted_positions(painted: &str) -> Vec<f32> {
    painted
        .split("TextShape { pos: [")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next()?.parse().ok())
        .collect()
}

/// The axis under the tape speaks only when the tape has fallen behind,
/// and the window label does not move when it does.
///
/// The caption is what stops an empty tape from reading as a still market,
/// so a regression here is silent by construction: the canvas would look
/// exactly like the defect this branch exists to end. The placement is
/// pinned for its own reason — a caption sliding under a tape being read
/// for flow is a cost the reading pays.
#[test]
fn the_tape_axis_speaks_only_when_the_tape_is_behind() {
    let pane = ChartPane::flow(1, BarSpec::Tick(100), "WINV26".to_owned());
    // Wide enough for the longest pair, derived rather than guessed: the
    // fit rule reserves the warning twice over plus two gaps on each side,
    // so a strip picked by eye lands inside the yield band and the test
    // starts asserting on the wrong branch.
    let widest = axis_label_width("last print 6 s back");
    let roomy_px = axis_label_width("tape · 30 s") + 2.0 * widest + 4.0 * LANE_AXIS_GAP_PX;
    let strip = egui::Rect::from_min_max(
        egui::pos2(600.0, 980.0),
        egui::pos2(600.0 + roomy_px + 1.0, 1000.0),
    );
    let axis = |age: Option<quantick_orderflow::TapeAge>| {
        painted(|painter| pane.draw_lane_time_axis(painter, Some(strip), 30_000, age))
    };

    let late_by = |ms| Some(quantick_orderflow::TapeAge::Behind(ms));
    let current = axis(None);
    assert!(
        current.contains("tape") && !current.contains("print"),
        "a current tape says what it is showing and nothing else"
    );
    // Under the threshold: an ordinary lull, and the axis is *identical*
    // to a tape that never paused. Identical rather than merely similar —
    // a caption that flickered on every quiet moment is one the trader
    // learns to stop reading.
    assert_eq!(axis(late_by(3_000)), current);

    let behind = axis(late_by(6_000));
    assert!(
        behind.contains("last print 6 s back"),
        "a tape 6 s behind has to say so: {behind}"
    );
    assert!(
        behind.contains("tape"),
        "and it keeps saying what it is showing: {behind}"
    );
    // Past the window nothing is left on the tape to point at, so the
    // wording stops describing a mark the reader would go looking for.
    let starved = axis(late_by(41_000));
    assert!(
        starved.contains("no print for 41 s"),
        "an empty tape must say why it is empty: {starved}"
    );

    // Three widths, each naming a way the fit rule can be wrong, all
    // derived from the labels' real widths so a font bump recalibrates
    // them instead of turning this red.
    //
    // The window keeps the *centre*, so it grows by half its width towards
    // the warning and meets it after only half the room. Subtracting the
    // warning once (`naive`) overlaps them outright; subtracting it twice
    // but counting the edge inset as the only gap (`touching`) leaves them
    // legal at zero pixels apart. Both are wrong, and neither shows up at
    // a comfortable window size.
    let warning_px = axis_label_width("no print for 41 s");
    let window_px = axis_label_width("tape · 30 s");
    let naive = window_px + warning_px + LANE_AXIS_GAP_PX;
    let touching = window_px + 2.0 * warning_px + 2.0 * LANE_AXIS_GAP_PX;
    let honest = window_px + 2.0 * warning_px + 4.0 * LANE_AXIS_GAP_PX;
    assert!(
        naive < touching && touching < honest,
        "the bands must exist"
    );

    let axis_at = |width: f32| {
        let strip =
            egui::Rect::from_min_max(egui::pos2(600.0, 980.0), egui::pos2(600.0 + width, 1000.0));
        painted(|painter| pane.draw_lane_time_axis(painter, Some(strip), 30_000, late_by(41_000)))
    };
    for (width, what) in [
        (
            (naive + touching) / 2.0,
            "the labels would overlap outright",
        ),
        (
            (touching + honest) / 2.0,
            "the labels would touch with no gap",
        ),
    ] {
        let colliding = axis_at(width);
        assert!(colliding.contains("no print for 41 s"), "{colliding}");
        assert!(
            !colliding.contains("tape ·"),
            "at {width} px {what}, so the window label has to yield: {colliding}"
        );
    }

    // Past the last band both are drawn, with the gap the constant exists
    // to provide genuinely between them.
    let both = axis_at(honest + 1.0);
    assert!(both.contains("no print for 41 s"), "{both}");
    assert!(both.contains("tape ·"), "{both}");
    let placed = painted_positions(&both);
    assert_eq!(placed.len(), 2, "two labels, two placements: {both}");
    let (window_x, warning_x) = (placed[0], placed[1]);
    assert!(
        warning_x - (window_x + window_px) >= LANE_AXIS_GAP_PX,
        "the two labels are {} px apart, closer than the gap they reserve",
        warning_x - (window_x + window_px)
    );

    // A lane too narrow for the warning: the label is right-aligned when
    // it fits and hard left when it does not, because the clip decides
    // *which end* gets cut and for this label that is the difference
    // between a shortened sentence and a wrong number. Right-aligned, a
    // 40 px strip cuts the head off "no print for 1 min 30 s" and leaves
    // "30 s" in warn colour — ninety seconds of silence read as three.
    let hair = egui::Rect::from_min_max(egui::pos2(600.0, 980.0), egui::pos2(640.0, 1000.0));
    let squeezed = painted(|painter| {
        pane.draw_lane_time_axis(
            painter,
            Some(hair),
            30_000,
            Some(quantick_orderflow::TapeAge::NothingYet(90_000)),
        )
    });
    for x in painted_positions(&squeezed) {
        assert!(
            x >= hair.left(),
            "the caption starts at {x}, left of a strip beginning at {} —                  clipped there it would read as a smaller number, not as a                  shorter sentence",
            hair.left()
        );
    }
    // And it is still clipped, because left-aligned it now overflows the
    // other way: running out of room is allowed, spilling into the pane
    // next door is not.
    let clips = painted_clips(&squeezed);
    assert!(!clips.is_empty(), "nothing was painted at all: {squeezed}");
    assert!(
        clips.iter().all(|clip| *clip == hair),
        "every shape must be clipped to the tape's own strip: {clips:?}"
    );

    // No lane, no axis.
    assert_eq!(
        painted(|painter| pane.draw_lane_time_axis(painter, None, 30_000, late_by(41_000))),
        painted(|_| {}),
        "a chart with no tape drew a tape axis"
    );
}

#[test]
fn the_tape_switch_sits_in_the_canvas_top_right_corner() {
    let chart = egui::Rect::from_min_max(egui::pos2(60.0, 80.0), egui::pos2(1_000.0, 700.0));
    let chip = tape_switch_rect(chart);
    assert!(chart.contains_rect(chip), "on the canvas, not off its edge");
    assert!(chip.right() < chart.right(), "inset from the right edge");
    assert!(chip.top() > chart.top(), "and from the top");
    assert!(
        chip.right() > chart.center().x && chip.bottom() < chart.center().y,
        "in the top-right quadrant: {chip:?}"
    );
    // Wherever the canvas is, the chip goes with it — a fixed offset from
    // one corner, so a resized window never leaves it behind.
    let moved = chart.translate(egui::vec2(37.0, -11.0));
    assert_eq!(
        tape_switch_rect(moved),
        chip.translate(egui::vec2(37.0, -11.0))
    );
}

/// The tape's lane is a live region, not a canvas. A drawing that ran
/// across it painted over the flow — and worse, the painted end and the
/// grabbable end were different pixels, because paint used the whole
/// chart while placement stopped at the divider.
#[test]
fn the_drawing_band_stops_at_the_live_lane() {
    let chart = egui::Rect::from_min_max(egui::pos2(60.0, 80.0), egui::pos2(1_000.0, 700.0));
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());

    pane.frame.lane_divider_x = None;
    assert_eq!(
        pane.drawing_area(chart),
        chart,
        "no lane, no carve — the whole chart is the canvas"
    );

    pane.frame.lane_divider_x = Some(880.0);
    let band = pane.drawing_area(chart);
    assert_eq!(band.right(), 880.0, "the band ends where the lane begins");
    assert_eq!(band.left(), chart.left());
    assert_eq!(band.y_range(), chart.y_range());

    // A divider reported outside the chart cannot make the band bigger
    // than the chart or invert it.
    pane.frame.lane_divider_x = Some(5_000.0);
    assert_eq!(pane.drawing_area(chart).right(), chart.right());
    pane.frame.lane_divider_x = Some(-40.0);
    let degenerate = pane.drawing_area(chart);
    assert!(degenerate.right() >= degenerate.left());
}

/// A pane cutting one bar per trade, with `count` bars a second apart —
/// so a market instant and a slot are the same statement, and a test can
/// say which bar it means by naming the moment.
fn pane_of_seconds(count: u64) -> ChartPane {
    use rust_decimal::Decimal;
    let mut pane = ChartPane::flow(1, BarSpec::Tick(1), "TESTUSDT".to_owned());
    let trades: Vec<_> = (0..count)
        .map(|index| quantick_engine::Trade {
            agg_id: index,
            timestamp_ms: 1_700_000_000_000 + index as i64 * 1_000,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        })
        .collect();
    pane.ingest_backfill(&trades);
    pane
}

/// The fixture tape's first print.
const TAPE_START_MS: i64 = 1_700_000_000_000;

/// One print of that tape: a second apart, flat at a hundred.
fn print_at(index: u64) -> quantick_engine::Trade {
    use rust_decimal::Decimal;
    quantick_engine::Trade {
        agg_id: index,
        timestamp_ms: TAPE_START_MS + index as i64 * 1_000,
        price: Decimal::from(100),
        quantity: Decimal::ONE,
        side: quantick_engine::Side::Buy,
    }
}

/// A pane holding `count` of those prints, `per_bar` of them to a bar —
/// the fixture the covering-slot edges are read off.
fn pane_of_prints(count: u64, per_bar: u64) -> ChartPane {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(per_bar), "TESTUSDT".to_owned());
    let trades: Vec<_> = (0..count).map(print_at).collect();
    pane.ingest_backfill(&trades);
    pane
}

/// The lookup the trade marks are painted through answers only inside
/// the tape: the first print to the last, edges included.
#[test]
fn a_covering_slot_answers_only_inside_the_tape() {
    // Ten prints a second apart, four to a bar: two closed bars and a
    // forming one holding the last two.
    let pane = pane_of_prints(10, 4);
    let newest = TAPE_START_MS + 9_000;

    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS - 1),
        None,
        "a millisecond before the first print is not on this tape"
    );
    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS),
        Some(0),
        "the first print itself is"
    );
    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS + 2_500),
        Some(0),
        "so is an instant between two prints of the first bar"
    );
    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS + 4_000),
        Some(1),
        "the second bar opens on its own first print"
    );
    assert_eq!(
        pane.covering_slot_at_time(newest),
        Some(2),
        "the forming bar answers for its last print"
    );
    assert_eq!(
        pane.covering_slot_at_time(newest + 1),
        None,
        "a millisecond past it is not covered"
    );
    assert_eq!(
        pane.covering_slot_at_time(newest + 3_600_000),
        None,
        "and neither is an hour past it"
    );
}

/// A pane whose venue candles are all it has yet: the window is the
/// prefix's own, so an instant inside it still finds its candle. A
/// session fill cannot land there in practice — a fill needs a print,
/// and a print puts a bar on the tape — but the lookup answers for the
/// bars a pane holds, whoever cut them.
#[test]
fn a_venue_prefix_covers_its_own_instants() {
    use rust_decimal::Decimal;
    let mut pane = pane_of_prints(0, 4);
    pane.history_prefix = (0..2)
        .map(|index: i64| quantick_engine::Bar {
            open_time: TAPE_START_MS - 120_000 + index * 60_000,
            close_time: TAPE_START_MS - 61_000 + index * 60_000,
            open: Decimal::from(100),
            high: Decimal::from(100),
            low: Decimal::from(100),
            close: Decimal::from(100),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        })
        .collect();

    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS - 121_000),
        None,
        "before the oldest venue candle"
    );
    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS - 90_000),
        Some(0),
        "inside the first one"
    );
    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS - 30_000),
        Some(1),
        "inside the second"
    );
    assert_eq!(
        pane.covering_slot_at_time(TAPE_START_MS),
        None,
        "and past the newest candle, where the tape has not started yet"
    );
}

#[test]
fn an_empty_pane_covers_no_instant_at_all() {
    let pane = pane_of_prints(0, 4);
    assert_eq!(pane.covering_slot_at_time(TAPE_START_MS), None);
}

/// The other lookup clamps, on purpose: a drawing anchor being re-hung
/// wants the closest bar and is told it was clamped. Pinned here so the
/// two answers cannot quietly converge.
#[test]
fn the_clamping_lookup_still_clamps() {
    let pane = pane_of_prints(10, 4);
    let newest = TAPE_START_MS + 9_000;
    assert_eq!(
        pane.slot_at_time(newest + 3_600_000),
        Some(2),
        "an hour past the tape lands on the newest slot"
    );
    assert_eq!(
        pane.slot_at_time(TAPE_START_MS - 3_600_000),
        Some(0),
        "an hour before it lands on the oldest"
    );
}

/// The replay seek's shape: the round trips survive the rebuild (they
/// happened), the bars do not. A fill the rebuilt tape has not reached
/// gets no bar until it does — instead of parking on the edge one and
/// accumulating there as the replay runs on.
#[test]
fn a_fill_ahead_of_the_tape_waits_for_it() {
    let fill = TAPE_START_MS + 6_000;
    let mut pane = pane_of_prints(4, 4);
    assert_eq!(
        pane.covering_slot_at_time(fill),
        None,
        "the rebuilt tape stops two prints short of the fill"
    );
    assert_eq!(
        pane.slot_at_time(fill),
        Some(0),
        "which is exactly where the clamping lookup would have parked it"
    );
    for index in 4..8 {
        pane.ingest_live_trade(&print_at(index));
    }
    assert_eq!(
        pane.covering_slot_at_time(fill),
        Some(1),
        "the tape reaches the instant and the bar holding it answers"
    );
}

/// A mark placed on the bar at `slot`, shared with the tab's other panes.
fn shared_mark_at(pane: &mut ChartPane, slot: usize, price: f64) {
    let time = pane.slot_open_time(slot).expect("the slot holds a bar");
    #[allow(clippy::cast_precision_loss)]
    let point = ChartPoint::at_time(slot as f32 + 0.5, price, Some(time));
    pane.drawings.place(
        drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "horizontal-line")
            .expect("the horizontal line is registered"),
        point,
    );
    pane.drawings
        .selected_mut()
        .expect("placement selects what it completed")
        .scope = drawings::DrawingScope::AllCharts;
}

#[test]
fn an_edit_from_the_other_pane_lands_on_this_panes_own_bars() {
    let mut pane = pane_of_seconds(20);
    shared_mark_at(&mut pane, 4, 100.0);

    // The other chart reports where it put the handle in market time; this
    // pane is the one that knows which of *its* bars that is.
    let moved_to = pane.slot_open_time(11).expect("the slot holds a bar");
    pane.apply_shared_edit(SharedEdit::MoveAnchor {
        index: 0,
        anchor: 0,
        time_ms: moved_to,
        price: 101.5,
    });

    let point = pane.drawings.items()[0].points[0];
    assert_eq!(point.time_ms, Some(moved_to));
    assert_eq!(slot_of(point.bar), 11, "resolved into this pane");
    assert!((point.price - 101.5).abs() < f64::EPSILON);
}

#[test]
fn a_body_drag_from_the_other_pane_moves_every_anchor_by_the_same_time() {
    let mut pane = pane_of_seconds(20);
    shared_mark_at(&mut pane, 4, 100.0);
    let before = pane.drawings.items()[0].points[0];

    pane.apply_shared_edit(SharedEdit::Translate {
        index: 0,
        delta_ms: 3_000,
        delta_price: 2.0,
    });

    let after = pane.drawings.items()[0].points[0];
    assert_eq!(
        after.time_ms,
        before.time_ms.map(|time| time + 3_000),
        "the drag is said in market time"
    );
    assert_eq!(
        slot_of(after.bar),
        7,
        "and three seconds is three bars on a one-trade-per-bar cut"
    );
    assert!((after.price - 102.0).abs() < f64::EPSILON);
}

#[test]
fn a_locked_mark_refuses_an_edit_from_the_other_pane_too() {
    let mut pane = pane_of_seconds(20);
    shared_mark_at(&mut pane, 4, 100.0);
    pane.drawings
        .selected_mut()
        .expect("the mark is selected")
        .locked = true;
    let before = pane.drawings.items()[0].points.clone();

    pane.apply_shared_edit(SharedEdit::Translate {
        index: 0,
        delta_ms: 3_000,
        delta_price: 2.0,
    });
    let moved_to = pane.slot_open_time(11).expect("the slot holds a bar");
    pane.apply_shared_edit(SharedEdit::MoveAnchor {
        index: 0,
        anchor: 0,
        time_ms: moved_to,
        price: 101.5,
    });

    assert_eq!(
        pane.drawings.items()[0].points,
        before,
        "a lock protects the geometry from both charts, not just its own"
    );
}

#[test]
fn a_drag_this_pane_cannot_place_moves_nothing_at_all() {
    let mut pane = pane_of_seconds(20);
    shared_mark_at(&mut pane, 4, 100.0);
    let before = pane.drawings.items()[0].points.clone();

    // Back past the first bar this pane holds: there is no slot for it,
    // and half a shape on a bar and half on an instant is not an option.
    pane.apply_shared_edit(SharedEdit::Translate {
        index: 0,
        delta_ms: -600_000,
        delta_price: 0.0,
    });

    assert_eq!(pane.drawings.items()[0].points, before);
}

#[test]
fn a_recut_keeps_the_mark_on_its_own_instant() {
    let mut pane = pane_of_seconds(20);
    shared_mark_at(&mut pane, 6, 100.0);
    let placed_at = pane.drawings.items()[0].points[0]
        .time_ms
        .expect("placed on a bar");
    let old_slots = pane.slots();

    // Two trades per bar: the same tape, half the bars.
    pane.tick_n = 2;
    let spec = pane.current_spec();
    pane.state.set_spec(spec);
    pane.reanchor_drawings(old_slots);

    let point = pane.drawings.items()[0].points[0];
    assert_eq!(point.time_ms, Some(placed_at), "the instant is untouched");
    assert_eq!(
        pane.slot_at_time(placed_at),
        Some(slot_of(point.bar)),
        "the bar is that instant, re-asked of the new cut"
    );
    assert!(!pane.drawings.items()[0].off_series);
}

/// A pane carrying one indicator pane of `kind`, with `columns` of
/// committed values — the fixture every band test starts from.
#[test]
fn ingesting_prints_hands_the_tape_grid_to_the_order_flow_engine() {
    // The whole fix hangs off this wiring: the detector can be right and
    // the engine willing, and the ladder still draws the wrong rows if
    // nothing carries the answer between them. Nothing else in the suite
    // would notice that call going missing.
    let mut pane = ChartPane::flow(1, BarSpec::Tick(1000), "WINV26".to_owned());
    assert!(
        pane.orderflow.is_some(),
        "a flow pane is the one that owns the engine under test",
    );
    let before = pane
        .orderflow
        .as_mut()
        .map(OrderflowView::base_capture_grouping_for_test)
        .expect("flow pane has an engine");

    // B3's mini index: every print a multiple of five, and no depth event
    // anywhere — exactly what a market replay delivers.
    for (i, price) in [
        "174565", "174570", "174560", "174570", "174585", "174580", "174570", "174575", "174590",
        "174585",
    ]
    .iter()
    .enumerate()
    {
        pane.ingest_live_trade(&quantick_engine::Trade {
            agg_id: i as u64,
            timestamp_ms: 1_000 + i as i64 * 100,
            price: price.parse::<Decimal>().unwrap(),
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        });
    }

    assert_eq!(pane.state.tape_price_step(), Some(Decimal::from(5)));
    let after = pane
        .orderflow
        .as_mut()
        .map(OrderflowView::base_capture_grouping_for_test)
        .expect("flow pane has an engine");
    assert_ne!(after, before, "the engine never heard the tape's grid");
    assert_eq!(after, Decimal::from(5));
}

#[test]
fn backfilled_history_hands_the_tape_grid_over_too() {
    // History arrives as one batch before the first live print, so a chart
    // wired only on the live path draws its whole first screen on the
    // wrong rows and then quietly corrects itself.
    let mut pane = ChartPane::flow(1, BarSpec::Tick(1000), "WDOU26".to_owned());
    let history: Vec<quantick_engine::Trade> = [
        "5216.0", "5216.5", "5217.0", "5216.5", "5215.5", "5217.5", "5218.0", "5217.0", "5216.0",
        "5215.5",
    ]
    .iter()
    .enumerate()
    .map(|(i, price)| quantick_engine::Trade {
        agg_id: i as u64,
        timestamp_ms: 1_000 + i as i64 * 100,
        price: price.parse::<Decimal>().unwrap(),
        quantity: Decimal::ONE,
        side: quantick_engine::Side::Buy,
    })
    .collect();
    pane.ingest_backfill(&history);

    assert_eq!(
        pane.orderflow
            .as_mut()
            .map(OrderflowView::base_capture_grouping_for_test),
        Some("0.5".parse::<Decimal>().unwrap()),
    );
}
fn pane_with_indicator(kind: &str, columns: Vec<Vec<f64>>) -> ChartPane {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    add_indicator_view(&mut pane, kind, columns);
    pane
}

fn add_indicator_view(pane: &mut ChartPane, kind: &str, columns: Vec<Vec<f64>>) -> SlotId {
    let slot = pane.indicators.allocate_slot(kind);
    let descriptor = quantick_indicators::IndicatorDescriptor {
        title: kind.to_owned(),
        short_title: None,
        overlay: false,
        plots: (0..columns.len().max(1))
            .map(|i| quantick_indicators::PlotSpec {
                id: quantick_indicators::PlotId::new(i),
                title: format!("p{i}"),
                style: quantick_indicators::PlotStyle::Line,
                base_color: quantick_indicators::Rgba8::opaque(255, 255, 255),
                width: 1.0,
                offset: 0,
                marker: None,
            })
            .collect(),
        fills: Vec::new(),
        inputs: Vec::new(),
    };
    pane.indicators
        .apply(IndicatorEvent::rebuilt(slot, descriptor, columns));
    slot
}

fn test_areas(pane: &ChartPane, rect: egui::Rect) -> PlotAreas {
    pane.plot_areas(rect, crate::config::FeedCapabilities::none())
}

const TEST_PLOT: egui::Rect = egui::Rect {
    min: egui::Pos2 { x: 0.0, y: 0.0 },
    max: egui::Pos2 {
        x: 1_000.0,
        y: 700.0,
    },
};

/// The invariant the whole feature rests on: a band's drawings are
/// projected through the range its *curve* was drawn with, which is the
/// auto-fit **after** the trader's own zoom. Build the scale from the
/// auto-fit alone and every level leaves its curve the moment that pane
/// is zoomed — the defect this test exists to make impossible.
#[test]
fn a_band_scale_is_the_range_its_curve_was_drawn_with() {
    let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 40.0, -20.0]]);
    let auto = (-100.0, 100.0);
    {
        let view = pane
            .indicators
            .visible_panes_mut()
            .next()
            .expect("one pane indicator");
        view.last_auto = Some(auto);
        // The trader drags the pane's own axis.
        view.scale.pan(25.0, auto);
    }
    let areas = test_areas(&pane, TEST_PLOT);
    let bands = pane.bands(&areas);
    let band = bands
        .iter()
        .find(|band| matches!(band.key, DrawingBand::Indicator(_)))
        .expect("the indicator band");
    let resolved = pane
        .indicators
        .visible_panes()
        .next()
        .expect("one pane indicator")
        .scale
        .resolve(auto);
    assert_ne!(resolved, auto, "the fixture has to actually be zoomed");
    assert_eq!(
        band.scale.expect("a drawable band has a scale").range(),
        resolved,
        "the band projects through the range the curve was drawn with"
    );
}

/// Build a pane holding one *overlay* indicator, with the projection a
/// frame would have cached, so the pick can be asked where its line is.
fn pane_with_overlay(values: Vec<f64>) -> ChartPane {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    // Plot rows map 1:1 onto bar slots, so the fixture needs the bars the
    // columns describe — without them the x mapping has nothing to place
    // the curve on, which is exactly what a chart before its first bar is.
    pane.history_prefix = (0..values.len())
        .map(|_| quantick_engine::Bar {
            open_time: 0,
            close_time: 0,
            open: rust_decimal::Decimal::ONE,
            high: rust_decimal::Decimal::ONE,
            low: rust_decimal::Decimal::ONE,
            close: rust_decimal::Decimal::ONE,
            buy_volume: rust_decimal::Decimal::ZERO,
            sell_volume: rust_decimal::Decimal::ZERO,
            trade_count: 1,
        })
        .collect();
    let slot = pane.indicators.allocate_slot("native.ema");
    let descriptor = quantick_indicators::IndicatorDescriptor {
        title: "EMA(9)".to_owned(),
        short_title: None,
        overlay: true,
        plots: vec![quantick_indicators::PlotSpec {
            id: quantick_indicators::PlotId::new(0),
            title: "ema".to_owned(),
            style: quantick_indicators::PlotStyle::Line,
            base_color: quantick_indicators::Rgba8::opaque(255, 255, 255),
            width: 1.0,
            offset: 0,
            marker: None,
        }],
        fills: Vec::new(),
        inputs: Vec::new(),
    };
    pane.indicators
        .apply(IndicatorEvent::rebuilt(slot, descriptor, vec![values]));
    pane.frame.chart_area = Some(TEST_PLOT);
    pane.frame.chart_top = TEST_PLOT.top();
    pane.frame.chart_height = TEST_PLOT.height();
    pane.frame.auto_range = Some((0.0, 100.0));
    pane
}

/// The slot a stored fractional anchor sits on, asked of the one owner.
///
/// A test that recomputes it — `bar.floor()`, as five of these did — is a
/// second copy of the projection rule, free to disagree with the
/// production one and to keep passing while it does. See
/// [`Viewport::slot_of`], whose whole doc is about two producers that
/// write these coordinates differently.
fn slot_of(bar: f32) -> usize {
    Viewport::slot_of(bar).expect("a placed anchor sits on a slot")
}

/// A pane holding `count` closed bars, each opening one minute after the
/// last — so a test can ask what time a slot is and get an answer that
/// could only have come from that slot.
fn pane_with_timed_bars(count: usize) -> ChartPane {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.history_prefix = (0..count)
        .map(|index| quantick_engine::Bar {
            open_time: BAR_ZERO_MS + index as i64 * 60_000,
            close_time: BAR_ZERO_MS + index as i64 * 60_000 + 59_999,
            open: rust_decimal::Decimal::ONE,
            high: rust_decimal::Decimal::TWO,
            low: rust_decimal::Decimal::ONE,
            close: rust_decimal::Decimal::TWO,
            buy_volume: rust_decimal::Decimal::ZERO,
            sell_volume: rust_decimal::Decimal::ZERO,
            trade_count: 1,
        })
        .collect();
    pane.frame.chart_area = Some(TEST_PLOT);
    pane.frame.chart_top = TEST_PLOT.top();
    pane.frame.chart_height = TEST_PLOT.height();
    pane.frame.auto_range = Some((0.0, 100.0));
    pane
}

/// When the fixture's first bar opened. An arbitrary instant, named once
/// so a failure prints a difference rather than two magic numbers.
const BAR_ZERO_MS: i64 = 1_700_000_000_000;

/// The mission in one assertion: point at a candle, learn *its* time.
///
/// Every pixel of the candle, because the left half is where this used to
/// answer with the bar before it — see
/// `every_pixel_of_a_candle_names_that_candle`, which holds the rule this
/// reads through.
#[test]
fn the_compass_names_the_candle_under_the_pointer() {
    let pane = pane_with_timed_bars(200);
    let (right, total) = (TEST_PLOT.right(), pane.slots());
    let width = pane.viewport.candle_width();
    for slot in [120_usize, 199] {
        let centre = pane.viewport.x_center(slot, right, total);
        for x in [
            centre - width / 2.0 + 0.01,
            centre,
            centre + width / 2.0 - 0.01,
        ] {
            assert_eq!(
                pane.pointer_bar(x, right, total),
                Some(pointer_compass::PointerBar {
                    slot,
                    open_time_unix_ms: BAR_ZERO_MS + slot as i64 * 60_000,
                }),
                "every pixel of candle {slot} reads candle {slot}'s own instant"
            );
        }
    }
}

/// Data honesty: the empty canvas past the newest bar holds no bar, and
/// neither does the tape's own band. Nothing is marked in either place
/// rather than the nearest candle's clock being stretched over it.
#[test]
fn the_compass_names_no_time_where_there_is_no_bar() {
    let pane = pane_with_timed_bars(200);
    let (right, total) = (TEST_PLOT.right(), pane.slots());
    assert_eq!(
        pane.pointer_bar(right + 60.0, right, total),
        None,
        "the projection margin is future the tape has not written"
    );
    let oldest = pane.viewport.x_center(0, right, total);
    assert_eq!(
        pane.pointer_bar(oldest - 60.0, right, total),
        None,
        "and before the first bar there is nothing either"
    );
    // The lane is a rolling window of market time, not bar slots: a bar
    // time written under it would be the wrong axis's answer.
    let divider = right - 150.0;
    assert_eq!(pane.pointer_bar(right - 40.0, divider, total), None);
    assert_eq!(
        pane_with_timed_bars(0).pointer_bar(500.0, right, 0),
        None,
        "and a chart with no bars at all names none"
    );
}

/// Two surfaces that must agree: the tag a trader reads off the axis and
/// the slot a control client reads out of the cursor scope are one answer,
/// because they come through one owner. Written as a test rather than
/// trusted from the diff — the two are computed in different files.
#[test]
fn the_axis_tag_and_the_control_cursor_name_one_bar() {
    let mut pane = pane_with_timed_bars(200);
    let ctx = egui::Context::default();
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let areas = test_areas(&pane, TEST_PLOT);
    pane.frame.bands = pane.bands(&areas);
    let right = pane.frame.lane_divider_x.unwrap_or(areas.chart.right());
    let total = pane.slots();
    // Deliberately in the left half of a candle, the half that used to
    // answer with its neighbour.
    let slot = 140_usize;
    let x = pane.viewport.x_center(slot, right, total) - pane.viewport.candle_width() / 2.0 + 0.5;
    pane.hover_pos = Some(egui::pos2(x, areas.chart.center().y));
    let compass = pane
        .pointer_bar(x, right, total)
        .expect("the pointer is on a candle");
    let cursor = pane
        .control_pointer_hit()
        .expect("and the control plane can see the same pointer");
    assert_eq!(compass.slot, slot);
    assert_eq!(
        cursor.slot,
        Some(compass.slot),
        "the axis and the cursor scope may not name two different bars"
    );
}

/// Drive `handle_navigation` for one frame with the given pointer events,
/// Drive `handle_navigation` for one frame with the given pointer events,
/// and hand back whatever settings request the gestures raised.
///
/// The pane's input pass needs a whole `PaneChrome` — the tool rail, the
/// preset store, the style, the simulator, the layer sink — which is why
/// none of these gestures had an end-to-end test before: there was nothing
/// to build one on. Everything here is a real default, and the preset store
/// is pointed at a path that does not exist, so the fixture reads nothing
/// off the developer's disk.
fn drive_navigation(
    pane: &mut ChartPane,
    ctx: &egui::Context,
    area: egui::Rect,
    events: Vec<egui::Event>,
) -> Option<SlotId> {
    let mut toolrail = crate::toolrail::ToolRail::new();
    let presets = drawings::presets::PresetStore::load_from(
        crate::scratch::thread_dir("pane-presets").join("no-such.toml"),
    );
    let mut begin_text_edit = false;
    let style = crate::style::ChartStyle::default();
    let mut paper = crate::paper_trading::PaperTrading::new();
    let footprint = crate::footprint_config::FootprintConfig::default();
    let mut layers = crate::chart_layers::LayerActions::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            area.size() + egui::vec2(area.left(), area.top()),
        )),
        events,
        ..egui::RawInput::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut chrome = PaneChrome {
                toolrail: &mut toolrail,
                presets: &presets,
                begin_text_edit: &mut begin_text_edit,
                style: &style,
                tz: crate::timezone::TzOffset::default(),
                feed_gaps: &[],
                symbol: "TESTUSDT",
                paper: &mut paper,
                paper_takes_input: false,
                paper_hud_here: false,
                shared_pick: None,
                shared: SharedInteraction::default(),
                capabilities: crate::config::FeedCapabilities::none(),
                side_inferred: false,
                footprint: &footprint,
                layers: &mut layers,
            };
            pane.handle_navigation(ui, area, &mut chrome);
        });
    });
    pane.take_settings_request()
}

/// Build a whole `PaneChrome` with `tool` armed and hand it to `body`.
///
/// The pane's input and draw passes both want one — the tool rail, the
/// preset store, the style, the simulator, the layer sink — and every
/// field here is a real default. The preset store is pointed at a path
/// that does not exist, so the fixture reads nothing off the developer's
/// disk.
fn with_chrome<R>(tool: Tool, body: impl FnOnce(&mut PaneChrome<'_>) -> R) -> R {
    let mut toolrail = crate::toolrail::ToolRail::new();
    toolrail.arm(tool);
    let presets = drawings::presets::PresetStore::load_from(
        crate::scratch::thread_dir("pane-presets").join("no-such.toml"),
    );
    let mut begin_text_edit = false;
    let style = crate::style::ChartStyle::default();
    let mut paper = crate::paper_trading::PaperTrading::new();
    let footprint = crate::footprint_config::FootprintConfig::default();
    let mut layers = crate::chart_layers::LayerActions::default();
    body(&mut PaneChrome {
        toolrail: &mut toolrail,
        presets: &presets,
        begin_text_edit: &mut begin_text_edit,
        style: &style,
        tz: crate::timezone::TzOffset::default(),
        feed_gaps: &[],
        symbol: "TESTUSDT",
        paper: &mut paper,
        paper_takes_input: false,
        paper_hud_here: false,
        shared_pick: None,
        shared: SharedInteraction::default(),
        capabilities: crate::config::FeedCapabilities::none(),
        side_inferred: false,
        footprint: &footprint,
        layers: &mut layers,
    })
}

/// Run `paint` with that chrome and hand back everything it put on the
/// canvas.
fn painted_with_tool(
    pane: &ChartPane,
    tool: Tool,
    paint: impl Fn(&ChartPane, &egui::Painter, &PaneChrome<'_>),
) -> String {
    with_chrome(tool, |chrome| {
        painted(|painter| paint(pane, painter, chrome))
    })
}

/// One price, one tag. The crosshair is a mode that writes its own price
/// on the axis; while it is armed the compass steps off that half rather
/// than stacking a second chip on the same pixel — and still supplies the
/// time half, which the crosshair has never drawn.
#[test]
fn the_armed_crosshair_keeps_the_price_tag_to_itself() {
    let mut pane = pane_with_timed_bars(200);
    let areas = test_areas(&pane, TEST_PLOT);
    let scale = test_scale();
    let mut discarded = crate::chart_layers::LayerActions::default();
    // The time half off, so this measures the price half alone.
    pane.set_layer_visible(ChartLayer::PointerTime, false, &mut discarded);
    pane.hover_pos = Some(areas.chart.center());
    let compass = |pane: &ChartPane, painter: &egui::Painter, chrome: &PaneChrome<'_>| {
        if let Some(decided) = pane.pointer_compass(
            areas.chart,
            areas.chart.right(),
            pane.slots(),
            &scale,
            chrome,
        ) {
            pane.draw_pointer_compass(
                painter,
                &decided,
                areas.chart.right(),
                areas.time_strip,
                chrome,
            );
        }
    };
    let alone = painted_with_tool(&pane, Tool::Pointer, compass);
    assert!(
        alone.contains("Text"),
        "the plain pointer gets the compass's price tag"
    );
    let with_crosshair = painted_with_tool(&pane, Tool::Crosshair, compass);
    assert_eq!(
        with_crosshair, "[]",
        "and the armed crosshair writes it instead, so the compass adds nothing"
    );

    // The armed tool alone decides it, and that is not a shortcut: arming
    // the crosshair turns its layer back on through
    // `unhide_layer_for_armed_tool`, so "armed but not drawing" is a state
    // the app does not have. A second conjunct testing the layer would
    // read as a condition that can be met and never be false — and the
    // assertion guarding it would pass only by setting the field behind
    // `handle_navigation`'s back, which is how this test used to prove a
    // rule the code did not deliver.
    pane.set_layer_visible(ChartLayer::Crosshair, false, &mut discarded);
    with_chrome(Tool::Crosshair, |chrome| {
        pane.unhide_layer_for_armed_tool(chrome);
    });
    assert!(
        pane.layer_visible(ChartLayer::Crosshair, &crate::style::ChartStyle::default()),
        "arming the tool brings its layer back, so the compass never sees              an armed crosshair that is not drawing"
    );
}

/// Every level the pane would write on the price axis this frame.
/// Every level the pane would write on the price axis this frame.
fn axis_levels_of(pane: &ChartPane, scale: &PriceScale) -> Vec<PriceAxisLevel> {
    let mut levels = Vec::new();
    pane.price_axis_levels(
        TEST_PLOT,
        TEST_PLOT.right(),
        pane.slots(),
        scale,
        &mut levels,
    );
    levels
}

fn test_scale() -> PriceScale {
    PriceScale::from_range(100.0, 200.0, TEST_PLOT.top(), TEST_PLOT.bottom())
}

/// How exact a tag can be. A declared level is a screen `y`, so the price
/// on the tag has been through an f32 pixel and back: it is as exact as
/// the axis is and no more, which is the right answer — the tag has to
/// agree with the *pixel* the line is painted on. A tenth of a pixel's
/// worth of price is far inside that and far outside f32 noise.
fn a_tenth_of_a_pixel() -> f64 {
    let (lo, hi) = test_scale().range();
    (hi - lo) / f64::from(TEST_PLOT.height()) / 10.0
}

/// The ProfitChart ask: a horizontal line marks where it is on the price
/// axis, in its own colour, so the level can be read without hunting for
/// where the line meets the gutter.
#[test]
fn a_horizontal_level_marks_itself_on_the_price_axis() {
    let mut pane = pane_with_timed_bars(200);
    let scale = test_scale();
    let red = egui::Color32::from_rgb(200, 40, 40);
    for (id, price) in [("horizontal-line", 150.0_f64), ("horizontal-ray", 120.0)] {
        let tool = drawings::DrawingTool::by_id(id).expect("a registered tool");
        assert!(pane.drawings.place(tool, ChartPoint::at(100.0, price)));
    }
    for item in pane.drawings.items_mut() {
        item.style.color = red;
    }

    let levels = axis_levels_of(&pane, &scale);
    assert_eq!(levels.len(), 2, "one tag per level: {levels:?}");
    for (level, price) in levels.iter().zip([150.0_f64, 120.0]) {
        assert!(
            (level.price - price).abs() < a_tenth_of_a_pixel(),
            "the tag reads the level's own price, got {}",
            level.price
        );
        assert!(
            (level.y - scale.y(price)).abs() < 1e-3,
            "and sits where the axis puts that price"
        );
        assert_eq!(level.color, red, "in the object's own colour");
    }
}

/// The tag is the object, said on the axis — so it moves when the object
/// moves, and it goes when the objects go.
#[test]
fn an_axis_tag_follows_its_object_and_leaves_with_it() {
    let mut pane = pane_with_timed_bars(200);
    let scale = test_scale();
    let tool = drawings::DrawingTool::by_id("horizontal-line").expect("a registered tool");
    assert!(pane.drawings.place(tool, ChartPoint::at(100.0, 150.0)));

    let before = axis_levels_of(&pane, &scale);
    assert_eq!(before.len(), 1);
    pane.drawings.select(Some(0));
    pane.drawings.begin_gesture();
    // Down the axis by a tenth of the visible range.
    pane.drawings.translate_selected(0.0, -10.0);
    pane.drawings.commit_gesture();
    let after = axis_levels_of(&pane, &scale);
    assert_eq!(after.len(), 1);
    assert!(
        (after[0].price - 140.0).abs() < a_tenth_of_a_pixel(),
        "the tag went with the line, to {}",
        after[0].price
    );

    // The layer that hides the objects hides what the axis says about
    // them: a gutter still marked at a level whose line is gone would be
    // the chart claiming something it is not drawing.
    let mut discarded = crate::chart_layers::LayerActions::default();
    pane.set_layer_visible(ChartLayer::Drawings, false, &mut discarded);
    assert!(
        axis_levels_of(&pane, &scale).is_empty(),
        "no objects on the chart, no levels on the axis"
    );
}

/// Precedence on a shared gutter: live market data over a static
/// annotation.
///
/// The last-price chip and a drawing's level land on the same pixel
/// exactly when price arrives at the level — the moment the level was
/// drawn for — and the one that has to stay legible then is the market's,
/// not the number the trader chose themselves and already knows.
///
/// Asserted on the shapes the pair really emits, in a paint call that
/// draws nothing else: whichever chip goes down last is the one a reader
/// sees, so the order in the shape list *is* the rule.
#[test]
fn the_live_price_is_painted_over_a_level_and_not_under_it() {
    let mut pane = pane_with_timed_bars(60);
    let scale = test_scale();
    // A level at the same price the last-price chip will report, which is
    // the only interesting case: anywhere else they do not overlap.
    let mut bar = pane.closed_bar(59).expect("the newest bar").clone();
    // Mid-range on the fixture's scale, so both chips land inside the
    // gutter the test paints rather than off the top of it.
    let close = 150.0_f64;
    bar.close = rust_decimal::Decimal::from(150);
    let tool = drawings::DrawingTool::by_id("horizontal-line").expect("a registered tool");
    assert!(pane.drawings.place(tool, ChartPoint::at(30.0, close)));
    // Dark, so `theme::ink_on` gives it the light ink and the two chips
    // are told apart by their text as well as by their fill.
    let level_colour = egui::Color32::from_rgb(0x0B, 0x1B, 0x3A);
    pane.drawings.items_mut()[0].style.color = level_colour;
    let levels = axis_levels_of(&pane, &scale);
    assert_eq!(levels.len(), 1, "one level to be covered or not");

    let painted = painted_with_tool(&pane, Tool::Pointer, |pane, painter, chrome| {
        pane.draw_axis_marks(
            painter,
            TEST_PLOT,
            TEST_PLOT.right(),
            &scale,
            &levels,
            Some(&bar),
            chrome,
        );
    });
    let level = painted
        .find(&format!("{level_colour:?}"))
        .expect("the level's chip is on the axis");
    let last_price = painted
        .find(&format!("{:?}", theme::CHIP_INK))
        .expect("the last-price chip's ink is on the axis");
    assert!(
        level < last_price,
        "the annotation goes down first, so the live price is legible over it: {painted}"
    );
}

/// Data honesty on the axis: a mark this chart's data does not back is
/// Data honesty on the axis: a mark this chart's data does not back is
/// painted faded, and its tag is faded with it. A full-strength chip on
/// the gutter would be the axis making a claim the stroke beside it is
/// explicitly not making.
#[test]
fn a_tag_wears_the_same_honesty_fade_its_stroke_does() {
    let mut pane = pane_with_timed_bars(200);
    let scale = test_scale();
    let tool = drawings::DrawingTool::by_id("horizontal-line").expect("a registered tool");
    assert!(pane.drawings.place(tool, ChartPoint::at(100.0, 150.0)));
    let full = axis_levels_of(&pane, &scale)[0].color;

    // The tab changed the instrument under the mark: the level is real,
    // the price means nothing on the chart it is now over.
    pane.drawings.items_mut()[0].foreign_market = true;
    let faded = axis_levels_of(&pane, &scale)[0].color;
    assert_ne!(faded, full, "the tag says the mark is not backed here");
    assert!(
        faded.a() < full.a(),
        "and says it by fading, not by shouting: {faded:?} vs {full:?}"
    );
    assert_eq!(
        faded,
        ChartPane::painted_color(&pane.drawings.items()[0]),
        "through the same call the stroke goes through"
    );
}

/// A level on an indicator pane is a value on *that* pane's axis.
/// A level on an indicator pane is a value on *that* pane's axis. Writing
/// it on the price gutter would put a CVD reading where a price goes.
#[test]
fn a_level_drawn_on_an_indicator_band_never_reaches_the_price_axis() {
    let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 40.0, -20.0]]);
    pane.frame.chart_area = Some(TEST_PLOT);
    pane.frame.auto_range = Some((0.0, 100.0));
    let band = DrawingBand::Indicator(
        pane.indicators.pane_key(
            pane.indicators
                .visible_panes()
                .next()
                .expect("one pane indicator"),
        ),
    );
    let tool = drawings::DrawingTool::by_id("horizontal-line").expect("a registered tool");
    assert!(
        pane.drawings
            .place_on(tool, &band, ChartPoint::at(1.0, 20.0))
    );
    assert!(
        axis_levels_of(&pane, &test_scale()).is_empty(),
        "the price axis says nothing about another pane's units"
    );
}

/// A right-click at `pos`, driven through the pane's own input pass, and
/// A right-click at `pos`, driven through the pane's own input pass, and
/// one more frame for the menu it opened to draw itself.
fn right_click_menu(pane: &mut ChartPane, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        let events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::default(),
            },
        ];
        let _ = drive_navigation(pane, ctx, TEST_PLOT, events);
    }
    // The frame the open menu is laid out on.
    let _ = drive_navigation(pane, ctx, TEST_PLOT, Vec::new());
}

/// Each axis carries the switch for the mark it wears, because that is
/// where a trader looks for something about that axis.
///
/// Driven through the right-click itself rather than by setting the
/// field: a switch that exists in the code and not in the menu is a
/// switch nobody can reach.
#[test]
fn each_axis_offers_the_half_of_the_compass_it_wears() {
    let ctx = egui::Context::default();
    let mut pane = pane_with_timed_bars(200);
    // One frame to publish the geometry the menus are opened on.
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let areas = test_areas(&pane, TEST_PLOT);

    right_click_menu(&mut pane, &ctx, areas.price_gutter.center());
    assert!(
        pane.layer_menu_rects
            .iter()
            .any(|(layer, _)| *layer == ChartLayer::PointerPrice),
        "the price axis offers the price half: {:?}",
        pane.layer_menu_rects
    );
    assert!(
        pane.layer_menu_rects
            .iter()
            .all(|(layer, _)| *layer != ChartLayer::PointerTime),
        "and not the other axis's half"
    );

    let strip = split_time_strip(areas.time_strip, pane.frame.lane_divider_x).0;
    right_click_menu(&mut pane, &ctx, strip.center());
    assert!(
        pane.layer_menu_rects
            .iter()
            .any(|(layer, _)| *layer == ChartLayer::PointerTime),
        "the time axis offers the time half: {:?}",
        pane.layer_menu_rects
    );
    assert!(
        pane.layer_menu_rects
            .iter()
            .all(|(layer, _)| *layer != ChartLayer::PointerPrice),
        "and not the other axis's half"
    );
}

/// Clicking that entry really switches the layer — the checkbox writes
/// the field the painter reads, not a copy beside it.
#[test]
fn the_axis_menu_entry_switches_the_layer_it_names() {
    let ctx = egui::Context::default();
    let mut pane = pane_with_timed_bars(200);
    let style = crate::style::ChartStyle::default();
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let areas = test_areas(&pane, TEST_PLOT);
    assert!(
        pane.layer_visible(ChartLayer::PointerPrice, &style),
        "the fixture opens with the compass on"
    );

    right_click_menu(&mut pane, &ctx, areas.price_gutter.center());
    let checkbox = pane
        .layer_menu_rects
        .iter()
        .find(|(layer, _)| *layer == ChartLayer::PointerPrice)
        .expect("the price axis offers the switch")
        .1
        .center();
    for pressed in [true, false] {
        let events = vec![
            egui::Event::PointerMoved(checkbox),
            egui::Event::PointerButton {
                pos: checkbox,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            },
        ];
        let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, events);
    }
    assert!(
        !pane.layer_visible(ChartLayer::PointerPrice, &style),
        "the click switched the layer the painter reads"
    );
}

/// A double click at `pos`, as two press/release pairs in two frames —
/// A double click at `pos`, as two press/release pairs in two frames —
/// egui reports the second as `double_clicked`.
fn double_click_at(
    pane: &mut ChartPane,
    ctx: &egui::Context,
    area: egui::Rect,
    pos: egui::Pos2,
) -> Option<SlotId> {
    let mut request = None;
    for pressed in [true, false, true, false] {
        let events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            },
        ];
        request = drive_navigation(pane, ctx, area, events).or(request);
    }
    request
}

/// Criterion 1, the pane's own targets: a double click on an open pane's
/// header asks for that indicator's settings, and the pane body still
/// means what it always did.
///
/// This is the binding the geometry tests could not reach — a rect
/// registered before the pan gesture instead of after, or a `clicked()`
/// written where `double_clicked()` was meant, would leave every other
/// test green and the gesture dead.
#[test]
fn a_double_click_on_a_pane_header_asks_for_that_indicators_settings() {
    let ctx = egui::Context::default();
    let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 40.0, -20.0]]);
    // One frame to lay the pane out, so the header rect is real.
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let areas = test_areas(&pane, TEST_PLOT);
    let slot = areas.indicator_panes.first().copied().expect("one pane");
    assert!(!slot.collapsed, "the fixture pane has room for its curve");
    let header = indicator_render::pane_header_rect(slot.rect, slot.collapsed);
    let expected = pane.indicators.all()[0].slot;

    assert_eq!(
        double_click_at(&mut pane, &ctx, TEST_PLOT, header.center()),
        Some(expected),
        "the header is the pane's handle into its settings"
    );

    // The body below it keeps its own meaning: a double click there resets
    // that pane's scale and asks for no dialog.
    let body = egui::pos2(slot.rect.center().x, slot.rect.bottom() - 8.0);
    assert_eq!(
        double_click_at(&mut pane, &ctx, TEST_PLOT, body),
        None,
        "the header took the gesture from the body, not the whole band"
    );
}

/// The fourth target, and the one that needed a behaviour change to exist:
/// a double click on a collapsed pane's strip.
///
/// The two clicks straddle a change of geometry — the first expands the
/// pane, so the second arrives at a strip that is no longer there. Before
/// this, that second click collapsed the pane again, which made
/// double-clicking a collapsed strip expand it and put it back: a gesture
/// that did nothing at all. The pane now stays open and its settings are
/// what the second click asks for.
///
/// The single click is untouched, and this asserts that too — a collapsed
/// pane still opens on one click, and an open one still closes.
#[test]
fn a_double_click_on_a_collapsed_strip_opens_it_and_asks_for_its_settings() {
    let ctx = egui::Context::default();
    let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 40.0]]);
    pane.indicators
        .visible_panes_mut()
        .next()
        .expect("one pane")
        .sizing = PaneSizing::Collapsed;
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let strip = test_areas(&pane, TEST_PLOT)
        .indicator_panes
        .first()
        .copied()
        .expect("one pane");
    assert!(strip.collapsed, "the fixture pane is collapsed by hand");
    let expected = pane.indicators.all()[0].slot;

    assert_eq!(
        double_click_at(&mut pane, &ctx, TEST_PLOT, strip.rect.center()),
        Some(expected),
        "the strip asks for the settings of the pane it belongs to"
    );
    assert!(
        !matches!(pane.indicators.all()[0].sizing, PaneSizing::Collapsed),
        "and leaves it open rather than putting it back, which is what \
             made this gesture a no-op"
    );

    // The single click either way is exactly what it was.
    let mut plain = pane_with_indicator("native.cvd", vec![vec![0.0, 40.0]]);
    plain
        .indicators
        .visible_panes_mut()
        .next()
        .expect("one pane")
        .sizing = PaneSizing::Collapsed;
    let _ = drive_navigation(&mut plain, &ctx, TEST_PLOT, Vec::new());
    let band = test_areas(&plain, TEST_PLOT)
        .indicator_panes
        .first()
        .copied()
        .expect("one pane");
    single_click_at(&mut plain, &ctx, TEST_PLOT, band.rect.center());
    assert!(
        !matches!(plain.indicators.all()[0].sizing, PaneSizing::Collapsed),
        "one click still opens a collapsed pane"
    );
    let open = test_areas(&plain, TEST_PLOT)
        .indicator_panes
        .first()
        .copied()
        .expect("one pane");
    let chevron = indicator_render::pane_disclosure_rect(open.rect, open.collapsed).center();
    single_click_at(&mut plain, &ctx, TEST_PLOT, chevron);
    assert!(
        matches!(plain.indicators.all()[0].sizing, PaneSizing::Collapsed),
        "and one click on the chevron still closes an open one"
    );
}

/// One press and release at `pos`, far enough apart in frames that egui
/// reports a plain click rather than half of a double one.
fn single_click_at(pane: &mut ChartPane, ctx: &egui::Context, area: egui::Rect, pos: egui::Pos2) {
    for pressed in [true, false] {
        let events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            },
        ];
        let _ = drive_navigation(pane, ctx, area, events);
    }
    // Idle frames, so the next click is not read as a double.
    for _ in 0..30 {
        let _ = drive_navigation(pane, ctx, area, Vec::new());
    }
}

/// And on the candles: a double click on an overlay's own line asks for
/// that overlay, while one in open chart still snaps back to the live edge
/// and asks for nothing.
///
/// `overlay_plot_at` is unit-tested on its own; this is the wiring around
/// it — that the chart's double click consults it at all, and that it does
/// not swallow the gesture everywhere else.
#[test]
fn a_double_click_on_an_overlays_line_asks_for_that_overlay() {
    let ctx = egui::Context::default();
    let mut pane = pane_with_overlay(vec![50.0; 8]);
    let _ = drive_navigation(&mut pane, &ctx, TEST_PLOT, Vec::new());
    let expected = pane.indicators.all()[0].slot;
    let (chart, right, total, scale) = pane.last_projection().expect("a drawn projection");
    let (start, _) = pane.viewport.visible_range(chart.width(), total);
    let on_the_line = egui::pos2(
        pane.viewport.x_center(start + 1, right, total),
        scale.y(50.0),
    );

    assert_eq!(
        double_click_at(&mut pane, &ctx, TEST_PLOT, on_the_line),
        Some(expected),
        "pointing at a curve and double clicking is asking about the curve"
    );

    let open_chart = on_the_line + egui::vec2(0.0, PLOT_PICK_TOLERANCE_PX * 10.0);
    assert_eq!(
        double_click_at(&mut pane, &ctx, TEST_PLOT, open_chart),
        None,
        "open chart still means the viewport"
    );
}

/// The fourth place a double click opens settings from, and the one that
/// needs real geometry: the pointer is on an overlay's own line.
///
/// Both halves matter. Landing on the curve has to name *that* indicator,
/// or the gesture opens the wrong dialog; landing in open chart has to name
/// nothing, or the double click that snaps back to the live edge — the one
/// every trader already uses — quietly stops working.
#[test]
fn a_double_click_on_an_overlays_line_picks_that_overlay_and_nothing_else() {
    let mut pane = pane_with_overlay(vec![50.0; 6]);
    let slot = pane.indicators.all()[0].slot;
    let (chart, right, total, scale) = pane.last_projection().expect("a drawn projection");
    let (start, _) = pane.viewport.visible_range(chart.width(), total);
    let on_the_line = egui::pos2(pane.viewport.x_center(start, right, total), scale.y(50.0));

    assert_eq!(
        pane.overlay_plot_at(on_the_line),
        Some(slot),
        "the pointer is on the curve"
    );
    assert_eq!(
        pane.overlay_plot_at(on_the_line + egui::vec2(0.0, PLOT_PICK_TOLERANCE_PX * 8.0)),
        None,
        "open chart still means the viewport, not the indicator"
    );

    // A plot the trader switched off in the Style tab is not drawn, so it
    // cannot be picked where it used to be.
    pane.indicators.view_mut(slot).expect("the view").style.set(
        0,
        crate::indicator_style::PlotOverride {
            visible: Some(false),
            ..crate::indicator_style::PlotOverride::default()
        },
    );
    assert_eq!(
        pane.overlay_plot_at(on_the_line),
        None,
        "a line nobody is drawing cannot be grabbed"
    );
}

/// A hidden indicator is not on the chart either, and a gap in a series is
/// a gap in what can be grabbed — the NaN break the renderer honours.
#[test]
fn the_pick_honours_hidden_indicators_and_nan_gaps() {
    let mut pane = pane_with_overlay(vec![50.0, f64::NAN, 50.0, 50.0]);
    let slot = pane.indicators.all()[0].slot;
    let (chart, right, total, scale) = pane.last_projection().expect("a drawn projection");
    let (start, _) = pane.viewport.visible_range(chart.width(), total);
    // Midway across the NaN cell: the renderer draws no segment here.
    let gap_x = (pane.viewport.x_center(start, right, total)
        + pane.viewport.x_center(start + 1, right, total))
        / 2.0;
    assert_eq!(
        pane.overlay_plot_at(egui::pos2(gap_x, scale.y(50.0))),
        None,
        "a gap in the data is a gap in what can be picked"
    );

    let joined_x = (pane.viewport.x_center(start + 2, right, total)
        + pane.viewport.x_center(start + 3, right, total))
        / 2.0;
    let on_the_line = egui::pos2(joined_x, scale.y(50.0));
    assert_eq!(pane.overlay_plot_at(on_the_line), Some(slot));

    pane.indicators.toggle_hidden(slot);
    assert_eq!(
        pane.overlay_plot_at(on_the_line),
        None,
        "the legend's eye takes the line off the chart, pick included"
    );
}

/// The pane's header is its handle into its own settings, so — like the
/// chevron and the divider before it — arming a drawing tool must not
/// silently take it away, and it must not overlap the chevron, which means
/// something else.
#[test]
fn the_pane_header_is_chrome_and_never_overlaps_the_chevron() {
    let pane = pane_with_indicator("native.cvd", vec![vec![0.0, 40.0]]);
    let areas = test_areas(&pane, TEST_PLOT);
    let slot = areas.indicator_panes.first().expect("one pane");
    let header = indicator_render::pane_header_rect(slot.rect, slot.collapsed);
    let chevron = indicator_render::pane_disclosure_rect(slot.rect, slot.collapsed);

    assert!(header.is_positive(), "an open pane has a header row");
    assert!(
        !header.intersects(chevron) || header.left() >= chevron.right(),
        "one rect, one meaning: {header:?} vs {chevron:?}"
    );
    assert!(
        slot.rect.contains(header.center()),
        "the header is inside its own pane"
    );
    assert!(ChartPane::pane_chrome_hit(&areas, header.center()));

    // A collapsed strip has no header of its own: the strip *is* the
    // disclosure, and reads its double click from there.
    assert!(!indicator_render::pane_header_rect(slot.rect, true).is_positive());
}

/// A CVD level and a price level can be one pixel apart on screen and
/// mean unrelated things, so a pick never crosses a band.
#[test]
fn hit_testing_never_crosses_bands() {
    let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0, 2.0]]);
    pane.indicators
        .visible_panes_mut()
        .next()
        .expect("one pane")
        .last_auto = Some((-10.0, 10.0));
    pane.frame.auto_range = Some((100.0, 110.0));
    let areas = test_areas(&pane, TEST_PLOT);
    let bands = pane.bands(&areas);
    let (price, indicator) = (&bands[0], &bands[1]);

    let key = indicator.key.clone();
    pane.drawings.place_on(
        drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "horizontal-line")
            .expect("the horizontal line is registered"),
        &key,
        ChartPoint::at(1.0, 0.0),
    );
    let scale = indicator.scale.expect("a drawable band");
    let on_the_level = egui::pos2(indicator.rect.center().x, scale.y(0.0));
    assert_eq!(
        pane.drawing_at(on_the_level, indicator, indicator.rect.right(), 3),
        Some(0),
        "found on its own band"
    );
    // The same object, asked for from the price band: a horizontal line
    // spans the whole width, so only the band rule can rule it out.
    let in_price_band = egui::pos2(price.rect.center().x, price.rect.center().y);
    assert_eq!(
        pane.drawing_at(in_price_band, price, price.rect.right(), 3),
        None,
        "a CVD level is not a price level"
    );
}

/// A vertical line marks an instant, so it belongs to no band and is
/// painted through all of them — as ONE object. Modelling it as one per
/// band would leave a half-deleted line behind every delete.
#[test]
fn a_time_only_object_is_one_item_that_every_band_paints() {
    let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0]]);
    pane.indicators
        .visible_panes_mut()
        .next()
        .expect("one pane")
        .last_auto = Some((-10.0, 10.0));
    pane.frame.auto_range = Some((100.0, 110.0));
    let areas = test_areas(&pane, TEST_PLOT);
    let bands = pane.bands(&areas);
    let vertical = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "vertical-line")
        .expect("the vertical line is registered");
    // Placed while pointing at the *indicator* band, and still not owned
    // by it.
    pane.drawings
        .place_on(vertical, &bands[1].key, ChartPoint::at(1.0, 0.0));
    assert_eq!(
        pane.drawings.items().len(),
        1,
        "one object, not one per band"
    );
    assert_eq!(pane.drawings.items()[0].band, DrawingBand::AllBands);
    for band in &bands {
        assert!(
            bands::drawing_in_band(&pane.drawings.items()[0], band),
            "every band paints its own clipped segment"
        );
    }
}

/// egui gives an overlapping rect to whoever registers last, and both the
/// chevron and the divider register after the canvas. The drawing path
/// reads the raw pointer instead of a response, so it has to honour that
/// order itself — or arming a tool silently kills both controls.
#[test]
fn the_chevron_and_the_divider_are_never_drawing_surfaces() {
    let pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0]]);
    let areas = test_areas(&pane, TEST_PLOT);
    let slot = areas.indicator_panes.first().expect("one indicator pane");
    let chevron = indicator_render::pane_disclosure_rect(slot.rect, slot.collapsed);
    assert!(ChartPane::pane_chrome_hit(&areas, chevron.center()));
    assert!(ChartPane::pane_chrome_hit(
        &areas,
        egui::pos2(slot.rect.center().x, slot.rect.top())
    ));
    assert!(
        !ChartPane::pane_chrome_hit(&areas, slot.rect.center()),
        "the middle of the pane is canvas"
    );
}

/// Remove an indicator and add it back — the most common thing a trader
/// does to a pane — and its drawings have to come home. Keyed on the slot
/// id (a monotonic counter) they would orphan every single time.
#[test]
fn a_band_key_survives_remove_and_re_add() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let first = add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
    let before = pane
        .indicators
        .pane_key(pane.indicators.all().first().expect("one view"));
    pane.indicators.remove(first);
    add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
    let after = pane
        .indicators
        .pane_key(pane.indicators.all().first().expect("one view"));
    assert_eq!(before, after, "the same pane, so the same key");

    // And a different indicator must never inherit it.
    add_indicator_view(&mut pane, "script.zigzag.pine", vec![vec![0.0]]);
    let other = pane
        .indicators
        .pane_key(pane.indicators.all().last().expect("two views"));
    assert_ne!(before, other);
}

/// Removing one pane must not renumber the ones that outlive it.
///
/// With a positional ordinal, removing the first of two CVD panes turns
/// the survivor into `{cvd, 0}` — the removed pane's key — so it starts
/// painting the *other* pane's annotations on its own axis while its own
/// go parked. That is one pane's marks shown on another's value scale.
#[test]
fn removing_a_pane_does_not_renumber_the_one_that_outlives_it() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let first = add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
    add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
    let survivor = pane
        .indicators
        .pane_key(pane.indicators.all().last().expect("two views"));
    pane.indicators.remove(first);
    assert_eq!(
        pane.indicators
            .pane_key(pane.indicators.all().first().expect("one view left")),
        survivor,
        "the surviving pane keeps the key its drawings were placed with"
    );

    // And the ordinal the removed pane left behind is what a re-added one
    // takes, so *its* drawings come home instead of piling onto the
    // survivor's.
    add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
    let readded = pane
        .indicators
        .pane_key(pane.indicators.all().last().expect("two views"));
    assert_ne!(readded, survivor);
    assert_eq!(readded.ordinal, 0);
}

/// A drawing whose indicator is off the chart is parked: kept, but not
/// painted and not pickable. `band_of` answering `None` is what every
/// caller reads to mean that.
#[test]
fn a_parked_drawing_belongs_to_no_band_on_screen() {
    let mut pane = pane_with_indicator("native.cvd", vec![vec![0.0, 1.0]]);
    pane.indicators
        .visible_panes_mut()
        .next()
        .expect("one pane")
        .last_auto = Some((-10.0, 10.0));
    pane.frame.auto_range = Some((100.0, 110.0));
    let areas = test_areas(&pane, TEST_PLOT);
    let carved = pane.bands(&areas);
    let key = carved[1].key.clone();
    pane.drawings.place_on(
        drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "horizontal-line")
            .expect("registered"),
        &key,
        ChartPoint::at(1.0, 0.0),
    );

    // The indicator goes away; the object does not.
    let slot = pane.indicators.all().first().expect("one view").slot;
    pane.indicators.remove(slot);
    let areas = test_areas(&pane, TEST_PLOT);
    let carved = pane.bands(&areas);
    assert_eq!(carved.len(), 1, "only the price band is left");
    assert_eq!(pane.drawings.items().len(), 1, "the object is kept");
    assert!(
        bands::band_of(&carved, &pane.drawings.items()[0]).is_none(),
        "parked: no band on screen owns it, so nothing paints or picks it"
    );
    assert!(
        !bands::drawing_in_band(&pane.drawings.items()[0], &carved[0]),
        "and it certainly does not fall back onto the price band"
    );
    assert!(matches!(
        pane.band_label(&pane.drawings.items()[0]),
        BandLabel::Parked(_)
    ));
}

/// The drawings read the candles through the band's scale, so the band
/// has to carry the chart's orientation: without it a click would anchor
/// at the mirror of the price under the pointer.
#[test]
fn the_price_band_scale_turns_over_with_the_chart() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.frame.auto_range = Some((100.0, 110.0));
    let areas = test_areas(&pane, TEST_PLOT);
    let upright = pane.bands(&areas)[0].scale.expect("a range is set");
    assert!(!upright.is_inverted());

    pane.price_view.set_inverted(true);
    let scale = pane.bands(&areas)[0].scale.expect("a range is set");
    assert!(scale.is_inverted(), "the band mirrors the candles");
    assert!(
        scale.y(100.0) < scale.y(110.0),
        "low prices ride at the top of the band"
    );
}

/// Two instances of one kind are told apart by ordinal, in add order.
#[test]
fn two_panes_of_the_same_kind_get_different_keys() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
    add_indicator_view(&mut pane, "native.cvd", vec![vec![0.0]]);
    let keys: Vec<_> = pane
        .indicators
        .all()
        .iter()
        .map(|view| pane.indicators.pane_key(view))
        .collect();
    assert_eq!(keys[0].ordinal, 0);
    assert_eq!(keys[1].ordinal, 1);
}

/// On a band the magnet snaps to the pane's own numbers — and to zero,
/// which is the most-drawn level on a signed series.
#[test]
fn the_band_magnet_takes_the_panes_own_values_and_zero() {
    let view_holder = pane_with_indicator("native.cvd", vec![vec![40.0], vec![-15.0]]);
    let view = view_holder
        .indicators
        .visible_panes()
        .next()
        .expect("one pane");
    let scale = PriceScale::from_range(-100.0, 100.0, 0.0, 200.0);
    assert_eq!(
        bands::magnet_value_of(view, 0, scale.y(40.0) + 2.0, &scale, MAGNET_REACH_PX),
        Some(40.0),
        "the plotted value nearest the pointer"
    );
    assert_eq!(
        bands::magnet_value_of(view, 0, scale.y(0.0) - 1.0, &scale, MAGNET_REACH_PX),
        Some(0.0),
        "zero is always a candidate"
    );
    assert_eq!(
        bands::magnet_value_of(view, 0, scale.y(80.0), &scale, MAGNET_REACH_PX),
        None,
        "out of reach snaps nothing, so a free diagonal stays free"
    );
}

/// The caret that says "your object is off the top of this band" has to
/// be a caret. Clamped to the raw edge, half of it fell outside the
/// band's clip and it read as a smudge — which is worse than nothing,
/// because a smudge is not a direction.
#[test]
fn the_off_band_caret_stays_whole_inside_its_band() {
    let band = egui::Rect::from_min_max(egui::pos2(60.0, 400.0), egui::pos2(900.0, 500.0));
    let mut drawing = drawings::Drawings::default();
    drawing.place_on(
        drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "horizontal-line")
            .expect("registered"),
        &DrawingBand::Indicator(drawings::PaneKey {
            kind: std::sync::Arc::from("native.cvd"),
            ordinal: 0,
        }),
        ChartPoint::at(1.0, 0.0),
    );
    let object = &drawing.items()[0];

    // An anchor off the left of the window, at a value above the band.
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("caret-test"),
        ));
        bands::paint_off_band_caret(&painter, band, &[egui::pos2(-500.0, 100.0)], object);
    });
    let points: Vec<egui::Pos2> = output
        .shapes
        .iter()
        .flat_map(|clipped| match &clipped.shape {
            egui::Shape::Path(path) => path.points.clone(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(points.len(), 3, "the caret is one triangle");
    for point in points {
        assert!(
            point.x >= band.left() && point.x <= band.right(),
            "every corner is inside the band it marks: {point:?}"
        );
    }
}

/// A bar spanning 98 … 102, for the magnet.
fn magnet_candle() -> quantick_engine::Bar {
    use rust_decimal::Decimal;
    quantick_engine::Bar {
        open_time: 0,
        close_time: 59_999,
        open: Decimal::from(100),
        high: Decimal::from(102),
        low: Decimal::from(98),
        close: Decimal::from(101),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

/// Ten pixels per price unit over 90 … 110, so the bar's four levels are
/// far enough apart on screen for "nearest" to be unambiguous.
fn magnet_scale() -> PriceScale {
    PriceScale::from_range(90.0, 110.0, 0.0, 200.0)
}

#[test]
fn the_magnet_takes_the_nearest_ohlc_within_reach() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    let high_y = scale.y(102.0);
    assert_eq!(
        magnet_price_of(&candle, high_y, &scale, MAGNET_REACH_PX),
        Some(102.0),
        "exactly on the high"
    );
    assert_eq!(
        magnet_price_of(&candle, high_y + 4.0, &scale, MAGNET_REACH_PX),
        Some(102.0),
        "inside the reach, and still nearer the high than the close"
    );
}

/// The point of the reach: away from every level the anchor stays free,
/// so a diagonal drawn through open space is still a diagonal.
#[test]
fn the_magnet_lets_go_outside_its_reach() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    assert_eq!(
        magnet_price_of(&candle, scale.y(105.0), &scale, MAGNET_REACH_PX),
        None
    );
}

/// The candle magnet's time half: a bar right of the tape lands on the
/// newest bar, left of it on the oldest, and a one-bar tape is slot 0.
#[test]
fn the_candle_magnet_clamps_the_bar_onto_the_tape() {
    assert_eq!(snap_bar_to_tape(99.9, 20), 19.0);
    assert_eq!(snap_bar_to_tape(-3.0, 20), 0.0);
    assert_eq!(snap_bar_to_tape(7.25, 20), 7.25, "inside stays put");
    assert_eq!(snap_bar_to_tape(5.0, 1), 0.0);
    assert_eq!(snap_bar_to_tape(5.0, 0), 0.0, "an empty tape never panics");
}

/// The candle magnet has no reach: however far the pointer floats above
/// or below the candle, the anchor lands on its nearest level — the rule
/// [`AnchorSnap::NearestOhlc`] glues the anchored VWAP's ball with.
#[test]
fn the_candle_magnet_never_lets_go() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    // Far above every level (y of 110, the top of the scale).
    assert_eq!(
        magnet_price_of(&candle, scale.y(110.0), &scale, f32::INFINITY),
        Some(102.0),
        "way above the candle still lands on its high"
    );
    assert_eq!(
        magnet_price_of(&candle, scale.y(90.0), &scale, f32::INFINITY),
        Some(98.0),
        "way below still lands on its low"
    );
}

/// Near-ties resolve to the genuinely nearest level, never to the first
/// one in the list.
#[test]
fn the_magnet_picks_the_nearest_level_not_the_first() {
    let candle = magnet_candle();
    let scale = magnet_scale();
    // Just under the low: 98 is nearer than the open at 100.
    assert_eq!(
        magnet_price_of(&candle, scale.y(97.6), &scale, MAGNET_REACH_PX),
        Some(98.0)
    );
    // Just over the close: 101 is nearer than the high at 102.
    assert_eq!(
        magnet_price_of(&candle, scale.y(101.2), &scale, MAGNET_REACH_PX),
        Some(101.0)
    );
}

/// One geometry for the jump-to-live chip's click region and its paint
/// (audit F6): right-aligned inside the strip, inset on every side, so
/// the click can never miss the pixels.
#[test]
fn the_live_chip_sits_inside_the_strips_right_end() {
    let strip = egui::Rect::from_min_max(egui::pos2(0.0, 576.0), egui::pos2(800.0, 600.0));
    let chip = live_chip_rect(strip);
    assert!(strip.contains_rect(chip), "the chip never leaves its band");
    assert_eq!(chip.right(), strip.right() - LIVE_CHIP_MARGIN_PX);
    assert_eq!(chip.width(), LIVE_CHIP_WIDTH_PX);
    assert_eq!(chip.top(), strip.top() + LIVE_CHIP_VPAD_PX);
    assert_eq!(chip.bottom(), strip.bottom() - LIVE_CHIP_VPAD_PX);
}

/// The header is a strip carved off the pane, not an overlay: the selector
/// must never be painted across market data.
#[test]
fn the_time_pane_header_costs_the_chart_its_own_height() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 600.0));
    let areas = split_time_pane(area);
    assert_eq!(areas.header.height(), crate::time_header::HEIGHT_PX);
    assert_eq!(
        areas.header.bottom(),
        areas.chart.top(),
        "no gap, no overlap"
    );
    assert_eq!(areas.chart.bottom(), area.bottom());
    assert_eq!(areas.header.width(), area.width());
}

/// The rung budget is the lane's width in pixels, not a constant: a lane
/// narrow enough to be a sliver is not worth sixty evaluations, and a
/// chart with no lane at all is worth none.
#[test]
fn the_rung_budget_follows_the_lane_and_is_zero_without_one() {
    assert_eq!(lane_rungs(0.0), 0, "no lane, no ladder");
    assert_eq!(lane_rungs(-5.0), 0);
    assert_eq!(lane_rungs(f32::NAN), 0);

    assert_eq!(lane_rungs(60.0), 10);
    assert_eq!(
        lane_rungs(1.0),
        1,
        "a sliver of a lane still gets its live edge"
    );
    assert_eq!(
        lane_rungs(100_000.0),
        MAX_LANE_RUNGS,
        "the ceiling is a cost statement and holds at any width"
    );
}

/// Drag horizontally across `from`, over three frames: press, move,
/// release. egui reads a drag from the movement *between* frames, so a
/// single frame carrying both the press and the move reports no delta.
fn drag_across(
    pane: &mut ChartPane,
    ctx: &egui::Context,
    area: egui::Rect,
    from: egui::Pos2,
    dx: f32,
) {
    let to = from + egui::vec2(dx, 0.0);
    // The pointer arrives first: a widget only takes a press it can see
    // the pointer over, and the press in the same frame as the move is
    // read against last frame's position.
    let _ = drive_navigation(pane, ctx, area, vec![egui::Event::PointerMoved(from)]);
    let _ = drive_navigation(
        pane,
        ctx,
        area,
        vec![egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }],
    );
    let _ = drive_navigation(pane, ctx, area, vec![egui::Event::PointerMoved(to)]);
    let _ = drive_navigation(
        pane,
        ctx,
        area,
        vec![egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
    );
}

/// The x axis belongs to every chart. Dragging the time strip squeezes or
/// stretches the candles of the pane whose strip is under the pointer —
/// the flow pane *and* the timeframe pane beside it, which is the whole
/// point of the gesture living on each pane's own axis.
#[test]
fn dragging_the_time_strip_zooms_the_pane_it_belongs_to() {
    let ctx = egui::Context::default();
    for (label, mut pane) in [
        (
            "flow",
            ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned()),
        ),
        ("time", ChartPane::time(2, 60_000)),
    ] {
        let start = test_areas(&pane, TEST_PLOT).time_strip.center();

        let before = pane.viewport.px_per_bar();
        drag_across(&mut pane, &ctx, TEST_PLOT, start, -120.0);
        let squeezed = pane.viewport.px_per_bar();
        assert!(
            squeezed < before,
            "{label}: dragging left squeezes ({squeezed} vs {before})"
        );

        drag_across(&mut pane, &ctx, TEST_PLOT, start, 120.0);
        assert!(
            pane.viewport.px_per_bar() > squeezed,
            "{label}: dragging right stretches again"
        );
    }
}

/// Every reason a region cannot honestly be tested is one rule, and both
/// readers of it must agree: the gate shuts on it and the badge prints
/// the word. `off_series` used to shut the gate in silence — the bot
/// paused and the badge went on reading a bare "armed" — which is how a
/// trader watches a setup go by with nothing on the chart to explain it.
#[test]
fn every_paused_region_has_a_word_for_the_badge_and_shuts_the_gate() {
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
        .expect("the rectangle tool is registered");
    let mut drawings_store = drawings::Drawings::default();
    drawings_store.place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
    drawings_store.place(rectangle, drawings::ChartPoint::at(30.0, 110.0));

    // Nothing wrong: no word, and the gate is open.
    assert_eq!(region_pause(&drawings_store.items()[0], false), None);

    // Each fault in turn, most actionable first.
    /// One way a drawing can lose its footing, and the word it is owed.
    type Fault = (fn(&mut drawings::Drawing), &'static str);
    let cases: [Fault; 3] = [
        (
            |drawing| drawing.foreign_market = true,
            "region on another market — paused",
        ),
        (
            |drawing| drawing.off_series = true,
            "region off its series — paused",
        ),
        (|drawing| drawing.hidden = true, "region hidden — paused"),
    ];
    for (break_it, word) in cases {
        let mut store = drawings::Drawings::default();
        store.place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
        store.place(rectangle, drawings::ChartPoint::at(30.0, 110.0));
        break_it(&mut store.items_mut()[0]);
        assert_eq!(
            region_pause(&store.items()[0], false),
            Some(word),
            "the badge is owed a word for this one"
        );
    }

    // "Hide all" is the same pause reached from the toolbar.
    assert_eq!(
        region_pause(&drawings_store.items()[0], true),
        Some("region hidden — paused")
    );

    // The gate half of "one rule, two readers": whenever there is a
    // word, `strategy_region` refuses — so the badge can never paint a
    // running bot over a region every bar is being refused against.
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.drawings
        .place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
    pane.drawings
        .place(rectangle, drawings::ChartPoint::at(30.0, 110.0));
    let id = pane.drawings.items()[0].id;
    assert!(
        pane.strategy_region(id, 5).is_some(),
        "nothing wrong: the region is testable"
    );
    for break_it in [
        (|drawing: &mut drawings::Drawing| drawing.foreign_market = true) as fn(&mut _),
        |drawing: &mut drawings::Drawing| drawing.off_series = true,
        |drawing: &mut drawings::Drawing| drawing.hidden = true,
    ] {
        let index = pane.drawings.index_of(id).expect("drawing lives");
        let mut drawing = pane.drawings.items()[index].clone();
        break_it(&mut drawing);
        let word = region_pause(&drawing, false);
        pane.drawings.items_mut()[index] = drawing;
        assert!(word.is_some(), "this fault owes the badge a word");
        assert!(
            pane.strategy_region(id, 5).is_none(),
            "and shuts the gate: {word:?}"
        );
        // Put it back for the next fault.
        let index = pane.drawings.index_of(id).expect("drawing lives");
        pane.drawings.items_mut()[index].foreign_market = false;
        pane.drawings.items_mut()[index].off_series = false;
        pane.drawings.items_mut()[index].hidden = false;
    }
}

/// The badge states one condition once, in the words that carry the way
/// out. A paused region makes `strategy_region` refuse, which the kernel
/// records as "region not active on this bar" — true of a span, and a
/// lie about a band that is merely hidden. Two vocabularies for one fact
/// leave the trader deciding which clause to believe.
#[test]
fn a_paused_regions_badge_says_the_specific_thing_and_not_the_general_one() {
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
        .expect("the rectangle tool is registered");
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.drawings
        .place(rectangle, drawings::ChartPoint::at(0.0, 100.0));
    pane.drawings
        .place(rectangle, drawings::ChartPoint::at(30.0, 110.0));
    let id = pane.drawings.items()[0].id;
    let index = pane.drawings.index_of(id).expect("drawing lives");
    pane.drawings.items_mut()[index].hidden = true;

    let instance = crate::strategy_anchors::AnchoredInstance {
        drawing: id,
        preset: "BF".to_owned(),
        spec: crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell),
        armed: quantick_strategy::ArmedStrategy::new(
            quantick_strategy::StrategyParams {
                side: quantick_engine::Side::Sell,
                quantity: rust_decimal::Decimal::ONE,
                tp_mult: rust_decimal::Decimal::ONE,
                sl_mult: rust_decimal::Decimal::ONE,
                rearm: quantick_strategy::Rearm::OneShot,
                on_break: quantick_strategy::BreakPolicy::Ignore,
                execution: quantick_strategy::Execution::Paper,
            },
            Box::new(quantick_strategy::ForceTrigger::new(
                quantick_strategy::ForceParams::default_band(),
            )),
        ),
        alarm: None,
        cue: crate::audio::Cue::default(),
        mark: crate::strategy_anchors::AlarmMark::Quiet,
    };
    // Nothing was armed here before, so there is no replaced instance
    // whose resting order would need sweeping.
    assert!(pane.strategies.anchors.arm(instance).is_empty());

    let badge = pane.strategy_badge_text(id);
    assert!(
        badge.contains("region hidden — paused"),
        "the specific fault, with the fix in it: {badge}"
    );
    assert!(
        !badge.contains("region not active"),
        "and not the general one beside it, which is false here — the              span covers this bar perfectly, the band is hidden: {badge}"
    );
}

/// A bar with a one-point shadow either side of its body, at sell-zone
/// prices.
fn zone_bar(open: i64, close: i64) -> quantick_engine::Bar {
    let open = rust_decimal::Decimal::from(open);
    let close = rust_decimal::Decimal::from(close);
    quantick_engine::Bar {
        open_time: 0,
        close_time: 0,
        open,
        high: open.max(close) + rust_decimal::Decimal::ONE,
        low: open.min(close) - rust_decimal::Decimal::ONE,
        close,
        buy_volume: rust_decimal::Decimal::ONE,
        sell_volume: rust_decimal::Decimal::ONE,
        trade_count: 2,
    }
}

/// The badge must answer about the bar in front of the trader.
///
/// This is the bug as reported, reduced: a setup at the top of a sell
/// zone did not fire, and the badge read `last held: the body never cut
/// the region` — a true sentence about a *different* bar, further down,
/// judged earlier. The bar the trader was pointing at had been refused
/// by the **ruler**, which produces no signal at all, and the no-signal
/// path clears the note and leaves the previous refusal standing.
/// `status_line` narrates the ruler in that case and the right-click
/// menu shows it; this badge did not, so the chart carried a confident
/// answer to a question about another candle.
///
/// The assertion that matters is the *order*: what happened on this bar
/// leads, and the standing refusal rides behind it, marked as past.
#[test]
fn the_badge_names_the_rulers_own_refusal_and_not_only_an_older_bars() {
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
        .expect("the rectangle tool is registered");
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.drawings
        .place(rectangle, drawings::ChartPoint::at(0.0, 180_300.0));
    pane.drawings
        .place(rectangle, drawings::ChartPoint::at(30.0, 180_430.0));
    let id = pane.drawings.items()[0].id;

    let mut instance = crate::strategy_anchors::AnchoredInstance {
        drawing: id,
        preset: "SellGainAlarm".to_owned(),
        spec: crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell),
        armed: quantick_strategy::ArmedStrategy::new(
            quantick_strategy::StrategyParams {
                side: quantick_engine::Side::Sell,
                quantity: rust_decimal::Decimal::ONE,
                tp_mult: rust_decimal::Decimal::ONE,
                sl_mult: rust_decimal::Decimal::ONE,
                rearm: quantick_strategy::Rearm::Auto,
                on_break: quantick_strategy::BreakPolicy::Ignore,
                execution: quantick_strategy::Execution::Paper,
            },
            Box::new(quantick_strategy::ForceTrigger::new(
                quantick_strategy::ForceParams {
                    window: 3,
                    min_factor: rust_decimal::Decimal::new(15, 1),
                    max_factor: rust_decimal::Decimal::new(25, 1),
                    min_range: rust_decimal::Decimal::ZERO,
                },
            )),
        ),
        alarm: None,
        cue: crate::audio::Cue::default(),
        mark: crate::strategy_anchors::AlarmMark::Quiet,
    };

    let region = quantick_strategy::Region::new(
        rust_decimal::Decimal::from(180_300),
        rust_decimal::Decimal::from(180_430),
    );
    // Two quiet bars warm the 3-bar window, well under the zone.
    for bar in [zone_bar(180_200, 180_170), zone_bar(180_170, 180_140)] {
        assert!(
            instance
                .armed
                .on_closed_bar(&bar, &region, true, true)
                .is_empty()
        );
    }
    // A force bar whose body sits entirely below the zone: the region
    // gate refuses it by name, and that refusal now stands.
    assert!(
        instance
            .armed
            .on_closed_bar(&zone_bar(180_140, 180_080), &region, true, true)
            .is_empty()
    );
    assert_eq!(
        instance.armed.hold_reason().map(|held| held.reason),
        Some("the body never cut the region"),
        "the older bar's refusal, which is what used to reach the badge alone"
    );
    // Now the bar the trader points at: the ruler refuses it outright,
    // so no signal is produced and no gate records anything.
    assert!(
        instance
            .armed
            .on_closed_bar(&zone_bar(180_080, 180_075), &region, true, true)
            .is_empty()
    );

    assert!(pane.strategies.anchors.arm(instance).is_empty());
    let badge = pane.strategy_badge_text(id);

    let ruler = badge
        .find("quiet")
        .unwrap_or_else(|| panic!("the ruler's reading of this bar belongs on the badge: {badge}"));
    let stale = badge.find("last held:").unwrap_or_else(|| {
        panic!("the standing refusal is still worth showing, marked as past: {badge}")
    });
    assert!(
        ruler < stale,
        "what happened on this bar leads; the older refusal rides behind: {badge}"
    );
    assert!(
        badge.contains('×'),
        "the ruler's number is the one a trader would act on: {badge}"
    );
}
