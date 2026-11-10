//! Drawing-tool selection and the four-edge toolbar rail.
//!
//! The rail owns only chrome state. Drawing definitions and metadata live in
//! [`crate::drawings`], so registering a new drawing does not require another
//! matching list in this module. Geometry, states and docking behaviour
//! follow `docs/drawing-toolbar-ux.md`.

use std::collections::BTreeMap;

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::drawings::{
    DRAWING_TOOLS, DrawingTool, Drawings, IconDots, IconLetter, IconStrokes, ToolFamily,
};
use crate::theme;
use crate::widgets::{IconButton, MarkerEdge, TOOLRAIL_ICON};
use quantick_chart_interaction::scripted_hand::Area;
pub use quantick_chart_interaction::tool_rail::ToolboxDock;
use quantick_chart_interaction::tool_rail::{
    BAND_ARROW_LENGTH_PX, BandWindow, RailStage, TOOLBOX_GRIP_LENGTH_PX, TOOLBOX_ITEM_GAP_PX,
    TOOLBOX_MARGIN_PX, TOOLBOX_THICKNESS_PX, anchored_favorites, band_max_offset, band_scroll_step,
    band_viewport, band_visible_items, stage_for,
};
pub(crate) use quantick_chart_interaction::tool_rail::{RailControl, RailControlKind};

mod band;
mod flyout;
mod state;
#[cfg(any(feature = "drawing-harness", test))]
pub(crate) use state::ToolRailLaunch;

/// Inset of a separator hairline from each rail edge, so the rule floats.
const TOOLBOX_SEPARATOR_INSET_PX: f32 = 8.0;
/// Grip glyph font size.
const TOOLBOX_GRIP_GLYPH_PX: f32 = 14.0;
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
/// The favorite star holds the row's right edge (trader feedback: beside
/// the name, not on the icon), with the shortcut label stepped one slot
/// left so the two never collide. The hit zone is bigger than the glyph —
/// a 9 px star is no hit target — and stays clear of the icon and name, so
/// an arming click can never silently star.
/// Side of the icon box in a flyout row — the smallest box a vector icon is
/// ever painted into, and therefore the size the icon guards measure against.
pub(crate) const FLYOUT_ICON_BOX_PX: f32 = FLYOUT_GLYPH_PX - 4.0;
const FLYOUT_STAR_PX: f32 = 9.0;
const FLYOUT_STAR_RIGHT_INSET_PX: f32 = 10.0;
const FLYOUT_STAR_HIT_PX: f32 = 14.0;
/// Width the star column reserves at the row's right end; the shortcut
/// label right-aligns just left of it.
const FLYOUT_STAR_SLOT_PX: f32 = 20.0;
/// The corner star badge marking a pinned button in the rail's favorites
/// section.
const FAVORITE_BADGE_PX: f32 = 8.0;
const FAVORITE_BADGE_INSET_PX: f32 = 3.0;
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
/// Chevron glyph size, and the inset of the band's fade hint from its edge.
const BAND_ARROW_GLYPH_PX: f32 = 12.0;
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
    /// The stable wire name, for the places that name a tool rather than draw
    /// it. The drawing tools lend their own registered identifier, so a tool
    /// is called the same thing in the rail, in a saved workspace and in the
    /// semantic scene.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Crosshair => "crosshair",
            Self::Drawing(tool) => tool.id(),
        }
    }

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
    fn icon_strokes(self) -> IconStrokes {
        match self {
            Self::Pointer | Self::Crosshair => &[],
            Self::Drawing(tool) => tool.icon_strokes(),
        }
    }

    #[must_use]
    fn icon_dots(self) -> IconDots {
        match self {
            Self::Pointer | Self::Crosshair => &[],
            Self::Drawing(tool) => tool.icon_dots(),
        }
    }

    #[must_use]
    fn icon_letter(self) -> Option<IconLetter> {
        match self {
            Self::Pointer | Self::Crosshair => None,
            Self::Drawing(tool) => tool.icon_letter(),
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
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
                let key = shortcut.key.to_string();
                if shortcut.shift {
                    format!("Shift+{key}")
                } else {
                    key.to_owned()
                }
            }),
        }
    }
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

/// The magnet's state, and the flip a hand asked for this frame — which the
/// window applies through `annotate.magnet.set`, so it journals like a
/// script's.
#[derive(Debug, Default)]
struct MagnetSwitch {
    on: bool,
    asked: Option<bool>,
}

/// The button edge the active marker hugs: the one facing the window
/// border this dock sits against.
#[must_use]
const fn marker_edge(dock: ToolboxDock) -> MarkerEdge {
    match dock {
        ToolboxDock::Left => MarkerEdge::Left,
        ToolboxDock::Top => MarkerEdge::Top,
        ToolboxDock::Bottom => MarkerEdge::Bottom,
    }
}

/// [`ToolboxDock::nearest`] for egui's pointer and screen.
fn nearest_dock(pointer: egui::Pos2, screen: egui::Rect) -> ToolboxDock {
    let screen = Area::new(screen.left(), screen.top(), screen.right(), screen.bottom());
    ToolboxDock::nearest((pointer.x, pointer.y), screen)
}

/// Toolbar chrome state. The panel is always outside `CentralPanel`, so the
/// chart never renders behind it.
#[derive(Debug)]
pub struct ToolRail {
    tool: Tool,
    visible: bool,
    /// The stage the last draw settled on, or `None` before the first — and
    /// again once the rail is hidden, because nothing is painted then.
    ///
    /// Read by the semantic scene, never by the draw: the stage is a pure
    /// function of the extent and is recomputed every frame regardless.
    last_stage: Option<RailStage>,
    /// Which slice of the scrolling band the last draw actually showed.
    ///
    /// Only the Scroll stage has one; every other stage clears it. The band
    /// clips, so this is the difference between the buttons the rail owns and
    /// the buttons a trader can see — the scene reports the second.
    last_band: Option<BandWindow>,
    dock: ToolboxDock,
    /// The repeat pin: `true` keeps a drawing tool armed after it completes
    /// an object; the default is one-shot back to Pointer.
    repeat: bool,
    /// The magnet: anchors snap to the nearest OHLC of the bar under the
    /// pointer. Off by default — a magnet nobody asked for moves marks the
    /// trader placed deliberately.
    magnet: MagnetSwitch,
    /// Last-armed member of each tool family, keyed by family id.
    last_family_member: BTreeMap<&'static str, DrawingTool>,
    /// Starred tools, in the order the trader starred them — the pinned
    /// section at the rail's tool end. Star order, not registry order, so a
    /// favorite keeps the position the trader learned.
    favorites: Vec<DrawingTool>,
    /// A star was clicked and the choice has not been written down yet. Read
    /// and cleared by [`ToolRail::take_favorites_change`]; set by the toggle
    /// only, never by [`ToolRail::set_favorites`] — restoring the saved list
    /// is not the trader making a choice, and saving it straight back would
    /// rewrite the file on every launch for nothing.
    favorites_changed: bool,
    /// Scroll offset of the tool band along the rail's long axis, in px.
    /// Only the Scroll stage spends it; every other stage clamps it back to
    /// zero, so unstarring back down to a rail that fits leaves no residue.
    band_offset: f32,
    /// An offset a chevron click asked for, handed to the band on the next
    /// frame. `None` leaves the band to the wheel and to drag scrolling.
    band_target: Option<f32>,
    /// The armed tool changed and the band has not yet been asked to show
    /// it. The spec's standing promise is that the armed tool always keeps a
    /// real slot (§2.8); a keyboard shortcut can arm a tool the band has
    /// scrolled past, and a trader who cannot see what is armed does not
    /// know what their next click will draw. Set on arming only, never held,
    /// so scrolling away from the armed tool by hand stays where it was put.
    reveal_armed: bool,
    /// Currently-nearest drop edge while a grip drag is live.
    drag_preview: Option<ToolboxDock>,
    dragging: bool,
    drag_cancelled: bool,
    /// Open family flyout: the family id and the slot rect it anchors to.
    flyout: Option<(&'static str, egui::Rect)>,
    /// A family flyout a validation hook asked for before the first frame —
    /// honoured by the family slot once it knows its rect, then cleared.
    #[cfg(any(feature = "drawing-harness", test))]
    hook_flyout: Option<String>,
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
    #[cfg(test)]
    flyout_star_rects: Vec<(DrawingTool, egui::Rect)>,
    #[cfg(test)]
    favorite_rects: Vec<(DrawingTool, egui::Rect)>,
    /// The band chevrons and whether each one had somewhere to go.
    #[cfg(test)]
    band_leading_arrow: Option<(egui::Rect, bool)>,
    #[cfg(test)]
    band_trailing_arrow: Option<(egui::Rect, bool)>,
    /// The band's viewport. A button is only *reachable* if it lands inside
    /// this — allocation alone proves nothing once the band clips.
    #[cfg(test)]
    band_rect: Option<egui::Rect>,
}

impl Default for ToolRail {
    fn default() -> Self {
        Self {
            tool: Tool::Pointer,
            visible: true,
            last_stage: None,
            last_band: None,
            dock: ToolboxDock::Left,
            repeat: false,
            magnet: MagnetSwitch::default(),
            last_family_member: BTreeMap::new(),
            favorites: Vec::new(),
            favorites_changed: false,
            band_offset: 0.0,
            band_target: None,
            reveal_armed: false,
            drag_preview: None,
            dragging: false,
            drag_cancelled: false,
            flyout: None,
            #[cfg(any(feature = "drawing-harness", test))]
            hook_flyout: None,
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
            #[cfg(test)]
            flyout_star_rects: Vec::new(),
            #[cfg(test)]
            favorite_rects: Vec::new(),
            #[cfg(test)]
            band_leading_arrow: None,
            #[cfg(test)]
            band_trailing_arrow: None,
            #[cfg(test)]
            band_rect: None,
        }
    }
}

impl ToolRail {
    /// Draw the rail docked against its edge. Drag the grip and release: the
    /// nearest window edge becomes the new dock, previewed live by a band.
    /// The rail also hosts the object-manager entry and the global protection
    /// toggles (hide-all / lock-all), which act on the store.
    pub fn draw(&mut self, ctx: &egui::Context, drawings: &mut Drawings, manager_open: &mut bool) {
        let palette = theme::get(ctx);
        if !self.visible {
            return;
        }

        let panel_id = |name| egui::Id::new((ctx.viewport_id(), name));
        match self.dock {
            ToolboxDock::Left => egui::SidePanel::left(panel_id("drawing_toolbox_left"))
                .exact_width(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame(&palette))
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
            ToolboxDock::Top => egui::TopBottomPanel::top(panel_id("drawing_toolbox_top"))
                .exact_height(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame(&palette))
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
            ToolboxDock::Bottom => egui::TopBottomPanel::bottom(panel_id("drawing_toolbox_bottom"))
                .exact_height(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame(&palette))
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
        let palette = theme::get(ui.ctx());
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
            self.flyout_star_rects.clear();
            self.favorite_rects.clear();
            self.band_leading_arrow = None;
            self.band_trailing_arrow = None;
            self.band_rect = None;
        }

        let vertical = self.dock.is_vertical();
        let available = if vertical {
            ui.available_height()
        } else {
            ui.available_width()
        } + 2.0 * TOOLBOX_MARGIN_PX;
        let slots = tool_slots();
        let favorite_count = self.favorites.len();
        let stage = stage_for(available, slots.len(), favorite_count);
        // The one thing the rail writes down for a later reader: which stage
        // it drew. A single enum store per frame, and the only way the
        // semantic scene can answer "what has a button right now" without
        // re-deriving a layout it did not measure.
        self.last_stage = Some(stage);

        // Chart-facing hairline: the only stroke the rail paints — a
        // four-sided stroke would draw a seam against the window edge.
        let rail_rect = ui.max_rect().expand(TOOLBOX_MARGIN_PX);
        let edge = match self.dock {
            ToolboxDock::Left => [rail_rect.right_top(), rail_rect.right_bottom()],
            ToolboxDock::Top => [rail_rect.left_bottom(), rail_rect.right_bottom()],
            ToolboxDock::Bottom => [rail_rect.left_top(), rail_rect.right_top()],
        };
        ui.painter()
            .line_segment(edge, egui::Stroke::new(1.0_f32, palette.border()));

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

        // Both clusters share the same cross-axis band. Sequential layout
        // children would reserve a second row outside a top panel's paint clip.
        let band = ui.max_rect();
        let mut leading_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("leading")
                .max_rect(band)
                .layout(leading),
        );
        {
            let ui = &mut leading_ui;
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
                    self.band_offset = 0.0;
                    self.band_target = None;
                    self.last_band = None;
                    self.draw_favorites_section(ui, vertical, drawings, 0..favorite_count);
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
                RailStage::Scroll => {
                    let anchored = anchored_favorites(available, favorite_count);
                    self.draw_favorites_section(ui, vertical, drawings, 0..anchored);
                    self.draw_band(ui, vertical, drawings, slots, anchored, available);
                }
                RailStage::Compact | RailStage::Minimal => {
                    self.band_offset = 0.0;
                    self.band_target = None;
                    self.last_band = None;
                    if let Some(armed) = self.tool.drawing_tool() {
                        self.draw_button(ui, Tool::Drawing(armed), drawings);
                    }
                    self.draw_more_menu(ui, drawings, stage);
                }
            }
        }

        // The trailing cluster is pinned to the rail's far end — laid from
        // that end backwards, so the flexible gap sits between the clusters.
        let mut trailing_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("trailing")
                .max_rect(band)
                .layout(trailing),
        );
        {
            let ui = &mut trailing_ui;
            ui.spacing_mut().item_spacing = egui::vec2(TOOLBOX_ITEM_GAP_PX, TOOLBOX_ITEM_GAP_PX);
            self.draw_objects_button(ui, drawings, manager_open);
            self.draw_separator(ui, vertical);
            if stage != RailStage::Minimal {
                self.draw_global_buttons(ui, drawings);
                self.draw_repeat_button(ui);
                self.draw_magnet_button(ui);
                self.draw_separator(ui, vertical);
            }
        }
        ui.advance_cursor_after_rect(band);
        self.draw_family_flyout(ui.ctx());
    }

    /// The grip: hold and drag to dock the rail against another window edge.
    fn draw_grip(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let palette = theme::get(ui.ctx());
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
                palette.text_muted()
            } else {
                palette.text_faint()
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                palette.body_font(TOOLBOX_GRIP_GLYPH_PX),
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
                self.drag_preview = Some(nearest_dock(pointer, ui.ctx().screen_rect()));
            }
        }
        if grip.drag_stopped() {
            if self.dragging
                && !self.drag_cancelled
                && let Some(pointer) = ui.ctx().input(|input| input.pointer.interact_pos())
            {
                self.dock = nearest_dock(pointer, ui.ctx().screen_rect());
            }
            self.dragging = false;
            self.drag_cancelled = false;
            self.drag_preview = None;
        }
    }

    /// A separator: a 1 px hairline across the rail, floated off both edges.
    /// The 4 px clear space each side comes from the cluster's item spacing.
    fn draw_separator(&self, ui: &mut egui::Ui, vertical: bool) {
        let palette = theme::get(ui.ctx());
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
            .line_segment([from, to], egui::Stroke::new(1.0_f32, palette.border()));
    }

    /// The repeat pin: keep the armed drawing tool active after it completes
    /// an object, instead of the one-shot return to Pointer.
    fn draw_repeat_button(&mut self, ui: &mut egui::Ui) {
        let response = IconButton::new(icons::ARROW_CLOCKWISE, TOOLRAIL_ICON)
            .active(self.repeat)
            .active_marker(marker_edge(self.dock))
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
            .active(self.magnet.on)
            .active_marker(marker_edge(self.dock))
            .hover_text("Snap anchors to the bar's open / high / low / close")
            .show(ui);
        #[cfg(test)]
        {
            self.magnet_rect = Some(response.rect);
        }
        if response.clicked() {
            self.magnet.asked = Some(!self.magnet.on);
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
                    .add(egui::Button::new("Snap anchors to OHLC").selected(self.magnet.on))
                    .clicked()
                {
                    self.magnet.asked = Some(!self.magnet.on);
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
        let palette = theme::get(ui.ctx());
        let response = IconButton::new(icons::LIST, TOOLRAIL_ICON)
            .active(*manager_open)
            .active_marker(marker_edge(self.dock))
            .hover_text("Drawn objects")
            .show(ui);
        let count = drawings.items().len();
        if count > 0 {
            paint_badge(ui, response.rect, &count.to_string(), palette.text_muted());
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
            .active_marker(marker_edge(self.dock))
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
            .active_marker(marker_edge(self.dock))
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
            .vector_icon(tool.icon_strokes(), tool.icon_dots(), tool.icon_letter())
            .active(self.tool == tool)
            .active_marker(marker_edge(self.dock))
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
        let palette = theme::get(ui.ctx());
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
        paint_badge(ui, response.rect, &text, palette.accent());
        true
    }
}

/// The rail's frame: chrome fill, margins, and no stroke — the chart-facing
/// hairline is painted by the rail itself.
fn rail_frame(palette: &crate::theme::Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(palette.chrome())
        .inner_margin(egui::Margin::same(TOOLBOX_MARGIN_PX))
}

/// The drop preview: the band the rail will occupy on the candidate edge,
/// with an accent line on its chart-facing side.
fn paint_drop_preview(ctx: &egui::Context, target: ToolboxDock) {
    let palette = theme::get(ctx);
    let screen = ctx.screen_rect();
    let thickness = TOOLBOX_THICKNESS_PX;
    let band = match target {
        ToolboxDock::Left => egui::Rect::from_min_max(
            screen.min,
            egui::pos2(screen.left() + thickness, screen.bottom()),
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
    painter.rect_filled(band, 0.0, theme::active_tint(palette.accent()));
    painter.rect_filled(line, 0.0, palette.accent());
}

/// A pill badge in a button's trailing-bottom corner (§2.5).
fn paint_badge(ui: &egui::Ui, button: egui::Rect, text: &str, color: egui::Color32) {
    let palette = theme::get(ui.ctx());
    let painter = ui.painter();
    let font = palette.mono_font(BADGE_TEXT_PX);
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
    painter.rect_filled(rect, egui::Rounding::same(BADGE_RADIUS_PX), palette.inset());
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
mod tests;

#[cfg(any(feature = "drawing-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_DRAWING_TOOL",
    "QUANTICK_DRAWING_MAGNET",
    "QUANTICK_TOOL_FAVORITES",
    "QUANTICK_TOOLBOX_DOCK",
    "QUANTICK_TOOLBAR_SCROLL",
    "QUANTICK_TOOLBOX_FLYOUT",
];
