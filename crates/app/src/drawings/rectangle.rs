use std::any::Any;

use eframe::egui;
use egui_phosphor::regular as icons;
use serde::{Deserialize, Serialize};

use super::shape_core::SHAPES_FAMILY;
use super::{
    Constrain, DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, Handles,
    PresetHost, ToolFamily, ToolShortcut, drawing_fill, drawing_stroke,
};

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
        true
    }
}

/// The rectangle's screen span: its two anchor corners, run to the chart's
/// right edge when the payload extends it. Paint and hit-test share this so
/// the clickable area is exactly the painted one.
fn screen_rect(points: &[egui::Pos2], chart_rect: egui::Rect, payload: &RectanglePayload) -> egui::Rect {
    let mut rect = egui::Rect::from_two_pos(points[0], points[1]);
    if payload.extend_right {
        rect.max.x = rect.max.x.max(chart_rect.right());
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
fn rectangle_handles(points: &[egui::Pos2]) -> Option<Handles> {
    let [first, second] = points else {
        return None;
    };
    Some(Handles::from_slice(&[
        *first,
        egui::pos2(second.x, first.y),
        *second,
        egui::pos2(first.x, second.y),
    ]))
}

/// Move the two anchor components owned by one corner while the diagonally
/// opposite corner stays fixed.
fn resize_from_corner(
    points: &[egui::Pos2],
    handle: usize,
    to: egui::Pos2,
) -> Option<Handles> {
    let [first, second] = points else {
        return None;
    };
    let (first, second) = match handle {
        0 => (to, *second),
        1 => (egui::pos2(first.x, to.y), egui::pos2(to.x, second.y)),
        2 => (*first, to),
        3 => (egui::pos2(to.x, first.y), egui::pos2(second.x, to.y)),
        _ => return None,
    };
    Some(Handles::from_slice(&[first, second]))
}

pub(super) static TOOL: Rectangle = Rectangle;

pub(super) struct Rectangle;

impl DrawingToolImpl for Rectangle {
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
            key: egui::Key::R,
            shift: false,
        })
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
    fn draw_extra_tab(
        &self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        _host: &mut dyn PresetHost,
    ) -> bool {
        let payload = drawing
            .payload
            .as_any_mut()
            .downcast_mut::<RectanglePayload>()
            .expect("a rectangle always carries a rectangle payload");
        ui.checkbox(&mut payload.extend_right, "extend right")
            .on_hover_text(
                "run the band to the chart's right edge until further notice — an armed \
                 strategy then keeps watching past the drawn end instead of expiring there",
            )
            .changed()
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        if points.len() == 2 {
            let rect = screen_rect(points, chart_rect, payload_of(ctxt));
            painter.rect_filled(rect, egui::Rounding::ZERO, drawing_fill(style));
            painter.rect_stroke(
                rect,
                egui::Rounding::ZERO,
                drawing_stroke(style),
            );
        }
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
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
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        rectangle_handles(points)
    }
    fn drag_handle(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        handle: usize,
        to: egui::Pos2,
        _ctxt: &DrawContext<'_>,
        // A rectangle corner changes two independent edges; it has no line
        // angle for the level constraint to preserve.
        _constrain: Constrain,
    ) -> Option<Handles> {
        resize_from_corner(points, handle, to)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)],
            egui::pos2(150.0, 150.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::{ChartPoint, Constrain, PriceScale, ValueUnit};

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
        }
    }

    #[test]
    fn a_rectangle_exposes_all_four_corners_as_handles() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let points = [egui::pos2(300.0, 220.0), egui::pos2(100.0, 80.0)];
        let anchors = [ChartPoint::at(30.0, 90.0), ChartPoint::at(10.0, 110.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        let payload = RectanglePayload::default();
        let ctxt = context(&payload, &anchors, &scale);

        assert_eq!(
            TOOL.handles(chart, &points, &ctxt)
                .expect("the rectangle owns its four handles")
                .as_slice(),
            &[
                egui::pos2(300.0, 220.0),
                egui::pos2(100.0, 220.0),
                egui::pos2(100.0, 80.0),
                egui::pos2(300.0, 80.0),
            ]
        );
    }

    #[test]
    fn every_corner_expands_and_contracts_around_its_opposite_corner() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let points = [egui::pos2(100.0, 80.0), egui::pos2(300.0, 220.0)];
        let anchors = [ChartPoint::at(10.0, 110.0), ChartPoint::at(30.0, 90.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        let payload = RectanglePayload::default();
        let ctxt = context(&payload, &anchors, &scale);
        let opposite = [
            egui::pos2(300.0, 220.0),
            egui::pos2(100.0, 220.0),
            egui::pos2(100.0, 80.0),
            egui::pos2(300.0, 80.0),
        ];
        let outward = [
            egui::pos2(60.0, 40.0),
            egui::pos2(340.0, 40.0),
            egui::pos2(340.0, 260.0),
            egui::pos2(60.0, 260.0),
        ];
        let inward = [
            egui::pos2(140.0, 120.0),
            egui::pos2(260.0, 120.0),
            egui::pos2(260.0, 180.0),
            egui::pos2(140.0, 180.0),
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
                let resized = egui::Rect::from_two_pos(moved[0], moved[1]);
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
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let points = [egui::pos2(100.0, 80.0), egui::pos2(300.0, 220.0)];
        let anchors = [ChartPoint::at(10.0, 110.0), ChartPoint::at(30.0, 90.0)];
        let scale = PriceScale::from_range(80.0, 120.0, 0.0, 300.0);
        let payload = RectanglePayload::default();
        let ctxt = context(&payload, &anchors, &scale);
        let target = egui::pos2(340.0, 250.0);

        let moved = TOOL
            .drag_handle(chart, &points, 0, target, &ctxt, Constrain::Free)
            .expect("crossing remains a rectangle resize");
        assert_eq!(
            egui::Rect::from_two_pos(moved[0], moved[1]),
            egui::Rect::from_two_pos(target, egui::pos2(300.0, 220.0))
        );
    }
}
