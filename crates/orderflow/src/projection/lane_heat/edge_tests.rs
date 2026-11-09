//! The tape's insets at every bubble size the trader can reach, in every
//! state the tape is drawn in: live at its edge, live with a book that is no
//! longer current, and held in the past. Each edge of the lane is covered by
//! recorded depth or by a labelled gap, never left a dark strip.

use super::lead_tests::{
    LANE_HEIGHT_PX, LANE_START_MS, NOW_MS, WINDOW_MS, config, history_from, level, painted,
    timeline,
};
use super::*;
use crate::config::{BubbleScaleTarget, MAX_BUBBLE_DISPLAY_SCALE, MAX_BUBBLE_MAX_RADIUS};
use crate::projection::{
    BOOK_BEYOND_LEAD, BOOK_NOT_CURRENT, DotRungMemory, HeatmapProjection, PaneGeometry,
    TapeHorizontalGeometry, project_past_heat, stale_book_gap,
};
use crate::{DisplayGrouping, HeatmapConfig};

/// The smallest disc and the slider's top: the largest radius at the
/// largest Tape gain.
const SIZES: [(f32, f32); 2] = [
    (4.0, 1.0),
    (MAX_BUBBLE_MAX_RADIUS, MAX_BUBBLE_DISPLAY_SCALE),
];

/// A wide lane; the trader's, where the largest discs leave 22 px of time
/// span; one so narrow its lead stops at the cap; and one no wider than the
/// largest disc, with no time span left at all.
const WIDTHS: [f32; 4] = [400.0, 310.0, 290.0, 280.0];

/// A book recorded long before the tape's window, so the insets read depth.
const RECORDED_FROM_MS: i64 = -20 * WINDOW_MS;

/// The newest change [`history_from`] records.
const LATEST_BOOK_MS: i64 = 1_900;

/// A held end with recorded book on both sides of its window.
const HELD_DEEP_MS: i64 = -2 * WINDOW_MS;

fn sized(max_radius: f32, gain: f32) -> HeatmapConfig {
    let mut config = config(max_radius, 1_000);
    config
        .bubbles
        .display_scale
        .set(BubbleScaleTarget::Tape, gain);
    config
}

/// The lead the view asks for on a lane `width` wide, and whether it stops
/// at its cap.
fn lead_at(config: &HeatmapConfig, width: f32) -> (i64, bool) {
    let zoom = DotRungMemory::default().choose(
        PaneGeometry {
            px_per_bar: 8.0,
            lane_width_px: width,
            lane_window_ms: WINDOW_MS,
            height_px: LANE_HEIGHT_PX,
            lane_bars: Vec::new(),
        },
        config,
        (94.0, 102.0),
        None,
    );
    (zoom.tape_lead_ms(WINDOW_MS), zoom.tape_lead_capped())
}

fn grouping() -> EffectiveGrouping {
    EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, 8.into())
}

fn prices() -> PriceWindow {
    PriceWindow::new(94.into(), 102.into()).unwrap()
}

/// Pixels each gap is painted over, as the painter places it.
fn gap_px(geometry: TapeHorizontalGeometry, gaps: &[GapPrimitive], regions: usize) -> Vec<Mark> {
    let fraction = |x: f64| x * regions as f64 - (regions - 1) as f64;
    gaps.iter()
        .map(|gap| {
            let span = geometry
                .lead_px(gap.lead_span)
                .unwrap_or_else(|| (geometry.x(fraction(gap.x0)), geometry.x(fraction(gap.x1))));
            (span, Some(gap.reason.clone()))
        })
        .collect()
}

/// A painted span, and the gap reason when it marks one.
type Mark = ((f32, f32), Option<String>);

/// What covers the lane's left and right edges: `None` when nothing does,
/// else the reason of a gap there, or no reason for depth.
fn edges(marks: &[Mark], width: f32) -> (Option<Option<String>>, Option<Option<String>>) {
    let at = |touches: &dyn Fn(f32, f32) -> bool| {
        let mut covering = marks
            .iter()
            .filter(|((x0, x1), _)| x1 > x0 && touches(*x0, *x1));
        // Depth first: a gap behind a band does not hide it.
        covering
            .clone()
            .find(|(_, reason)| reason.is_none())
            .or_else(|| covering.next())
            .map(|(_, reason)| reason.clone())
    };
    (at(&|x0, _| x0 <= 1e-3), at(&|_, x1| x1 >= width - 1e-3))
}

fn book(spans: Vec<(f32, f32)>) -> Vec<Mark> {
    spans.into_iter().map(|span| (span, None)).collect()
}

#[test]
fn the_live_tape_covers_both_edges_at_every_size() {
    for (max_radius, gain) in SIZES {
        for width in WIDTHS {
            let case = format!("radius {max_radius} x{gain}, {width} px");
            let config = sized(max_radius, gain);
            let geometry = TapeHorizontalGeometry::from_config(&config, width, LANE_HEIGHT_PX)
                .expect("an averaged native tape");
            let history = history_from(RECORDED_FROM_MS, config.clone());
            let (lead_ms, capped) = lead_at(&config, width);
            let timeline = timeline(lead_ms).with_lane_lead_capped(capped);
            let regions = timeline.region_count();
            let (cells, _, source) =
                project_lane_heat(&history, &timeline, prices(), grouping(), 130.into());
            let gaps = lead_gaps(&history, &timeline);
            let mut live = book(painted(geometry, &cells, regions, true));
            live.extend(gap_px(geometry, &gaps, regions));
            let (left, right) = edges(&live, width);
            assert_eq!(right, Some(None), "{case}: the live book reaches the edge");
            match &left {
                Some(None) => {}
                Some(Some(reason)) => assert_eq!(reason.as_str(), BOOK_BEYOND_LEAD, "{case}"),
                None => panic!("{case}: a dark strip left of the oldest dot"),
            }
            if width >= 310.0 {
                assert_eq!(left, Some(None), "{case}: the lead reads depth there");
            }

            // The same frame drawn once its book is no longer current.
            let mut projection = HeatmapProjection::empty(true, grouping());
            projection.current_book = source;
            let stale = stale_book_gap(&projection, false);
            assert!(stale_book_gap(&projection, true).is_none());
            let mut held_back = book(painted(geometry, &cells, regions, false));
            held_back.extend(gap_px(geometry, &gaps, regions));
            held_back.extend(gap_px(geometry, stale.as_slice(), regions));
            let (_, right) = edges(&held_back, width);
            assert_eq!(
                right,
                Some(Some(BOOK_NOT_CURRENT.to_owned())),
                "{case}: no stale book is carried past NOW, and the inset says so"
            );
        }
    }
}

#[test]
fn a_held_tape_covers_both_edges_at_every_size() {
    for (max_radius, gain) in SIZES {
        for width in WIDTHS {
            // Deep in the recorded book, and at the live edge, where the
            // trail runs past the newest recorded book.
            for end_ms in [HELD_DEEP_MS, NOW_MS] {
                let case = format!("radius {max_radius} x{gain}, {width} px, held at {end_ms}");
                let config = sized(max_radius, gain);
                let geometry =
                    TapeHorizontalGeometry::from_config(&config, width, LANE_HEIGHT_PX).unwrap();
                let history = history_from(RECORDED_FROM_MS, config.clone());
                let (lead_ms, capped) = lead_at(&config, width);
                let heat = project_past_heat(
                    &history,
                    (end_ms - WINDOW_MS, end_ms),
                    (lead_ms, capped),
                    prices(),
                    grouping(),
                );
                let placed = heat.placed(end_ms, WINDOW_MS, 1);
                let mut marks = book(painted(geometry, &placed.cells, 1, false));
                marks.extend(gap_px(geometry, &placed.gaps, 1));
                let (left, right) = edges(&marks, width);
                assert!(
                    left.is_some(),
                    "{case}: a dark strip left of the oldest dot"
                );
                if width >= 310.0 {
                    assert_eq!(left, Some(None), "{case}: the lead reads depth there");
                }
                let recorded = end_ms + lead_ms <= LATEST_BOOK_MS;
                match right {
                    Some(None) => {}
                    Some(Some(reason)) => {
                        assert!(
                            !recorded || reason == BOOK_BEYOND_LEAD,
                            "{case}: the trail reads depth there"
                        );
                        assert!(
                            [BOOK_NOT_CURRENT, BOOK_BEYOND_LEAD].contains(&reason.as_str()),
                            "{case}: {reason}"
                        );
                    }
                    None => panic!("{case}: a dark strip right of NOW"),
                }
                if end_ms == NOW_MS && !capped {
                    assert!(!recorded, "{case}: the trail runs past the book");
                }
                // Nothing placed in an inset is a stretched oldest column: it
                // is on the dots' clock, beyond the window on its side.
                for cell in placed.cells.iter() {
                    if let Some((from, to)) = cell.lead_span {
                        assert!(to <= 0.0 || from >= 1.0, "{case}: {cell:?}");
                    }
                }
            }
        }
    }
}

/// The held book's own bands do not move when the insets read beside them:
/// not their place, not their colour, and not under a tight cell cap.
#[test]
fn the_held_lead_never_moves_the_windows_own_bands() {
    for cap in [1_000, 2] {
        let mut config = sized(MAX_BUBBLE_MAX_RADIUS, MAX_BUBBLE_DISPLAY_SCALE);
        config.max_visible_cells = cap;
        let history = history_from(RECORDED_FROM_MS, config.clone());
        let window = (LANE_START_MS - WINDOW_MS, LANE_START_MS);
        let read = |lead| {
            project_past_heat(&history, window, lead, prices(), grouping()).placed(
                LANE_START_MS,
                WINDOW_MS,
                1,
            )
        };
        let inside = |projection: HeatmapProjection| {
            projection
                .cells
                .iter()
                .filter(|cell| cell.lead_span.is_none())
                .map(|cell| {
                    let at = (cell.x0, cell.x1, cell.intensity);
                    (cell.price_bucket, cell.quantity, at)
                })
                .collect::<Vec<_>>()
        };
        let lead = lead_at(&config, 310.0);
        assert!(lead.0 > 0);
        let padded = read(lead);
        assert!(padded.cells.len() <= 2 * cap, "a cap for each");
        // A cap the insets outgrow leaves no dark strip: past it is a gap.
        let width = 310.0;
        let geometry = TapeHorizontalGeometry::from_config(&config, width, LANE_HEIGHT_PX).unwrap();
        let mut marks = book(painted(geometry, &padded.cells, 1, false));
        marks.extend(gap_px(geometry, &padded.gaps, 1));
        let (left, right) = edges(&marks, width);
        assert!(
            left.is_some() && right.is_some(),
            "cap {cap}: {left:?} {right:?}"
        );
        if cap == 2 {
            assert!(
                padded.gaps.iter().any(|gap| gap.reason == BOOK_BEYOND_LEAD),
                "the insets past the cap are labelled"
            );
        }
        assert_eq!(inside(padded), inside(read((0, false))), "cap {cap}");
    }
}

/// A held lead reaching before the retained book marks that stretch as
/// before capture, as the live lead does.
#[test]
fn a_held_lead_before_the_retained_book_is_marked() {
    let mut config = sized(MAX_BUBBLE_MAX_RADIUS, MAX_BUBBLE_DISPLAY_SCALE);
    config.retention_ms = 1_000;
    let mut history = history_from(0, config.clone());
    history
        .apply_delta(
            2_450,
            &quantick_orderbook::BookDelta::new(13, 13, vec![level(99, 50)], vec![]),
        )
        .unwrap();
    assert_eq!(history.retention_start_ms(), Some(1_450));
    // Read to the pane's edge, and stopped at the cap short of it: either
    // way the never-recorded stretch reads before capture, never unread.
    for width in [400.0, 290.0] {
        let geometry = TapeHorizontalGeometry::from_config(&config, width, LANE_HEIGHT_PX).unwrap();
        let end_ms = 1_900;
        let lead = lead_at(&config, width);
        assert_eq!(lead.1, width < 300.0, "{width} px");
        let placed = project_past_heat(
            &history,
            (end_ms - WINDOW_MS, end_ms),
            lead,
            prices(),
            grouping(),
        )
        .placed(end_ms, WINDOW_MS, 1);
        let mut marks = book(painted(geometry, &placed.cells, 1, false));
        marks.extend(gap_px(geometry, &placed.gaps, 1));
        let (left, _) = edges(&marks, width);
        assert_eq!(left, Some(Some(BEFORE_CAPTURE.to_owned())), "{width} px");
        assert!(
            placed
                .gaps
                .iter()
                .all(|gap| gap.reason != BOOK_BEYOND_LEAD || gap.lead_span.unwrap().0 > 0.0),
            "{width} px: nothing before capture reads as unread"
        );
    }
}
