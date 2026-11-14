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

/// A magnetic buy/sell mark hangs from the forming bar's own extreme — the
/// live use of the tool — and without the magnet keeps the pointer's price.
#[test]
fn a_magnetic_mark_on_the_forming_bar_hangs_from_its_extreme() {
    with_prints(&[50, 60, 40], |projection, bands| {
        assert_eq!(projection.series.closed_slots(), 0, "one forming bar");
        let x = projection.viewport.x_at_bar_position(0.0, 400.0, 1);
        let at = pos2(x, bands[0].scale.as_ref().unwrap().y(45.0));
        let commit = |tool: &str, magnet: bool| {
            let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
            gestures.update_placement(
                &mut store,
                projection,
                frame(bands, at, true, false),
                PlacementOptions {
                    tool: DrawingTool::by_id(tool),
                    magnet,
                    constrain: Constrain::Free,
                    parked_position: None,
                },
                PlacementDefaults {
                    presets: &NullPresetHost,
                    repeat: false,
                },
            );
            let [mark] = store.items() else {
                panic!("{tool} commits on the press");
            };
            mark.points[0].price
        };
        assert_eq!(commit("arrow-mark-up", true), 40.0, "the forming low");
        assert_eq!(commit("arrow-mark-down", true), 60.0, "the forming high");
        for tool in ["arrow-mark-up", "arrow-mark-down"] {
            let free = commit(tool, false);
            assert!((free - 45.0).abs() < 0.01, "{tool} free stays at {free}");
        }
    });
}

/// A held stroke takes its last allowed point and refuses the next.
#[test]
fn a_freehand_stroke_stops_at_its_point_cap() {
    with_projection(|projection, bands| {
        let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
        let at = |gestures: &mut PaneGestures, store: &mut Marks, x: f32, released: bool| {
            let pressed = x == 10.0;
            let frame = frame(bands, pos2(x, 100.0), pressed, released);
            update(gestures, store, projection, frame, "brush");
        };
        at(&mut gestures, &mut store, 10.0, false);
        let origin = store.draft().unwrap().points[0];
        let brush = DrawingTool::by_id("brush").unwrap();
        for index in 1..FREEHAND_MAX_POINTS - 1 {
            let bar = origin.bar + index as f32 * 0.01;
            assert!(!store.place(brush, ChartPoint { bar, ..origin }));
        }
        for x in [14.0, 18.0] {
            at(&mut gestures, &mut store, x, false);
            assert_eq!(store.draft_len(), FREEHAND_MAX_POINTS, "at {x}");
        }
        at(&mut gestures, &mut store, 18.0, true);
        assert_eq!(store.items()[0].points.len(), FREEHAND_MAX_POINTS);
    });
}

/// A pencil press released where it began is a click that missed: it leaves
/// neither an object nor a draft behind.
#[test]
fn a_freehand_click_that_never_moved_leaves_nothing() {
    with_projection(|projection, bands| {
        let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
        let at = pos2(100.0, 100.0);
        for (pressed, released) in [(true, false), (false, true)] {
            let frame = frame(bands, at, pressed, released);
            update(&mut gestures, &mut store, projection, frame, "brush");
        }
        assert!(store.items().is_empty());
        assert!(store.draft().is_none());
    });
}

/// A stroke belongs to the band its first point landed in: a hand that runs
/// off it is held to the band's edge, never read in another band's units.
#[test]
fn a_freehand_stroke_stays_on_the_band_it_started_in() {
    with_projection(|projection, bands| {
        let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
        let path = [
            (100.0, 100.0),
            (130.0, 600.0),
            (160.0, 600.0),
            (190.0, 600.0),
        ];
        for (index, (x, y)) in path.into_iter().enumerate() {
            let frame = frame(bands, pos2(x, y), index == 0, false);
            update(&mut gestures, &mut store, projection, frame, "brush");
        }
        let release = frame(bands, pos2(220.0, 110.0), false, true);
        update(&mut gestures, &mut store, projection, release, "brush");
        let prices: Vec<f64> = store.items()[0].points.iter().map(|p| p.price).collect();
        assert_eq!(prices.len(), 4, "{prices:?}");
        assert!(
            prices[1..].iter().all(|price| price.abs() < 1e-6),
            "{prices:?}"
        );
    });
}

/// Shift lays a dragged ruler level to the bit: the far end copies the near
/// price rather than reading it back off the screen, where the magnet would
/// pull it onto the print under it.
#[test]
fn a_levelled_ruler_copies_its_near_price_under_the_magnet() {
    let tape: Vec<i64> = [50; 20].into_iter().chain([46; 10]).collect();
    with_prints(&tape, |projection, bands| {
        let (mut gestures, mut store) = (PaneGestures::default(), Marks::default());
        let total = projection.series.slots();
        let x = |bar: f32| projection.viewport.x_at_bar_position(bar, 400.0, total);
        let y = bands[0].scale.as_ref().unwrap().y(50.0);
        for (at, pressed) in [(pos2(x(0.5), y), true), (pos2(x(2.5), y - 60.0), false)] {
            gestures.update_placement(
                &mut store,
                projection,
                frame(bands, at, pressed, !pressed),
                PlacementOptions {
                    tool: DrawingTool::by_id("measure"),
                    magnet: true,
                    constrain: Constrain::Level,
                    parked_position: None,
                },
                PlacementDefaults {
                    presets: &NullPresetHost,
                    repeat: false,
                },
            );
        }
        let points = &store.items()[0].points;
        assert_eq!(points[0].price, points[1].price, "{points:?}");
        assert!(points[1].bar > points[0].bar, "{points:?}");
    });
}
