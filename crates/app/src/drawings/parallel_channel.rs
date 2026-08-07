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
    DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, Handles, PresetHost,
    ToolShortcut, distance_to_segment, drawing_fill, drawing_stroke,
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

/// The part of `span` that crosses the baseline. Width is measured across the
/// channel and nowhere else: sliding a handle *along* the rails must leave the
/// corridor exactly as wide as the trader set it.
fn across_baseline(baseline: egui::Vec2, span: egui::Vec2) -> egui::Vec2 {
    let baseline_length_sq = baseline.length_sq();
    if baseline_length_sq <= f32::EPSILON {
        return span;
    }
    span - baseline * (span.dot(baseline) / baseline_length_sq)
}

/// The perpendicular offset from the baseline to the opposite rail.
fn channel_offset(points: &[egui::Pos2]) -> egui::Vec2 {
    across_baseline(points[1] - points[0], points[2] - points[0])
}

/// The trader's grab points, in the order [`ParallelChannel::drag_handle`]
/// reads them: the two ends of the trend line, then the centre of the near
/// rail and the centre of the far one.
///
/// The rail handles sit on the *anchored* span, never on the extended rails:
/// extension is a view affordance, and a handle that ran off to the edge of
/// the chart with it would be a grab point for a place the trader never put
/// anything.
const HANDLE_NEAR_CENTRE: usize = 2;
const HANDLE_FAR_CENTRE: usize = 3;

fn channel_handles(points: &[egui::Pos2]) -> Option<Handles> {
    if points.len() < 3 {
        return None;
    }
    let offset = channel_offset(points);
    let centre = points[0] + (points[1] - points[0]) / 2.0;
    Some(Handles::from_slice(&[
        points[0],
        points[1],
        centre,
        centre + offset,
    ]))
}

/// Where the anchors land after dragging one handle to `to`.
///
/// The two rail handles are the point of the tool: each widens the channel
/// from *its own* edge and leaves the opposite rail where the trader put it.
/// Dragging the far rail moves the far rail; dragging the near rail moves the
/// trend line and holds the far rail still, so the corridor grows downward
/// instead of upward. Neither gesture may quietly re-angle the channel, which
/// is why both keep only the component across the baseline.
fn channel_drag(points: &[egui::Pos2], handle: usize, to: egui::Pos2) -> Option<Handles> {
    if points.len() < 3 {
        return None;
    }
    let baseline = points[1] - points[0];
    let centre = points[0] + baseline / 2.0;
    Some(match handle {
        0 => Handles::from_slice(&[to, points[1], points[2]]),
        1 => Handles::from_slice(&[points[0], to, points[2]]),
        HANDLE_NEAR_CENTRE => {
            // The far rail is pinned: it stays where it is, and the trend
            // line slides across to it.
            let far = points[0] + channel_offset(points);
            let shift = across_baseline(baseline, to - centre);
            Handles::from_slice(&[
                points[0] + shift,
                points[1] + shift,
                width_anchor_on(far, baseline, points[2]),
            ])
        }
        HANDLE_FAR_CENTRE => {
            let far = points[0] + across_baseline(baseline, to - centre);
            Handles::from_slice(&[
                points[0],
                points[1],
                width_anchor_on(far, baseline, points[2]),
            ])
        }
        _ => return None,
    })
}

/// Where to park the width anchor on a rail that runs through `on_rail`.
///
/// Any point of the rail encodes the same channel — only the offset across
/// the baseline is geometry. So keep the bar the trader originally put the
/// anchor on: the Coordinates tab shows this anchor by number, and a width
/// drag that silently rewrote its *bar* would look like the object moved
/// sideways when nothing of the kind happened.
fn width_anchor_on(on_rail: egui::Pos2, baseline: egui::Vec2, previous: egui::Pos2) -> egui::Pos2 {
    if baseline.x.abs() <= f32::EPSILON {
        return on_rail;
    }
    on_rail + baseline * ((previous.x - on_rail.x) / baseline.x)
}

/// Everything a channel is made of.
///
/// **Open at both ends, on purpose.** An earlier version closed the corridor
/// with cross-rail connectors at the anchors, and a closed parallelogram is
/// the visual signature of a rectangle — the two tools became one mark. No
/// professional platform caps a channel; where the trader anchored it is told
/// by the selection handles, which is what handles are for.
///
/// One source of truth for the geometry, so paint and hit-test cannot come to
/// disagree about what the object even is.
struct Channel {
    near: (egui::Pos2, egui::Pos2),
    far: (egui::Pos2, egui::Pos2),
    /// The line traders actually trade off, when it is on.
    midline: Option<(egui::Pos2, egui::Pos2)>,
}

impl Channel {
    fn new(chart_rect: egui::Rect, points: &[egui::Pos2], payload: ChannelPayload) -> Option<Self> {
        if points.len() < 3 {
            return None;
        }
        let offset = channel_offset(points);
        let (start, end) = baseline(chart_rect, points, payload.extend());
        let half = offset / 2.0;
        Some(Self {
            near: (start, end),
            far: (start + offset, end + offset),
            midline: payload.midline.then_some((start + half, end + half)),
        })
    }

    /// Every stroke the object has, for hit-testing. The corridor's interior
    /// is not one of them: a channel is grabbed by its lines, exactly like
    /// the trend line it is built from.
    fn segments(&self) -> impl Iterator<Item = (egui::Pos2, egui::Pos2)> + '_ {
        [self.near, self.far].into_iter().chain(self.midline)
    }
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
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        match placed {
            1 => Some("Click the end of the trend line"),
            2 => Some("Click the channel width"),
            _ => None,
        }
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
        let Some(channel) = Channel::new(chart_rect, points, payload_of(ctxt)) else {
            return;
        };

        if style.fill_alpha > 0 {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    channel.near.0,
                    channel.near.1,
                    channel.far.1,
                    channel.far.0,
                ],
                drawing_fill(style),
                egui::Stroke::NONE,
            ));
        }
        painter.line_segment([channel.near.0, channel.near.1], stroke);
        painter.line_segment([channel.far.0, channel.far.1], stroke);

        // The halo pass is a widened copy of the stroke; a dashed line
        // widened underneath itself reads as a smear, so the midline is
        // geometry-only.
        if let Some((from, to)) = channel.midline.filter(|_| !ctxt.halo) {
            dashed_segment(painter, from, to, stroke);
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
        Channel::new(chart_rect, points, payload_of(ctxt)).is_some_and(|channel| {
            channel
                .segments()
                .any(|(start, end)| distance_to_segment(position, start, end) <= radius_px)
        })
    }

    fn handles(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        channel_handles(points)
    }

    fn drag_handle(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        handle: usize,
        to: egui::Pos2,
        _ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        channel_drag(points, handle, to)
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

    /// A channel is a corridor, not a box. Closing the ends with cross-rail
    /// connectors made it read as a rectangle — the two tools became the same
    /// mark on the chart. Nothing the object is made of may join one rail to
    /// the other.
    #[test]
    fn a_channel_is_never_closed_at_its_ends() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 160.0),
            egui::pos2(100.0, 200.0),
        ];
        let channel = Channel::new(chart, &points, ChannelPayload::default()).expect("three anchors");
        let baseline_direction = (points[1] - points[0]).normalized();
        for (start, end) in channel.segments() {
            let direction = (end - start).normalized();
            let along = direction.dot(baseline_direction).abs();
            assert!(
                (along - 1.0).abs() < 1e-3,
                "every segment runs along the channel, never across it: {start:?} -> {end:?}"
            );
        }
    }

    /// Three strokes with the middle line, two without — and never a fourth.
    #[test]
    fn a_channel_is_two_rails_plus_the_middle_line() {
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 160.0),
            egui::pos2(100.0, 200.0),
        ];
        assert_eq!(
            Channel::new(chart, &points, ChannelPayload::default())
                .expect("anchors")
                .segments()
                .count(),
            3
        );
        assert_eq!(
            Channel::new(
                chart,
                &points,
                ChannelPayload {
                    midline: false,
                    ..ChannelPayload::default()
                }
            )
            .expect("anchors")
            .segments()
            .count(),
            2
        );
    }

    /// The session's request: the width handle is not a lone dot off in a
    /// corner. There is one per rail, each at the centre of its own rail, so
    /// the trader can widen the channel upward *or* downward.
    #[test]
    fn a_channel_is_grabbed_by_its_two_ends_and_the_centre_of_each_rail() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 100.0),
            egui::pos2(180.0, 160.0),
        ];
        let handles = channel_handles(&points).expect("three anchors");
        assert_eq!(handles.len(), 4);
        assert_eq!(handles[0], points[0]);
        assert_eq!(handles[1], points[1]);
        assert_eq!(handles[HANDLE_NEAR_CENTRE], egui::pos2(200.0, 100.0));
        assert_eq!(handles[HANDLE_FAR_CENTRE], egui::pos2(200.0, 160.0));
        assert!(
            !handles.contains(&points[2]),
            "the raw width anchor is not a grab point — that corner dot is what this replaces"
        );
    }

    /// Extension is a view affordance. The handles stay on the span the
    /// trader anchored, so widening never means chasing a ring to the edge of
    /// the chart.
    #[test]
    fn the_rail_handles_ignore_extension() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 100.0),
            egui::pos2(100.0, 160.0),
        ];
        let extended = ChannelPayload {
            extend_left: true,
            extend_right: true,
            ..ChannelPayload::default()
        };
        let chart = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let (start, end) = baseline(chart, &points, extended.extend());
        assert!(start.x < points[0].x && end.x > points[1].x, "it extends");
        let handles = channel_handles(&points).expect("three anchors");
        assert_eq!(handles[HANDLE_NEAR_CENTRE], egui::pos2(200.0, 100.0));
    }

    /// Dragging the far rail widens the channel from that edge alone: the
    /// trend line the trader drew stays exactly where they drew it.
    #[test]
    fn widening_by_the_far_rail_leaves_the_trend_line_alone() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 100.0),
            egui::pos2(100.0, 160.0),
        ];
        let moved = channel_drag(&points, HANDLE_FAR_CENTRE, egui::pos2(240.0, 200.0))
            .expect("the far rail moves");
        assert_eq!(moved[0], points[0]);
        assert_eq!(moved[1], points[1]);
        assert_eq!(
            channel_offset(&moved),
            egui::vec2(0.0, 100.0),
            "only the component across the baseline counts, so sliding along it changes no width"
        );
    }

    /// The other half of the request: grabbing the *near* rail grows the
    /// channel on that side, and the far rail does not budge.
    #[test]
    fn widening_by_the_near_rail_pins_the_far_one() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 100.0),
            egui::pos2(100.0, 160.0),
        ];
        let far_before = points[0] + channel_offset(&points);
        let moved = channel_drag(&points, HANDLE_NEAR_CENTRE, egui::pos2(200.0, 60.0))
            .expect("the near rail moves");
        assert_eq!(moved[0], egui::pos2(100.0, 60.0));
        assert_eq!(moved[1], egui::pos2(300.0, 60.0));
        assert_eq!(
            moved[0] + channel_offset(&moved),
            far_before,
            "the far rail is pinned: the corridor grew, it did not slide"
        );
    }

    /// A rail drag is a width gesture, never a rotation: sliding along the
    /// rails leaves the corridor exactly where it was.
    ///
    /// The *corridor* — not necessarily the stored anchors. Only the
    /// component of the width anchor across the baseline is geometry, so a
    /// rail drag is free to rewrite it to the foot of the perpendicular. What
    /// the trader may never see move is the two rails.
    #[test]
    fn a_rail_drag_never_re_angles_the_channel() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 160.0),
            egui::pos2(100.0, 200.0),
        ];
        let rails = |points: &[egui::Pos2]| {
            (points[0], points[1], points[0] + channel_offset(points))
        };
        let (near_start, near_end, far) = rails(&points);
        let along = (points[1] - points[0]).normalized() * 50.0;
        let handles = channel_handles(&points).expect("three anchors");
        for handle in [HANDLE_NEAR_CENTRE, HANDLE_FAR_CENTRE] {
            // From where the handle *is*: a slide is a gesture that starts on
            // the ring under the pointer.
            let moved = channel_drag(&points, handle, handles[handle] + along)
                .expect("a rail moves");
            let (moved_start, moved_end, moved_far) = rails(&moved);
            for (before, after) in [
                (near_start, moved_start),
                (near_end, moved_end),
                (far, moved_far),
            ] {
                assert!(
                    (after - before).length() < 0.05,
                    "sliding along the rails is not a gesture: {before:?} -> {after:?}"
                );
            }
        }
    }

    /// A width drag changes the width, and says so honestly in the numbers:
    /// the Coordinates tab shows the width anchor by bar and price, so the
    /// bar it sits on must survive a gesture that never moved it sideways.
    #[test]
    fn a_width_drag_leaves_the_width_anchors_bar_alone() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 160.0),
            egui::pos2(220.0, 220.0),
        ];
        let handles = channel_handles(&points).expect("three anchors");
        for handle in [HANDLE_NEAR_CENTRE, HANDLE_FAR_CENTRE] {
            let moved = channel_drag(&points, handle, handles[handle] + egui::vec2(0.0, 40.0))
                .expect("a rail moves");
            assert!(
                (moved[2].x - points[2].x).abs() < 1e-3,
                "handle {handle} moved the width anchor's bar: {:?} -> {:?}",
                points[2],
                moved[2]
            );
        }
    }

    /// A channel whose trend line is vertical — both ends on one bar. There
    /// is no bar to preserve then, so the width anchor simply lands on the
    /// rail, and the geometry must still come out as the trader dragged it.
    #[test]
    fn a_vertical_channel_still_widens_from_either_rail() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(100.0, 300.0),
            egui::pos2(160.0, 200.0),
        ];
        let handles = channel_handles(&points).expect("three anchors");
        assert_eq!(handles[HANDLE_NEAR_CENTRE], egui::pos2(100.0, 200.0));
        assert_eq!(handles[HANDLE_FAR_CENTRE], egui::pos2(160.0, 200.0));

        let widened = channel_drag(&points, HANDLE_FAR_CENTRE, egui::pos2(200.0, 240.0))
            .expect("the far rail moves");
        assert_eq!(widened[0], points[0], "the trend line is untouched");
        assert_eq!(channel_offset(&widened), egui::vec2(100.0, 0.0));

        let far_before = points[0] + channel_offset(&points);
        let grown = channel_drag(&points, HANDLE_NEAR_CENTRE, egui::pos2(60.0, 200.0))
            .expect("the near rail moves");
        assert_eq!(grown[0], egui::pos2(60.0, 100.0));
        assert_eq!(
            grown[0] + channel_offset(&grown),
            far_before,
            "the far rail is pinned"
        );
    }

    /// The end handles are still the anchors they always were.
    #[test]
    fn the_end_handles_move_their_own_anchor_and_nothing_else() {
        let points = [
            egui::pos2(100.0, 100.0),
            egui::pos2(300.0, 100.0),
            egui::pos2(100.0, 160.0),
        ];
        let moved = channel_drag(&points, 0, egui::pos2(120.0, 90.0)).expect("the start moves");
        assert_eq!(moved[0], egui::pos2(120.0, 90.0));
        assert_eq!(moved[1], points[1]);
        assert_eq!(moved[2], points[2]);
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
