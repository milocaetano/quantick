//! Native window chrome; market commands still use the existing arrangement owner.
use crate::theme;
use eframe::egui;

use quantick_control_schema::native_window::WindowState;
use quantick_control_schema::window_menu::{AGENT_PILL_CONTROL, AGENT_PILL_LABEL, KEBAB_CONTROL};
mod resize;

#[derive(Default)]
pub(crate) struct TitleBarState {
    pub(crate) maximized: bool,
    pub(crate) minimized: bool,
    pub(crate) surface: quantick_workspace::arrangement::strip::StripSurface,
    pub(crate) strip: crate::tabstrip::StripDrawn,
    pub(crate) controls: Vec<(&'static str, egui::Rect)>,
    pub(crate) requests: quantick_control_schema::native_window::WindowRequests,
    /// Local agent access is on, so the strip shows its pill.
    pub(crate) agent_on: bool,
    pub(crate) menu_open: bool,
    pub(crate) kebab_clicked: bool,
    /// Held down, or its menu is open: the scene and the paint read this one.
    pub(crate) kebab_pressed: bool,
    pub(crate) agent_clicked: bool,
}

impl TitleBarState {
    /// Where a control was painted this frame, by its scene name.
    pub(crate) fn control_rect(&self, id: &str) -> Option<egui::Rect> {
        self.controls
            .iter()
            .find(|(control, _)| *control == id)
            .map(|(_, rect)| *rect)
    }
}

fn send_state(ctx: &egui::Context, state: WindowState) {
    match state {
        WindowState::Normal => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        WindowState::Maximized => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        WindowState::Minimized => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
    }
}

pub(super) const HEIGHT: f32 = 36.0;
pub(super) const WINDOW_BUTTON_WIDTH: f32 = 46.0;
const DRAG_WIDTH: f32 = 32.0;
/// The window buttons' strokes, at the size the Windows Terminal draws its own.
const WINDOW_GLYPH_PX: f32 = 10.0;
/// Keeps the drag region's edge off the `+`, so a press on one never reads as
/// the other.
const DRAG_GAP: f32 = 1.0;
/// The `⋮` button: wide enough to find, as tall as a tab.
pub(super) const KEBAB_WIDTH: f32 = 36.0;
const KEBAB_GLYPH_PX: f32 = 18.0;
const AGENT_PILL_WIDTH: f32 = 72.0;
/// The outline's gap to the pill's hit area, so it reads as a pill on the strip.
const AGENT_OUTLINE_PX: f32 = 1.0;
const AGENT_OUTLINE_INSET: egui::Vec2 = egui::vec2(3.0, 5.0);
const AGENT_TEXT_PX: f32 = 11.0;

/// One chip per tab of this strip. The workspace and its mark read the row the
/// snapshot reads, so the two cannot disagree.
pub(super) fn chips_of<'a>(
    tabs: &'a super::arrangement_host::ArrangementHost,
    workspaces: &quantick_stores::workspaces::WorkspaceBookStore,
    indices: &[usize],
) -> Vec<crate::tabstrip::TabChip<'a>> {
    indices
        .iter()
        .map(|index| {
            let tab = &tabs[*index];
            let id = tabs.id_at(*index);
            let (workspace, modified) = workspaces.tab_row(id);
            crate::tabstrip::TabChip {
                id,
                label: tab.chip_label(),
                replaying: tab.replay.is_some(),
                dot: quantick_workspace::arrangement::strip::tab_feed_dot(
                    tab.replay.is_some(),
                    tab.feed_connection == quantick_feed::FeedConnectionState::Connected
                        && !tab.needs_attention(),
                ),
                workspace,
                modified,
            }
        })
        .collect()
}

pub(crate) fn draw(
    ctx: &egui::Context,
    tabs: &super::arrangement_host::ArrangementHost,
    workspaces: &quantick_stores::workspaces::WorkspaceBookStore,
    title: &mut TitleBarState,
) -> Option<crate::tabstrip::TabAction> {
    resize::draw(ctx);
    let active = tabs.presented_index();
    title.strip.allow_close_last = tabs.plan_close(tabs.active_index()).is_ok();
    let indices: Vec<_> = tabs.window_indices(tabs.windows().presented()).collect();
    let chips = chips_of(tabs, workspaces, &indices);
    if let Some(state) = title.requests.take() {
        send_state(ctx, state);
    }
    title.controls.clear();
    ctx.input(|input| {
        title.maximized = input.viewport().maximized.unwrap_or(false);
        title.minimized = input.viewport().minimized.unwrap_or(false);
    });
    let mut action = None;
    egui::TopBottomPanel::top(egui::Id::new((ctx.viewport_id(), "market_title_bar")))
        .exact_height(HEIGHT)
        .frame(
            egui::Frame::none()
                .fill(theme::INSET)
                .inner_margin(egui::Margin {
                    left: crate::tabstrip::STRIP_EDGE_PX,
                    right: 0.0,
                    top: 0.0,
                    bottom: 0.0,
                }),
        )
        .show(ctx, |ui| {
            // Tabs and the menu sit on the row's floor; the window's own buttons
            // fill it from the top edge, as the Windows Terminal's do.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Max), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let pill_width = if title.agent_on {
                    AGENT_PILL_WIDTH
                } else {
                    0.0
                };
                let reserve_drag = if ui.available_width() >= 600.0 {
                    DRAG_WIDTH
                } else {
                    0.0
                };
                let tab_width = (ui.available_width()
                    - 3.0 * WINDOW_BUTTON_WIDTH
                    - KEBAB_WIDTH
                    - pill_width
                    - DRAG_GAP
                    - reserve_drag)
                    .max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(tab_width, crate::tabstrip::CHIP_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                        let surface = &title.surface;
                        action =
                            crate::tabstrip::draw(ui, &chips, active, surface, &mut title.strip);
                    },
                );
                ui.add_space(DRAG_GAP);
                let drag_width =
                    (ui.available_width() - 3.0 * WINDOW_BUTTON_WIDTH - KEBAB_WIDTH - pill_width)
                        .max(0.0);
                let (_, drag) = ui.allocate_exact_size(
                    egui::vec2(drag_width, HEIGHT),
                    egui::Sense::click_and_drag(),
                );
                let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                let bounds = drag.rect.intersect(ui.clip_rect());
                if bounds.is_positive() {
                    title.controls.push(("drag", bounds));
                }
                if drag.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if title.agent_on && agent_pill(ui, title) {
                    title.agent_clicked = true;
                }
                if kebab(ui, title) {
                    title.kebab_clicked = true;
                }
                for (id, hint, state) in [
                    ("minimize", "Minimize window", Some(WindowState::Minimized)),
                    (
                        "maximize",
                        if maximized {
                            "Restore window"
                        } else {
                            "Maximize window"
                        },
                        Some(if maximized {
                            WindowState::Normal
                        } else {
                            WindowState::Maximized
                        }),
                    ),
                    ("close", "Close window", None),
                ] {
                    let button = window_button(ui, id, hint, maximized).on_hover_text(hint);
                    let bounds = button.rect.intersect(ui.clip_rect());
                    if bounds.is_positive() {
                        title.controls.push((id, bounds));
                    }
                    if button.clicked() {
                        if let Some(state) = state {
                            send_state(ctx, state);
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            });
        });
    // The strip lays out local positions; the rest of the app addresses the
    // stable physical runtime store, including its existing control scenes.
    for (index, _) in &mut title.strip.tabs {
        *index = indices[*index];
    }
    action.map(|action| match action {
        crate::tabstrip::TabAction::Activate(index) => {
            crate::tabstrip::TabAction::Activate(indices[index])
        }
        crate::tabstrip::TabAction::Close(index) => {
            crate::tabstrip::TabAction::Close(indices[index])
        }
        action => action,
    })
}

/// The `⋮` button. Returns whether it was pressed.
fn kebab(ui: &mut egui::Ui, title: &mut TitleBarState) -> bool {
    let glyph = egui::RichText::new(super::menu_bar::KEBAB_GLYPH).size(KEBAB_GLYPH_PX);
    let surface = ui.painter().add(egui::Shape::Noop);
    let button = ui
        .add_sized(
            [KEBAB_WIDTH, crate::tabstrip::CHIP_HEIGHT],
            egui::Button::new(glyph).frame(false),
        )
        .on_hover_ui(|ui| {
            let keys =
                super::menu_bar::shortcut_text(ui.ctx(), &super::menu_bar::WINDOW_MENU_SHORTCUT);
            ui.label(format!("Window menu ({keys})"));
        });
    title.kebab_pressed = title.menu_open || button.is_pointer_button_down_on();
    if title.kebab_pressed {
        let fill = egui::Shape::rect_filled(button.rect, 0.0, theme::CONTROL);
        ui.painter().set(surface, fill);
    }
    let bounds = button.rect.intersect(ui.clip_rect());
    if bounds.is_positive() {
        title.controls.push((KEBAB_CONTROL, bounds));
    }
    button.clicked()
}

/// The pill naming local agent access, shown only while it is on.
fn agent_pill(ui: &mut egui::Ui, title: &mut TitleBarState) -> bool {
    let face = egui::RichText::new(AGENT_PILL_LABEL)
        .size(AGENT_TEXT_PX)
        .color(theme::AMBER);
    let pill = ui
        .add_sized(
            [AGENT_PILL_WIDTH, crate::tabstrip::CHIP_HEIGHT],
            egui::Button::new(face).frame(false),
        )
        .on_hover_text("Local agent access is on. Click to manage it.");
    ui.painter().rect_stroke(
        pill.rect.shrink2(AGENT_OUTLINE_INSET),
        pill.rect.height(),
        egui::Stroke::new(AGENT_OUTLINE_PX, theme::AMBER),
    );
    let bounds = pill.rect.intersect(ui.clip_rect());
    if bounds.is_positive() {
        title.controls.push((AGENT_PILL_CONTROL, bounds));
    }
    pill.clicked()
}

/// Vector strokes keep native controls crisp and equally legible at every DPI.
fn window_button(ui: &mut egui::Ui, id: &str, hint: &str, maximized: bool) -> egui::Response {
    let response = ui.add_sized(
        [WINDOW_BUTTON_WIDTH, HEIGHT],
        egui::Button::new("").frame(false),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), hint));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            response.rect,
            0.0,
            if id == "close" {
                theme::WARN
            } else {
                theme::BARE_HOVER
            },
        );
    }
    let ink = if id == "close" && (response.hovered() || response.has_focus()) {
        theme::ink_on(theme::WARN)
    } else if response.hovered() || response.has_focus() {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };
    let stroke = egui::Stroke::new(1.0_f32, ink);
    let icon =
        egui::Rect::from_center_size(response.rect.center(), egui::Vec2::splat(WINDOW_GLYPH_PX));
    match id {
        "minimize" => {
            ui.painter()
                .line_segment([icon.left_center(), icon.right_center()], stroke);
        }
        "maximize" if maximized => {
            let back = icon.translate(egui::vec2(2.0, -2.0)).shrink(2.0);
            ui.painter()
                .line_segment([back.left_top(), back.right_top()], stroke);
            ui.painter()
                .line_segment([back.right_top(), back.right_bottom()], stroke);
            ui.painter().rect_stroke(
                icon.translate(egui::vec2(-2.0, 2.0)).shrink(2.0),
                0.0,
                stroke,
            );
        }
        "maximize" => {
            ui.painter().rect_stroke(icon, 0.0, stroke);
        }
        _ => {
            ui.painter()
                .line_segment([icon.left_top(), icon.right_bottom()], stroke);
            ui.painter()
                .line_segment([icon.right_top(), icon.left_bottom()], stroke);
        }
    }
    response
}
