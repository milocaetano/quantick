//! The chain at the toolbar's right end: the floppy (while the workspace holds
//! unsaved edits), the +, the live layout miniature and the tab's workspace,
//! each its own button, linked in the workspace's colour (design A-3).
//!
//! It draws what the app hands it and answers what was pressed; the dirty
//! comparison, the saved layout and the tree it pictures are the headless
//! model's.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_appearance::{PremultipliedRgba, layout_picker::ColourInks};
use quantick_workspace::canvas::PaneTree;

use super::ToolbarAction;
use crate::theme;
use quantick_appearance::{Token, tab_strip::WORKSPACE_CAPSULE_BAR_PT as BAR};
use quantick_ui_model::layout_picker as picker;

/// The capsule's height: History's.
const HEIGHT_PX: f32 = 22.0;
/// The live layout miniature, inside the layout button.
const MINIATURE: egui::Vec2 = egui::vec2(40.0, 14.0);
/// Air around the miniature, the name and the carets.
const PAD_PX: f32 = 7.0;
/// The link between two buttons of the chain: its length and thickness.
const LINK_PX: f32 = 8.0;
const LINK_WIDTH_PX: f32 = 2.0;
/// The unsaved dot's radius.
const DOT_RADIUS_PX: f32 = 2.5;
/// Rounding of the chain's buttons.
const ROUNDING_PX: f32 = 4.0;

/// What the capsule shows this frame.
pub struct CapsuleModel<'a> {
    /// The docked panes the canvas draws, as the model's tree.
    pub tree: &'a PaneTree,
    /// The tab's workspace name, and its colour: the tint, border and left bar.
    pub workspace: &'a str,
    pub colour: Option<quantick_workspace::workspace_colour::WorkspaceColour>,
    /// Auto save is off and the workspace differs from its save.
    pub unsaved: bool,
    /// What differs is the layout: the miniature wears the dot too.
    pub layout_unsaved: bool,
    /// The workspace menu hangs from the name.
    pub menu_open: bool,
    /// The layout popover hangs from the miniature.
    pub popover_open: bool,
    /// The ADD TO CANVAS menu hangs from the + left of the capsule.
    pub add_open: bool,
    /// A short row: the name gives way to its caret (its hover still names
    /// it), so TRADE keeps its place.
    pub compact: bool,
}

/// Where the capsule's parts ended up, for the popover, the workspace menu
/// and the observer scene.
#[derive(Clone, Copy, Debug)]
pub struct CapsuleRects {
    pub add: Option<egui::Rect>,
    pub floppy: Option<egui::Rect>,
    pub miniature: egui::Rect,
    pub name: egui::Rect,
}

impl Default for CapsuleRects {
    fn default() -> Self {
        Self {
            add: None,
            floppy: None,
            miniature: egui::Rect::NOTHING,
            name: egui::Rect::NOTHING,
        }
    }
}

/// The name the capsule paints: none while compact.
fn shown_name<'a>(model: &CapsuleModel<'a>) -> &'a str {
    if model.compact { "" } else { model.workspace }
}

/// The layout and workspace buttons' widths: air, face and caret.
fn widths(ui: &egui::Ui, model: &CapsuleModel<'_>) -> (f32, f32) {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = |text: &str| {
        let galley =
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE);
        galley.size().x
    };
    let caret = text(icons::CARET_DOWN);
    let name = text(shown_name(model));
    let layout = PAD_PX + MINIATURE.x + PAD_PX + caret + PAD_PX;
    (layout, PAD_PX + name + PAD_PX + caret + PAD_PX)
}

/// The width [`draw`] takes, + and floppy included, for the collapse plan.
pub fn width(ui: &egui::Ui, model: &CapsuleModel<'_>) -> f32 {
    let (layout, workspace) = widths(ui, model);
    let floppy = if model.unsaved {
        LINK_PX + HEIGHT_PX
    } else {
        0.0
    };
    workspace + LINK_PX + layout + LINK_PX + crate::add_widget::BUTTON_PX + floppy
}

/// A short line in `colour` across the row's middle, joining two buttons.
fn link(ui: &mut egui::Ui, colour: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(LINK_PX, HEIGHT_PX), egui::Sense::hover());
    let y = rect.center().y.round();
    ui.painter()
        .hline(rect.x_range(), y, (LINK_WIDTH_PX, colour));
}

/// Draw the chain right to left (the toolbar's right run grows inwards):
/// workspace, layout, +, then the floppy while unsaved, each its own button,
/// joined by short links in the workspace's colour.
pub fn draw(
    ui: &mut egui::Ui,
    model: &CapsuleModel<'_>,
    actions: &mut Vec<ToolbarAction>,
) -> CapsuleRects {
    let palette = theme::get(ui.ctx());
    let theme = &palette.resolved;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let caret = |ui: &egui::Ui, rect: egui::Rect| {
        let at = egui::pos2(rect.right() - PAD_PX, rect.center().y);
        let (anchor, ink) = (egui::Align2::RIGHT_CENTER, palette.text_muted());
        ui.painter()
            .text(at, anchor, icons::CARET_DOWN, font.clone(), ink);
    };
    let stroke = ui.visuals().widgets.inactive.bg_stroke;
    let hovered = ui.visuals().widgets.hovered.weak_bg_fill;
    // A workspace's colour tints the two fills and takes every border and link.
    let fill = |lit: bool| match model.colour {
        Some(colour) => {
            let [r, g, b, a] = quantick_stores::workspaces::capsule_fill(colour, theme, lit);
            egui::Color32::from_rgba_premultiplied(r, g, b, a)
        }
        None if lit => hovered,
        None => palette.control(),
    };
    let edge: Option<egui::Stroke> = model.colour.map(|colour| {
        let [r, g, b] = colour.rgb(theme.dark);
        (stroke.width, egui::Color32::from_rgb(r, g, b)).into()
    });
    let border = edge.unwrap_or(stroke);
    let (layout_w, name_w) = widths(ui, model);
    let (name_rect, menu) =
        ui.allocate_exact_size(egui::vec2(name_w, HEIGHT_PX), egui::Sense::click());
    let lit = model.menu_open || menu.hovered();
    ui.painter().rect(name_rect, ROUNDING_PX, fill(lit), border);
    if let Some(edge) = edge {
        let bar = ui
            .painter()
            .with_clip_rect(name_rect.with_max_x(name_rect.left() + BAR));
        bar.rect_filled(name_rect, ROUNDING_PX, edge.color);
    }
    let name = ui.painter().layout_no_wrap(
        shown_name(model).to_owned(),
        font.clone(),
        palette.text_primary(),
    );
    let mid = name_rect.center().y;
    let text_at = egui::pos2(name_rect.left() + PAD_PX, mid - name.size().y / 2.0);
    ui.painter().galley(text_at, name, palette.text_primary());
    caret(ui, name_rect);
    if menu
        .on_hover_text(format!("Workspace \u{201c}{}\u{201d}", model.workspace))
        .clicked()
    {
        actions.push(ToolbarAction::ToggleWorkspaceMenu);
    }
    link(ui, border.color);
    let (miniature, layout) =
        ui.allocate_exact_size(egui::vec2(layout_w, HEIGHT_PX), egui::Sense::click());
    let ground = fill(model.popover_open || layout.hovered());
    ui.painter().rect(miniature, ROUNDING_PX, ground, border);
    let face = egui::pos2(miniature.left() + PAD_PX, mid - MINIATURE.y / 2.0);
    let face = egui::Rect::from_min_size(face.round(), MINIATURE);
    let at = picker::Rect::new(face.left(), face.top(), face.width(), face.height());
    let tree = picker::miniature(model.tree, at, model.colour, ground.to_array(), theme);
    super::layout_popover::paint(ui.painter(), egui::Pos2::ZERO, &tree);
    if model.layout_unsaved {
        ui.painter()
            .circle_filled(face.right_top(), DOT_RADIUS_PX, palette.accent());
    }
    caret(ui, miniature);
    if layout.on_hover_text("Layout (Ctrl+Shift+1…)").clicked() {
        actions.push(ToolbarAction::ToggleLayoutPopover);
    }
    link(ui, border.color);
    let add = crate::add_widget::button(ui, model.add_open, edge);
    if add.clicked() {
        actions.push(ToolbarAction::ToggleAddMenu);
    }
    let floppy = model.unsaved.then(|| {
        link(ui, border.color);
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(HEIGHT_PX), egui::Sense::click());
        let painter = ui.painter();
        let fill = if response.hovered() {
            palette.control()
        } else {
            ui.visuals().widgets.inactive.weak_bg_fill
        };
        painter.rect(rect, ROUNDING_PX, fill, border);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::FLOPPY_DISK,
            egui::FontId::proportional(13.0),
            palette.text_primary(),
        );
        painter.circle_filled(
            rect.right_top() + egui::vec2(-3.0, 3.0),
            DOT_RADIUS_PX,
            palette.accent(),
        );
        if response
            .on_hover_text(format!("Save \u{201c}{}\u{201d} (Ctrl+S)", model.workspace))
            .clicked()
        {
            actions.push(ToolbarAction::SaveWorkspace);
        }
        rect
    });
    CapsuleRects {
        add: Some(add.rect),
        floppy,
        miniature,
        name: name_rect,
    }
}

/// The tree's panes inside `rect`, exactly as the canvas lays them out: a
/// timeframe chart outlined, the flow pane filled with `flow`'s tint.
pub fn paint_tree(painter: &egui::Painter, rect: egui::Rect, tree: &PaneTree, flow: Token) {
    let palette = theme::get(painter.ctx());
    let ink = |colour: egui::Color32| PremultipliedRgba::from_premultiplied(colour.to_array());
    let inks = ColourInks::panes(ink(*palette.get(flow)), ink(palette.text_faint()));
    let at = picker::Rect::new(rect.left(), rect.top(), rect.width(), rect.height());
    super::layout_popover::paint(
        painter,
        egui::Pos2::ZERO,
        &picker::thumbnail(tree, at, &inks),
    );
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A capsule for a toolbar under test: the flow pane alone, saved.
    pub(crate) fn model() -> CapsuleModel<'static> {
        CapsuleModel {
            tree: crate::canvas_layout::LAYOUT_PRESETS[0].tree,
            workspace: "Default",
            colour: None,
            unsaved: false,
            layout_unsaved: false,
            menu_open: false,
            popover_open: false,
            add_open: false,
            compact: false,
        }
    }
}
