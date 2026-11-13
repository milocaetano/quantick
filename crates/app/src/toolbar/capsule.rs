//! The capsule at the toolbar's right end: the live layout miniature and the
//! tab's workspace in one bordered control, with the floppy glued to its left
//! while the workspace holds unsaved edits (designer 01, idea 1).
//!
//! It draws what the app hands it and answers what was pressed; the dirty
//! comparison, the saved layout and the tree it pictures are the headless
//! model's.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_workspace::canvas::{CanvasRect, PaneKind, PaneTree};

use super::ToolbarAction;
use crate::theme;
use quantick_appearance::tab_strip::{WORKSPACE_CAPSULE_BAR_PT as BAR, WORKSPACE_TINT};

/// The capsule's height: History's.
const HEIGHT_PX: f32 = 22.0;
/// The live layout miniature, inside the capsule's left half.
const MINIATURE: egui::Vec2 = egui::vec2(40.0, 14.0);
/// Air either side of the miniature and the name.
const PAD_PX: f32 = 7.0;
/// Gap between the floppy and the capsule.
const FLOPPY_GAP_PX: f32 = 4.0;
/// The unsaved dot's radius.
const DOT_RADIUS_PX: f32 = 2.5;
/// Rounding of the capsule and the floppy.
const ROUNDING_PX: f32 = 4.0;
/// Stroke of a miniature pane.
const PANE_STROKE_PX: f32 = 1.0;

/// What the capsule shows this frame.
pub struct CapsuleModel<'a> {
    /// The docked panes the canvas draws, as the model's tree.
    pub tree: &'a PaneTree,
    /// The tab's workspace name, and its colour on the capsule's underline.
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

/// The width [`draw`] takes, floppy included, for the collapse plan.
pub fn width(ui: &egui::Ui, model: &CapsuleModel<'_>) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = |text: &str| {
        let galley =
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE);
        galley.size().x
    };
    let floppy = if model.unsaved {
        HEIGHT_PX + FLOPPY_GAP_PX
    } else {
        0.0
    };
    PAD_PX * 5.0 + MINIATURE.x + 1.0 + text(shown_name(model)) + text(icons::CARET_DOWN) + floppy
}

/// Draw the capsule right to left (the toolbar's right run grows inwards).
pub fn draw(
    ui: &mut egui::Ui,
    model: &CapsuleModel<'_>,
    actions: &mut Vec<ToolbarAction>,
) -> CapsuleRects {
    let palette = theme::get(ui.ctx());
    let font = egui::TextStyle::Button.resolve(ui.style());
    let name = ui.painter().layout_no_wrap(
        shown_name(model).to_owned(),
        font.clone(),
        palette.text_primary(),
    );
    let caret =
        ui.painter()
            .layout_no_wrap(icons::CARET_DOWN.to_owned(), font, palette.text_muted());
    let name_w = PAD_PX + name.size().x + PAD_PX + caret.size().x + PAD_PX;
    let mini_w = PAD_PX + MINIATURE.x + PAD_PX;
    let (whole, _) = ui.allocate_exact_size(
        egui::vec2(mini_w + 1.0 + name_w, HEIGHT_PX),
        egui::Sense::hover(),
    );
    let miniature = egui::Rect::from_min_size(whole.min, egui::vec2(mini_w, HEIGHT_PX));
    let name_rect =
        egui::Rect::from_min_max(egui::pos2(miniature.right() + 1.0, whole.top()), whole.max);
    let layout = ui.interact(
        miniature,
        ui.id().with("capsule_layout"),
        egui::Sense::click(),
    );
    let menu = ui.interact(
        name_rect,
        ui.id().with("capsule_workspace"),
        egui::Sense::click(),
    );
    let stroke = ui.visuals().widgets.inactive.bg_stroke;
    let painter = ui.painter();
    // A workspace's colour tints the fill (as `workspace_tint`) and takes the border.
    let (fill, edge) = match model.colour.and_then(|c| palette.workspace(c.index())) {
        Some(ink) => (
            palette.control().lerp_to_gamma(ink, WORKSPACE_TINT),
            (stroke.width, ink).into(),
        ),
        None => (palette.control(), stroke),
    };
    painter.rect(whole, ROUNDING_PX, fill, edge);
    if model.colour.is_some() {
        let bar = painter.with_clip_rect(whole.with_max_x(whole.left() + BAR));
        bar.rect_filled(whole, ROUNDING_PX, edge.color);
    }
    for (part, response, lit) in [
        (miniature, &layout, model.popover_open),
        (name_rect, &menu, model.menu_open),
    ] {
        if lit || response.hovered() {
            painter.rect_filled(part.shrink(1.0), ROUNDING_PX, fill);
        }
    }
    painter.vline(miniature.right() + 0.5, whole.y_range().shrink(4.0), stroke);
    let face = egui::Rect::from_center_size(miniature.center(), MINIATURE);
    paint_tree(painter, face, model.tree);
    if model.layout_unsaved {
        painter.circle_filled(face.right_top(), DOT_RADIUS_PX, palette.accent());
    }
    let text_left = name_rect.left() + PAD_PX;
    painter.galley(
        egui::pos2(text_left, whole.center().y - name.size().y / 2.0),
        name,
        palette.text_primary(),
    );
    painter.galley(
        egui::pos2(
            name_rect.right() - PAD_PX - caret.size().x,
            whole.center().y - caret.size().y / 2.0,
        ),
        caret,
        palette.text_muted(),
    );
    if layout.on_hover_text("Layout (Ctrl+Shift+1…)").clicked() {
        actions.push(ToolbarAction::ToggleLayoutPopover);
    }
    if menu
        .on_hover_text(format!("Workspace \u{201c}{}\u{201d}", model.workspace))
        .clicked()
    {
        actions.push(ToolbarAction::ToggleWorkspaceMenu);
    }
    let floppy = model.unsaved.then(|| {
        ui.add_space(FLOPPY_GAP_PX);
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(HEIGHT_PX), egui::Sense::click());
        let painter = ui.painter();
        let fill = if response.hovered() {
            palette.control()
        } else {
            ui.visuals().widgets.inactive.weak_bg_fill
        };
        painter.rect(rect, ROUNDING_PX, fill, stroke);
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
        add: None,
        floppy,
        miniature,
        name: name_rect,
    }
}

/// The tree's panes inside `rect`, exactly as the canvas lays them out: a
/// timeframe chart outlined, the flow pane filled with the accent.
pub fn paint_tree(painter: &egui::Painter, rect: egui::Rect, tree: &PaneTree) {
    let palette = theme::get(painter.ctx());
    let kinds = tree.panes();
    let area = CanvasRect::new(rect.left(), rect.top(), rect.width(), rect.height());
    for (slot, pane) in tree.rects(area) {
        let block =
            egui::Rect::from_min_size(egui::pos2(pane.x, pane.y), egui::vec2(pane.w, pane.h));
        let flow = kinds
            .iter()
            .any(|(held, kind)| *held == slot && *kind == PaneKind::Flow);
        let (fill, line) = if flow {
            (theme::active_tint(palette.accent()), palette.accent())
        } else {
            (egui::Color32::TRANSPARENT, palette.text_faint())
        };
        painter.rect(
            block.shrink(0.5),
            1.0,
            fill,
            egui::Stroke::new(PANE_STROKE_PX, line),
        );
    }
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
