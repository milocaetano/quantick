//! The placement state machine on plain frames: the cursor and refusal it
//! asks for, the 12 px release that completes a dragged-out object, and the
//! freehand stroke's 4 px step.

use super::*;
use crate::chart::PriceScale;
use crate::gesture::{Bands, PaneSeriesRead};
use crate::{NullPresetHost, pos2};
use quantick_chart::indicator_views::IndicatorViews;
use quantick_chart::state::{BarSpec, ChartState, SpecSelector};
use quantick_chart_interaction::viewport::Viewport;

fn with_projection(test: impl FnOnce(&DrawingProjection<'_>, &Bands)) {
    with_prints(&[50; 10], test);
}

/// [`with_projection`] over a tick:10 tape of one print at each of `prices`.
fn with_prints(prices: &[i64], test: impl FnOnce(&DrawingProjection<'_>, &Bands)) {
    let spec = SpecSelector::new(BarSpec::Tick(10));
    let mut state = ChartState::new(BarSpec::Tick(10));
    for (agg_id, price) in (0..).zip(prices) {
        state.ingest_live(&quantick_engine::Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64 * 100,
            price: (*price).into(),
            quantity: 1.into(),
            side: quantick_engine::Side::Buy,
        });
    }
    let viewport = Viewport::new();
    let indicators = IndicatorViews::new();
    let projection = DrawingProjection {
        series: PaneSeriesRead {
            history_prefix: &[],
            state: &state,
            spec: &spec,
        },
        viewport: &viewport,
        indicators: &indicators,
    };
    let bands = smallvec::smallvec![
        Band {
            key: DrawingBand::Price,
            rect: PxRect::from_min_max(Px::default(), pos2(400.0, 200.0)),
            scale: Some(PriceScale::from_range(0.0, 100.0, 0.0, 200.0)),
            label: "price".into(),
            refusal: None,
        },
        Band {
            key: DrawingBand::Price,
            rect: PxRect::from_min_max(pos2(0.0, 200.0), pos2(400.0, 260.0)),
            scale: None,
            label: "warming".into(),
            refusal: Some("still warming up"),
        },
    ];
    test(&projection, &bands);
}

/// The pointer as one frame reads it: hovering `at`, optionally pressing or
/// releasing there.
fn frame<'a>(bands: &'a Bands, at: Px, pressed: bool, released: bool) -> PlacementFrame<'a> {
    PlacementFrame {
        bands,
        surface: placement_surface(bands),
        history_right: 400.0,
        hover: Some(at),
        surface_active: true,
        latest: Some(at),
        pressed: pressed.then_some(at),
        released: released.then_some(at),
        down: pressed || !released,
        over_chrome: &|_| false,
        on_pane_chrome: &|_| false,
    }
}

fn update(
    gestures: &mut PaneGestures,
    store: &mut Marks,
    projection: &DrawingProjection<'_>,
    frame: PlacementFrame<'_>,
    tool: &str,
) -> PlacementOutcome {
    gestures.update_placement(
        store,
        projection,
        frame,
        PlacementOptions {
            tool: DrawingTool::by_id(tool),
            magnet: false,
            constrain: Constrain::Free,
            parked_position: None,
        },
        PlacementDefaults {
            presets: &NullPresetHost,
            repeat: false,
        },
    )
}

#[test]
fn an_armed_tool_asks_for_a_crosshair_and_a_refusing_band_says_why() {
    with_projection(|projection, bands| {
        let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
        let drawable = update(
            &mut gestures,
            &mut store,
            projection,
            frame(bands, pos2(100.0, 100.0), false, false),
            "trend-line",
        );
        assert_eq!(drawable.cursor, Some(GestureCursor::Crosshair));
        assert_eq!(drawable.refusal, None);
        assert_eq!(gestures.band_hint, Some(bands[0].rect));
        let refusing = update(
            &mut gestures,
            &mut store,
            projection,
            frame(bands, pos2(100.0, 230.0), false, false),
            "trend-line",
        );
        assert_eq!(refusing.cursor, Some(GestureCursor::NotAllowed));
        assert_eq!(refusing.refusal, Some("still warming up"));
        let mut idle = frame(bands, pos2(100.0, 100.0), false, false);
        idle.surface_active = false;
        let idle = update(&mut gestures, &mut store, projection, idle, "trend-line");
        assert_eq!(idle.cursor, None, "an unhovered surface sets no cursor");
    });
}

#[test]
fn a_release_completes_a_dragged_object_only_from_twelve_pixels() {
    with_projection(|projection, bands| {
        for (travel, completes) in [(DRAWING_DRAG_COMPLETES_PX - 0.5, false), (12.0, true)] {
            let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
            let start = pos2(100.0, 100.0);
            update(
                &mut gestures,
                &mut store,
                projection,
                frame(bands, start, true, false),
                "trend-line",
            );
            assert_eq!(store.draft_len(), 1);
            let end = pos2(start.x + travel, start.y);
            let release = update(
                &mut gestures,
                &mut store,
                projection,
                frame(bands, end, false, true),
                "trend-line",
            );
            assert_eq!(
                store.items().len(),
                usize::from(completes),
                "travel {travel}"
            );
            assert_eq!(release.completion.arm_pointer, completes, "travel {travel}");
            assert_eq!(gestures.press_position, None);
        }
    });
}

#[test]
fn a_freehand_stroke_records_a_point_only_every_four_pixels() {
    with_projection(|projection, bands| {
        let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
        let start = pos2(100.0, 100.0);
        update(
            &mut gestures,
            &mut store,
            projection,
            frame(bands, start, true, false),
            "brush",
        );
        assert_eq!(store.draft_len(), 1);
        let near = pos2(start.x + FREEHAND_MIN_STEP_PX - 0.5, start.y);
        update(
            &mut gestures,
            &mut store,
            projection,
            frame(bands, near, false, false),
            "brush",
        );
        assert_eq!(store.draft_len(), 1, "under the step records nothing");
        let far = pos2(start.x + FREEHAND_MIN_STEP_PX, start.y);
        update(
            &mut gestures,
            &mut store,
            projection,
            frame(bands, far, false, false),
            "brush",
        );
        assert_eq!(store.draft_len(), 2);
    });
}

/// The magnet reads the forming bar: a trend line armed over the bar still
/// printing previews its low, and the press commits that same snapped anchor.
#[test]
fn the_magnet_previews_and_commits_a_snap_on_the_forming_bar() {
    with_prints(&[50, 60, 40], |projection, bands| {
        assert_eq!(projection.series.closed_slots(), 0, "one forming bar");
        let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
        let x = projection.viewport.x_at_bar_position(0.0, 400.0, 1);
        let at = pos2(x, bands[0].scale.as_ref().unwrap().y(41.0));
        let mut magnet = |gestures: &mut PaneGestures, pressed: bool| {
            gestures.update_placement(
                &mut store,
                projection,
                frame(bands, at, pressed, false),
                PlacementOptions {
                    tool: DrawingTool::by_id("trend-line"),
                    magnet: true,
                    constrain: Constrain::Free,
                    parked_position: None,
                },
                PlacementDefaults {
                    presets: &NullPresetHost,
                    repeat: false,
                },
            );
        };
        magnet(&mut gestures, false);
        assert_eq!(
            gestures.hover.map(|point| point.price),
            Some(40.0),
            "the preview"
        );
        magnet(&mut gestures, true);
        let draft = store.draft().expect("the press placed the first anchor");
        assert_eq!(draft.points[0].price, 40.0, "the commit");
    });
}
