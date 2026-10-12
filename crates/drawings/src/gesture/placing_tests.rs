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
    let spec = SpecSelector::new(BarSpec::Tick(10));
    let mut state = ChartState::new(BarSpec::Tick(10));
    for agg_id in 0..10 {
        state.ingest_live(&quantick_engine::Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64 * 100,
            price: 50.into(),
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
