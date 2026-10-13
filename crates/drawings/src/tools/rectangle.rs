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
    use crate::{ChartPoint, Constrain, ValueUnit};

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

    fn double_click_at(payload: &mut RectanglePayload, x: f32) -> bool {
        TOOL.double_click(CHART, &BAND, pos2(x, 150.0), payload)
    }

    fn hint_at(payload: &RectanglePayload, x: f32) -> Option<DoubleClickHint> {
        let anchors = [ChartPoint::at(10.0, 110.0), ChartPoint::at(30.0, 90.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        TOOL.double_click_hint(CHART, &BAND, pos2(x, 150.0), &context(payload, &anchors, &scale))
    }

    #[test]
    fn the_zones_are_the_sides_inside_the_edge_distance_and_the_centre_between() {
        let rect = PxRect::from_two_pos(BAND[0], BAND[1]);
        let at = |x: f32| extend_zone(rect, rect, pos2(x, 150.0));
        assert_eq!(at(100.0), Some(ExtendZone::Left));
        assert_eq!(at(100.0 + EDGE_ZONE_PX), Some(ExtendZone::Left));
        assert_eq!(at(100.0 + EDGE_ZONE_PX + 1.0), Some(ExtendZone::Center));
        assert_eq!(at(300.0 - EDGE_ZONE_PX - 1.0), Some(ExtendZone::Center));
        assert_eq!(at(300.0 - EDGE_ZONE_PX), Some(ExtendZone::Right));
        assert_eq!(at(305.0), Some(ExtendZone::Right), "the border's slack counts");
        assert_eq!(at(100.0 - EDGE_ZONE_SLACK_PX - 1.0), None);
        assert_eq!(extend_zone(rect, rect, pos2(200.0, 10.0)), None);
        // A narrow band keeps a centre: each side takes a third at most.
        let narrow = PxRect::from_min_max(pos2(0.0, 0.0), pos2(30.0, 10.0));
        assert_eq!(
            extend_zone(narrow, narrow, pos2(15.0, 5.0)),
            Some(ExtendZone::Center)
        );
    }

    fn extent(payload: &RectanglePayload) -> (bool, bool) {
        (payload.extend_left, payload.extend_right)
    }

    #[test]
    fn a_side_double_click_toggles_that_side_alone() {
        let mut payload = RectanglePayload::default();
        assert!(double_click_at(&mut payload, 295.0));
        assert_eq!(extent(&payload), (false, true));
        // The other side extends too: a side never restores the last extent.
        assert!(double_click_at(&mut payload, 105.0));
        assert_eq!(extent(&payload), (true, true));
        // The same side again takes back that side only.
        assert!(double_click_at(&mut payload, 295.0));
        assert_eq!(extent(&payload), (true, false));
        assert!(double_click_at(&mut payload, 105.0));
        assert_eq!(payload, RectanglePayload::default());
    }

    #[test]
    fn the_centre_extends_both_sides_and_the_next_centre_double_click_restores() {
        let by_hand = RectanglePayload {
            extend_right: true,
            ..RectanglePayload::default()
        };
        let mut payload = by_hand.clone();
        assert!(double_click_at(&mut payload, 200.0));
        assert_eq!(extent(&payload), (true, true));
        assert_eq!(payload.extended_from, Some((false, true)));
        assert!(double_click_at(&mut payload, 200.0));
        assert_eq!(payload, by_hand, "the trader's own extension comes back");
        // Both sides already at the edge: the centre has nothing to add.
        let mut both = RectanglePayload {
            extend_left: true,
            extend_right: true,
            extended_from: None,
        };
        assert!(!double_click_at(&mut both, 200.0));
    }

    #[test]
    fn a_side_double_click_after_the_centre_drops_the_restore_point() {
        let mut payload = RectanglePayload::default();
        assert!(double_click_at(&mut payload, 200.0));
        assert!(double_click_at(&mut payload, 105.0));
        assert_eq!(extent(&payload), (false, true));
        assert_eq!(payload.extended_from, None);
        // The centre now extends from what is there, not a stale extent.
        assert!(double_click_at(&mut payload, 200.0));
        assert_eq!(extent(&payload), (true, true));
        assert_eq!(payload.extended_from, Some((false, true)));
    }

    #[test]
    fn the_hint_is_every_side_zone_and_never_the_centre() {
        for payload in [
            RectanglePayload::default(),
            RectanglePayload {
                extend_left: true,
                extend_right: true,
                extended_from: None,
            },
        ] {
            for x in [105.0, 295.0] {
                assert_eq!(
                    hint_at(&payload, x),
                    Some(DoubleClickHint::ExtendSideways),
                    "at x={x}, {payload:?}"
                );
            }
            assert_eq!(hint_at(&payload, 200.0), None, "the centre is the move grip");
        }
        assert_eq!(hint_at(&RectanglePayload::default(), 50.0), None, "off the band");
        // Run to the edge, the painted part past the drawn side is that side.
        let extended = RectanglePayload {
            extend_left: true,
            ..RectanglePayload::default()
        };
        assert_eq!(hint_at(&extended, 5.0), Some(DoubleClickHint::ExtendSideways));
        let mut payload = extended.clone();
        assert!(double_click_at(&mut payload, 5.0), "the painted edge takes its side back");
        assert_eq!(payload, RectanglePayload::default());
    }

    #[test]
    fn the_painted_run_between_the_edge_and_the_drawn_side_leaves_the_double_click_alone() {
        let mut extended = RectanglePayload {
            extend_left: true,
            ..RectanglePayload::default()
        };
        assert_eq!(hint_at(&extended, 50.0), None, "the band, not a side");
        assert!(!double_click_at(&mut extended, 50.0), "the chart keeps its double click");
        assert!(extended.extend_left);
        // The drawn side's slack still reaches just outside it.
        assert_eq!(hint_at(&extended, 92.0), Some(DoubleClickHint::ExtendSideways));
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
    fn a_drawn_side_keeps_its_zone_after_the_band_runs_past_it() {
        // Extended left by hand: the drawn left side is still the left zone,
        // so a double click there takes the left side back - it does not
        // read as the centre and extend the right side too.
        let mut payload = RectanglePayload {
            extend_left: true,
            ..RectanglePayload::default()
        };
        assert!(double_click_at(&mut payload, 105.0));
        assert_eq!(extent(&payload), (false, false));
        assert!(double_click_at(&mut payload, 295.0));
        assert_eq!(extent(&payload), (false, true));
        // The drawn side keeps its hint after the band runs past it.
        assert!(double_click_at(&mut payload, 105.0));
        assert_eq!(hint_at(&payload, 105.0), Some(DoubleClickHint::ExtendSideways));
    }

    #[test]
    fn the_preset_carries_both_extensions_but_never_the_restore_point() {
        let payload = RectanglePayload {
            extend_left: true,
            extend_right: true,
            extended_from: Some((false, true)),
        };
        let exported = payload.export_preset().expect("the rectangle exports its preset");
        // A tool default, named preset or layout written from a gestured
        // band hands the next rectangle its extent, not a stale restore.
        let mut restored = RectanglePayload {
            extended_from: Some((true, false)),
            ..RectanglePayload::default()
        };
        assert!(restored.import_preset(&exported));
        assert_eq!(
            restored,
            RectanglePayload {
                extended_from: None,
                ..payload
            }
        );
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
