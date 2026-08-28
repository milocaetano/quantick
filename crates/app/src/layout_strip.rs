//! The layout strip: one tab per layout along the bottom of the canvas,
//! above the status bar — the door to [`crate::layouts`] a trader reaches
//! with the mouse.
//!
//! Drawn from the book and nothing else: a tab per layout in strip order,
//! the active one lit, a `+` after the last. Click switches; double-click
//! renames in place; the context menu holds Rename and Delete, so the two
//! rarer actions are discoverable without a hover hint. Every action goes
//! back to the app as a [`StripAction`] and is applied there — the strip
//! never edits the book, which is what keeps the keyboard, the menu and the
//! control plane on the same code path as the click.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::layouts::{ChartLayout, LayoutId, MAX_LAYOUT_NAME};
use crate::theme;

/// The strip's height, in pixels. One text line and its padding: a strip,
/// not a toolbar.
pub const STRIP_HEIGHT: f32 = 24.0;
/// Horizontal padding inside a tab.
const TAB_PAD_X_PX: f32 = 10.0;
/// Gap between tabs.
const TAB_GAP_PX: f32 = 2.0;
/// Corner radius of a tab's background.
const TAB_RADIUS_PX: f32 = 3.0;
/// The accent rule under the active tab — the same 1 px the focused pane
/// wears, so "which one is on" reads the same way twice.
const ACTIVE_RULE_PX: f32 = 1.5;
/// Width of the rename box: room for [`MAX_LAYOUT_NAME`] characters of body
/// text, so the longest legal name is typed without scrolling.
const RENAME_WIDTH_PX: f32 = 150.0;
/// Font size of a tab's label — the status bar's, so the two strips read as
/// one band.
const LABEL_SIZE_PX: f32 = 11.5;
/// Size of the `+` glyph, a touch larger than the labels so it reads as a
/// button rather than a tab called "+".
const ADD_ICON_SIZE_PX: f32 = 12.0;
/// How much brighter than the chrome the active tab's background is.
const ACTIVE_TAB_BRIGHTNESS: f32 = 1.6;
/// How much brighter than the chrome a hovered tab's background is.
const HOVER_TAB_BRIGHTNESS: f32 = 1.3;
/// Vertical inset of a tab's background inside the strip.
const TAB_INSET_Y_PX: f32 = 2.0;
/// Horizontal inset of the active rule inside its tab.
const RULE_INSET_X_PX: f32 = 2.0;

/// What the app hands the strip each frame.
pub struct StripModel<'a> {
    pub layouts: &'a [ChartLayout],
    pub active: LayoutId,
    /// The layout under rename and its draft, owned by the app so a rename
    /// begun from the keyboard or a hook opens the same box.
    pub rename: &'a mut Option<(LayoutId, String)>,
    /// Whether another layout may be added.
    pub can_add: bool,
    /// Whether a layout may be deleted (never the last).
    pub can_delete: bool,
}

/// What the strip asked for this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripAction {
    Switch(LayoutId),
    Create,
    BeginRename(LayoutId),
    CommitRename(LayoutId, String),
    CancelRename,
    Delete(LayoutId),
}

/// Draw the strip and collect what it asked for.
pub fn draw(ctx: &egui::Context, model: StripModel<'_>) -> Vec<StripAction> {
    let mut actions = Vec::new();
    egui::TopBottomPanel::bottom("layout_strip")
        .exact_height(STRIP_HEIGHT)
        .frame(
            egui::Frame::none()
                .fill(theme::CHROME)
                .inner_margin(egui::Margin::symmetric(6.0, 0.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = TAB_GAP_PX;
                for layout in model.layouts {
                    let renaming = model
                        .rename
                        .as_ref()
                        .is_some_and(|(id, _)| *id == layout.id);
                    if renaming {
                        draw_rename_box(ui, layout.id, model.rename, &mut actions);
                    } else {
                        draw_tab(
                            ui,
                            layout,
                            layout.id == model.active,
                            model.can_delete,
                            &mut actions,
                        );
                    }
                }
                let add = ui.add_enabled(
                    model.can_add,
                    egui::Button::new(
                        egui::RichText::new(icons::PLUS)
                            .size(ADD_ICON_SIZE_PX)
                            .color(theme::TEXT_MUTED),
                    )
                    .frame(false),
                );
                let add = add
                    .on_hover_text("New layout")
                    .on_disabled_hover_text("The workspace holds as many layouts as it can");
                if add.clicked() {
                    actions.push(StripAction::Create);
                }
            });
        });
    actions
}

fn draw_tab(
    ui: &mut egui::Ui,
    layout: &ChartLayout,
    active: bool,
    can_delete: bool,
    actions: &mut Vec<StripAction>,
) {
    let text = egui::RichText::new(&layout.name)
        .size(LABEL_SIZE_PX)
        .color(if active {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_MUTED
        });
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let size = egui::vec2(galley.size().x + 2.0 * TAB_PAD_X_PX, ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_text("Click to switch · double-click to rename");
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if active || response.hovered() {
            painter.rect_filled(
                rect.shrink2(egui::vec2(0.0, TAB_INSET_Y_PX)),
                TAB_RADIUS_PX,
                if active {
                    theme::CHROME.gamma_multiply(ACTIVE_TAB_BRIGHTNESS)
                } else {
                    theme::CHROME.gamma_multiply(HOVER_TAB_BRIGHTNESS)
                },
            );
        }
        let text_pos = egui::pos2(
            rect.left() + TAB_PAD_X_PX,
            rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(text_pos, galley, theme::TEXT_PRIMARY);
        if active {
            // A filled rule rather than a stroked segment: it is chrome, and
            // it must not read as a drawing to anything counting strokes.
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(
                        rect.left() + RULE_INSET_X_PX,
                        rect.bottom() - ACTIVE_RULE_PX,
                    ),
                    egui::pos2(rect.right() - RULE_INSET_X_PX, rect.bottom()),
                ),
                0.0,
                theme::ACCENT,
            );
        }
    }
    if response.double_clicked() {
        actions.push(StripAction::BeginRename(layout.id));
    } else if response.clicked() && !active {
        actions.push(StripAction::Switch(layout.id));
    }
    response.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            actions.push(StripAction::BeginRename(layout.id));
            ui.close_menu();
        }
        let delete = ui
            .add_enabled(can_delete, egui::Button::new("Delete"))
            .on_disabled_hover_text("The last layout stays");
        if delete.clicked() {
            actions.push(StripAction::Delete(layout.id));
            ui.close_menu();
        }
    });
}

fn draw_rename_box(
    ui: &mut egui::Ui,
    id: LayoutId,
    rename: &mut Option<(LayoutId, String)>,
    actions: &mut Vec<StripAction>,
) {
    let Some((_, draft)) = rename.as_mut() else {
        return;
    };
    let edit = egui::TextEdit::singleline(draft)
        .desired_width(RENAME_WIDTH_PX)
        .char_limit(MAX_LAYOUT_NAME)
        .font(egui::TextStyle::Body)
        .hint_text("Layout name");
    let response = ui.add(edit);
    if !response.has_focus() && !response.lost_focus() {
        response.request_focus();
    }
    // Consumed, not read: a key the box answered must not fall through to
    // the chart, where Escape drops a selection and Enter places an anchor.
    let escape = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let enter =
        !escape && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    if escape {
        actions.push(StripAction::CancelRename);
    } else if enter || response.lost_focus() {
        actions.push(StripAction::CommitRename(id, draft.clone()));
    }
}
