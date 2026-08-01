//! Modular user-authored chart drawings.
//!
//! Each drawing tool implements [`DrawingToolImpl`] in its own file. The
//! registry macro is the only docking point: add a module name there and the
//! toolbox, placement state, renderer and hit-testing all see the new tool.
//! Market data remains immutable and the deterministic engine never learns
//! about UI marks.

use std::fmt;

use eframe::egui;

use crate::theme;

pub const DEFAULT_DRAWING_COLOR: egui::Color32 = egui::Color32::from_rgb(138, 180, 248);
pub const DEFAULT_DRAWING_WIDTH_PX: f32 = 1.5;
pub const DEFAULT_DRAWING_FILL_ALPHA: u8 = 24;
pub const MIN_DRAWING_WIDTH_PX: f32 = 0.5;
pub const MAX_DRAWING_WIDTH_PX: f32 = 6.0;
pub const MAX_DRAWING_FILL_ALPHA: u8 = 160;
/// Undo history depth. One entry per committed command (a whole drag or
/// slider gesture is one command), so this bounds memory without cutting a
/// working session short.
const UNDO_HISTORY_LIMIT: usize = 64;
const SELECTED_ANCHOR_RADIUS_PX: f32 = 4.0;
const SELECTED_ANCHOR_FILL: egui::Color32 = egui::Color32::WHITE;
const SELECTED_ANCHOR_RING_WIDTH_PX: f32 = 1.5;
/// Selection never repaints the object white: it keeps the configured colour
/// and paints this soft halo underneath instead, plus white anchor handles.
/// Premultiplied ~16% white, matching the UX spec's `.halo` treatment.
const SELECTION_HALO_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(40, 40, 40, 40);
/// How much wider than the object's own stroke the halo pass paints.
const SELECTION_HALO_EXTRA_WIDTH_PX: f32 = 3.5;
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

/// The implementation port every drawing plugs into. Selection visuals (halo
/// and anchor handles) are common chrome painted by the wrapper, so a tool
/// only ever paints its own geometry in the style it is given.
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

    /// Paint the object. Selection adds a halo *under* the geometry and, when
    /// `show_handles` (not locked), white anchor handles on top — the object's
    /// configured colour keeps carrying meaning either way.
    pub fn paint(
        self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        selected: bool,
        show_handles: bool,
    ) {
        if selected {
            let halo = DrawingStyle {
                color: SELECTION_HALO_COLOR,
                width_px: style.width_px + SELECTION_HALO_EXTRA_WIDTH_PX,
                fill_alpha: 0,
            };
            self.0.paint(painter, chart_rect, halo, points);
        }
        self.0.paint(painter, chart_rect, style, points);
        if selected && show_handles {
            let ring = egui::Stroke::new(SELECTED_ANCHOR_RING_WIDTH_PX, theme::ACCENT);
            for point in points {
                painter.circle_filled(*point, SELECTED_ANCHOR_RADIUS_PX, SELECTED_ANCHOR_FILL);
                painter.circle_stroke(*point, SELECTED_ANCHOR_RADIUS_PX, ring);
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
    /// A locked drawing keeps rejecting geometry edits and unforced deletes;
    /// its style stays editable.
    pub locked: bool,
    /// A hidden drawing neither paints nor hit-tests, and stays recoverable.
    pub hidden: bool,
}

/// What a delete request did. Locked objects demand an explicit `force`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    NeedsConfirmation,
    NothingSelected,
}

/// One undo step: the whole collection plus the global-hide layer. Selection,
/// viewport and inspector state deliberately stay out, so undo never yanks
/// the camera or the UI around.
#[derive(Debug, Clone, PartialEq)]
struct UndoEntry {
    items: Vec<Drawing>,
    all_hidden: bool,
}

#[derive(Debug, Default)]
pub struct Drawings {
    items: Vec<Drawing>,
    draft: Option<Drawing>,
    selected: Option<usize>,
    /// Global hide layer. Independent from each drawing's own eye, so
    /// "show all" restores exactly the per-object visibility it found.
    all_hidden: bool,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    /// Snapshot taken when a pointer gesture starts; committed (as one undo
    /// entry) on release, so a whole drag coalesces into one command.
    gesture_baseline: Option<UndoEntry>,
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

    #[must_use]
    pub fn all_hidden(&self) -> bool {
        self.all_hidden
    }

    /// Whether the object at `index` paints and hit-tests this frame.
    #[must_use]
    pub fn is_visible(&self, index: usize) -> bool {
        !self.all_hidden && self.items.get(index).is_some_and(|item| !item.hidden)
    }

    fn snapshot(&self) -> UndoEntry {
        UndoEntry {
            items: self.items.clone(),
            all_hidden: self.all_hidden,
        }
    }

    /// Push `before` as one undo step if the store actually changed since.
    fn record(&mut self, before: UndoEntry) {
        if before == self.snapshot() {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > UNDO_HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Start coalescing: the next [`Self::commit_gesture`] records everything
    /// mutated in between as a single undo entry. Idempotent within a gesture.
    pub fn begin_gesture(&mut self) {
        if self.gesture_baseline.is_none() {
            self.gesture_baseline = Some(self.snapshot());
        }
    }

    /// End coalescing. A gesture that changed nothing records nothing.
    pub fn commit_gesture(&mut self) {
        if let Some(baseline) = self.gesture_baseline.take() {
            self.record(baseline);
        }
    }

    /// Record an already-applied edit to one object, given its pre-edit
    /// state. Used by the inspector to coalesce slider/color gestures.
    pub fn record_edit_of(&mut self, index: usize, before_drawing: Drawing) {
        let mut before = self.snapshot();
        let Some(slot) = before.items.get_mut(index) else {
            return;
        };
        *slot = before_drawing;
        self.record(before);
    }

    #[cfg(test)]
    pub(crate) fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    fn restore(&mut self, entry: UndoEntry) {
        self.items = entry.items;
        self.all_hidden = entry.all_hidden;
        self.draft = None;
        self.selected = self.selected.filter(|&index| index < self.items.len());
    }

    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(entry);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(entry);
        true
    }

    /// Add a placement anchor. `true` means the object became complete;
    /// completing an object records one undo entry for the whole creation.
    pub fn place(&mut self, tool: DrawingTool, point: ChartPoint) -> bool {
        if self.draft.as_ref().is_none_or(|draft| draft.tool != tool) {
            self.draft = Some(Drawing {
                tool,
                points: Vec::with_capacity(tool.required_points()),
                style: DrawingStyle::default(),
                locked: false,
                hidden: false,
            });
        }
        let draft = self.draft.as_mut().expect("draft was installed above");
        draft.points.push(point);
        if draft.points.len() == tool.required_points() {
            let before = self.snapshot();
            self.items
                .push(self.draft.take().expect("draft has points"));
            self.selected = Some(self.items.len() - 1);
            self.record(before);
            true
        } else {
            false
        }
    }

    pub fn cancel_draft(&mut self) {
        self.draft = None;
    }

    /// Honest reset: the anchors cannot survive a source or bar-spec change,
    /// and neither can a history that would resurrect them onto different
    /// market data.
    pub fn clear(&mut self) {
        self.items.clear();
        self.draft = None;
        self.selected = None;
        self.all_hidden = false;
        self.undo.clear();
        self.redo.clear();
        self.gesture_baseline = None;
    }

    pub fn shift_bars(&mut self, added: usize) {
        let delta = added as f32;
        let shift = |items: &mut Vec<Drawing>| {
            for drawing in items {
                for point in &mut drawing.points {
                    point.bar += delta;
                }
            }
        };
        shift(&mut self.items);
        if let Some(draft) = &mut self.draft {
            for point in &mut draft.points {
                point.bar += delta;
            }
        }
        // History snapshots hold the same bar-index coordinates, so a prepend
        // shifts them too — undoing later must not re-anchor objects to bars
        // that moved underneath them.
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            shift(&mut entry.items);
        }
        if let Some(baseline) = &mut self.gesture_baseline {
            shift(&mut baseline.items);
        }
    }

    /// One delete command for every trigger (button, manager, keyboard).
    /// A locked object is never deleted without `force`.
    pub fn delete_selected(&mut self, force: bool) -> DeleteOutcome {
        let Some(index) = self.selected.filter(|&index| index < self.items.len()) else {
            return DeleteOutcome::NothingSelected;
        };
        if self.items[index].locked && !force {
            return DeleteOutcome::NeedsConfirmation;
        }
        let before = self.snapshot();
        self.items.remove(index);
        self.selected = None;
        self.record(before);
        DeleteOutcome::Deleted
    }

    pub fn set_selected_locked(&mut self, locked: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.selected_mut() {
            drawing.locked = locked;
            self.record(before);
        }
    }

    pub fn set_selected_hidden(&mut self, hidden: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.selected_mut() {
            drawing.hidden = hidden;
            self.record(before);
        }
    }

    pub fn set_all_hidden(&mut self, hidden: bool) {
        let before = self.snapshot();
        self.all_hidden = hidden;
        self.record(before);
    }

    /// Whether every drawing is individually locked (used by the toolbox's
    /// lock-all toggle). An empty collection is not "all locked".
    #[must_use]
    pub fn all_locked(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|item| item.locked)
    }

    /// Reversible bulk protection: locks (or unlocks) every drawing as one
    /// undo entry. Never deletes anything.
    pub fn set_all_locked(&mut self, locked: bool) {
        let before = self.snapshot();
        for item in &mut self.items {
            item.locked = locked;
        }
        self.record(before);
    }

    /// Rigid translation of the selected object. Locked geometry stays put.
    pub fn translate_selected(&mut self, delta_bar: f32, delta_price: f64) {
        let Some(drawing) = self.selected_mut() else {
            return;
        };
        if drawing.locked {
            return;
        }
        for point in &mut drawing.points {
            point.bar += delta_bar;
            point.price += delta_price;
        }
    }

    /// Move one anchor of one object. Locked geometry stays put.
    pub fn move_anchor(
        &mut self,
        drawing_index: usize,
        point_index: usize,
        point: ChartPoint,
    ) -> bool {
        let Some(drawing) = self.items.get_mut(drawing_index) else {
            return false;
        };
        if drawing.locked {
            return false;
        }
        let Some(anchor) = drawing.points.get_mut(point_index) else {
            return false;
        };
        *anchor = point;
        true
    }
}

pub(super) fn drawing_stroke(style: DrawingStyle) -> egui::Stroke {
    egui::Stroke::new(style.width_px, style.color)
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

        assert_eq!(drawings.delete_selected(false), DeleteOutcome::Deleted);

        assert!(drawings.items().is_empty());
        assert_eq!(drawings.selected(), None);
    }

    #[test]
    fn a_locked_drawing_ignores_geometry_edits_until_unlocked() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 2.0,
                price: 100.0,
            },
        );
        drawings.set_selected_locked(true);

        drawings.translate_selected(3.0, 5.0);
        assert!(!drawings.move_anchor(
            0,
            0,
            ChartPoint {
                bar: 9.0,
                price: 50.0,
            }
        ));
        assert_eq!(
            drawings.items()[0].points[0],
            ChartPoint {
                bar: 2.0,
                price: 100.0,
            },
            "locked geometry must not move"
        );

        drawings.set_selected_locked(false);
        drawings.translate_selected(3.0, 5.0);
        assert_eq!(
            drawings.items()[0].points[0],
            ChartPoint {
                bar: 5.0,
                price: 105.0,
            }
        );
    }

    #[test]
    fn deleting_a_locked_drawing_requires_explicit_force() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );
        drawings.set_selected_locked(true);

        assert_eq!(
            drawings.delete_selected(false),
            DeleteOutcome::NeedsConfirmation
        );
        assert_eq!(drawings.items().len(), 1, "unforced delete must not land");

        assert_eq!(drawings.delete_selected(true), DeleteOutcome::Deleted);
        assert!(drawings.items().is_empty());

        assert!(drawings.undo(), "a forced delete is still undoable");
        assert_eq!(drawings.items().len(), 1);
        assert!(drawings.items()[0].locked, "undo restores the lock too");
    }

    #[test]
    fn creating_dragging_and_deleting_are_one_undo_entry_each() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        // Creation: two anchors, one entry.
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
        assert_eq!(drawings.undo_depth(), 1);

        // One drag over many frames: one entry.
        drawings.begin_gesture();
        for _ in 0..5 {
            drawings.translate_selected(0.5, 1.0);
        }
        drawings.commit_gesture();
        assert_eq!(drawings.undo_depth(), 2);

        // A gesture that changes nothing records nothing.
        drawings.begin_gesture();
        drawings.commit_gesture();
        assert_eq!(drawings.undo_depth(), 2);

        drawings.delete_selected(false);
        assert_eq!(drawings.undo_depth(), 3);

        assert!(drawings.undo(), "undo the delete");
        assert_eq!(drawings.items().len(), 1);
        assert!(drawings.undo(), "undo the drag");
        assert_eq!(
            drawings.items()[0].points[0],
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            }
        );
        assert!(drawings.undo(), "undo the creation");
        assert!(drawings.items().is_empty());
        assert!(!drawings.undo(), "history is exhausted");
    }

    #[test]
    fn redo_replays_an_undone_edit_until_a_new_command_clears_it() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );
        drawings.undo();
        assert!(drawings.items().is_empty());
        assert!(drawings.redo());
        assert_eq!(drawings.items().len(), 1);

        drawings.undo();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 4.0,
                price: 90.0,
            },
        );
        assert!(!drawings.redo(), "a new command clears the redo stack");
    }

    #[test]
    fn hide_all_is_a_layer_over_each_drawings_own_eye() {
        let mut drawings = Drawings::default();
        for price in [100.0, 105.0] {
            drawings.place(tool("horizontal-line"), ChartPoint { bar: 1.0, price });
        }
        drawings.select(Some(0));
        drawings.set_selected_hidden(true);
        assert!(!drawings.is_visible(0));
        assert!(drawings.is_visible(1));

        drawings.set_all_hidden(true);
        assert!(!drawings.is_visible(0));
        assert!(!drawings.is_visible(1));

        drawings.set_all_hidden(false);
        assert!(
            !drawings.is_visible(0),
            "show-all must preserve the individual eye"
        );
        assert!(drawings.is_visible(1));

        drawings.select(Some(0));
        drawings.set_selected_hidden(false);
        assert!(drawings.is_visible(0));
    }

    #[test]
    fn lock_all_is_one_reversible_undo_entry_and_never_deletes() {
        let mut drawings = Drawings::default();
        for price in [100.0, 105.0] {
            drawings.place(tool("horizontal-line"), ChartPoint { bar: 1.0, price });
        }
        let depth_before = drawings.undo_depth();

        drawings.set_all_locked(true);
        assert!(drawings.all_locked());
        assert_eq!(drawings.items().len(), 2);
        assert_eq!(drawings.undo_depth(), depth_before + 1);

        assert!(drawings.undo());
        assert!(!drawings.all_locked(), "undo releases the bulk lock");
        assert_eq!(drawings.items().len(), 2);
    }

    #[test]
    fn undo_snapshots_shift_with_prepended_history() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 2.0,
                price: 100.0,
            },
        );
        drawings.begin_gesture();
        drawings.translate_selected(1.0, 0.0);
        drawings.commit_gesture();

        drawings.shift_bars(3);
        assert_eq!(drawings.items()[0].points[0].bar, 6.0);

        drawings.undo();
        assert_eq!(
            drawings.items()[0].points[0].bar,
            5.0,
            "the undone position must sit on the shifted bars, not the stale ones"
        );
    }

    #[test]
    fn selection_preserves_the_drawing_color_and_adds_white_handles() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let color = egui::Color32::from_rgb(0xFF, 0x9F, 0x43);
        let style = DrawingStyle {
            color,
            ..DrawingStyle::default()
        };
        let line = tool("horizontal-line");
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(chart),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("selection-test"),
            ));
            line.paint(
                &painter,
                chart,
                style,
                &[egui::pos2(100.0, 120.0)],
                true,
                true,
            );
        });

        let mut kept_color = false;
        let mut white_handle = false;
        for clipped in &output.shapes {
            match &clipped.shape {
                egui::Shape::LineSegment { stroke, .. } => {
                    kept_color |= stroke.color == egui::epaint::ColorMode::Solid(color);
                }
                egui::Shape::Circle(circle) => {
                    white_handle |= circle.fill == SELECTED_ANCHOR_FILL;
                }
                _ => {}
            }
        }
        assert!(
            kept_color,
            "selection must keep painting the configured colour"
        );
        assert!(white_handle, "selection must add white anchor handles");
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
                drawing_tool.paint(
                    &painter,
                    chart,
                    DrawingStyle::default(),
                    &points,
                    false,
                    false,
                );
            });
            assert!(
                !output.shapes.is_empty(),
                "{} rendered no geometry",
                drawing_tool.id()
            );
        }
    }
}
