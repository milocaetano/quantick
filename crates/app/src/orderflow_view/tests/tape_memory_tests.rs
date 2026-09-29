//! The live painter owns a persistent display history within each pane.

use super::*;
use quantick_orderflow::projection::{
    PriceWindow, TapeDotFrame, TapeDotGeometry, TapeDotView, TapeHorizontalGeometry,
};
use rust_decimal::prelude::FromPrimitive as _;

const WIN_TIME_MS: i64 = 1_790_078_859_000;
const WINDOW_MS: i64 = 30_000;
const WIN_PRICES: (f64, f64) = (187_450.0, 187_550.0);

fn memory_view() -> (OrderflowView, Hold) {
    let (mut view, _, hold) = held_view(true);
    let before = view.config.clone();
    view.config.bubbles.min_radius = 2.0;
    view.config.bubbles.max_radius = 15.0;
    view.config.bubbles.detail_min_radius = 32.0;
    view.config.bubbles.hollow_small_buys = false;
    view.config.bubbles.halo_strength = 0.0;
    view.config.bubbles.trail_length = 0.0;
    view.config.bubbles.show_quantity_labels = false;
    view.config.bubbles.show_trade_count = false;
    view.commit_config_changes(before);
    view.set_live_lane_window(LaneWindow::Fixed { ms: WINDOW_MS });
    (view, hold)
}

fn real_win_prints() -> [Trade; 3] {
    [
        print(64_198, WIN_TIME_MS + 122, 187_505, 1, Side::Buy),
        print(64_206, WIN_TIME_MS + 336, 187_510, 1, Side::Buy),
        print(64_264, WIN_TIME_MS + 1_076, 187_535, 105, Side::Buy),
    ]
}

fn paint(
    view: &mut OrderflowView,
    shown: &VisibleOrderflow,
    now_ms: i64,
    prices: (f64, f64),
) -> Vec<(egui::Pos2, f32)> {
    view.set_replay_clock_at(now_ms, Some(now_ms), None);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 400.0));
    let output = egui::Context::default().run(egui::RawInput::default(), |ctx| {
        view.draw_aggressions(
            &ctx.layer_painter(egui::LayerId::background()),
            rect,
            &Viewport::new(),
            1,
            shown,
            egui::Color32::BLACK,
            rect.width(),
            false,
            prices,
        );
    });
    let mut discs: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle) if circle.fill != egui::Color32::TRANSPARENT => {
                Some((circle.center, circle.radius))
            }
            _ => None,
        })
        .collect();
    discs.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
    discs
}

/// Observe what the real paint pass retained, without feeding it any facts.
fn retained(view: &OrderflowView, shown: &VisibleOrderflow, prices: (f64, f64)) -> TapeDotFrame {
    let horizontal = TapeHorizontalGeometry::resolve(640.0, 400.0, &view.config.bubbles);
    let scale = shown.volume_dots.as_ref().unwrap();
    let sizing = view.dot_rungs.sizing(scale, 400.0).unwrap();
    let mut bubbles = view.config.bubbles.clone();
    bubbles.max_radius = horizontal.max_radius;
    view.tape_dots.borrow_mut().project(
        &[],
        TapeDotView {
            now_ms: view.lane_now_ms().unwrap(),
            window_ms: WINDOW_MS,
            dot_window_ms: scale.tape_window_ms,
            evicted_through_ms: None,
            prices: PriceWindow::new(
                Decimal::from_f64(prices.0).unwrap(),
                Decimal::from_f64(prices.1).unwrap(),
            )
            .unwrap(),
            geometry: TapeDotGeometry {
                left_x: 1.0 - 1.0 / shown.slot_count.max(1) as f64,
                right_x: 1.0,
                width_px: horizontal.span_px,
                height_px: 400.0,
            },
        },
        sizing,
        &bubbles,
        &view.config.live_lane,
        &[],
    )
}

fn seed_closed_history(view: &mut OrderflowView) -> Arc<VisibleOrderflow> {
    let prints = real_win_prints();
    for trade in &prints[..2] {
        view.record_trade(trade);
    }
    let shown = frame(view, &prints[..2]).unwrap();
    let _ = paint(view, &shown, WIN_TIME_MS + 400, WIN_PRICES);
    let _ = paint(view, &shown, WIN_TIME_MS + 2_000, WIN_PRICES);
    shown
}

#[test]
fn the_live_painter_keeps_closed_membership_when_a_large_print_arrives_on_this_frame() {
    let (mut view, hold) = memory_view();
    let shown = seed_closed_history(&mut view);
    let old = retained(&view, &shown, WIN_PRICES);
    let prints = real_win_prints();
    view.record_trade(&prints[2]);
    let next = frame(&mut view, &prints).unwrap();
    let shapes = paint(&mut view, &next, WIN_TIME_MS + 2_100, WIN_PRICES);
    let current = retained(&view, &next, WIN_PRICES);
    let worker_has_no_frame = view.worker.published().frame.is_none();
    hold.release();
    assert!(
        worker_has_no_frame,
        "receipt must not wait for a worker frame"
    );
    assert_eq!(old.marks.len(), 1);
    assert_eq!(old.marks[0].agg_ids, vec![64_198, 64_206]);
    let previous = current
        .marks
        .iter()
        .find(|mark| mark.agg_ids == old.marks[0].agg_ids)
        .expect("a distant new maximum cannot split the painted past");
    for member in [&old.marks[0], previous] {
        assert_eq!(member.quantity, Decimal::TWO);
        assert_eq!(member.price, Decimal::new(1_875_075, 1));
        assert_eq!(
            member.timestamp_quantity,
            Decimal::from(3_580_157_718_458_i64)
        );
    }
    assert_eq!(current.marks.len(), 2);
    assert_eq!(shapes.len(), 2, "the new print paints on this frame");
    assert_eq!(
        current
            .marks
            .iter()
            .map(|mark| mark.quantity)
            .sum::<Decimal>(),
        Decimal::from(107)
    );
}

#[test]
fn tape_history_is_cleared_by_source_replay_mode_and_explicit_visual_changes() {
    for change in ["source", "replay", "mode", "side", "full", "style"] {
        let (mut view, hold) = memory_view();
        let _ = seed_closed_history(&mut view);
        let before_count = view.tape_dots.borrow().retained_group_count();
        match change {
            "source" => view.reset_for_symbol("WINV26"),
            "replay" => view.reset_pending_tape(),
            _ => {
                let before = view.config.clone();
                match change {
                    "mode" => view.config.live_lane.tape_only = false,
                    "side" => view.config.show_buy_aggressions = false,
                    "full" => view.config.volume_dots.auto_full = false,
                    "style" => view.config.bubbles.max_radius = 18.0,
                    _ => unreachable!(),
                }
                view.commit_config_changes(before);
            }
        }
        let after_count = view.tape_dots.borrow().retained_group_count();
        hold.release();
        assert!(
            before_count > 0,
            "fixture has painted history before {change}"
        );
        assert_eq!(after_count, 0, "{change} starts a fresh display epoch");
    }
}

#[test]
fn retained_weighted_prices_participate_in_fit_after_an_old_member_expires() {
    let (mut view, hold) = memory_view();
    let _ = seed_closed_history(&mut view);
    view.set_replay_clock_at(
        WIN_TIME_MS + WINDOW_MS + 200,
        Some(WIN_TIME_MS + WINDOW_MS + 200),
        None,
    );
    let price_range = view.tape_price_range();
    hold.release();
    assert_eq!(
        price_range,
        Some((187_507.5, 187_510.0)),
        "the sealed centroid is visible after its oldest native member expires"
    );
}

#[test]
fn a_clearance_radius_change_never_moves_the_tape_time_inset() {
    let (mut view, hold) = memory_view();
    let prints = [
        print(1, 1_011, 100, 1, Side::Buy),
        print(2, 1_211, 110, 4, Side::Buy),
        print(3, 4_011, 180, 16, Side::Buy),
    ];
    for trade in &prints {
        view.record_trade(trade);
    }
    let shown = frame(&mut view, &prints).unwrap();
    let before = paint(&mut view, &shown, 5_000, (90.0, 190.0));
    let after = paint(&mut view, &shown, 5_040, (0.0, 1_000.0));
    let horizontal = TapeHorizontalGeometry::resolve(640.0, 400.0, &view.config.bubbles);
    hold.release();
    assert_eq!(before.len(), 3);
    assert_eq!(after.len(), 3, "axis changes cannot regroup closed dots");
    let shift = 40.0 / WINDOW_MS as f32 * horizontal.span_px;
    let radius_ratio = after[2].1 / before[2].1;
    assert!(
        radius_ratio < 1.0,
        "the compressed axis needs a uniform smaller cap"
    );
    for (old, new) in before.iter().zip(&after) {
        assert!((old.0.x - new.0.x - shift).abs() < 0.001);
        assert!((new.1 / old.1 - radius_ratio).abs() < 0.0001);
    }
}

#[test]
fn hiding_a_side_cannot_repaint_its_retained_historical_members() {
    let (mut view, hold) = memory_view();
    let shown = seed_closed_history(&mut view);
    let before = view.config.clone();
    view.config.show_buy_aggressions = false;
    view.commit_config_changes(before);
    let shapes = paint(&mut view, &shown, WIN_TIME_MS + 2_100, WIN_PRICES);
    hold.release();
    assert!(
        shapes.is_empty(),
        "a visibility edit cannot leak cached buy dots"
    );
}

/// A visit to the past never touches the live tape's memory: back at live
/// the tape paints exactly what a pane that never left it paints.
#[test]
fn a_visit_to_the_past_leaves_the_live_tape_exactly_as_it_was() {
    use quantick_orderflow::tape_view::TapeEnd;
    let (mut control, control_hold) = memory_view();
    let control_shown = seed_closed_history(&mut control);
    let (mut view, hold) = memory_view();
    let shown = seed_closed_history(&mut view);
    let retained = view.tape_dots.borrow().retained_group_count();
    let past = TapeEnd::Past {
        end_ms: WIN_TIME_MS + 900,
    };
    view.set_tape_end(past);
    assert_eq!(view.tape_end(), past);
    let _ = paint(&mut view, &shown, WIN_TIME_MS + 2_050, WIN_PRICES);
    assert_eq!(
        view.tape_dots.borrow().retained_group_count(),
        retained,
        "the past paints from a memory of its own"
    );
    view.set_tape_end(TapeEnd::Live);
    let live = paint(&mut view, &shown, WIN_TIME_MS + 2_100, WIN_PRICES);
    let expected = paint(
        &mut control,
        &control_shown,
        WIN_TIME_MS + 2_100,
        WIN_PRICES,
    );
    hold.release();
    control_hold.release();
    assert!(!expected.is_empty());
    assert_eq!(live, expected);
}
