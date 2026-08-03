//! Modular user-authored chart drawings.
//!
//! Each drawing tool implements [`DrawingToolImpl`] in its own file. The
//! registry macro is the only docking point: add a module name there and the
//! toolbox, placement state, renderer and hit-testing all see the new tool.
//! Market data remains immutable and the deterministic engine never learns
//! about UI marks.

pub mod action_bar;
pub mod fib;
pub mod presets;

use std::any::Any;
use std::fmt;

use eframe::egui;

use crate::chart::PriceScale;
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

/// Tool-owned state beyond anchors and common style. A property unique to
/// one tool lives in that tool's payload, never in the shared envelope, so
/// the next tool cannot force the model or the central inspector open.
pub trait DrawingPayload: fmt::Debug {
    fn clone_box(&self) -> Box<dyn DrawingPayload>;
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Serialize the payload for a named preset. Coordinates, lock and
    /// visibility never travel with a preset, only the tool-owned config.
    fn export_preset(&self) -> Option<toml::Value> {
        None
    }
    /// Apply a previously exported preset. `false` leaves the payload alone.
    fn import_preset(&mut self, _value: &toml::Value) -> bool {
        false
    }
}

impl Clone for Box<dyn DrawingPayload> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Payload of tools whose whole state is anchors + common style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoPayload;

impl DrawingPayload for NoPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(Self)
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other.as_any().downcast_ref::<Self>().is_some()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Named-preset storage a tool's inspector tab can talk to without knowing
/// where presets live. Presets carry an opaque payload export; the host
/// stores them per tool id, versioned, surviving restarts.
pub trait PresetHost {
    fn custom_preset_names(&self, tool_id: &str) -> Vec<String>;
    fn load_custom_preset(&self, tool_id: &str, name: &str) -> Option<toml::Value>;
    /// `false` means the name exists and `overwrite` was not set — the
    /// caller asks the user before trying again.
    fn save_custom_preset(
        &mut self,
        tool_id: &str,
        name: &str,
        value: toml::Value,
        overwrite: bool,
    ) -> bool;
    fn delete_custom_preset(&mut self, tool_id: &str, name: &str);
    fn default_preset(&self, tool_id: &str) -> Option<String>;
    fn set_default_preset(&mut self, tool_id: &str, name: Option<String>);
}

/// A host with no storage: custom presets are absent, saving reports success
/// and drops the value. For contexts without a store (tests, previews).
#[cfg(test)]
#[derive(Debug, Default)]
pub struct NullPresetHost;

#[cfg(test)]
impl PresetHost for NullPresetHost {
    fn custom_preset_names(&self, _tool_id: &str) -> Vec<String> {
        Vec::new()
    }
    fn load_custom_preset(&self, _tool_id: &str, _name: &str) -> Option<toml::Value> {
        None
    }
    fn save_custom_preset(
        &mut self,
        _tool_id: &str,
        _name: &str,
        _value: toml::Value,
        _overwrite: bool,
    ) -> bool {
        true
    }
    fn delete_custom_preset(&mut self, _tool_id: &str, _name: &str) {}
    fn default_preset(&self, _tool_id: &str) -> Option<String> {
        None
    }
    fn set_default_preset(&mut self, _tool_id: &str, _name: Option<String>) {}
}

/// Everything a tool may need beyond raw screen anchors when painting or
/// hit-testing: its own payload, the chart-space anchors and the price scale
/// that projected them (log-scaled tools compute prices, then project).
#[derive(Clone, Copy)]
pub struct DrawContext<'a> {
    pub payload: &'a dyn DrawingPayload,
    pub anchors: &'a [ChartPoint],
    pub scale: &'a PriceScale,
    /// The object's own style — hit-testing reads it too (an invisible fill
    /// takes no part in the interior hit-test).
    pub style: DrawingStyle,
    pub selected: bool,
    /// True while the wrapper paints the selection halo pass: tools draw
    /// only their stroke geometry then — no fills, no labels.
    pub halo: bool,
}

/// A tool's arming shortcut, declared by the tool itself so the keyboard
/// map never becomes a central match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolShortcut {
    pub key: egui::Key,
    pub shift: bool,
}

/// The implementation port every drawing plugs into. Selection visuals (halo
/// and anchor handles) are common chrome painted by the wrapper, so a tool
/// only ever paints its own geometry in the style it is given. Capability
/// methods drive which inspector sections exist for the tool — an
/// unsupported property is absent, never disabled.
trait DrawingToolImpl: Sync {
    fn id(&self) -> &'static str;
    /// Human name shown in the inspector header and the object manager.
    fn name(&self) -> &'static str;
    fn settings_title(&self) -> &'static str;
    fn icon(&self) -> &'static str;
    fn hover_text(&self) -> &'static str;
    fn required_points(&self) -> usize;
    /// The key that arms this tool from the chart, if it has one.
    fn shortcut(&self) -> Option<ToolShortcut> {
        None
    }
    /// Whether the tool paints an interior that the fill controls affect.
    fn supports_fill(&self) -> bool {
        false
    }
    /// Fresh tool-owned state for a newly placed object.
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(NoPayload)
    }
    /// Title of the tool-owned inspector tab, if the tool brings one.
    fn extra_tab(&self) -> Option<&'static str> {
        None
    }
    /// Draw the tool-owned inspector tab. Returns whether anything was
    /// edited (the caller folds it into the shared undo coalescing).
    fn draw_extra_tab(
        &self,
        _ui: &mut egui::Ui,
        _drawing: &mut Drawing,
        _host: &mut dyn PresetHost,
    ) -> bool {
        false
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    );
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
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
    pub fn name(self) -> &'static str {
        self.0.name()
    }

    #[must_use]
    pub fn settings_title(self) -> &'static str {
        self.0.settings_title()
    }

    #[must_use]
    pub fn supports_fill(self) -> bool {
        self.0.supports_fill()
    }

    #[must_use]
    pub fn default_payload(self) -> Box<dyn DrawingPayload> {
        self.0.default_payload()
    }

    #[must_use]
    pub fn shortcut(self) -> Option<ToolShortcut> {
        self.0.shortcut()
    }

    #[must_use]
    pub fn extra_tab(self) -> Option<&'static str> {
        self.0.extra_tab()
    }

    /// Draw the tool-owned inspector tab; returns whether anything changed.
    pub fn draw_extra_tab(
        self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        host: &mut dyn PresetHost,
    ) -> bool {
        self.0.draw_extra_tab(ui, drawing, host)
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
        ctxt: &DrawContext<'_>,
        show_handles: bool,
    ) {
        if ctxt.selected {
            let halo_style = DrawingStyle {
                color: SELECTION_HALO_COLOR,
                width_px: style.width_px + SELECTION_HALO_EXTRA_WIDTH_PX,
                fill_alpha: 0,
            };
            let halo_ctxt = DrawContext {
                halo: true,
                ..*ctxt
            };
            self.0
                .paint(painter, chart_rect, halo_style, points, &halo_ctxt);
        }
        self.0.paint(painter, chart_rect, style, points, ctxt);
        if ctxt.selected && show_handles {
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
        ctxt: &DrawContext<'_>,
    ) -> bool {
        points
            .iter()
            .any(|point| point.distance_sq(position) <= radius_px * radius_px)
            || self
                .0
                .hit_test(chart_rect, points, position, radius_px, ctxt)
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

#[derive(Debug, Clone)]
pub struct Drawing {
    pub tool: DrawingTool,
    pub points: Vec<ChartPoint>,
    pub style: DrawingStyle,
    /// A locked drawing keeps rejecting geometry edits and unforced deletes;
    /// its style stays editable.
    pub locked: bool,
    /// A hidden drawing neither paints nor hit-tests, and stays recoverable.
    pub hidden: bool,
    /// Tool-owned state (Fib levels, a future tool's own properties). The
    /// registry creates it; the shared envelope never learns its fields.
    pub payload: Box<dyn DrawingPayload>,
}

impl PartialEq for Drawing {
    fn eq(&self, other: &Self) -> bool {
        self.tool == other.tool
            && self.points == other.points
            && self.style == other.style
            && self.locked == other.locked
            && self.hidden == other.hidden
            && self.payload.eq_dyn(other.payload.as_ref())
    }
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

    /// [`Self::place_with`] with the tool's stock payload — the test-side
    /// shorthand; the app always goes through `place_with` to honour the
    /// user's default preset.
    #[cfg(test)]
    pub fn place(&mut self, tool: DrawingTool, point: ChartPoint) -> bool {
        self.place_with(tool, point, DrawingTool::default_payload)
    }

    /// [`Self::place`] with a caller-chosen payload for a new draft — how the
    /// app applies the user's default preset to newly created objects only.
    pub fn place_with(
        &mut self,
        tool: DrawingTool,
        point: ChartPoint,
        new_payload: impl FnOnce(DrawingTool) -> Box<dyn DrawingPayload>,
    ) -> bool {
        if self.draft.as_ref().is_none_or(|draft| draft.tool != tool) {
            self.draft = Some(Drawing {
                tool,
                points: Vec::with_capacity(tool.required_points()),
                style: DrawingStyle::default(),
                locked: false,
                hidden: false,
                payload: new_payload(tool),
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

    /// Backspace during placement: drop the last placed anchor; dropping the
    /// only one cancels the draft.
    pub fn remove_last_draft_anchor(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.points.pop();
            if draft.points.is_empty() {
                self.draft = None;
            }
        }
    }

    /// Duplicate the selected object as one undo entry: the copy lands
    /// `offset_bars` to the right, unlocked, and becomes the selection.
    pub fn duplicate_selected(&mut self, offset_bars: f32) {
        let Some(index) = self.selected.filter(|&index| index < self.items.len()) else {
            return;
        };
        let before = self.snapshot();
        let mut copy = self.items[index].clone();
        for point in &mut copy.points {
            point.bar += offset_bars;
        }
        copy.locked = false;
        self.items.push(copy);
        self.selected = Some(self.items.len() - 1);
        self.record(before);
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

    pub fn shift_bars(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }
        let delta = delta as f32;
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
        if let Some(index) = self.selected {
            self.set_locked_at(index, locked);
        }
    }

    pub fn set_selected_hidden(&mut self, hidden: bool) {
        if let Some(index) = self.selected {
            self.set_hidden_at(index, hidden);
        }
    }

    pub fn set_locked_at(&mut self, index: usize, locked: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.items.get_mut(index) {
            drawing.locked = locked;
            self.record(before);
        }
    }

    pub fn set_hidden_at(&mut self, index: usize, hidden: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.items.get_mut(index) {
            drawing.hidden = hidden;
            self.record(before);
        }
    }

    /// Z-order: painting walks the list front-to-back, hit-testing walks it
    /// back-to-front, so the last item is the topmost object.
    pub fn bring_to_front(&mut self, index: usize) {
        if index >= self.items.len() || index + 1 == self.items.len() {
            return;
        }
        let before = self.snapshot();
        let drawing = self.items.remove(index);
        self.items.push(drawing);
        // Selection follows the object, not the slot it used to occupy.
        self.selected = self.selected.map(|selected| {
            if selected == index {
                self.items.len() - 1
            } else if selected > index {
                selected - 1
            } else {
                selected
            }
        });
        self.record(before);
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
    fn duplicate_lands_offset_unlocked_and_selected_as_one_entry() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint {
                bar: 4.0,
                price: 100.0,
            },
        );
        drawings.set_selected_locked(true);
        let depth = drawings.undo_depth();

        drawings.duplicate_selected(2.0);

        assert_eq!(drawings.items().len(), 2);
        assert_eq!(drawings.selected(), Some(1), "the copy becomes selected");
        assert_eq!(drawings.items()[1].points[0].bar, 6.0, "the copy is offset");
        assert!(
            !drawings.items()[1].locked,
            "a copy starts unlocked even when the source was locked"
        );
        assert_eq!(drawings.undo_depth(), depth + 1);
        drawings.undo();
        assert_eq!(drawings.items().len(), 1);
    }

    #[test]
    fn backspace_steps_back_one_draft_anchor_at_a_time() {
        let mut drawings = Drawings::default();
        let channel = tool("parallel-channel");
        for bar in [1.0, 2.0] {
            drawings.place(channel, ChartPoint { bar, price: 1.0 });
        }
        assert_eq!(drawings.draft_len(), 2);

        drawings.remove_last_draft_anchor();
        assert_eq!(drawings.draft_len(), 1);
        drawings.remove_last_draft_anchor();
        assert!(
            drawings.draft().is_none(),
            "dropping the only anchor cancels the draft"
        );
    }

    #[test]
    fn rectangle_interior_hit_tests_only_while_the_fill_is_visible() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let rectangle = tool("rectangle");
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        let payload = rectangle.default_payload();
        let points = [egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)];
        let anchors = anchors_for(&points, &scale);
        let center = egui::pos2(150.0, 150.0);
        let border = egui::pos2(100.0, 150.0);

        let filled = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            style: DrawingStyle::default(),
            selected: false,
            halo: false,
        };
        assert!(rectangle.hit_test(chart, &points, center, 5.0, &filled));

        let outline_only = DrawContext {
            style: DrawingStyle {
                fill_alpha: 0,
                ..DrawingStyle::default()
            },
            ..filled
        };
        assert!(
            !rectangle.hit_test(chart, &points, center, 5.0, &outline_only),
            "with no visible fill the interior belongs to the chart"
        );
        assert!(
            rectangle.hit_test(chart, &points, border, 5.0, &outline_only),
            "the border stays selectable without a fill"
        );
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
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        let payload = line.default_payload();
        let anchors = [ChartPoint {
            bar: 0.0,
            price: scale.price_at(120.0),
        }];
        let output = ctx.run(input, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("selection-test"),
            ));
            let ctxt = DrawContext {
                payload: payload.as_ref(),
                anchors: &anchors,
                scale: &scale,
                style,
                selected: true,
                halo: false,
            };
            line.paint(
                &painter,
                chart,
                style,
                &[egui::pos2(100.0, 120.0)],
                &ctxt,
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

    /// Chart anchors consistent with `points` under `scale` — the identity
    /// projection the tool tests run with.
    fn anchors_for(points: &[egui::Pos2], scale: &PriceScale) -> Vec<ChartPoint> {
        points
            .iter()
            .enumerate()
            .map(|(index, point)| ChartPoint {
                bar: index as f32,
                price: scale.price_at(point.y),
            })
            .collect()
    }

    #[test]
    fn horizontal_line_is_selectable_from_anywhere_on_its_stroke() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let line = tool("horizontal-line");
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        let payload = line.default_payload();
        let points = [egui::pos2(100.0, 120.0)];
        let anchors = anchors_for(&points, &scale);
        let ctxt = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            style: DrawingStyle::default(),
            selected: false,
            halo: false,
        };
        assert!(line.hit_test(chart, &points, egui::pos2(450.0, 123.0), 5.0, &ctxt));
    }

    #[test]
    fn every_registered_tool_paints_and_hits_its_finished_geometry() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        for drawing_tool in DRAWING_TOOLS {
            let (points, hit) = drawing_tool.test_geometry();
            assert_eq!(points.len(), drawing_tool.required_points());
            let payload = drawing_tool.default_payload();
            let anchors = anchors_for(&points, &scale);
            let ctxt = DrawContext {
                payload: payload.as_ref(),
                anchors: &anchors,
                scale: &scale,
                style: DrawingStyle::default(),
                selected: false,
                halo: false,
            };
            assert!(
                drawing_tool.hit_test(chart, &points, hit, 5.0, &ctxt),
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
                    &ctxt,
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

    /// The extension port, proven end to end: a fake tool with a property no
    /// other tool has (`ray_count`) docks through the registry surface alone.
    /// Its payload rides the envelope, survives undo snapshots and renders
    /// its own inspector tab — with zero edits to the shared model or the
    /// central inspector code.
    #[test]
    fn a_fake_tool_with_its_own_payload_docks_without_touching_the_model() {
        #[derive(Debug, Clone, PartialEq)]
        struct RayPayload {
            ray_count: u32,
        }
        impl DrawingPayload for RayPayload {
            fn clone_box(&self) -> Box<dyn DrawingPayload> {
                Box::new(self.clone())
            }
            fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
                other
                    .as_any()
                    .downcast_ref::<Self>()
                    .is_some_and(|other| self == other)
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
        struct RayTool;
        impl DrawingToolImpl for RayTool {
            fn id(&self) -> &'static str {
                "test-ray"
            }
            fn name(&self) -> &'static str {
                "Ray fan"
            }
            fn settings_title(&self) -> &'static str {
                "Ray fan settings"
            }
            fn icon(&self) -> &'static str {
                "R"
            }
            fn hover_text(&self) -> &'static str {
                "Ray fan - test tool"
            }
            fn required_points(&self) -> usize {
                1
            }
            fn default_payload(&self) -> Box<dyn DrawingPayload> {
                Box::new(RayPayload { ray_count: 2 })
            }
            fn extra_tab(&self) -> Option<&'static str> {
                Some("Rays")
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
                    .downcast_mut::<RayPayload>()
                    .expect("a ray tool always carries a ray payload");
                ui.add(egui::Slider::new(&mut payload.ray_count, 1..=8).text("rays"))
                    .changed()
            }
            fn paint(
                &self,
                painter: &egui::Painter,
                _chart_rect: egui::Rect,
                style: DrawingStyle,
                points: &[egui::Pos2],
                ctxt: &DrawContext<'_>,
            ) {
                let payload = ctxt
                    .payload
                    .as_any()
                    .downcast_ref::<RayPayload>()
                    .expect("ray payload");
                if let Some(origin) = points.first() {
                    for ray in 0..payload.ray_count {
                        let target = *origin + egui::vec2(40.0, 10.0 * (ray as f32 + 1.0));
                        painter.line_segment([*origin, target], drawing_stroke(style));
                    }
                }
            }
            fn hit_test(
                &self,
                _chart_rect: egui::Rect,
                points: &[egui::Pos2],
                position: egui::Pos2,
                radius_px: f32,
                _ctxt: &DrawContext<'_>,
            ) -> bool {
                points
                    .first()
                    .is_some_and(|point| point.distance(position) <= radius_px * 10.0)
            }
            #[cfg(test)]
            fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
                (vec![egui::pos2(50.0, 50.0)], egui::pos2(60.0, 60.0))
            }
        }
        static RAY: RayTool = RayTool;
        let ray_tool = DrawingTool(&RAY);

        let mut drawings = Drawings::default();
        assert!(drawings.place(
            ray_tool,
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            }
        ));
        // The unique property lives in the payload and rides the undo
        // history exactly like the shared fields do.
        drawings.begin_gesture();
        drawings
            .selected_mut()
            .expect("placement selects")
            .payload
            .as_any_mut()
            .downcast_mut::<RayPayload>()
            .expect("ray payload")
            .ray_count = 5;
        drawings.commit_gesture();
        assert!(drawings.undo(), "the payload edit is one undo entry");
        let restored = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<RayPayload>()
            .expect("ray payload")
            .ray_count;
        assert_eq!(restored, 2, "undo restores the tool-owned property");

        // The tool-owned inspector tab renders through the same port every
        // tool uses — no central match, no central form edit.
        assert_eq!(ray_tool.extra_tab(), Some("Rays"));
        let ctx = egui::Context::default();
        let mut host = NullPresetHost;
        let mut painted = Vec::new();
        let input = egui::RawInput::default();
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let drawing = drawings.selected_mut().expect("still selected");
                ray_tool.draw_extra_tab(ui, drawing, &mut host);
            });
        });
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                painted.push(text.galley.text().to_owned());
            }
        }
        assert!(
            painted.iter().any(|text| text.contains("rays")),
            "the fake tool's own section rendered: {painted:?}"
        );
    }
}
