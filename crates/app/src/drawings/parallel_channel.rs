//! The rising / falling channel.
//!
//! Three anchors: the trend line, then the width. What the previous version
//! lacked is the line traders actually trade off — the middle
//! (`docs/ux/drawing-tools-2026-08.md` §D4) — plus the ability to project the
//! rails past the swing that defined them, which is the whole point of
//! drawing a channel on a live chart.

use std::any::Any;

use eframe::egui;
use egui_phosphor::regular as icons;

use super::line_core::{Extend, line_ends};
use super::{
    DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, PresetHost, ToolShortcut,
    distance_to_segment, drawing_fill, drawing_stroke,
};

pub(super) static TOOL: ParallelChannel = ParallelChannel;

pub(super) struct ParallelChannel;

/// Dash geometry of the middle line: present enough to trade off, dashed
/// enough that it never reads as one of the two rails.
const MIDLINE_DASH_PX: f32 = 5.0;
const MIDLINE_GAP_PX: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelPayload {
    /// The middle line. Default **on**: it is what a channel is drawn for,
    /// and a channel without one is the odd one out, not the baseline.
    pub midline: bool,
    pub extend_left: bool,
    pub extend_right: bool,
}

impl Default for ChannelPayload {
    fn default() -> Self {
        Self {
            midline: true,
            extend_left: false,
            extend_right: false,
        }
    }
}

impl ChannelPayload {
    /// The two flags as the one rule the line core already implements.
    fn extend(self) -> Option<Extend> {
        match (self.extend_left, self.extend_right) {
            (false, false) => None,
            (false, true) => Some(Extend::Forward),
            (true, false) => Some(Extend::Backward),
            (true, true) => Some(Extend::Both),
        }
    }
}

impl DrawingPayload for ChannelPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(*self)
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| other == self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn export_preset(&self) -> Option<toml::Value> {
        let mut table = toml::value::Table::new();
        table.insert("midline".to_owned(), toml::Value::Boolean(self.midline));
        table.insert(
            "extend_left".to_owned(),
            toml::Value::Boolean(self.extend_left),
        );
        table.insert(
            "extend_right".to_owned(),
            toml::Value::Boolean(self.extend_right),
        );
        Some(toml::Value::Table(table))
    }
    /// A preset applies the flags it names and leaves the rest alone, so an
    /// older preset written before a flag existed does not silently reset it.
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Some(table) = value.as_table() else {
            return false;
        };
        let flag = |key: &str| table.get(key).and_then(toml::Value::as_bool);
        let mut applied = false;
        for (found, target) in [
            (flag("midline"), &mut self.midline),
            (flag("extend_left"), &mut self.extend_left),
            (flag("extend_right"), &mut self.extend_right),
        ] {
            if let Some(found) = found {
                *target = found;
                applied = true;
            }
        }
        applied
    }
}

fn payload_of(ctxt: &DrawContext<'_>) -> ChannelPayload {
    ctxt.payload
        .as_any()
        .downcast_ref::<ChannelPayload>()
        .copied()
        .unwrap_or_default()
}

/// The perpendicular offset from the baseline to the opposite rail.
fn channel_offset(points: &[egui::Pos2]) -> egui::Vec2 {
    let baseline = points[1] - points[0];
    let to_width_anchor = points[2] - points[0];
    let baseline_length_sq = baseline.length_sq();
    if baseline_length_sq <= f32::EPSILON {
        return to_width_anchor;
    }
    to_width_anchor - baseline * (to_width_anchor.dot(baseline) / baseline_length_sq)
}

/// The baseline after extension. Rails are parallel, so extending this one
/// and adding the offset extends all three by construction.
fn baseline(
    chart_rect: egui::Rect,
    points: &[egui::Pos2],
    extend: Option<Extend>,
) -> (egui::Pos2, egui::Pos2) {
    match extend {
        None => (points[0], points[1]),
        Some(extend) => {
            line_ends(chart_rect, points, extend).unwrap_or((points[0], points[1]))
        }
    }
}

/// Paint a dashed segment. egui's dashed helper allocates a `Vec` of shapes
/// per call; a channel repaints every frame, so the midline walks the segment
/// itself and emits plain line segments.
fn dashed_segment(painter: &egui::Painter, from: egui::Pos2, to: egui::Pos2, stroke: egui::Stroke) {
    let span = to - from;
    let length = span.length();
    if length <= f32::EPSILON {
        return;
    }
    let step = MIDLINE_DASH_PX + MIDLINE_GAP_PX;
    let direction = span / length;
    let mut travelled = 0.0_f32;
    while travelled < length {
        let dash_end = (travelled + MIDLINE_DASH_PX).min(length);
        painter.line_segment(
            [from + direction * travelled, from + direction * dash_end],
            stroke,
        );
        travelled += step;
    }
}

impl DrawingToolImpl for ParallelChannel {
    fn id(&self) -> &'static str {
        "parallel-channel"
    }
    fn name(&self) -> &'static str {
        "Parallel channel"
    }
    fn settings_title(&self) -> &'static str {
        "Parallel channel settings"
    }
    fn icon(&self) -> &'static str {
        icons::PARALLELOGRAM
    }
    fn hover_text(&self) -> &'static str {
        "Rising / falling channel - draw the trend line, then click the channel width (C)"
    }
    fn required_points(&self) -> usize {
        3
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::C,
            shift: false,
        })
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(ChannelPayload::default())
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("Channel")
    }
    fn draw_extra_tab(
        &self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        _host: &mut dyn PresetHost,
    ) -> bool {
        let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<ChannelPayload>() else {
            return false;
        };
        let mut changed = ui.checkbox(&mut payload.midline, "Middle line").changed();
        ui.add_space(4.0);
        changed |= ui
            .checkbox(&mut payload.extend_left, "Extend left")
            .changed();
        changed |= ui
            .checkbox(&mut payload.extend_right, "Extend right")
            .changed();
        changed
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        let stroke = drawing_stroke(style);
        if points.len() == 2 {
            painter.line_segment([points[0], points[1]], stroke);
            return;
        }
        if points.len() != 3 {
            return;
        }
        let payload = payload_of(ctxt);
        let offset = channel_offset(points);
        let (near_start, near_end) = baseline(chart_rect, points, payload.extend());
        let (far_start, far_end) = (near_start + offset, near_end + offset);

        if style.fill_alpha > 0 {
            painter.add(egui::Shape::convex_polygon(
                vec![near_start, near_end, far_end, far_start],
                drawing_fill(style),
                egui::Stroke::NONE,
            ));
        }
        painter.line_segment([near_start, near_end], stroke);
        painter.line_segment([far_start, far_end], stroke);
        // The end caps stay on the anchors even when the rails run on: they
        // mark where the trader actually placed the channel.
        painter.line_segment([points[0], points[0] + offset], stroke);
        painter.line_segment([points[1], points[1] + offset], stroke);

        // The halo pass is a widened copy of the stroke; a dashed line
        // widened underneath itself reads as a smear, so the midline is
        // geometry-only.
        if payload.midline && !ctxt.halo {
            let half = offset / 2.0;
            dashed_segment(painter, near_start + half, near_end + half, stroke);
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
        if points.len() != 3 {
            return false;
        }
        let payload = payload_of(ctxt);
        let offset = channel_offset(points);
        let (near_start, near_end) = baseline(chart_rect, points, payload.extend());
        let half = offset / 2.0;
        let rails = [
            (near_start, near_end),
            (near_start + offset, near_end + offset),
            (points[0], points[0] + offset),
            (points[1], points[1] + offset),
        ];
        let midline = payload
            .midline
            .then_some((near_start + half, near_end + half));
        rails
            .into_iter()
            .chain(midline)
            .any(|(start, end)| distance_to_segment(position, start, end) <= radius_px)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![
                egui::pos2(100.0, 100.0),
                egui::pos2(200.0, 120.0),
                egui::pos2(100.0, 160.0),
            ],
            egui::pos2(150.0, 170.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The session's request, and the reason it is on by default: the middle
    /// line is what the channel is drawn for.
    #[test]
    fn a_fresh_channel_has_its_middle_line() {
        let fresh = ChannelPayload::default();
        assert!(fresh.midline);
        assert!(!fresh.extend_left);
        assert!(!fresh.extend_right);
    }

    #[test]
    fn the_extension_flags_map_to_one_line_rule() {
        assert_eq!(ChannelPayload::default().extend(), None);
        assert_eq!(
            ChannelPayload {
                extend_right: true,
                ..ChannelPayload::default()
            }
            .extend(),
            Some(Extend::Forward)
        );
        assert_eq!(
            ChannelPayload {
                extend_left: true,
                ..ChannelPayload::default()
            }
            .extend(),
            Some(Extend::Backward)
        );
        assert_eq!(
            ChannelPayload {
                extend_left: true,
                extend_right: true,
                ..ChannelPayload::default()
            }
            .extend(),
            Some(Extend::Both)
        );
    }

    /// The midline is exactly halfway on the rails' normal — the level a
    /// trader reads as "the middle of the channel", not an eyeballed one.
    #[test]
    fn the_middle_line_sits_halfway_between_the_rails() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(200.0, 100.0),
            egui::pos2(100.0, 160.0),
        ];
        let offset = channel_offset(&points);
        assert_eq!(offset, egui::vec2(0.0, 60.0));
        assert_eq!(points[0] + offset / 2.0, egui::pos2(100.0, 130.0));
    }

    /// Extending is a view affordance: the caps still mark the anchors, so
    /// the trader can always see where the channel was actually defined.
    #[test]
    fn extending_moves_the_rails_but_not_the_anchors() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(200.0, 100.0),
            egui::pos2(100.0, 160.0),
        ];
        let (start, end) = baseline(
            chart,
            &points,
            ChannelPayload {
                extend_left: true,
                extend_right: true,
                ..ChannelPayload::default()
            }
            .extend(),
        );
        assert_eq!(start.x, 0.0);
        assert_eq!(end.x, 800.0);
        assert_eq!(points[0], egui::pos2(100.0, 100.0));
        assert_eq!(points[1], egui::pos2(200.0, 100.0));
    }

    #[test]
    fn a_preset_round_trips_every_flag() {
        let source = ChannelPayload {
            midline: false,
            extend_left: true,
            extend_right: false,
        };
        let exported = source.export_preset().expect("the channel exports one");
        let mut target = ChannelPayload::default();
        assert!(target.import_preset(&exported));
        assert_eq!(target, source);
    }

    /// A preset written before a flag existed must not reset it: applying an
    /// old preset changes what it names and nothing else.
    #[test]
    fn an_older_preset_leaves_flags_it_never_knew_about_alone() {
        let mut table = toml::value::Table::new();
        table.insert("midline".to_owned(), toml::Value::Boolean(false));
        let mut target = ChannelPayload {
            extend_right: true,
            ..ChannelPayload::default()
        };
        assert!(target.import_preset(&toml::Value::Table(table)));
        assert!(!target.midline);
        assert!(target.extend_right, "the flag it never named survives");
    }
}
