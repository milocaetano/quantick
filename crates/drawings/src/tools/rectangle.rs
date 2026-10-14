use crate::{MarkScene, pos2, Px, PxRect};
use std::any::Any;

use crate::icons;
use serde::{Deserialize, Serialize};

use super::shape_core::SHAPES_FAMILY;
use super::{Constrain, DrawContext, DrawingPayload, DrawingStyle, MarkTool, Handles, PointerHint, ToolFamily, ToolShortcut, drawing_fill, drawing_stroke};

/// Registry id, named like `frvp::TOOL_ID` for the callers that gate on
/// this one shape (the strategy seat does: two anchors honestly bound a
/// price region).
pub const TOOL_ID: &str = "rectangle";

/// On-disk preset shape. Only the tool-owned config travels; coordinates
/// never do.
const PRESET_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RectanglePresetData {
    version: u32,
    #[serde(default)]
    extend_right: bool,
    /// Same vintage rule as `extend_right`: a preset or saved layout written
    /// before the left extension existed loads with it off.
    #[serde(default)]
    extend_left: bool,
}

/// The rectangle's own state beyond anchors and style.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RectanglePayload {
    /// Keep the band running to the chart's right edge, whatever the second
    /// anchor says — the "this zone holds until further notice" reading. The
    /// anchors are untouched, so switching it off restores exactly the span
    /// that was drawn. An armed strategy reads this too: with it on, the
    /// region never expires off the right anchor.
    pub extend_right: bool,
    /// The mirror of `extend_right`: run the band back to the chart's left
    /// edge. An armed strategy reads it too: the region is then active
    /// before its left anchor as well.
    pub extend_left: bool,
}

/// About half a centimetre on a typical desktop display: how far inside the
/// left or right side the band reaches whose click extends the rectangle
/// that way.
pub(crate) const EDGE_ZONE_PX: f32 = 18.0;
/// Half the width of the strip around a side line that grabs that side: the
/// line itself, thin enough to leave the band inside it room.
const BORDER_PX: f32 = 4.0;
/// How near a corner the corner's own resize handle takes the pointer - the
/// reach the pane gives a handle - so no side zone reaches a corner.
const CORNER_PX: f32 = crate::gesture::DRAWING_ANCHOR_RADIUS_PX;
/// The side handles' indices follow the four corner handles: the side line
/// of the first anchor, then the second's.
const FIRST_SIDE_HANDLE: usize = 4;
const SECOND_SIDE_HANDLE: usize = FIRST_SIDE_HANDLE + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// What the pointer is on, as the trader sketched it. Cursor, hit-test and
/// click all read this one answer, so each zone acts exactly where its
/// cursor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    /// On a side line, not a corner: a drag moves that side sideways.
    Border(Side),
    /// The band just inside a side: one click runs that side to the chart
    /// edge.
    Extend(Side),
    /// Anywhere else inside: a double click puts back the original size.
    Inside,
}

/// The zone under `position`, or `None` off the rectangle and on its
/// corners. A side already run to the chart edge has no line to drag and
/// nothing left to extend: all of it is inside. A narrow rectangle keeps an
/// inside: each band takes at most a third of the drawn width.
fn zone_at(
    points: &[Px],
    chart_rect: PxRect,
    payload: &RectanglePayload,
    position: Px,
) -> Option<Zone> {
    let [first, second] = points else {
        return None;
    };
    let drawn = PxRect::from_two_pos(*first, *second);
    let on_corner = [
        drawn.left_top(),
        drawn.right_top(),
        drawn.right_bottom(),
        drawn.left_bottom(),
    ]
    .iter()
    .any(|corner| corner.distance_sq(position) <= CORNER_PX * CORNER_PX);
    if on_corner || position.y < drawn.top() || position.y > drawn.bottom() {
        return None;
    }
    let band = EDGE_ZONE_PX.min(drawn.width() / 3.0);
    for (side, edge, inward, extended) in [
        (Side::Left, drawn.left(), 1.0, payload.extend_left),
        (Side::Right, drawn.right(), -1.0, payload.extend_right),
    ] {
        if extended {
            continue;
        }
        let depth = (position.x - edge) * inward;
        if depth.abs() <= BORDER_PX {
            return Some(Zone::Border(side));
        }
        if depth > BORDER_PX && depth <= band {
            return Some(Zone::Extend(side));
        }
    }
    screen_rect(points, chart_rect, payload)
        .contains(position)
        .then_some(Zone::Inside)
}

impl RectanglePayload {
    /// One click: a band runs its side to the chart edge. Answers whether
    /// anything changed.
    fn apply_click(&mut self, zone: Zone) -> bool {
        match zone {
            Zone::Extend(Side::Left) => self.extend_left = true,
            Zone::Extend(Side::Right) => self.extend_right = true,
            Zone::Border(_) | Zone::Inside => return false,
        }
        true
    }

    /// A double click inside, the bands included: back to the original
    /// size. Answers whether anything changed - a rectangle at its original
    /// size leaves the double click to the chart.
    fn apply_double_click(&mut self, zone: Zone) -> bool {
        let extended = self.extend_left || self.extend_right;
        if matches!(zone, Zone::Border(_)) || !extended {
            return false;
        }
        (self.extend_left, self.extend_right) = (false, false);
        true
    }
}

impl DrawingPayload for RectanglePayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(self.clone())
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self == other)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn export_preset(&self) -> Option<toml::Value> {
        toml::Value::try_from(RectanglePresetData {
            version: PRESET_FORMAT_VERSION,
            extend_right: self.extend_right,
            extend_left: self.extend_left,
        })
        .ok()
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = RectanglePresetData::deserialize(value.clone()) else {
            return false;
        };
        if data.version != PRESET_FORMAT_VERSION {
            return false;
        }
        self.extend_right = data.extend_right;
        self.extend_left = data.extend_left;
        true
    }
}

/// The rectangle's screen span: its two anchor corners, run to the chart's
/// left and right edges when the payload extends it. Paint, hit-test and
/// the double click share this so the clickable area is exactly the painted
/// one.
fn screen_rect(points: &[Px], chart_rect: PxRect, payload: &RectanglePayload) -> PxRect {
    let mut rect = PxRect::from_two_pos(points[0], points[1]);
    if payload.extend_right {
        rect.max.x = rect.max.x.max(chart_rect.right());
    }
    if payload.extend_left {
        rect.min.x = rect.min.x.min(chart_rect.left());
    }
    rect
}

fn payload_of<'a>(ctxt: &'a DrawContext<'_>) -> &'a RectanglePayload {
    ctxt.payload
        .as_any()
        .downcast_ref::<RectanglePayload>()
        .expect("a rectangle always carries a rectangle payload")
}

/// Four stable grab points built from the two diagonal anchors.
///
/// Handle identity follows anchor components instead of sorted screen sides.
/// That distinction matters while a drag crosses the opposite corner: the
/// active handle must keep following the pointer after left becomes right or
/// top becomes bottom on the next frame.
fn rectangle_handles(points: &[Px]) -> Option<Handles> {
    let [first, second] = points else {
        return None;
    };
    Some(Handles::from_slice(&[
        *first,
        pos2(second.x, first.y),
        *second,
        pos2(first.x, second.y),
    ]))
}

/// Move the two anchor components owned by one corner while the diagonally
/// opposite corner stays fixed; a side handle moves one anchor's x.
fn resize_from_corner(
    points: &[Px],
    handle: usize,
    to: Px,
) -> Option<Handles> {
    let [first, second] = points else {
        return None;
    };
    let (first, second) = match handle {
        0 => (to, *second),
        1 => (pos2(first.x, to.y), pos2(to.x, second.y)),
        2 => (*first, to),
        3 => (pos2(to.x, first.y), pos2(second.x, to.y)),
        // A side line moves its own anchor's x alone.
        FIRST_SIDE_HANDLE => (pos2(to.x, first.y), *second),
        SECOND_SIDE_HANDLE => (*first, pos2(to.x, second.y)),
        _ => return None,
    };
    Some(Handles::from_slice(&[first, second]))
}

/// The handle of the side line on `side`. Handles follow anchors, not
/// screen sides, so a drag that carries one side past the other keeps hold
/// of it.
fn side_handle(points: &[Px], side: Side) -> usize {
    let first_is_left = points[0].x <= points[1].x;
    if (side == Side::Left) == first_is_left {
        FIRST_SIDE_HANDLE
    } else {
        SECOND_SIDE_HANDLE
    }
}

pub(super) static TOOL: Rectangle = Rectangle;

pub(super) struct Rectangle;

impl MarkTool for Rectangle {
    fn id(&self) -> &'static str {
        TOOL_ID
    }
    fn name(&self) -> &'static str {
        "Rectangle"
    }
    fn settings_title(&self) -> &'static str {
        "Rectangle settings"
    }
    fn icon(&self) -> &'static str {
        icons::RECTANGLE
    }
    fn hover_text(&self) -> &'static str {
        "Rectangle - click two corners or drag (R)"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: 'R',
            shift: false,
        })
    }
    fn levels_span_the_body(&self) -> bool {
        true
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(SHAPES_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(RectanglePayload::default())
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("Region")
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        if points.len() == 2 {
            let rect = screen_rect(points, chart_rect, payload_of(ctxt));
            painter.rect_filled(rect, 0.0, drawing_fill(style));
            painter.rect_stroke(
                rect,
                0.0,
                drawing_stroke(style),
            );
        }
    }
    fn hit_test(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        if points.len() != 2 {
            return false;
        }
        let rect = screen_rect(points, chart_rect, payload_of(ctxt));
        if !rect.expand(radius_px).contains(position) {
            return false;
        }
        // The interior only takes part in the hit-test while the fill is
        // visible; an outline-only rectangle is selectable by its border,
        // and clicks through its middle keep belonging to the chart.
        ctxt.style.fill_alpha > 0 || !rect.shrink(radius_px).contains(position)
    }
    fn handles(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        _ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        rectangle_handles(points)
    }
    fn drag_handle(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        handle: usize,
        to: Px,
        _ctxt: &DrawContext<'_>,
        // A rectangle corner changes two independent edges; it has no line
        // angle for the level constraint to preserve.
        _constrain: Constrain,
    ) -> Option<Handles> {
        resize_from_corner(points, handle, to)
    }
    fn handle_hint(&self, handle: usize) -> Option<PointerHint> {
        matches!(handle, FIRST_SIDE_HANDLE | SECOND_SIDE_HANDLE)
            .then_some(PointerHint::ResizeSide { handle })
    }
    fn pointer_hint(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        ctxt: &DrawContext<'_>,
    ) -> Option<PointerHint> {
        match zone_at(points, chart_rect, payload_of(ctxt), position)? {
            Zone::Border(side) => Some(PointerHint::ResizeSide {
                handle: side_handle(points, side),
            }),
            Zone::Extend(_) => Some(PointerHint::ExtendSide),
            // The inside is where the trader grabs the band to move it.
            Zone::Inside => None,
        }
    }
    fn click(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        payload: &mut dyn DrawingPayload,
    ) -> bool {
        let Some(payload) = payload.as_any_mut().downcast_mut::<RectanglePayload>() else {
            return false;
        };
        zone_at(points, chart_rect, payload, position).is_some_and(|zone| payload.apply_click(zone))
    }
    fn double_click(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        payload: &mut dyn DrawingPayload,
    ) -> bool {
        let Some(payload) = payload.as_any_mut().downcast_mut::<RectanglePayload>() else {
            return false;
        };
        zone_at(points, chart_rect, payload, position)
            .is_some_and(|zone| payload.apply_double_click(zone))
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 100.0), pos2(200.0, 200.0)],
            pos2(150.0, 150.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_prelude::*;
    use crate::chart::PriceScale;
    use crate::{ChartPoint, Constrain, DrawingTool, ValueUnit};

    fn context<'a>(
        payload: &'a RectanglePayload,
        anchors: &'a [ChartPoint],
        scale: &'a PriceScale,
    ) -> DrawContext<'a> {
        DrawContext {
            payload,
            anchors,
            scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: true,
            halo: false,
            content_editing: false,
            text: &crate::UNMEASURED,
        }
    }

    #[test]
    fn a_rectangle_exposes_all_four_corners_as_handles() {
        let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(500.0, 300.0));
        let points = [pos2(300.0, 220.0), pos2(100.0, 80.0)];
        let anchors = [ChartPoint::at(30.0, 90.0), ChartPoint::at(10.0, 110.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        let payload = RectanglePayload::default();
        let ctxt = context(&payload, &anchors, &scale);

        assert_eq!(
            TOOL.handles(chart, &points, &ctxt)
                .expect("the rectangle owns its four handles")
                .as_slice(),
            &[
                pos2(300.0, 220.0),
                pos2(100.0, 220.0),
                pos2(100.0, 80.0),
                pos2(300.0, 80.0),
            ]
        );
    }

    #[test]
    fn every_corner_expands_and_contracts_around_its_opposite_corner() {
        let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(500.0, 300.0));
        let points = [pos2(100.0, 80.0), pos2(300.0, 220.0)];
        let anchors = [ChartPoint::at(10.0, 110.0), ChartPoint::at(30.0, 90.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        let payload = RectanglePayload::default();
        let ctxt = context(&payload, &anchors, &scale);
        let opposite = [
            pos2(300.0, 220.0),
            pos2(100.0, 220.0),
            pos2(100.0, 80.0),
            pos2(300.0, 80.0),
        ];
        let outward = [
            pos2(60.0, 40.0),
            pos2(340.0, 40.0),
            pos2(340.0, 260.0),
            pos2(60.0, 260.0),
        ];
        let inward = [
            pos2(140.0, 120.0),
            pos2(260.0, 120.0),
            pos2(260.0, 180.0),
            pos2(140.0, 180.0),
        ];

        for handle in 0..4 {
            for target in [outward[handle], inward[handle]] {
                let moved = TOOL
                    .drag_handle(
                        chart,
                        &points,
                        handle,
                        target,
                        &ctxt,
                        Constrain::Free,
                    )
                    .expect("the rectangle owns every corner drag");
                let resized = PxRect::from_two_pos(moved[0], moved[1]);
                assert!(resized.contains(target), "handle {handle} follows the pointer");
                assert!(
                    resized.contains(opposite[handle]),
                    "handle {handle} preserves its opposite corner"
                );
            }
        }
    }

    #[test]
    fn a_corner_may_cross_its_opposite_without_losing_resize_control() {
        let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(500.0, 300.0));
        let points = [pos2(100.0, 80.0), pos2(300.0, 220.0)];
        let anchors = [ChartPoint::at(10.0, 110.0), ChartPoint::at(30.0, 90.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        let payload = RectanglePayload::default();
        let ctxt = context(&payload, &anchors, &scale);
        let target = pos2(340.0, 250.0);

        let moved = TOOL
            .drag_handle(chart, &points, 0, target, &ctxt, Constrain::Free)
            .expect("crossing remains a rectangle resize");
        assert_eq!(
            PxRect::from_two_pos(moved[0], moved[1]),
            PxRect::from_two_pos(target, pos2(300.0, 220.0))
        );
    }

    const CHART: PxRect = PxRect {
        min: pos2(0.0, 0.0),
        max: pos2(500.0, 300.0),
    };
    const BAND: [Px; 2] = [pos2(100.0, 80.0), pos2(300.0, 220.0)];
    const MID_Y: f32 = 150.0;

    fn click_at(payload: &mut RectanglePayload, x: f32) -> bool {
        TOOL.click(CHART, &BAND, pos2(x, MID_Y), payload)
    }

    fn double_click_at(payload: &mut RectanglePayload, x: f32) -> bool {
        TOOL.double_click(CHART, &BAND, pos2(x, MID_Y), payload)
    }

    fn with_context<T>(payload: &RectanglePayload, read: impl FnOnce(&DrawContext<'_>) -> T) -> T {
        let anchors = [ChartPoint::at(10.0, 110.0), ChartPoint::at(30.0, 90.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        read(&context(payload, &anchors, &scale))
    }

    fn hint_at(payload: &RectanglePayload, position: Px) -> Option<PointerHint> {
        with_context(payload, |ctxt| TOOL.pointer_hint(CHART, &BAND, position, ctxt))
    }

    fn point_handle_at(payload: &RectanglePayload, position: Px) -> Option<usize> {
        with_context(payload, |ctxt| {
            DrawingTool(&TOOL).hit_handle(CHART, &BAND, position, 12.0, ctxt)
        })
    }

    fn side_handle_at(payload: &RectanglePayload, position: Px) -> Option<usize> {
        match hint_at(payload, position)? {
            PointerHint::ResizeSide { handle } => Some(handle),
            PointerHint::ExtendSide => None,
        }
    }

    fn extent(payload: &RectanglePayload) -> (bool, bool) {
        (payload.extend_left, payload.extend_right)
    }

    fn right_only() -> RectanglePayload {
        RectanglePayload {
            extend_right: true,
            ..RectanglePayload::default()
        }
    }

    /// The sketch, left to right: the border line, the band just inside it,
    /// the inside, the band just inside the right end, the right border line.
    #[test]
    fn each_zone_sits_where_the_sketch_puts_it() {
        let payload = RectanglePayload::default();
        let at = |x: f32| zone_at(&BAND, CHART, &payload, pos2(x, MID_Y));
        assert_eq!(at(100.0 - BORDER_PX - 1.0), None, "outside the rectangle");
        assert_eq!(at(100.0 - BORDER_PX), Some(Zone::Border(Side::Left)));
        assert_eq!(at(100.0), Some(Zone::Border(Side::Left)));
        assert_eq!(at(100.0 + BORDER_PX), Some(Zone::Border(Side::Left)));
        assert_eq!(at(100.0 + BORDER_PX + 1.0), Some(Zone::Extend(Side::Left)));
        assert_eq!(at(100.0 + EDGE_ZONE_PX), Some(Zone::Extend(Side::Left)));
        assert_eq!(at(100.0 + EDGE_ZONE_PX + 1.0), Some(Zone::Inside));
        assert_eq!(at(200.0), Some(Zone::Inside));
        assert_eq!(at(300.0 - EDGE_ZONE_PX - 1.0), Some(Zone::Inside));
        assert_eq!(at(300.0 - EDGE_ZONE_PX), Some(Zone::Extend(Side::Right)));
        assert_eq!(at(300.0 - BORDER_PX - 1.0), Some(Zone::Extend(Side::Right)));
        assert_eq!(at(300.0 - BORDER_PX), Some(Zone::Border(Side::Right)));
        assert_eq!(at(300.0 + BORDER_PX), Some(Zone::Border(Side::Right)));
        assert_eq!(at(300.0 + BORDER_PX + 1.0), None);
        assert_eq!(zone_at(&BAND, CHART, &payload, pos2(200.0, 10.0)), None);
    }

    /// Corners keep their corner resize: no side zone reaches them.
    #[test]
    fn the_corners_belong_to_no_zone() {
        let payload = RectanglePayload::default();
        for corner in [
            pos2(100.0, 80.0),
            pos2(300.0, 80.0),
            pos2(300.0, 220.0),
            pos2(100.0, 220.0),
            pos2(102.0, 84.0),
            pos2(100.0 + BORDER_PX + 2.0, 220.0 - 3.0),
        ] {
            assert_eq!(zone_at(&BAND, CHART, &payload, corner), None, "at {corner:?}");
        }
        // Just past the corner's reach, the side line is a side line again.
        assert_eq!(
            zone_at(&BAND, CHART, &payload, pos2(100.0, 80.0 + CORNER_PX + 1.0)),
            Some(Zone::Border(Side::Left))
        );
    }

    /// A narrow rectangle keeps an inside: each band takes a third at most.
    #[test]
    fn a_narrow_rectangle_keeps_its_inside() {
        let narrow = [pos2(0.0, 0.0), pos2(30.0, 100.0)];
        let payload = RectanglePayload::default();
        let at = |x: f32| zone_at(&narrow, CHART, &payload, pos2(x, 50.0));
        assert_eq!(at(BORDER_PX + 1.0), Some(Zone::Extend(Side::Left)));
        assert_eq!(at(15.0), Some(Zone::Inside));
        assert_eq!(at(30.0 - BORDER_PX - 1.0), Some(Zone::Extend(Side::Right)));
    }

    /// A side already run to the chart edge has no border to drag and
    /// nothing left to extend: all of it is inside.
    #[test]
    fn an_extended_side_is_all_inside() {
        let payload = right_only();
        let at = |x: f32| zone_at(&BAND, CHART, &payload, pos2(x, MID_Y));
        assert_eq!(at(300.0), Some(Zone::Inside));
        assert_eq!(at(300.0 - BORDER_PX - 1.0), Some(Zone::Inside));
        assert_eq!(at(495.0), Some(Zone::Inside), "the painted run is inside");
        assert_eq!(at(100.0), Some(Zone::Border(Side::Left)));
        assert_eq!(at(100.0 + BORDER_PX + 1.0), Some(Zone::Extend(Side::Left)));
    }

    #[test]
    fn one_click_in_a_band_extends_that_side() {
        let mut payload = RectanglePayload::default();
        assert!(click_at(&mut payload, 300.0 - BORDER_PX - 2.0));
        assert_eq!(extent(&payload), (false, true), "the right band");
        assert!(click_at(&mut payload, 100.0 + BORDER_PX + 2.0));
        assert_eq!(extent(&payload), (true, true), "then the left band too");
    }

    #[test]
    fn a_click_outside_the_bands_changes_nothing() {
        for x in [100.0, 200.0, 300.0, 50.0] {
            let mut payload = RectanglePayload::default();
            assert!(!click_at(&mut payload, x), "at x={x}");
            assert_eq!(payload, RectanglePayload::default(), "at x={x}");
        }
    }

    #[test]
    fn a_double_click_inside_puts_back_the_original_size() {
        for x in [105.0, 200.0, 295.0, 495.0] {
            let mut payload = RectanglePayload {
                extend_left: false,
                extend_right: true,
            };
            assert!(double_click_at(&mut payload, x), "at x={x}");
            assert_eq!(payload, RectanglePayload::default(), "at x={x}");
        }
        // Already at its original size: nothing to put back.
        let mut payload = RectanglePayload::default();
        assert!(!double_click_at(&mut payload, 200.0));
    }

    /// The double click's first click lands in a band and extends that
    /// side; the double click itself then ends at the original size.
    #[test]
    fn a_double_click_in_a_band_ends_at_the_original_size() {
        let mut payload = RectanglePayload::default();
        let x = 300.0 - BORDER_PX - 2.0;
        assert!(click_at(&mut payload, x));
        assert!(double_click_at(&mut payload, x));
        assert_eq!(payload, RectanglePayload::default());
    }

    #[test]
    fn a_double_click_on_a_corner_or_a_border_changes_nothing() {
        let both = RectanglePayload {
            extend_left: true,
            extend_right: true,
        };
        let mut payload = both.clone();
        let corner = pos2(100.0, 80.0);
        assert!(!TOOL.double_click(CHART, &BAND, corner, &mut payload));
        let mut payload = right_only();
        assert!(!double_click_at(&mut payload, 100.0), "the left border line");
        assert_eq!(payload, right_only());
    }

    /// Each zone has its own hint, so its own cursor; the inside and the
    /// corners have none.
    #[test]
    fn each_zone_names_its_own_hint() {
        let payload = RectanglePayload::default();
        let side = |handle| Some(PointerHint::ResizeSide { handle });
        assert_eq!(hint_at(&payload, pos2(100.0, MID_Y)), side(FIRST_SIDE_HANDLE));
        assert_eq!(hint_at(&payload, pos2(300.0, MID_Y)), side(SECOND_SIDE_HANDLE));
        assert_eq!(hint_at(&payload, pos2(110.0, MID_Y)), Some(PointerHint::ExtendSide));
        assert_eq!(hint_at(&payload, pos2(290.0, MID_Y)), Some(PointerHint::ExtendSide));
        assert_eq!(hint_at(&payload, pos2(200.0, MID_Y)), None);
        assert_eq!(hint_at(&payload, pos2(100.0, 80.0)), None);
    }

    /// The side line is a handle of its own, named by its hint: grabbing it
    /// moves that side alone, and its drag says so with the same hint.
    #[test]
    fn a_side_line_is_a_handle_that_moves_that_side_alone() {
        let payload = RectanglePayload::default();
        let left = side_handle_at(&payload, pos2(100.0, MID_Y)).expect("the left line grabs");
        let right =
            side_handle_at(&payload, pos2(300.0 + 2.0, MID_Y)).expect("the right line grabs");
        assert_ne!(left, right);
        assert_eq!(point_handle_at(&payload, pos2(100.0, 82.0)), Some(0), "the corner's");
        assert_eq!(side_handle_at(&payload, pos2(100.0, 82.0)), None, "no side line there");
        assert_eq!(point_handle_at(&payload, pos2(100.0, MID_Y)), None, "no point handle");
        assert_eq!(side_handle_at(&payload, pos2(110.0, MID_Y)), None, "the band is no handle");
        for handle in [left, right] {
            assert_eq!(TOOL.handle_hint(handle), Some(PointerHint::ResizeSide { handle }));
        }
        for corner in 0..4 {
            assert_eq!(TOOL.handle_hint(corner), None);
        }
        let moved = with_context(&payload, |ctxt| {
            TOOL.drag_handle(CHART, &BAND, left, pos2(60.0, 10.0), ctxt, Constrain::Free)
        })
        .expect("the rectangle owns its side drag");
        assert_eq!(moved.as_slice(), &[pos2(60.0, 80.0), pos2(300.0, 220.0)]);
        let moved = with_context(&payload, |ctxt| {
            TOOL.drag_handle(CHART, &BAND, right, pos2(250.0, 290.0), ctxt, Constrain::Free)
        })
        .expect("the rectangle owns its side drag");
        assert_eq!(moved.as_slice(), &[pos2(100.0, 80.0), pos2(250.0, 220.0)]);
        // A side run to the chart edge has no line to grab.
        assert_eq!(side_handle_at(&right_only(), pos2(300.0, MID_Y)), None);
    }

    #[test]
    fn extend_left_runs_paint_and_hit_test_to_the_chart_left_edge() {
        let payload = RectanglePayload {
            extend_left: true,
            ..RectanglePayload::default()
        };
        let rect = screen_rect(&BAND, CHART, &payload);
        assert_eq!(rect.left(), CHART.left());
        assert_eq!(rect.right(), 300.0);
        let anchors = [ChartPoint::at(10.0, 110.0), ChartPoint::at(30.0, 90.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        let ctxt = context(&payload, &anchors, &scale);
        assert!(TOOL.hit_test(CHART, &BAND, pos2(20.0, 80.0), 4.0, &ctxt));
    }

    #[test]
    fn the_preset_carries_both_extensions() {
        let payload = RectanglePayload {
            extend_left: true,
            extend_right: true,
        };
        let exported = payload.export_preset().expect("the rectangle exports its preset");
        let mut restored = RectanglePayload::default();
        assert!(restored.import_preset(&exported));
        assert_eq!(restored, payload);
    }

    #[test]
    fn a_preset_written_before_extend_left_loads_with_it_off() {
        let old: toml::Value = toml::from_str("version = 1\nextend_right = true\n").unwrap();
        let mut payload = RectanglePayload::default();
        assert!(payload.import_preset(&old));
        assert_eq!(
            payload,
            RectanglePayload {
                extend_right: true,
                ..RectanglePayload::default()
            }
        );
    }
}
