use crate::{MarkScene, pos2, Px, PxRect};
use std::any::Any;

use crate::icons;
use serde::{Deserialize, Serialize};

use super::shape_core::SHAPES_FAMILY;
use super::{Constrain, DoubleClickHint, DrawContext, DrawingPayload, DrawingStyle, MarkTool, Handles, ToolFamily, ToolShortcut, drawing_fill, drawing_stroke};

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
    /// The `(left, right)` extension a double click on the centre replaced,
    /// held so the next centre double click puts back exactly the extent
    /// that was there. `None` while the extension is the trader's own
    /// setting, and after a side's double click, which owns only its side.
    ///
    /// Session state, never exported: a tool default or a named preset that
    /// carried it would hand every new rectangle a restore point it never
    /// had, and the layout shares that export, so a reload forgets it too.
    pub extended_from: Option<(bool, bool)>,
}

/// About half a centimetre on a typical desktop display: how far inside the
/// left or right side the pointer is "on that side" for the double click
/// that extends the band that way.
pub(crate) const EDGE_ZONE_PX: f32 = 18.0;
/// Reach outside the drawn border that still counts as on the rectangle -
/// the same slack the pane gives a click that selects a drawing.
const EDGE_ZONE_SLACK_PX: f32 = 10.0;
/// Where on the rectangle a double click lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtendZone {
    Left,
    Right,
    Center,
}

/// The zone under `position` of a band drawn as `drawn` and painted as
/// `painted` (the drawn rectangle run to the chart edges it extends to), or
/// `None` off the band. A side's zone is the strip by its drawn side, which
/// keeps its place after the band runs past it, and the strip by the painted
/// edge the band now runs to - the side the trader sees. The painted run
/// between them is no zone: there the band is only a band, and the chart
/// keeps its double click. A narrow rectangle keeps a centre: each side zone
/// takes at most a third of the drawn width.
fn extend_zone(drawn: PxRect, painted: PxRect, position: Px) -> Option<ExtendZone> {
    if !painted.expand(EDGE_ZONE_SLACK_PX).contains(position) {
        return None;
    }
    let zone = side_zone_width(drawn);
    let x = position.x;
    let by_drawn = |side: f32| (x - side).abs() <= zone.max(EDGE_ZONE_SLACK_PX);
    if x <= drawn.left() + zone && (by_drawn(drawn.left()) || x <= painted.left() + zone) {
        Some(ExtendZone::Left)
    } else if x >= drawn.right() - zone && (by_drawn(drawn.right()) || x >= painted.right() - zone)
    {
        Some(ExtendZone::Right)
    } else {
        (x > drawn.left() && x < drawn.right()).then_some(ExtendZone::Center)
    }
}

fn side_zone_width(rect: PxRect) -> f32 {
    EDGE_ZONE_PX.min(rect.width() / 3.0)
}

impl RectanglePayload {
    /// The double click. A side zone toggles that side alone - run it to
    /// the chart edge, or take it back - so the trader can extend one way
    /// and then the other without a restore in between. The centre restores
    /// the extent a previous centre double click replaced, else extends both
    /// sides. Answers whether anything changed - extending from the centre
    /// when both sides already run to the edge is a no-op.
    fn apply_double_click(&mut self, zone: ExtendZone) -> bool {
        match zone {
            ExtendZone::Left => {
                self.extend_left = !self.extend_left;
                self.extended_from = None;
                return true;
            }
            ExtendZone::Right => {
                self.extend_right = !self.extend_right;
                self.extended_from = None;
                return true;
            }
            ExtendZone::Center => {}
        }
        if let Some((left, right)) = self.extended_from.take() {
            self.extend_left = left;
            self.extend_right = right;
            return true;
        }
        let before = (self.extend_left, self.extend_right);
        let after = (true, true);
        if after == before {
            return false;
        }
        self.extended_from = Some(before);
        (self.extend_left, self.extend_right) = after;
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
        // The extension is now the preset's setting, not a gesture's.
        self.extended_from = None;
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
/// opposite corner stays fixed.
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
        _ => return None,
    };
    Some(Handles::from_slice(&[first, second]))
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
    fn double_click_hint(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        ctxt: &DrawContext<'_>,
    ) -> Option<DoubleClickHint> {
        if points.len() != 2 {
            return None;
        }
        let drawn = PxRect::from_two_pos(points[0], points[1]);
        let painted = screen_rect(points, chart_rect, payload_of(ctxt));
        // The hint lives in the side zones only, where a double click always
        // toggles that side: the centre is where the trader grabs the band
        // to move it, and keeps the move cursor. Its double click still works.
        extend_zone(drawn, painted, position)
            .is_some_and(|zone| zone != ExtendZone::Center)
            .then_some(DoubleClickHint::ExtendSideways)
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
        if points.len() != 2 {
            return false;
        }
        let drawn = PxRect::from_two_pos(points[0], points[1]);
        let painted = screen_rect(points, chart_rect, payload);
        extend_zone(drawn, painted, position).is_some_and(|zone| payload.apply_double_click(zone))
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

    fn handle_at(payload: &RectanglePayload, position: Px) -> Option<usize> {
        with_context(payload, |ctxt| {
            DrawingTool(&TOOL).hit_handle(CHART, &BAND, position, 12.0, ctxt)
        })
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
        assert_eq!(hint_at(&payload, pos2(100.0, MID_Y)), Some(PointerHint::ResizeSide));
        assert_eq!(hint_at(&payload, pos2(300.0, MID_Y)), Some(PointerHint::ResizeSide));
        assert_eq!(hint_at(&payload, pos2(110.0, MID_Y)), Some(PointerHint::ExtendSide));
        assert_eq!(hint_at(&payload, pos2(290.0, MID_Y)), Some(PointerHint::ExtendSide));
        assert_eq!(hint_at(&payload, pos2(200.0, MID_Y)), None);
        assert_eq!(hint_at(&payload, pos2(100.0, 80.0)), None);
        assert_ne!(PointerHint::ResizeSide, PointerHint::ExtendSide);
    }

    /// The side line is a handle of its own: grabbing it moves that side
    /// alone, and says so with the side's hint.
    #[test]
    fn a_side_line_is_a_handle_that_moves_that_side_alone() {
        let payload = RectanglePayload::default();
        let left = handle_at(&payload, pos2(100.0, MID_Y)).expect("the left line grabs");
        let right = handle_at(&payload, pos2(300.0 + 2.0, MID_Y)).expect("the right line grabs");
        assert_ne!(left, right);
        assert_eq!(handle_at(&payload, pos2(100.0, 82.0)), Some(0), "the corner wins");
        assert_eq!(handle_at(&payload, pos2(110.0, MID_Y)), None, "the band is no handle");
        for handle in [left, right] {
            assert_eq!(TOOL.handle_hint(handle), Some(PointerHint::ResizeSide));
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
        assert_eq!(handle_at(&right_only(), pos2(300.0, MID_Y)), None);
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
