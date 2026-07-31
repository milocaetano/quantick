//! Modular user-authored chart drawings.
//!
//! Each drawing tool implements [`DrawingToolImpl`] in its own file. The
//! registry macro is the only docking point: add a module name there and the
//! toolbox, placement state, renderer and hit-testing all see the new tool.
//! Market data remains immutable and the deterministic engine never learns
//! about UI marks.

use std::fmt;

use eframe::egui;

pub const DEFAULT_DRAWING_COLOR: egui::Color32 = egui::Color32::from_rgb(138, 180, 248);
pub const DEFAULT_DRAWING_WIDTH_PX: f32 = 1.5;
pub const DEFAULT_DRAWING_FILL_ALPHA: u8 = 24;
pub const MIN_DRAWING_WIDTH_PX: f32 = 0.5;
pub const MAX_DRAWING_WIDTH_PX: f32 = 6.0;
pub const MAX_DRAWING_FILL_ALPHA: u8 = 160;
const SELECTED_ANCHOR_RADIUS_PX: f32 = 4.0;
const SELECTED_ANCHOR_FILL: egui::Color32 = egui::Color32::WHITE;
pub(super) const FIB_LABEL_OFFSET_PX: f32 = 3.0;
pub(super) const FIB_LABEL_SIZE_PX: f32 = 10.0;

pub(super) struct FibGeometry {
    pub left: f32,
    pub right: f32,
    pub first_y: f32,
    pub second_y: f32,
    pub origin_y: f32,
}

#[derive(Clone, Copy)]
pub(super) struct FibLevel {
    ratio: f64,
    label: &'static str,
}

impl FibLevel {
    pub(super) const fn new(ratio: f64, label: &'static str) -> Self {
        Self { ratio, label }
    }
}

/// The implementation port every drawing plugs into.
trait DrawingToolImpl: Sync {
    fn id(&self) -> &'static str;
    fn settings_title(&self) -> &'static str;
    fn icon(&self) -> &'static str;
    fn hover_text(&self) -> &'static str;
    fn required_points(&self) -> usize;
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        selected: bool,
    );
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
    ) -> bool;
    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2);
}

/// A cheap, copyable reference to one registered implementation.
#[derive(Clone, Copy)]
pub struct DrawingTool(&'static dyn DrawingToolImpl);

impl DrawingTool {
    #[must_use]
    pub fn id(self) -> &'static str {
        self.0.id()
    }

    #[must_use]
    pub fn settings_title(self) -> &'static str {
        self.0.settings_title()
    }

    #[must_use]
    pub fn icon(self) -> &'static str {
        self.0.icon()
    }

    #[must_use]
    pub fn hover_text(self) -> &'static str {
        self.0.hover_text()
    }

    #[must_use]
    pub fn required_points(self) -> usize {
        self.0.required_points()
    }

    pub fn paint(
        self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        selected: bool,
    ) {
        self.0.paint(painter, chart_rect, style, points, selected);
        if selected {
            let stroke = drawing_stroke(style, true);
            for point in points {
                painter.circle_filled(*point, SELECTED_ANCHOR_RADIUS_PX, SELECTED_ANCHOR_FILL);
                painter.circle_stroke(*point, SELECTED_ANCHOR_RADIUS_PX, stroke);
            }
        }
    }

    #[must_use]
    pub fn hit_test(
        self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
    ) -> bool {
        points
            .iter()
            .any(|point| point.distance_sq(position) <= radius_px * radius_px)
            || self.0.hit_test(chart_rect, points, position, radius_px)
    }

    #[cfg(test)]
    fn test_geometry(self) -> (Vec<egui::Pos2>, egui::Pos2) {
        self.0.test_geometry()
    }
}

impl PartialEq for DrawingTool {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for DrawingTool {}

impl fmt::Debug for DrawingTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DrawingTool")
            .field(&self.id())
            .finish()
    }
}

macro_rules! register_drawing_tools {
    ($($module:ident),+ $(,)?) => {
        $(mod $module;)+
        pub const DRAWING_TOOLS: [DrawingTool; [$(stringify!($module)),+].len()] = [
            $(DrawingTool(&$module::TOOL)),+
        ];
    };
}

// The extension port: a new tool is one implementation file plus one name here.
register_drawing_tools!(
    horizontal_line,
    rectangle,
    parallel_channel,
    fib_retracement,
    fib_extension,
);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartPoint {
    pub bar: f32,
    pub price: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawingStyle {
    pub color: egui::Color32,
    pub width_px: f32,
    pub fill_alpha: u8,
}

impl Default for DrawingStyle {
    fn default() -> Self {
        Self {
            color: DEFAULT_DRAWING_COLOR,
            width_px: DEFAULT_DRAWING_WIDTH_PX,
            fill_alpha: DEFAULT_DRAWING_FILL_ALPHA,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub tool: DrawingTool,
    pub points: Vec<ChartPoint>,
    pub style: DrawingStyle,
}

#[derive(Debug, Default)]
pub struct Drawings {
    items: Vec<Drawing>,
    draft: Option<Drawing>,
    selected: Option<usize>,
}

impl Drawings {
    #[must_use]
    pub fn items(&self) -> &[Drawing] {
        &self.items
    }

    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, selected: Option<usize>) {
        self.selected = selected.filter(|&index| index < self.items.len());
    }

    #[must_use]
    pub fn selected_mut(&mut self) -> Option<&mut Drawing> {
        self.selected.and_then(|index| self.items.get_mut(index))
    }

    #[must_use]
    pub fn draft(&self) -> Option<&Drawing> {
        self.draft.as_ref()
    }

    #[must_use]
    pub fn draft_len(&self) -> usize {
        self.draft.as_ref().map_or(0, |draft| draft.points.len())
    }

    /// Add a placement anchor. `true` means the object became complete.
    pub fn place(&mut self, tool: DrawingTool, point: ChartPoint) -> bool {
        if self.draft.as_ref().is_none_or(|draft| draft.tool != tool) {
            self.draft = Some(Drawing {
                tool,
                points: Vec::with_capacity(tool.required_points()),
                style: DrawingStyle::default(),
            });
        }
        let draft = self.draft.as_mut().expect("draft was installed above");
        draft.points.push(point);
        if draft.points.len() == tool.required_points() {
            self.items
                .push(self.draft.take().expect("draft has points"));
            self.selected = Some(self.items.len() - 1);
            true
        } else {
            false
        }
    }

    pub fn cancel_draft(&mut self) {
        self.draft = None;
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.draft = None;
        self.selected = None;
    }

    pub fn shift_bars(&mut self, added: usize) {
        let delta = added as f32;
        for drawing in &mut self.items {
            for point in &mut drawing.points {
                point.bar += delta;
            }
        }
        if let Some(draft) = &mut self.draft {
            for point in &mut draft.points {
                point.bar += delta;
            }
        }
    }

    pub fn delete_selected(&mut self) {
        if let Some(index) = self.selected.take()
            && index < self.items.len()
        {
            self.items.remove(index);
        }
    }

    pub fn translate_selected(&mut self, delta_bar: f32, delta_price: f64) {
        let Some(drawing) = self.selected_mut() else {
            return;
        };
        for point in &mut drawing.points {
            point.bar += delta_bar;
            point.price += delta_price;
        }
    }

    pub fn move_anchor(
        &mut self,
        drawing_index: usize,
        point_index: usize,
        point: ChartPoint,
    ) -> bool {
        let Some(anchor) = self
            .items
            .get_mut(drawing_index)
            .and_then(|drawing| drawing.points.get_mut(point_index))
        else {
            return false;
        };
        *anchor = point;
        true
    }
}

pub(super) fn drawing_stroke(style: DrawingStyle, selected: bool) -> egui::Stroke {
    egui::Stroke::new(
        style.width_px,
        if selected {
            egui::Color32::WHITE
        } else {
            style.color
        },
    )
}

pub(super) fn drawing_fill(style: DrawingStyle) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        style.color.r(),
        style.color.g(),
        style.color.b(),
        style.fill_alpha,
    )
}

pub(super) fn distance_to_segment(position: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_sq = segment.length_sq();
    if length_sq <= f32::EPSILON {
        return position.distance(start);
    }
    let projection = ((position - start).dot(segment) / length_sq).clamp(0.0, 1.0);
    position.distance(start + segment * projection)
}

pub(super) fn paint_fib_levels(
    painter: &egui::Painter,
    geometry: FibGeometry,
    levels: &[FibLevel],
    stroke: egui::Stroke,
    color: egui::Color32,
) {
    for level in levels {
        let y = geometry.origin_y + (geometry.second_y - geometry.first_y) * level.ratio as f32;
        painter.line_segment(
            [egui::pos2(geometry.left, y), egui::pos2(geometry.right, y)],
            stroke,
        );
        painter.text(
            egui::pos2(geometry.left + FIB_LABEL_OFFSET_PX, y),
            egui::Align2::LEFT_BOTTOM,
            level.label,
            egui::FontId::monospace(FIB_LABEL_SIZE_PX),
            color,
        );
    }
}

pub(super) fn hit_fib_levels(
    position: egui::Pos2,
    points: &[egui::Pos2],
    levels: &[FibLevel],
    origin_y: f32,
    radius_px: f32,
) -> bool {
    let left = points
        .iter()
        .map(|point| point.x)
        .fold(f32::INFINITY, f32::min);
    let right = points
        .iter()
        .map(|point| point.x)
        .fold(f32::NEG_INFINITY, f32::max);
    position.x >= left - radius_px
        && position.x <= right + radius_px
        && levels.iter().any(|level| {
            let y = origin_y + (points[1].y - points[0].y) * level.ratio as f32;
            (position.y - y).abs() <= radius_px
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &str) -> DrawingTool {
        DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == id)
            .expect("registered test tool")
    }

    #[test]
    fn every_registered_tool_has_metadata_and_a_valid_point_count() {
        let mut ids = Vec::with_capacity(DRAWING_TOOLS.len());
        for tool in DRAWING_TOOLS {
            assert!(!tool.id().is_empty());
            assert!(!ids.contains(&tool.id()), "duplicate tool id {}", tool.id());
            ids.push(tool.id());
            assert!(!tool.icon().is_empty());
            assert!(!tool.settings_title().is_empty());
            assert!(!tool.hover_text().is_empty());
            assert!(tool.required_points() > 0);
        }
    }

    #[test]
    fn horizontal_line_completes_with_one_point() {
        let mut drawings = Drawings::default();
        assert!(drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 3.0,
                price: 100.0
            }
        ));
    }

    #[test]
    fn channel_needs_three_points() {
        let mut drawings = Drawings::default();
        let channel = tool("parallel-channel");
        for bar in [1.0, 2.0] {
            assert!(!drawings.place(channel, ChartPoint { bar, price: 1.0 }));
        }
        assert!(drawings.place(
            channel,
            ChartPoint {
                bar: 3.0,
                price: 1.0
            }
        ));
    }

    #[test]
    fn moving_a_selected_drawing_preserves_its_shape() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        drawings.place(
            rectangle,
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );
        drawings.place(
            rectangle,
            ChartPoint {
                bar: 3.0,
                price: 110.0,
            },
        );
        drawings.translate_selected(2.0, -5.0);
        assert_eq!(
            drawings.items()[0].points,
            [
                ChartPoint {
                    bar: 3.0,
                    price: 95.0
                },
                ChartPoint {
                    bar: 5.0,
                    price: 105.0
                }
            ]
        );
    }

    #[test]
    fn moving_one_anchor_does_not_move_the_other_points() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        drawings.place(
            rectangle,
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );
        drawings.place(
            rectangle,
            ChartPoint {
                bar: 3.0,
                price: 110.0,
            },
        );
        let opposite = drawings.items()[0].points[1];
        let replacement = ChartPoint {
            bar: 0.5,
            price: 95.0,
        };

        assert!(drawings.move_anchor(0, 0, replacement));

        assert_eq!(drawings.items()[0].points[0], replacement);
        assert_eq!(drawings.items()[0].points[1], opposite);
        assert!(!drawings.move_anchor(5, 0, replacement));
        assert!(!drawings.move_anchor(0, 5, replacement));
    }

    #[test]
    fn deleting_the_selected_drawing_clears_both_object_and_selection() {
        let mut drawings = Drawings::default();
        let line = tool("horizontal-line");
        assert!(drawings.place(
            line,
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            }
        ));
        assert_eq!(drawings.selected(), Some(0));

        drawings.delete_selected();

        assert!(drawings.items().is_empty());
        assert_eq!(drawings.selected(), None);
    }

    #[test]
    fn clearing_drawings_also_discards_an_unfinished_draft() {
        let mut drawings = Drawings::default();
        assert!(!drawings.place(
            tool("rectangle"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            }
        ));
        drawings.clear();
        assert!(drawings.items().is_empty());
        assert!(drawings.draft().is_none());
        assert_eq!(drawings.selected(), None);
    }

    #[test]
    fn prepending_history_shifts_completed_and_draft_bar_anchors() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 2.5,
                price: 100.0,
            },
        );
        drawings.place(
            tool("rectangle"),
            ChartPoint {
                bar: 4.0,
                price: 101.0,
            },
        );

        drawings.shift_bars(3);

        assert_eq!(drawings.items()[0].points[0].bar, 5.5);
        assert_eq!(
            drawings.draft().expect("rectangle draft").points[0].bar,
            7.0
        );
    }

    #[test]
    fn horizontal_line_is_selectable_from_anywhere_on_its_stroke() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        assert!(tool("horizontal-line").hit_test(
            chart,
            &[egui::pos2(100.0, 120.0)],
            egui::pos2(450.0, 123.0),
            5.0
        ));
    }

    #[test]
    fn every_registered_tool_paints_and_hits_its_finished_geometry() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        for drawing_tool in DRAWING_TOOLS {
            let (points, hit) = drawing_tool.test_geometry();
            assert_eq!(points.len(), drawing_tool.required_points());
            assert!(
                drawing_tool.hit_test(chart, &points, hit, 5.0),
                "{} cannot be selected from its visible geometry",
                drawing_tool.id()
            );

            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(chart),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new(drawing_tool.id()),
                ));
                drawing_tool.paint(&painter, chart, DrawingStyle::default(), &points, false);
            });
            assert!(
                !output.shapes.is_empty(),
                "{} rendered no geometry",
                drawing_tool.id()
            );
        }
    }
}
