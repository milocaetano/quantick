//! Drawing-tool selection and the four-edge toolbar rail.
//!
//! The rail owns only chrome state. Drawing definitions and metadata live in
//! [`crate::drawings`], so registering a new drawing does not require another
//! matching list in this module. Geometry, states and docking behaviour
//! follow `docs/drawing-toolbar-ux.md`.

use std::collections::BTreeMap;

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::drawings::{DRAWING_TOOLS, DrawingTool, Drawings, ToolFamily};
use crate::theme;
use crate::widgets::{IconButton, MarkerEdge, TOOLRAIL_ICON};

/// Rail cross axis, all four docks: `44 = 6 + 32 + 6`.
const TOOLBOX_THICKNESS_PX: f32 = 44.0;
/// Outer margin, both axes.
const TOOLBOX_MARGIN_PX: f32 = 6.0;
/// Gap between buttons inside a cluster.
const TOOLBOX_ITEM_GAP_PX: f32 = 4.0;
/// Inset of a separator hairline from each rail edge, so the rule floats.
const TOOLBOX_SEPARATOR_INSET_PX: f32 = 8.0;
/// Grip extent along the rail.
const TOOLBOX_GRIP_LENGTH_PX: f32 = 18.0;
/// Grip glyph font size.
const TOOLBOX_GRIP_GLYPH_PX: f32 = 14.0;
/// Smallest allowed gap between the leading and trailing clusters.
const TOOLBOX_MIN_CLUSTER_GAP_PX: f32 = 12.0;
/// Square corner zone of a family button that opens its flyout.
const TOOLBOX_CARET_ZONE_PX: f32 = 10.0;
/// Family flyout popup width.
const TOOLBOX_FLYOUT_WIDTH_PX: f32 = 208.0;
/// One family flyout row.
const TOOLBOX_FLYOUT_ROW_HEIGHT_PX: f32 = 26.0;
/// Chart-facing accent line of the drop preview band.
const TOOLBOX_DROP_BAND_EDGE_PX: f32 = 3.0;
/// Flyout paint metrics: popup corner radius, row backdrop radius, the
/// glyph centre / name / shortcut columns and their font sizes.
const FLYOUT_CORNER_RADIUS_PX: f32 = 6.0;
const FLYOUT_ROW_RADIUS_PX: f32 = 4.0;
const FLYOUT_GLYPH_CENTER_X_PX: f32 = 12.0;
const FLYOUT_NAME_X_PX: f32 = 26.0;
const FLYOUT_SHORTCUT_INSET_PX: f32 = 6.0;
const FLYOUT_GLYPH_PX: f32 = 18.0;
const FLYOUT_NAME_TEXT_PX: f32 = 12.0;
const FLYOUT_SHORTCUT_TEXT_PX: f32 = 11.0;
const FLYOUT_HEADER_TEXT_PX: f32 = 11.0;
/// Side of the caret triangle on a family slot.
const CARET_SIDE_PX: f32 = 5.0;
/// Inset of the caret from the button's trailing-bottom corner.
const CARET_INSET_PX: f32 = 3.0;
/// Badge pill geometry (§2.5 of the spec).
const BADGE_HEIGHT_PX: f32 = 12.0;
const BADGE_RADIUS_PX: f32 = 3.0;
const BADGE_TEXT_PX: f32 = 9.0;
const BADGE_PAD_X_PX: f32 = 3.0;
const BADGE_CORNER_INSET_PX: f32 = 2.0;
/// A separator block along the long axis: the hairline; its 4 px clear space
/// each side comes from the cluster's item spacing.
const SEPARATOR_BLOCK_PX: f32 = 2.0 * TOOLBOX_ITEM_GAP_PX + 1.0;
/// The grip block: its extent plus the item gap that follows.
const GRIP_BLOCK_PX: f32 = TOOLBOX_GRIP_LENGTH_PX + TOOLBOX_ITEM_GAP_PX;
#[cfg(test)]
const TOOLBOX_BUTTON_COUNT: usize = DRAWING_TOOLS.len() + 4;

/// A chart-acting tool. Only one is armed at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Pointer,
    Crosshair,
    Drawing(DrawingTool),
}

impl Tool {
    #[must_use]
    pub fn drawing_tool(self) -> Option<DrawingTool> {
        match self {
            Self::Drawing(tool) => Some(tool),
            Self::Pointer | Self::Crosshair => None,
        }
    }

    #[must_use]
    fn icon(self) -> &'static str {
        match self {
            Self::Pointer => icons::CURSOR,
            Self::Crosshair => icons::CROSSHAIR,
            Self::Drawing(tool) => tool.icon(),
        }
    }

    #[must_use]
    fn name(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer",
            Self::Crosshair => "Crosshair",
            Self::Drawing(tool) => tool.name(),
        }
    }

    #[must_use]
    fn hover_text(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer - pan, zoom, select and move (1, Esc)",
            Self::Crosshair => "Crosshair (2)",
            Self::Drawing(tool) => tool.hover_text(),
        }
    }

    /// The shortcut label shown in menus, `None` when the tool has no key.
    #[must_use]
    fn shortcut_label(self) -> Option<String> {
        match self {
            Self::Pointer => Some("1".to_owned()),
            Self::Crosshair => Some("2".to_owned()),
            Self::Drawing(tool) => tool.shortcut().map(|shortcut| {
                let key = shortcut.key.name();
                if shortcut.shift {
                    format!("Shift+{key}")
                } else {
                    key.to_owned()
                }
            }),
        }
    }
}

/// One of the four window edges the toolbar can dock against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolboxDock {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl ToolboxDock {
    #[must_use]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    /// The nearest edge by *normalised* distance — raw pixels would bias
    /// every drop toward top/bottom on a wide window. Ties resolve
    /// Left > Right > Top > Bottom, so the result is deterministic.
    #[must_use]
    fn nearest(pointer: egui::Pos2, screen: egui::Rect) -> Self {
        let pointer = pointer.clamp(screen.min, screen.max);
        let width = screen.width().max(f32::EPSILON);
        let height = screen.height().max(f32::EPSILON);
        let candidates = [
            (Self::Left, (pointer.x - screen.left()) / width),
            (Self::Right, (screen.right() - pointer.x) / width),
            (Self::Top, (pointer.y - screen.top()) / height),
            (Self::Bottom, (screen.bottom() - pointer.y) / height),
        ];
        let mut best = candidates[0];
        for candidate in &candidates[1..] {
            if candidate.1 < best.1 {
                best = *candidate;
            }
        }
        best.0
    }

    /// The button edge the active marker hugs: the one facing the window
    /// border this dock sits against.
    #[must_use]
    const fn marker_edge(self) -> MarkerEdge {
        match self {
            Self::Left => MarkerEdge::Left,
            Self::Right => MarkerEdge::Right,
            Self::Top => MarkerEdge::Top,
            Self::Bottom => MarkerEdge::Bottom,
        }
    }
}

/// How much of the rail fits along its long axis. Stages are pure functions
/// of the available extent, so a resize is hysteresis-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailStage {
    /// Every tool slot visible.
    Full,
    /// Pointer, Crosshair, the armed tool and More; full trailing cluster.
    Compact,
    /// Pointer, the armed tool, More and Objects.
    Minimal,
}

/// One leading-cluster slot after family folding: a lone tool, or a family
/// of consecutive registry entries sharing the slot.
enum RailSlot {
    Single(DrawingTool),
    Family {
        family: ToolFamily,
        members: Vec<DrawingTool>,
    },
}

/// Fold the registry into rail slots. Consecutive entries with the same
/// family id share one slot — consecutive, not sorted, so rail order stays
/// registry order and adding a tool cannot silently reorder the rail.
/// Folded once per process: the registry is `const`, so rebuilding this
/// every frame would be per-frame allocation of stable data.
fn tool_slots() -> &'static [RailSlot] {
    static SLOTS: std::sync::OnceLock<Vec<RailSlot>> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| {
        let mut slots: Vec<RailSlot> = Vec::new();
        for tool in DRAWING_TOOLS {
            match tool.family() {
                Some(family) => {
                    if let Some(RailSlot::Family {
                        family: previous,
                        members,
                    }) = slots.last_mut()
                        && previous.id == family.id
                    {
                        members.push(tool);
                    } else {
                        slots.push(RailSlot::Family {
                            family,
                            members: vec![tool],
                        });
                    }
                }
                None => slots.push(RailSlot::Single(tool)),
            }
        }
        slots
    })
}

/// Long-axis length of the full rail (§2.8 of the spec).
fn full_length(tool_slot_count: usize) -> f32 {
    let n = tool_slot_count as f32;
    2.0 * TOOLBOX_MARGIN_PX
        + GRIP_BLOCK_PX
        + (2.0 * TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX)
        + SEPARATOR_BLOCK_PX
        + (n * TOOLRAIL_ICON.hit + (n - 1.0) * TOOLBOX_ITEM_GAP_PX)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + trailing_length()
}

/// Long-axis length at the Compact stage: the tool run gives way to the
/// armed slot plus More.
fn compact_length() -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + GRIP_BLOCK_PX
        + (2.0 * TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX)
        + SEPARATOR_BLOCK_PX
        + (2.0 * TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + trailing_length()
}

/// Long-axis length at the Minimal stage: grip, Pointer, the armed tool,
/// More, the cluster gap, one separator and Objects. The spec's 191 px
/// floor — `main.rs` sets a minimum window size that keeps it unreachable.
#[cfg(test)]
fn minimal_length() -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + GRIP_BLOCK_PX
        + (3.0 * TOOLRAIL_ICON.hit + 2.0 * TOOLBOX_ITEM_GAP_PX)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + SEPARATOR_BLOCK_PX
        + TOOLRAIL_ICON.hit
}

/// The full trailing cluster: separator, magnet, repeat, hide-all, lock-all,
/// separator, Objects.
fn trailing_length() -> f32 {
    SEPARATOR_BLOCK_PX
        + (4.0 * TOOLRAIL_ICON.hit + 3.0 * TOOLBOX_ITEM_GAP_PX)
        + SEPARATOR_BLOCK_PX
        + TOOLRAIL_ICON.hit
}

/// Resolve the stage for an available long-axis extent (margins included).
fn stage_for(available: f32, tool_slot_count: usize) -> RailStage {
    if available >= full_length(tool_slot_count) {
        RailStage::Full
    } else if available >= compact_length() {
        RailStage::Compact
    } else {
        RailStage::Minimal
    }
}

/// Toolbar chrome state. The panel is always outside `CentralPanel`, so the
/// chart never renders behind it.
#[derive(Debug)]
pub struct ToolRail {
    tool: Tool,
    visible: bool,
    dock: ToolboxDock,
    /// The repeat pin: `true` keeps a drawing tool armed after it completes
    /// an object; the default is one-shot back to Pointer.
    repeat: bool,
    /// The magnet: anchors snap to the nearest OHLC of the bar under the
    /// pointer. Off by default — a magnet nobody asked for moves marks the
    /// trader placed deliberately.
    magnet: bool,
    /// Last-armed member of each tool family, keyed by family id.
    last_family_member: BTreeMap<&'static str, DrawingTool>,
    /// Currently-nearest drop edge while a grip drag is live.
    drag_preview: Option<ToolboxDock>,
    dragging: bool,
    drag_cancelled: bool,
    /// Open family flyout: the family id and the slot rect it anchors to.
    flyout: Option<(&'static str, egui::Rect)>,
    #[cfg(test)]
    button_rects: [Option<(Tool, egui::Rect)>; TOOLBOX_BUTTON_COUNT],
    #[cfg(test)]
    grip_rect: Option<egui::Rect>,
    #[cfg(test)]
    magnet_rect: Option<egui::Rect>,
    #[cfg(test)]
    more_rect: Option<egui::Rect>,
    #[cfg(test)]
    hide_all_rect: Option<egui::Rect>,
    #[cfg(test)]
    lock_all_rect: Option<egui::Rect>,
    #[cfg(test)]
    objects_rect: Option<egui::Rect>,
    #[cfg(test)]
    rail_rect: Option<egui::Rect>,
    #[cfg(test)]
    flyout_rects: Vec<(DrawingTool, egui::Rect)>,
}

impl Default for ToolRail {
    fn default() -> Self {
        Self {
            tool: Tool::Pointer,
            visible: true,
            dock: ToolboxDock::Left,
            repeat: false,
            magnet: false,
            last_family_member: BTreeMap::new(),
            drag_preview: None,
            dragging: false,
            drag_cancelled: false,
            flyout: None,
            #[cfg(test)]
            button_rects: [None; TOOLBOX_BUTTON_COUNT],
            #[cfg(test)]
            grip_rect: None,
            #[cfg(test)]
            magnet_rect: None,
            #[cfg(test)]
            more_rect: None,
            #[cfg(test)]
            hide_all_rect: None,
            #[cfg(test)]
            lock_all_rect: None,
            #[cfg(test)]
            objects_rect: None,
            #[cfg(test)]
            rail_rect: None,
            #[cfg(test)]
            flyout_rects: Vec::new(),
        }
    }
}

impl ToolRail {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }

    #[must_use]
    pub fn dock(&self) -> ToolboxDock {
        self.dock
    }

    /// Dock the rail against `dock` — the menu path beside dragging.
    pub fn set_dock(&mut self, dock: ToolboxDock) {
        self.dock = dock;
    }

    /// Show or hide the rail outright, for a saved workspace restoring the
    /// state it recorded rather than toggling from whatever this launch
    /// happens to be in.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.toggle_visible();
        }
    }

    /// Whether a grip drag is live this frame — the app's escape stack must
    /// yield Esc to the drag while it is.
    #[must_use]
    pub fn drag_active(&self) -> bool {
        self.dragging
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.tool = Tool::Pointer;
        }
    }

    pub fn arm(&mut self, tool: Tool) {
        if let Tool::Drawing(drawing_tool) = tool
            && let Some(family) = drawing_tool.family()
        {
            self.last_family_member.insert(family.id, drawing_tool);
        }
        self.tool = tool;
    }

    /// Whether the repeat pin keeps the tool armed after an object completes.
    #[must_use]
    pub fn repeat(&self) -> bool {
        self.repeat
    }

    #[cfg(test)]
    pub(crate) fn set_repeat(&mut self, repeat: bool) {
        self.repeat = repeat;
    }

    /// Whether placed anchors snap to the bar's open / high / low / close.
    #[must_use]
    pub fn magnet(&self) -> bool {
        self.magnet
    }

    /// Arm the magnet without a click — the `QUANTICK_DRAWING_MAGNET` hook
    /// and the tests both come through here, so neither can drift from what
    /// the button does.
    pub(crate) fn set_magnet(&mut self, magnet: bool) {
        self.magnet = magnet;
    }

    #[cfg(test)]
    pub(crate) fn button_rect(&self, tool: Tool) -> Option<egui::Rect> {
        self.button_rects
            .iter()
            .flatten()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }

    #[cfg(test)]
    pub(crate) fn objects_button_rect(&self) -> Option<egui::Rect> {
        self.objects_rect
    }

    #[cfg(test)]
    pub(crate) fn flyout_row_rect(&self, tool: DrawingTool) -> Option<egui::Rect> {
        self.flyout_rects
            .iter()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }

    /// Tool-arming keys. Escape lives in the app's escape stack (rail drag →
    /// input → draft → selection → Pointer), not here. Each drawing tool
    /// declares its own shortcut through the registry.
    pub fn handle_keys(&mut self, ctx: &egui::Context) {
        // A hidden rail arms nothing (audit M9): with no rail on screen an
        // armed tool has no indication anywhere, and the next chart click
        // would draw instead of pan — the keyboard twin of the invariant
        // `hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed`.
        if !self.visible {
            return;
        }
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let armed = ctx.input(|input| {
            if input.modifiers.command || input.modifiers.alt {
                return None;
            }
            if input.key_pressed(egui::Key::Num1) {
                return Some(Tool::Pointer);
            }
            if input.key_pressed(egui::Key::Num2) {
                return Some(Tool::Crosshair);
            }
            DRAWING_TOOLS.into_iter().find_map(|tool| {
                tool.shortcut()
                    .filter(|shortcut| {
                        input.key_pressed(shortcut.key) && input.modifiers.shift == shortcut.shift
                    })
                    .map(|_| Tool::Drawing(tool))
            })
        });
        if let Some(tool) = armed {
            self.arm(tool);
        }
    }

    /// Draw the rail docked against its edge. Drag the grip and release: the
    /// nearest window edge becomes the new dock, previewed live by a band.
    /// The rail also hosts the object-manager entry and the global protection
    /// toggles (hide-all / lock-all), which act on the store.
    pub fn draw(&mut self, ctx: &egui::Context, drawings: &mut Drawings, manager_open: &mut bool) {
        if !self.visible {
            return;
        }

        match self.dock {
            ToolboxDock::Left => egui::SidePanel::left("drawing_toolbox_left")
                .exact_width(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame())
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
            ToolboxDock::Right => egui::SidePanel::right("drawing_toolbox_right")
                .exact_width(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame())
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
            ToolboxDock::Top => egui::TopBottomPanel::top("drawing_toolbox_top")
                .exact_height(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame())
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
            ToolboxDock::Bottom => egui::TopBottomPanel::bottom("drawing_toolbox_bottom")
                .exact_height(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame())
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
        };

        if let Some(target) = self.drag_preview.take() {
            paint_drop_preview(ctx, target);
        }
    }

    fn draw_contents(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &mut Drawings,
        manager_open: &mut bool,
    ) {
        #[cfg(test)]
        {
            self.button_rects.fill(None);
            self.grip_rect = None;
            self.magnet_rect = None;
            self.more_rect = None;
            self.hide_all_rect = None;
            self.lock_all_rect = None;
            self.objects_rect = None;
            self.rail_rect = Some(ui.max_rect().expand(TOOLBOX_MARGIN_PX));
            self.flyout_rects.clear();
        }

        let vertical = self.dock.is_vertical();
        let available = if vertical {
            ui.available_height()
        } else {
            ui.available_width()
        } + 2.0 * TOOLBOX_MARGIN_PX;
        let slots = tool_slots();
        let stage = stage_for(available, slots.len());

        // Chart-facing hairline: the only stroke the rail paints — a
        // four-sided stroke would draw a seam against the window edge.
        let rail_rect = ui.max_rect().expand(TOOLBOX_MARGIN_PX);
        let edge = match self.dock {
            ToolboxDock::Left => [rail_rect.right_top(), rail_rect.right_bottom()],
            ToolboxDock::Right => [rail_rect.left_top(), rail_rect.left_bottom()],
            ToolboxDock::Top => [rail_rect.left_bottom(), rail_rect.right_bottom()],
            ToolboxDock::Bottom => [rail_rect.left_top(), rail_rect.right_top()],
        };
        ui.painter()
            .line_segment(edge, egui::Stroke::new(1.0_f32, theme::BORDER));

        let leading = if vertical {
            egui::Layout::top_down(egui::Align::Center)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        };
        let trailing = if vertical {
            egui::Layout::bottom_up(egui::Align::Center)
        } else {
            egui::Layout::right_to_left(egui::Align::Center)
        };

        ui.with_layout(leading, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(TOOLBOX_ITEM_GAP_PX, TOOLBOX_ITEM_GAP_PX);
            ui.set_min_size(egui::Vec2::ZERO);
            self.draw_grip(ui, vertical);
            self.draw_button(ui, Tool::Pointer, drawings);
            if stage != RailStage::Minimal {
                self.draw_button(ui, Tool::Crosshair, drawings);
                self.draw_separator(ui, vertical);
            }
            match stage {
                RailStage::Full => {
                    for slot in slots {
                        match slot {
                            RailSlot::Single(tool) => {
                                self.draw_button(ui, Tool::Drawing(*tool), drawings);
                            }
                            RailSlot::Family { family, members } => {
                                self.draw_family_slot(ui, *family, members, drawings);
                            }
                        }
                    }
                }
                RailStage::Compact | RailStage::Minimal => {
                    if let Some(armed) = self.tool.drawing_tool() {
                        self.draw_button(ui, Tool::Drawing(armed), drawings);
                    }
                    self.draw_more_menu(ui, drawings, stage);
                }
            }
        });

        // The trailing cluster is pinned to the rail's far end — laid from
        // that end backwards, so the flexible gap sits between the clusters.
        ui.with_layout(trailing, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(TOOLBOX_ITEM_GAP_PX, TOOLBOX_ITEM_GAP_PX);
            self.draw_objects_button(ui, drawings, manager_open);
            self.draw_separator(ui, vertical);
            if stage != RailStage::Minimal {
                self.draw_global_buttons(ui, drawings);
                self.draw_repeat_button(ui);
                self.draw_magnet_button(ui);
                self.draw_separator(ui, vertical);
            }
        });

        self.draw_family_flyout(ui.ctx());
    }

    /// The grip: hold and drag to dock the rail against another window edge.
    fn draw_grip(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let size = if vertical {
            egui::vec2(TOOLRAIL_ICON.hit, TOOLBOX_GRIP_LENGTH_PX)
        } else {
            egui::vec2(TOOLBOX_GRIP_LENGTH_PX, TOOLRAIL_ICON.hit)
        };
        let (rect, grip) = ui.allocate_exact_size(size, egui::Sense::drag());
        #[cfg(test)]
        {
            self.grip_rect = Some(rect);
        }
        if ui.is_rect_visible(rect) {
            // The dots always run across the rail, reading as a handle.
            let glyph = if vertical {
                icons::DOTS_SIX
            } else {
                icons::DOTS_SIX_VERTICAL
            };
            let color = if grip.hovered() || self.dragging {
                theme::TEXT_MUTED
            } else {
                theme::TEXT_FAINT
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(TOOLBOX_GRIP_GLYPH_PX),
                color,
            );
        }
        let grip = grip.on_hover_text("Drag to dock the toolbar on another edge");
        if grip.hovered() && !self.dragging {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if grip.drag_started() {
            self.dragging = true;
            self.drag_cancelled = false;
        }
        if self.dragging && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
            // Esc aborts the drag and keeps the current dock — the topmost
            // level of the app's escape stack.
            self.dragging = false;
            self.drag_cancelled = true;
        }
        if self.dragging {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(pointer) = ui.ctx().input(|input| input.pointer.interact_pos()) {
                self.drag_preview = Some(ToolboxDock::nearest(pointer, ui.ctx().screen_rect()));
            }
        }
        if grip.drag_stopped() {
            if self.dragging
                && !self.drag_cancelled
                && let Some(pointer) = ui.ctx().input(|input| input.pointer.interact_pos())
            {
                self.dock = ToolboxDock::nearest(pointer, ui.ctx().screen_rect());
            }
            self.dragging = false;
            self.drag_cancelled = false;
            self.drag_preview = None;
        }
    }

    /// A separator: a 1 px hairline across the rail, floated off both edges.
    /// The 4 px clear space each side comes from the cluster's item spacing.
    fn draw_separator(&self, ui: &mut egui::Ui, vertical: bool) {
        let size = if vertical {
            egui::vec2(ui.available_width(), 1.0)
        } else {
            egui::vec2(1.0, ui.available_height())
        };
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        // The rail edge sits one margin outside the content rect; the spec
        // insets the hairline from the rail edge itself.
        let inset = TOOLBOX_SEPARATOR_INSET_PX - TOOLBOX_MARGIN_PX;
        let (from, to) = if vertical {
            (
                egui::pos2(rect.left() + inset, rect.center().y),
                egui::pos2(rect.right() - inset, rect.center().y),
            )
        } else {
            (
                egui::pos2(rect.center().x, rect.top() + inset),
                egui::pos2(rect.center().x, rect.bottom() - inset),
            )
        };
        ui.painter()
            .line_segment([from, to], egui::Stroke::new(1.0_f32, theme::BORDER));
    }

    /// The repeat pin: keep the armed drawing tool active after it completes
    /// an object, instead of the one-shot return to Pointer.
    fn draw_repeat_button(&mut self, ui: &mut egui::Ui) {
        let response = IconButton::new(icons::ARROW_CLOCKWISE, TOOLRAIL_ICON)
            .active(self.repeat)
            .active_marker(self.dock.marker_edge())
            .hover_text("Keep the drawing tool active after drawing")
            .show(ui);
        if response.clicked() {
            self.repeat = !self.repeat;
        }
    }

    /// The magnet: anchors land on the bar's open / high / low / close when
    /// one is within reach of the pointer. A state, like the repeat pin, so
    /// it reads off the rail without a menu.
    fn draw_magnet_button(&mut self, ui: &mut egui::Ui) {
        let response = IconButton::new(icons::MAGNET, TOOLRAIL_ICON)
            .active(self.magnet)
            .active_marker(self.dock.marker_edge())
            .hover_text("Snap anchors to the bar's open / high / low / close")
            .show(ui);
        #[cfg(test)]
        {
            self.magnet_rect = Some(response.rect);
        }
        if response.clicked() {
            self.magnet = !self.magnet;
        }
    }

    /// The More flyout of a collapsed rail: everything that lost its slot
    /// stays reachable by name with its shortcut, in registry order.
    fn draw_more_menu(&mut self, ui: &mut egui::Ui, drawings: &mut Drawings, stage: RailStage) {
        let response = ui.menu_button(icons::DOTS_THREE, |ui| {
            for tool in self.swallowed_tools(stage) {
                let mut button = egui::Button::new(tool.name());
                if let Some(shortcut) = tool.shortcut_label() {
                    button = button.shortcut_text(shortcut);
                }
                if ui.add(button).clicked() {
                    self.arm(tool);
                    ui.close_menu();
                }
            }
            if stage == RailStage::Minimal {
                ui.separator();
                if ui
                    .add(egui::Button::new("Keep tool active after drawing").selected(self.repeat))
                    .clicked()
                {
                    self.repeat = !self.repeat;
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Snap anchors to OHLC").selected(self.magnet))
                    .clicked()
                {
                    self.magnet = !self.magnet;
                    ui.close_menu();
                }
                let all_hidden = drawings.all_hidden();
                if ui
                    .button(if all_hidden { "Show all" } else { "Hide all" })
                    .clicked()
                {
                    drawings.set_all_hidden(!all_hidden);
                    ui.close_menu();
                }
                let all_locked = drawings.all_locked();
                if ui
                    .button(if all_locked { "Unlock all" } else { "Lock all" })
                    .clicked()
                {
                    drawings.set_all_locked(!all_locked);
                    ui.close_menu();
                }
            }
        });
        #[cfg(test)]
        {
            self.more_rect = Some(response.response.rect);
        }
        response.response.on_hover_text("More tools");
    }

    /// The tools the given stage swallowed into the More flyout — exactly
    /// the ones without a rail slot, in registry order.
    fn swallowed_tools(&self, stage: RailStage) -> Vec<Tool> {
        let armed = self.tool.drawing_tool();
        let mut swallowed = Vec::new();
        if stage == RailStage::Minimal {
            swallowed.push(Tool::Crosshair);
        }
        swallowed.extend(
            DRAWING_TOOLS
                .into_iter()
                .filter(|tool| Some(*tool) != armed)
                .map(Tool::Drawing),
        );
        swallowed
    }

    /// The entry to the drawn-objects manager. A toggle, not a tool: it never
    /// changes which tool is armed. Carries the object count as a badge.
    fn draw_objects_button(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &Drawings,
        manager_open: &mut bool,
    ) {
        let response = IconButton::new(icons::LIST, TOOLRAIL_ICON)
            .active(*manager_open)
            .active_marker(self.dock.marker_edge())
            .hover_text("Drawn objects")
            .show(ui);
        let count = drawings.items().len();
        if count > 0 {
            paint_badge(ui, response.rect, &count.to_string(), theme::TEXT_MUTED);
        }
        #[cfg(test)]
        {
            self.objects_rect = Some(response.rect);
        }
        if response.clicked() {
            *manager_open = !*manager_open;
        }
    }

    /// The reversible global protections. Hide-all is a view layer over each
    /// drawing's own eye; lock-all mutates every lock at once. Neither is a
    /// delete, and both are one undo entry. Drawn lock-first because the
    /// trailing layout lays from the rail's far end backwards.
    fn draw_global_buttons(&mut self, ui: &mut egui::Ui, drawings: &mut Drawings) {
        let all_locked = drawings.all_locked();
        let lock_icon = if all_locked {
            icons::LOCK_SIMPLE
        } else {
            icons::LOCK_SIMPLE_OPEN
        };
        let lock_hover = if all_locked {
            "Unlock all drawings"
        } else {
            "Lock all drawings"
        };
        let lock = IconButton::new(lock_icon, TOOLRAIL_ICON)
            .active(all_locked)
            .active_marker(self.dock.marker_edge())
            .hover_text(lock_hover)
            .show(ui);
        #[cfg(test)]
        {
            self.lock_all_rect = Some(lock.rect);
        }
        if lock.clicked() {
            drawings.set_all_locked(!all_locked);
        }

        let all_hidden = drawings.all_hidden();
        let eye_icon = if all_hidden {
            icons::EYE_SLASH
        } else {
            icons::EYE
        };
        let eye_hover = if all_hidden {
            "Show all drawings"
        } else {
            "Hide all drawings"
        };
        let eye = IconButton::new(eye_icon, TOOLRAIL_ICON)
            .active(all_hidden)
            .active_marker(self.dock.marker_edge())
            .hover_text(eye_hover)
            .show(ui);
        #[cfg(test)]
        {
            self.hide_all_rect = Some(eye.rect);
        }
        if eye.clicked() {
            drawings.set_all_hidden(!all_hidden);
        }
    }

    fn draw_button(&mut self, ui: &mut egui::Ui, tool: Tool, drawings: &Drawings) {
        let response = IconButton::new(tool.icon(), TOOLRAIL_ICON)
            .active(self.tool == tool)
            .active_marker(self.dock.marker_edge())
            .hover_text(tool.hover_text())
            .show(ui);
        self.paint_draft_badge(ui, &response, tool, drawings);
        #[cfg(test)]
        if let Some(slot) = self.button_rects.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((tool, response.rect));
        }
        if response.clicked() {
            self.arm(tool);
        }
    }

    /// Draft progress on the armed tool while a multi-point object is being
    /// placed — answers "how many more clicks?" in place. Returns whether a
    /// badge occupies the button's corner this frame.
    fn paint_draft_badge(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        tool: Tool,
        drawings: &Drawings,
    ) -> bool {
        let Some(drawing_tool) = tool.drawing_tool() else {
            return false;
        };
        if self.tool != tool || drawings.draft().is_none() || drawing_tool.required_points() < 2 {
            return false;
        }
        let text = format!(
            "{}/{}",
            drawings.draft_len(),
            drawing_tool.required_points()
        );
        paint_badge(ui, response.rect, &text, theme::ACCENT);
        true
    }

    /// A family's shown member: the last-armed one, or `None` before any
    /// member has been used.
    fn family_member(&self, family: ToolFamily, members: &[DrawingTool]) -> Option<DrawingTool> {
        self.last_family_member
            .get(family.id)
            .copied()
            .filter(|member| members.contains(member))
    }

    /// The family split button: left-click arms the shown member; the caret
    /// zone or a right-click opens the member flyout.
    fn draw_family_slot(
        &mut self,
        ui: &mut egui::Ui,
        family: ToolFamily,
        members: &[DrawingTool],
        drawings: &Drawings,
    ) {
        let shown = self.family_member(family, members);
        let armed = self
            .tool
            .drawing_tool()
            .is_some_and(|tool| members.contains(&tool));
        let icon = shown.map_or(family.icon, DrawingTool::icon);
        let hover = shown.map_or(family.title, DrawingTool::hover_text);
        let response = IconButton::new(icon, TOOLRAIL_ICON)
            .active(armed)
            .active_marker(self.dock.marker_edge())
            .hover_text(hover)
            .show(ui);
        let badge_shown = shown
            .map(Tool::Drawing)
            .is_some_and(|tool| self.paint_draft_badge(ui, &response, tool, drawings));

        // The caret marks the flyout; it yields the corner to a draft badge,
        // because a tool mid-draft is unambiguously armed already.
        let caret_zone = egui::Rect::from_min_max(
            egui::pos2(
                response.rect.right() - TOOLBOX_CARET_ZONE_PX,
                response.rect.bottom() - TOOLBOX_CARET_ZONE_PX,
            ),
            response.rect.max,
        );
        if !badge_shown && ui.is_rect_visible(response.rect) {
            let caret_color = if armed {
                theme::ACCENT
            } else if response.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_FAINT
            };
            let corner = egui::pos2(
                response.rect.right() - CARET_INSET_PX,
                response.rect.bottom() - CARET_INSET_PX,
            );
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(corner.x - CARET_SIDE_PX, corner.y),
                    corner,
                    egui::pos2(corner.x, corner.y - CARET_SIDE_PX),
                ],
                caret_color,
                egui::Stroke::NONE,
            ));
        }

        #[cfg(test)]
        if let Some(slot) = self.button_rects.iter_mut().find(|slot| slot.is_none()) {
            let recorded = shown.unwrap_or(members[0]);
            *slot = Some((Tool::Drawing(recorded), response.rect));
        }

        let caret_clicked = response.clicked()
            && response
                .interact_pointer_pos()
                .is_some_and(|position| caret_zone.contains(position));
        if response.secondary_clicked() || caret_clicked {
            self.flyout = Some((family.id, response.rect));
        } else if response.clicked() {
            self.arm(Tool::Drawing(shown.unwrap_or(members[0])));
        }
    }

    /// The open family flyout, on the rail's chart-facing side, first row
    /// aligned with the slot's leading edge.
    fn draw_family_flyout(&mut self, ctx: &egui::Context) {
        let Some((family_id, anchor)) = self.flyout else {
            return;
        };
        let Some((family, members)) = tool_slots().iter().find_map(|slot| match slot {
            RailSlot::Family { family, members } if family.id == family_id => {
                Some((*family, members.as_slice()))
            }
            _ => None,
        }) else {
            self.flyout = None;
            return;
        };

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.flyout = None;
            return;
        }

        let screen = ctx.screen_rect();
        let height =
            TOOLBOX_FLYOUT_ROW_HEIGHT_PX * (members.len() + 1) as f32 + 2.0 * TOOLBOX_ITEM_GAP_PX;
        let position = match self.dock {
            ToolboxDock::Left => egui::pos2(anchor.right() + TOOLBOX_MARGIN_PX, anchor.top()),
            ToolboxDock::Right => egui::pos2(
                anchor.left() - TOOLBOX_MARGIN_PX - TOOLBOX_FLYOUT_WIDTH_PX,
                anchor.top(),
            ),
            ToolboxDock::Top => egui::pos2(anchor.left(), anchor.bottom() + TOOLBOX_MARGIN_PX),
            ToolboxDock::Bottom => {
                egui::pos2(anchor.left(), anchor.top() - TOOLBOX_MARGIN_PX - height)
            }
        };
        let max_position = egui::pos2(
            (screen.right() - TOOLBOX_FLYOUT_WIDTH_PX).max(screen.left()),
            (screen.bottom() - height).max(screen.top()),
        );
        let position = position.clamp(screen.min, max_position);

        let area = egui::Area::new(egui::Id::new("toolbox_family_flyout"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme::CONTROL)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(egui::Rounding::same(FLYOUT_CORNER_RADIUS_PX))
                    .show(ui, |ui| {
                        ui.set_width(TOOLBOX_FLYOUT_WIDTH_PX - 2.0 * TOOLBOX_ITEM_GAP_PX);
                        ui.label(
                            egui::RichText::new(family.title)
                                .color(theme::TEXT_MUTED)
                                .size(FLYOUT_HEADER_TEXT_PX),
                        );
                        for member in members {
                            if self.draw_flyout_row(ui, *member) {
                                self.arm(Tool::Drawing(*member));
                                self.flyout = None;
                            }
                        }
                    });
            });

        // A press anywhere outside closes the flyout without arming.
        if self.flyout.is_some() {
            let pressed_outside = ctx.input(|input| {
                input.pointer.any_pressed()
                    && input.pointer.interact_pos().is_some_and(|position| {
                        !area.response.rect.contains(position) && !anchor.contains(position)
                    })
            });
            if pressed_outside {
                self.flyout = None;
            }
        }
    }

    /// One flyout row: glyph, name, right-aligned shortcut. Returns whether
    /// the row was clicked.
    fn draw_flyout_row(&mut self, ui: &mut egui::Ui, member: DrawingTool) -> bool {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, TOOLBOX_FLYOUT_ROW_HEIGHT_PX),
            egui::Sense::click(),
        );
        #[cfg(test)]
        self.flyout_rects.push((member, rect));
        if ui.is_rect_visible(rect) {
            let armed = self.tool == Tool::Drawing(member);
            if armed {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(FLYOUT_ROW_RADIUS_PX),
                    theme::active_tint(theme::ACCENT),
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(FLYOUT_ROW_RADIUS_PX),
                    theme::BORDER,
                );
            }
            let glyph_color = if armed {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            };
            ui.painter().text(
                egui::pos2(rect.left() + FLYOUT_GLYPH_CENTER_X_PX, rect.center().y),
                egui::Align2::CENTER_CENTER,
                member.icon(),
                egui::FontId::proportional(FLYOUT_GLYPH_PX),
                glyph_color,
            );
            ui.painter().text(
                egui::pos2(rect.left() + FLYOUT_NAME_X_PX, rect.center().y),
                egui::Align2::LEFT_CENTER,
                member.name(),
                egui::FontId::proportional(FLYOUT_NAME_TEXT_PX),
                theme::TEXT_PRIMARY,
            );
            if let Some(shortcut) = Tool::Drawing(member).shortcut_label() {
                ui.painter().text(
                    egui::pos2(rect.right() - FLYOUT_SHORTCUT_INSET_PX, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    shortcut,
                    egui::FontId::proportional(FLYOUT_SHORTCUT_TEXT_PX),
                    theme::TEXT_FAINT,
                );
            }
        }
        response.clicked()
    }
}

/// The rail's frame: chrome fill, margins, and no stroke — the chart-facing
/// hairline is painted by the rail itself.
fn rail_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(theme::CHROME)
        .inner_margin(egui::Margin::same(TOOLBOX_MARGIN_PX))
}

/// The drop preview: the band the rail will occupy on the candidate edge,
/// with an accent line on its chart-facing side.
fn paint_drop_preview(ctx: &egui::Context, target: ToolboxDock) {
    let screen = ctx.screen_rect();
    let thickness = TOOLBOX_THICKNESS_PX;
    let band = match target {
        ToolboxDock::Left => egui::Rect::from_min_max(
            screen.min,
            egui::pos2(screen.left() + thickness, screen.bottom()),
        ),
        ToolboxDock::Right => egui::Rect::from_min_max(
            egui::pos2(screen.right() - thickness, screen.top()),
            screen.max,
        ),
        ToolboxDock::Top => egui::Rect::from_min_max(
            screen.min,
            egui::pos2(screen.right(), screen.top() + thickness),
        ),
        ToolboxDock::Bottom => egui::Rect::from_min_max(
            egui::pos2(screen.left(), screen.bottom() - thickness),
            screen.max,
        ),
    };
    let edge = TOOLBOX_DROP_BAND_EDGE_PX;
    let line = match target {
        ToolboxDock::Left => egui::Rect::from_min_max(
            egui::pos2(band.right() - edge, band.top()),
            band.right_bottom(),
        ),
        ToolboxDock::Right => egui::Rect::from_min_max(
            band.left_top(),
            egui::pos2(band.left() + edge, band.bottom()),
        ),
        ToolboxDock::Top => egui::Rect::from_min_max(
            egui::pos2(band.left(), band.bottom() - edge),
            band.right_bottom(),
        ),
        ToolboxDock::Bottom => {
            egui::Rect::from_min_max(band.left_top(), egui::pos2(band.right(), band.top() + edge))
        }
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("toolbox_drop_preview"),
    ));
    painter.rect_filled(band, 0.0, theme::active_tint(theme::ACCENT));
    painter.rect_filled(line, 0.0, theme::ACCENT);
}

/// A pill badge in a button's trailing-bottom corner (§2.5).
fn paint_badge(ui: &egui::Ui, button: egui::Rect, text: &str, color: egui::Color32) {
    let painter = ui.painter();
    let font = egui::FontId::monospace(BADGE_TEXT_PX);
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let size = egui::vec2(galley.size().x + 2.0 * BADGE_PAD_X_PX, BADGE_HEIGHT_PX);
    let rect = egui::Rect::from_min_max(
        egui::pos2(
            button.right() - BADGE_CORNER_INSET_PX - size.x,
            button.bottom() - BADGE_CORNER_INSET_PX - size.y,
        ),
        egui::pos2(
            button.right() - BADGE_CORNER_INSET_PX,
            button.bottom() - BADGE_CORNER_INSET_PX,
        ),
    );
    painter.rect_filled(rect, egui::Rounding::same(BADGE_RADIUS_PX), theme::INSET);
    painter.galley(
        egui::pos2(
            rect.left() + BADGE_PAD_X_PX,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbox_opens_outside_the_chart_docked_left() {
        let rail = ToolRail::new();
        assert!(rail.visible());
        assert_eq!(rail.tool(), Tool::Pointer);
        assert_eq!(rail.dock, ToolboxDock::Left);
    }

    #[test]
    fn the_rail_never_borrows_the_provenance_amber() {
        // Amber is reserved for data honesty (replay, backfill, inferred
        // data) and is never rail decoration — grep-guarded like the
        // indicators crate's libm rule.
        let source = include_str!("toolrail.rs");
        assert!(
            !source.contains(concat!("theme::", "AM", "BER")),
            "the rail's only accent is ACCENT"
        );
    }

    #[test]
    fn rail_thickness_is_margin_button_margin() {
        assert_eq!(
            TOOLBOX_THICKNESS_PX,
            2.0 * TOOLBOX_MARGIN_PX + TOOLRAIL_ICON.hit
        );
    }

    #[test]
    fn stage_lengths_match_the_spec_for_the_shipped_registry() {
        let slots = tool_slots().len();
        assert_eq!(
            slots, 7,
            "the registry folds into Lines, Channels, Marks, Shapes, Fib, Measure and Text"
        );
        assert_eq!(full_length(slots), 561.0);
        assert_eq!(compact_length(), 381.0);
        assert_eq!(minimal_length(), 191.0);
    }

    #[test]
    fn hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed() {
        let mut rail = ToolRail::new();
        rail.arm(Tool::Drawing(DRAWING_TOOLS[0]));
        rail.toggle_visible();
        assert!(!rail.visible());
        assert_eq!(rail.tool(), Tool::Pointer);
    }

    /// The keyboard twin of the invariant above (audit M9): the shortcut
    /// path must be as gated as the toggle path, or `H` with the rail hidden
    /// arms a tool nothing on screen reports.
    #[test]
    fn shortcuts_cannot_arm_a_tool_while_the_rail_is_hidden() {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        rail.toggle_visible();
        let shortcut = DRAWING_TOOLS[0].shortcut().expect("the first tool has one");
        let press = egui::RawInput {
            events: vec![egui::Event::Key {
                key: shortcut.key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(press, |ctx| rail.handle_keys(ctx));
        assert_eq!(
            rail.tool(),
            Tool::Pointer,
            "a hidden rail must swallow its shortcuts"
        );

        // The same press with the rail visible arms the tool — proving the
        // gate above is the visibility, not a broken key path.
        rail.toggle_visible();
        let press = egui::RawInput {
            events: vec![egui::Event::Key {
                key: shortcut.key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(press, |ctx| rail.handle_keys(ctx));
        assert_eq!(rail.tool(), Tool::Drawing(DRAWING_TOOLS[0]));
    }

    #[test]
    fn nearest_picks_each_edge_from_its_midpoint() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(10.0, 300.0), screen),
            ToolboxDock::Left
        );
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(790.0, 300.0), screen),
            ToolboxDock::Right
        );
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(400.0, 10.0), screen),
            ToolboxDock::Top
        );
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(400.0, 590.0), screen),
            ToolboxDock::Bottom
        );
    }

    #[test]
    fn nearest_normalises_so_a_wide_screen_does_not_bias_top_bottom() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 600.0));
        // Raw pixel distances tie at 300 vs 300; normalised 0.156 vs 0.5.
        assert_eq!(
            ToolboxDock::nearest(egui::pos2(300.0, 300.0), screen),
            ToolboxDock::Left
        );
    }

    fn draw_rail_frame(
        rail: &mut ToolRail,
        ctx: &egui::Context,
        screen: egui::Rect,
        events: Vec<egui::Event>,
    ) {
        let mut drawings = Drawings::default();
        rail_frame_with(rail, &mut drawings, ctx, screen, events);
    }

    fn primary_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn dragging_the_real_grip_docks_at_each_screen_edge() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        draw_rail_frame(&mut rail, &ctx, screen, Vec::new());

        for (target, expected) in [
            (egui::pos2(790.0, 300.0), ToolboxDock::Right),
            (egui::pos2(400.0, 10.0), ToolboxDock::Top),
            (egui::pos2(400.0, 590.0), ToolboxDock::Bottom),
            (egui::pos2(10.0, 300.0), ToolboxDock::Left),
        ] {
            let start = rail.grip_rect.expect("grip was rendered").center();
            draw_rail_frame(
                &mut rail,
                &ctx,
                screen,
                vec![
                    egui::Event::PointerMoved(start),
                    primary_button(start, true),
                ],
            );
            draw_rail_frame(
                &mut rail,
                &ctx,
                screen,
                vec![egui::Event::PointerMoved(target)],
            );
            draw_rail_frame(
                &mut rail,
                &ctx,
                screen,
                vec![
                    egui::Event::PointerMoved(target),
                    primary_button(target, false),
                ],
            );
            assert_eq!(rail.dock, expected);
            draw_rail_frame(&mut rail, &ctx, screen, Vec::new());
        }
    }

    #[test]
    fn escape_during_a_grip_drag_keeps_the_current_dock() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        draw_rail_frame(&mut rail, &ctx, screen, Vec::new());

        let start = rail.grip_rect.expect("grip was rendered").center();
        draw_rail_frame(
            &mut rail,
            &ctx,
            screen,
            vec![
                egui::Event::PointerMoved(start),
                primary_button(start, true),
            ],
        );
        let target = egui::pos2(400.0, 590.0);
        draw_rail_frame(
            &mut rail,
            &ctx,
            screen,
            vec![egui::Event::PointerMoved(target)],
        );
        assert!(rail.drag_active());
        draw_rail_frame(
            &mut rail,
            &ctx,
            screen,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!rail.drag_active());
        draw_rail_frame(
            &mut rail,
            &ctx,
            screen,
            vec![
                egui::Event::PointerMoved(target),
                primary_button(target, false),
            ],
        );
        assert_eq!(rail.dock, ToolboxDock::Left, "Esc aborted the drag");
    }

    #[test]
    fn drawing_tool_carries_the_registered_implementation_without_an_adapter_match() {
        for tool in DRAWING_TOOLS {
            assert_eq!(Tool::Drawing(tool).drawing_tool(), Some(tool));
        }
    }

    fn rail_frame_with(
        rail: &mut ToolRail,
        drawings: &mut Drawings,
        ctx: &egui::Context,
        screen: egui::Rect,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut manager_open = false;
        let _ = ctx.run(input, |ctx| rail.draw(ctx, drawings, &mut manager_open));
    }

    fn click_at(
        rail: &mut ToolRail,
        drawings: &mut Drawings,
        ctx: &egui::Context,
        screen: egui::Rect,
        position: egui::Pos2,
    ) {
        rail_frame_with(
            rail,
            drawings,
            ctx,
            screen,
            vec![
                egui::Event::PointerMoved(position),
                primary_button(position, true),
            ],
        );
        rail_frame_with(
            rail,
            drawings,
            ctx,
            screen,
            vec![
                egui::Event::PointerMoved(position),
                primary_button(position, false),
            ],
        );
    }

    #[test]
    fn tool_shortcuts_arm_their_declared_tools() {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        let send = |rail: &mut ToolRail, key: egui::Key, modifiers: egui::Modifiers| {
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                modifiers,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| rail.handle_keys(ctx));
        };

        send(&mut rail, egui::Key::H, egui::Modifiers::NONE);
        assert_eq!(
            rail.tool().drawing_tool().map(DrawingTool::id),
            Some("horizontal-line")
        );
        send(&mut rail, egui::Key::R, egui::Modifiers::NONE);
        assert_eq!(
            rail.tool().drawing_tool().map(DrawingTool::id),
            Some("rectangle")
        );
        send(&mut rail, egui::Key::C, egui::Modifiers::NONE);
        assert_eq!(
            rail.tool().drawing_tool().map(DrawingTool::id),
            Some("parallel-channel")
        );
        send(&mut rail, egui::Key::F, egui::Modifiers::NONE);
        assert_eq!(
            rail.tool().drawing_tool().map(DrawingTool::id),
            Some("fib-retracement")
        );
        send(&mut rail, egui::Key::F, egui::Modifiers::SHIFT);
        assert_eq!(
            rail.tool().drawing_tool().map(DrawingTool::id),
            Some("fib-extension")
        );
        send(&mut rail, egui::Key::Num1, egui::Modifiers::NONE);
        assert_eq!(rail.tool(), Tool::Pointer);
        // A held command modifier keeps the letters out of the tool map.
        send(&mut rail, egui::Key::R, egui::Modifiers::COMMAND);
        assert_eq!(rail.tool(), Tool::Pointer);
    }

    #[test]
    fn arming_a_family_member_becomes_the_slot_memory() {
        let mut rail = ToolRail::new();
        let extension = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "fib-extension")
            .expect("registered");
        rail.arm(Tool::Drawing(extension));
        let family = extension.family().expect("fib family");
        assert_eq!(
            rail.family_member(family, &[extension]),
            Some(extension),
            "the slot remembers the last-armed member"
        );
    }

    #[test]
    fn the_full_vertical_rail_folds_the_fib_family_into_one_slot() {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        let mut drawings = Drawings::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

        let retracement = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "fib-retracement")
            .expect("registered");
        let extension = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "fib-extension")
            .expect("registered");
        assert!(
            rail.button_rect(Tool::Drawing(retracement)).is_some(),
            "the family slot records its shown member"
        );
        assert!(
            rail.button_rect(Tool::Drawing(extension)).is_none(),
            "the second fib entry must not hold its own slot"
        );
    }

    /// The magnet is a state, not a command: it reads off the rail and it
    /// starts off, because a magnet nobody asked for moves marks the trader
    /// placed deliberately (`docs/ux/drawing-tools-2026-08.md` §D6).
    #[test]
    fn the_magnet_is_a_rail_toggle_that_starts_off() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        let mut drawings = Drawings::default();
        assert!(!rail.magnet(), "the magnet opens off");

        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
        let button = rail
            .magnet_rect
            .expect("the magnet button rendered")
            .center();
        click_at(&mut rail, &mut drawings, &ctx, screen, button);
        assert!(rail.magnet());
        click_at(&mut rail, &mut drawings, &ctx, screen, button);
        assert!(!rail.magnet(), "the same button turns it back off");
    }

    /// The scripted-validation hook (`QUANTICK_DRAWING_MAGNET`) sets the same
    /// flag the button does; nothing may fork the two.
    #[test]
    fn the_hook_setter_moves_the_same_flag_the_button_does() {
        let mut rail = ToolRail::new();
        rail.set_magnet(true);
        assert!(rail.magnet());
        rail.set_magnet(false);
        assert!(!rail.magnet());
    }

    #[test]
    fn stages_are_pure_functions_of_extent() {
        let slots = tool_slots().len();
        for extent in [100.0_f32, 200.0, 380.9, 381.0, 560.9, 561.0, 1000.0] {
            let first = stage_for(extent, slots);
            let second = stage_for(extent, slots);
            assert_eq!(first, second);
        }
        // The full stage costs one slot more since the Marks family landed.
        assert_eq!(stage_for(561.0, slots), RailStage::Full);
        assert_eq!(stage_for(560.9, slots), RailStage::Compact);
        assert_eq!(stage_for(381.0, slots), RailStage::Compact);
        assert_eq!(stage_for(380.9, slots), RailStage::Minimal);
    }

    #[test]
    fn the_compact_rail_keeps_pointer_crosshair_armed_tool_and_objects() {
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        rail.set_dock(ToolboxDock::Top);
        rail.arm(Tool::Drawing(DRAWING_TOOLS[1]));
        let mut drawings = Drawings::default();

        // 400 px wide: Compact for a horizontal rail.
        let compact = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0));
        rail_frame_with(&mut rail, &mut drawings, &ctx, compact, Vec::new());
        assert!(rail.button_rect(Tool::Pointer).is_some());
        assert!(rail.button_rect(Tool::Crosshair).is_some());
        assert!(rail.button_rect(Tool::Drawing(DRAWING_TOOLS[1])).is_some());
        assert!(
            rail.button_rect(Tool::Drawing(DRAWING_TOOLS[0])).is_none(),
            "unarmed tools give up their slots at Compact"
        );
        assert!(rail.more_rect.is_some());
        assert!(rail.objects_rect.is_some());
        assert!(rail.hide_all_rect.is_some(), "trailing cluster survives");

        // 250 px: Minimal — Crosshair and the globals fold into More.
        let minimal = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(250.0, 600.0));
        rail_frame_with(&mut rail, &mut drawings, &ctx, minimal, Vec::new());
        assert!(rail.button_rect(Tool::Pointer).is_some());
        assert!(rail.button_rect(Tool::Crosshair).is_none());
        assert!(rail.button_rect(Tool::Drawing(DRAWING_TOOLS[1])).is_some());
        assert!(rail.hide_all_rect.is_none());
        assert!(rail.lock_all_rect.is_none());
        assert!(rail.objects_rect.is_some());

        // Wide again: every slot returns.
        let wide = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        rail_frame_with(&mut rail, &mut drawings, &ctx, wide, Vec::new());
        for slot in tool_slots() {
            let shown = match slot {
                RailSlot::Single(tool) => *tool,
                RailSlot::Family { family, members } => {
                    rail.family_member(*family, members).unwrap_or(members[0])
                }
            };
            assert!(
                rail.button_rect(Tool::Drawing(shown)).is_some(),
                "{} lost its slot on a wide rail",
                shown.id()
            );
        }
    }

    #[test]
    fn the_more_flyout_lists_exactly_what_each_stage_swallowed() {
        let mut rail = ToolRail::new();
        rail.arm(Tool::Drawing(DRAWING_TOOLS[1]));

        let compact = rail.swallowed_tools(RailStage::Compact);
        assert!(!compact.contains(&Tool::Crosshair));
        assert!(!compact.contains(&Tool::Drawing(DRAWING_TOOLS[1])));
        for tool in DRAWING_TOOLS {
            if tool != DRAWING_TOOLS[1] {
                assert!(compact.contains(&Tool::Drawing(tool)));
            }
        }

        let minimal = rail.swallowed_tools(RailStage::Minimal);
        assert!(minimal.contains(&Tool::Crosshair));
        assert!(!minimal.contains(&Tool::Drawing(DRAWING_TOOLS[1])));
    }

    #[test]
    fn orientation_changes_positions_never_inventory() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
        let mut inventories: Vec<Vec<&'static str>> = Vec::new();
        for dock in [
            ToolboxDock::Left,
            ToolboxDock::Right,
            ToolboxDock::Top,
            ToolboxDock::Bottom,
        ] {
            let ctx = egui::Context::default();
            let mut rail = ToolRail::new();
            rail.set_dock(dock);
            let mut drawings = Drawings::default();
            rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());
            let mut tools: Vec<&'static str> = rail
                .button_rects
                .iter()
                .flatten()
                .map(|(tool, _)| tool.name())
                .collect();
            tools.sort_unstable();
            inventories.push(tools);
        }
        assert!(
            inventories.windows(2).all(|pair| pair[0] == pair[1]),
            "every dock shows the same buttons at the same extent: {inventories:?}"
        );
    }

    #[test]
    fn the_grip_leads_and_objects_trails_in_every_dock() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
        for dock in [
            ToolboxDock::Left,
            ToolboxDock::Right,
            ToolboxDock::Top,
            ToolboxDock::Bottom,
        ] {
            let ctx = egui::Context::default();
            let mut rail = ToolRail::new();
            rail.set_dock(dock);
            let mut drawings = Drawings::default();
            rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

            let grip = rail.grip_rect.expect("grip rendered");
            let objects = rail.objects_rect.expect("objects rendered");
            let rail_rect = rail.rail_rect.expect("rail rect recorded");
            let along = |rect: egui::Rect| {
                if dock.is_vertical() {
                    rect.top()
                } else {
                    rect.left()
                }
            };
            for (_, rect) in rail.button_rects.iter().flatten() {
                assert!(
                    along(grip) <= along(*rect),
                    "the grip precedes every button in {dock:?}"
                );
            }
            // The trailing cluster is pinned: Objects sits one margin off
            // the rail's far end.
            let far_gap = if dock.is_vertical() {
                rail_rect.bottom() - objects.bottom()
            } else {
                rail_rect.right() - objects.right()
            };
            assert!(
                (far_gap - TOOLBOX_MARGIN_PX).abs() < 0.6,
                "objects must trail one margin off the rail end in {dock:?}, gap {far_gap}"
            );
        }
    }

    #[test]
    fn every_dock_position_reserves_space_outside_the_central_chart() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        for dock in [
            ToolboxDock::Left,
            ToolboxDock::Right,
            ToolboxDock::Top,
            ToolboxDock::Bottom,
        ] {
            let ctx = egui::Context::default();
            let mut rail = ToolRail {
                tool: Tool::Pointer,
                visible: true,
                dock,
                ..ToolRail::default()
            };
            let mut central = egui::Rect::NOTHING;
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut drawings = Drawings::default();
            let mut manager_open = false;
            let _ = ctx.run(input, |ctx| {
                rail.draw(ctx, &mut drawings, &mut manager_open);
                egui::CentralPanel::default().show(ctx, |ui| {
                    central = ui.max_rect();
                });
            });
            match dock {
                ToolboxDock::Left => assert!(central.left() >= TOOLBOX_THICKNESS_PX),
                ToolboxDock::Right => {
                    assert!(central.right() <= screen.right() - TOOLBOX_THICKNESS_PX);
                }
                ToolboxDock::Top => assert!(central.top() >= TOOLBOX_THICKNESS_PX),
                ToolboxDock::Bottom => {
                    assert!(central.bottom() <= screen.bottom() - TOOLBOX_THICKNESS_PX);
                }
            }
        }
    }

    #[test]
    fn global_eye_and_lock_buttons_protect_every_drawing_without_deleting() {
        use crate::drawings::ChartPoint;

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        let mut rail = ToolRail::new();
        let mut drawings = Drawings::default();
        // Registry-order-proof: place whatever the first tool needs rather
        // than assuming it is a one-click tool.
        let tool = DRAWING_TOOLS[0];
        for anchor in 0..tool.required_points() {
            drawings.place(tool, ChartPoint::at(anchor as f32, 100.0 + anchor as f64));
        }
        assert_eq!(drawings.items().len(), 1, "the object was placed");
        rail_frame_with(&mut rail, &mut drawings, &ctx, screen, Vec::new());

        let eye = rail.hide_all_rect.expect("hide-all rendered").center();
        click_at(&mut rail, &mut drawings, &ctx, screen, eye);
        assert!(drawings.all_hidden(), "hide-all engages the global layer");
        click_at(&mut rail, &mut drawings, &ctx, screen, eye);
        assert!(!drawings.all_hidden(), "hide-all toggles back off");

        let lock = rail.lock_all_rect.expect("lock-all rendered").center();
        click_at(&mut rail, &mut drawings, &ctx, screen, lock);
        assert!(drawings.all_locked(), "lock-all locks every drawing");
        click_at(&mut rail, &mut drawings, &ctx, screen, lock);
        assert!(!drawings.all_locked(), "lock-all toggles back to unlocked");
        assert_eq!(
            drawings.items().len(),
            1,
            "global protections must never delete"
        );
    }
}
