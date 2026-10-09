//! The native tape beside the tick candles draws what the full-width tape
//! drew at the same lane width, and nothing the candles do moves it.
use super::*;
use quantick_engine::Side;
use quantick_orderflow::engine::VisibleOrderflow;
use quantick_orderflow::projection::PaneGeometry;
use std::sync::Arc;

const LANE_PX: f32 = 600.0;
const HEIGHT_PX: f32 = 400.0;
const WINDOW_MS: i64 = 15_000;
const PRICES: (f64, f64) = (90.0, 115.0);
/// Beside the candles the chart is wider than its tape.
const SPLIT_CHART_PX: f32 = 1_500.0;

/// The WIN preset in one of its two presentations: the full-width tape the
/// trader approved (`tape_only`), or the same tape beside the candles.
fn win_view(tape_only: bool) -> OrderflowView {
    let mut view = OrderflowView::new("WINV26");
    let before = view.config.clone();
    assert!(view.apply_preset("mini index regions"));
    view.config.live_lane.tape_only = tape_only;
    view.config.live_lane.native_tape = !tape_only;
    view.commit_config_changes(before);
    view.set_live_lane_window(LaneWindow::Fixed { ms: WINDOW_MS });
    assert!(view.config.native_tape());
    assert_eq!(view.config.tape_only(), tape_only);
    view
}

/// Three-print tick candles over twelve seconds of both sides, and the
/// forming candle holding the last two prints.
fn session(view: &mut OrderflowView) -> (Vec<Bar>, Bar) {
    let mut closed = Vec::new();
    let mut forming: Option<Bar> = None;
    for index in 0..14_u32 {
        let trade = Trade {
            agg_id: u64::from(index) + 1,
            timestamp_ms: 1_000 + i64::from(index) * 870,
            price: Decimal::from(100 + i64::from(index % 5) * 2 - 4),
            quantity: Decimal::from(1 + index % 4),
            side: if index % 3 == 0 {
                Side::Sell
            } else {
                Side::Buy
            },
        };
        view.record_trade(&trade);
        match forming.as_mut() {
            Some(bar) if bar.trade_count < 3 => bar.extend(&trade),
            _ => {
                closed.extend(forming.take());
                forming = Some(Bar::opened_by(&trade));
            }
        }
    }
    let forming = forming.expect("a forming candle");
    view.set_replay_clock_at(13_000, Some(forming.close_time), None);
    (closed, forming)
}

/// What the pane asks of the tape for one frame, with the candles on
/// screen given by `closed`/`partial` and zoomed to `px_per_bar`. Asked
/// twice around a flush, as the pane does frame after frame, so the
/// returned frame composes the worker's answer with the accepted prints.
fn frame(
    view: &mut OrderflowView,
    closed: &[Bar],
    partial: Option<&Bar>,
    px_per_bar: f32,
) -> Arc<VisibleOrderflow> {
    frame_with_lane(view, closed, partial, px_per_bar, LANE_PX)
}

/// [`frame`] with a lane `lane_px` wide; `0` is the pane with the tape off.
fn frame_with_lane(
    view: &mut OrderflowView,
    closed: &[Bar],
    partial: Option<&Bar>,
    px_per_bar: f32,
    lane_px: f32,
) -> Arc<VisibleOrderflow> {
    let lane_bars: Vec<_> = closed
        .iter()
        .chain(partial)
        .map(|bar| (bar.open_time, bar.close_time))
        .collect();
    let ask = |view: &mut OrderflowView| {
        view.project_visible(
            VisibleBarTimeline::new(1, 0, closed, partial),
            lane_px > 0.0,
            partial.is_some(),
            Some(3_000),
            PRICES,
            Some(PaneGeometry {
                px_per_bar,
                lane_width_px: lane_px,
                lane_window_ms: WINDOW_MS,
                height_px: HEIGHT_PX,
                lane_bars: lane_bars.clone(),
            }),
        )
    };
    let _ = ask(view);
    view.flush_for_test();
    ask(view).expect("the tape projects")
}

/// Every shape the aggression pass painted, as its bounds measured from the
/// lane's left edge, sorted so two passes compare mark for mark.
fn drawn_tape(
    view: &OrderflowView,
    frame: &VisibleOrderflow,
    chart_px: f32,
    total: usize,
    px_per_bar: f32,
) -> Vec<[f32; 4]> {
    drawn_with_lane(view, frame, chart_px, total, px_per_bar, LANE_PX)
}

/// [`drawn_tape`] with a lane `lane_px` wide; `0` is the pane with the tape
/// off, whose bounds are then measured from the chart's right edge.
fn drawn_with_lane(
    view: &OrderflowView,
    frame: &VisibleOrderflow,
    chart_px: f32,
    total: usize,
    px_per_bar: f32,
    lane_px: f32,
) -> Vec<[f32; 4]> {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(chart_px, HEIGHT_PX));
    let mut viewport = Viewport::new();
    viewport.set_px_per_bar(px_per_bar);
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        view.draw_aggressions(
            &ctx.layer_painter(egui::LayerId::background()),
            rect,
            &viewport,
            total,
            frame,
            egui::Color32::BLACK,
            lane_px,
            false,
            PRICES,
        );
    });
    let lane_left = chart_px - lane_px;
    let mut bounds: Vec<[f32; 4]> = output
        .shapes
        .iter()
        .map(|shape| shape.shape.visual_bounding_rect())
        .filter(|bounds| bounds.is_positive())
        .map(|bounds| {
            [
                bounds.left() - lane_left,
                bounds.top(),
                bounds.right() - lane_left,
                bounds.bottom(),
            ]
        })
        .collect();
    bounds.sort_by(|a, b| a.partial_cmp(b).expect("finite bounds"));
    bounds
}

fn assert_same_marks(actual: &[[f32; 4]], expected: &[[f32; 4]], why: &str) {
    assert!(!expected.is_empty(), "the tape drew nothing: {why}");
    assert_eq!(actual.len(), expected.len(), "{why}");
    for (actual, expected) in actual.iter().zip(expected) {
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a - e).abs() < 0.01,
                "{why}: {actual:?} against {expected:?}"
            );
        }
    }
}

fn live_marks(frame: &VisibleOrderflow) -> Vec<(Vec<u64>, Decimal, Decimal, Decimal)> {
    frame
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .map(|mark| {
            (
                mark.agg_ids.clone(),
                mark.quantity,
                mark.buy_quantity,
                mark.price,
            )
        })
        .collect()
}

/// At equal lane width the tape beside the candles is the approved tape:
/// the same dots and pies at the same coordinates, the same memberships,
/// the same clock, the same axis range and the same sizing.
#[test]
fn the_native_split_tape_draws_what_the_tape_only_pane_drew_at_equal_lane_width() {
    let mut approved = win_view(true);
    let mut beside = win_view(false);
    let (closed, forming) = session(&mut approved);
    let _ = session(&mut beside);
    let total = closed.len() + 1;
    let expected = frame(&mut approved, &closed, Some(&forming), 10.0);
    let actual = frame(&mut beside, &closed, Some(&forming), 10.0);
    assert_eq!(*actual.projection, *expected.projection);
    assert_eq!(actual.live_edge, expected.live_edge, "the same tape clock");
    assert_eq!(actual.volume_dots, expected.volume_dots);
    assert_eq!(beside.lane_now_ms(), approved.lane_now_ms());
    assert_eq!(beside.tape_price_range(), approved.tape_price_range());
    assert_same_marks(
        &drawn_tape(&beside, &actual, SPLIT_CHART_PX, total, 10.0),
        &drawn_tape(&approved, &expected, LANE_PX, total, 10.0),
        "the tape beside the candles against the full-width tape",
    );
}

/// Panning the candles into history and widening them changes the bars on
/// screen and nothing on the tape: its marks, their memberships and radii,
/// its grouping and the range the shared axis fits to.
#[test]
fn panning_or_zooming_the_candles_leaves_the_native_tape_unchanged() {
    let mut following = win_view(false);
    let mut moved = win_view(false);
    let (closed, forming) = session(&mut following);
    let _ = session(&mut moved);
    let total = closed.len() + 1;
    let at_live = frame(&mut following, &closed, Some(&forming), 10.0);
    let in_history = frame(&mut moved, &closed[..2], None, 32.0);
    assert_eq!(live_marks(&in_history), live_marks(&at_live));
    assert_eq!(moved.tape_price_range(), following.tape_price_range());
    let rungs = |frame: &VisibleOrderflow| {
        frame
            .volume_dots
            .as_ref()
            .map(|dots| (dots.tape_window_ms, dots.tape_level_ticks))
    };
    assert_eq!(rungs(&in_history), rungs(&at_live), "the tape's grouping");
    assert_same_marks(
        &drawn_tape(&moved, &in_history, SPLIT_CHART_PX, total, 32.0),
        &drawn_tape(&following, &at_live, SPLIT_CHART_PX, total, 10.0),
        "panned and zoomed candles against candles following live",
    );
}

/// The WIN preset keeps its candles beside a tape whose divider still
/// moves; tape only keeps the whole canvas and no divider.
#[test]
fn the_mini_index_preset_keeps_candles_beside_a_resizable_native_tape() {
    let mut view = OrderflowView::new("WINV26");
    let before = view.config.clone();
    assert!(view.apply_preset("mini index regions"));
    view.commit_config_changes(before);
    assert!(view.config.native_tape() && !view.config.tape_only());
    let share = view.config.live_lane.width_share;
    assert!((view.lane_width_px(1_000.0) - share * 1_000.0).abs() < 0.5);
    view.resize_live_lane(-100.0, 1_000.0);
    assert!(
        (view.lane_width_px(1_000.0) - (share * 1_000.0 + 100.0)).abs() < 0.5,
        "dragging the divider left gives the tape more room"
    );
    assert!(view.config.native_tape(), "and keeps it native");
    let before = view.config.clone();
    view.config.live_lane.tape_only = true;
    view.commit_config_changes(before);
    view.resize_live_lane(-100.0, 1_000.0);
    assert_eq!(
        view.lane_width_px(1_000.0),
        1_000.0,
        "tape only has no divider"
    );
}

/// Tape off is the original tick chart. The WIN preset's native switch,
/// still set while the tape is off, changes none of the candles' volume
/// dots: not the frame's marks, not their rungs, not what is painted.
#[test]
fn with_the_tape_off_the_native_switch_leaves_the_tick_chart_as_it_was() {
    let tape_off = |native_tape: bool| {
        let mut view = win_view(false);
        let before = view.config.clone();
        view.config.live_lane.native_tape = native_tape;
        view.commit_config_changes(before);
        view.set_bubbles_enabled(true);
        view.set_lane_enabled(false);
        assert!(!view.config.native_tape() && !view.lane_enabled());
        view
    };
    let mut switched = tape_off(true);
    let mut ordinary = tape_off(false);
    let (closed, forming) = session(&mut ordinary);
    let _ = session(&mut switched);
    let total = closed.len() + 1;
    let expected = frame_with_lane(&mut ordinary, &closed, Some(&forming), 10.0, 0.0);
    let actual = frame_with_lane(&mut switched, &closed, Some(&forming), 10.0, 0.0);
    assert!(
        expected
            .projection
            .aggressions
            .iter()
            .any(|mark| !mark.live),
        "the tick chart carries volume dots on its candles"
    );
    assert_eq!(*actual.projection, *expected.projection);
    assert_eq!(actual.volume_dots, expected.volume_dots);
    assert_same_marks(
        &drawn_with_lane(&switched, &actual, SPLIT_CHART_PX, total, 10.0, 0.0),
        &drawn_with_lane(&ordinary, &expected, SPLIT_CHART_PX, total, 10.0, 0.0),
        "the candles with the native switch set against the candles without it",
    );
}

/// Beside a native tape the candles key no mark of their own: their volume
/// dots are drawn from the bars' footprints. The live strip still owes the
/// forming candle's whole aggression, so it reads that footprint — every
/// contract the candle traded, on its side, and nothing from the closed ones.
#[test]
fn the_live_strip_shows_the_forming_candle_beside_a_native_tape() {
    let mut view = win_view(false);
    view.set_projection_demand(true);
    let mut ladder = quantick_engine::FootprintBuilder::new(
        view.config.price_grouping,
        quantick_engine::DEFAULT_LEVEL_CAP,
    );
    let mut closed = Vec::new();
    let mut forming: Option<Bar> = None;
    for index in 0..14_u32 {
        let trade = Trade {
            agg_id: u64::from(index) + 1,
            timestamp_ms: 1_000 + i64::from(index) * 870,
            price: Decimal::from(100 + i64::from(index % 5) * 2 - 4),
            quantity: Decimal::from(1 + index % 4),
            side: if index % 3 == 0 {
                Side::Sell
            } else {
                Side::Buy
            },
        };
        view.record_trade(&trade);
        match forming.as_mut() {
            Some(bar) if bar.trade_count < 5 => bar.extend(&trade),
            _ => {
                closed.extend(forming.take());
                let _ = ladder.close();
                forming = Some(Bar::opened_by(&trade));
            }
        }
        ladder.push(&trade);
    }
    let forming = forming.expect("a forming candle");
    view.set_replay_clock_at(13_000, Some(forming.close_time), None);
    let _ = frame(&mut view, &closed, Some(&forming), 40.0);

    let rows = view.live_strip_rows(Some(forming.open_time), ladder.partial());
    assert!(!rows.is_empty(), "the strip draws the forming candle");
    let buy: Decimal = rows.iter().map(|row| row.buy).sum();
    let sell: Decimal = rows.iter().map(|row| row.sell).sum();
    assert_eq!(
        (buy, sell),
        (forming.buy_volume, forming.sell_volume),
        "every contract of the forming candle, on its side: {rows:?}"
    );
}
