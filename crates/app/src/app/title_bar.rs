//! Native window chrome; market commands still use the existing arrangement owner.
use crate::theme;
use eframe::egui;

use quantick_control_schema::native_window::WindowState;
mod resize;

#[derive(Default)]
pub(crate) struct TitleBarState {
    pub(crate) maximized: bool,
    pub(crate) minimized: bool,
    pub(crate) surface: quantick_workspace::arrangement::strip::StripSurface,
    pub(crate) strip: crate::tabstrip::StripDrawn,
    pub(crate) controls: Vec<(&'static str, egui::Rect)>,
    pub(crate) requests: quantick_control_schema::native_window::WindowRequests,
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

pub(super) const HEIGHT: f32 = 52.0;
pub(super) const WINDOW_BUTTON_WIDTH: f32 = 48.0;
const DRAG_WIDTH: f32 = 32.0;

pub(crate) fn draw(
    ctx: &egui::Context,
    tabs: &super::arrangement_host::ArrangementHost,
    title: &mut TitleBarState,
) -> Option<crate::tabstrip::TabAction> {
    resize::draw(ctx);
    let active = tabs.active_index();
    let chips: Vec<_> = tabs
        .iter_with_ids()
        .map(|(id, tab)| crate::tabstrip::TabChip {
            id,
            label: tab.chip_label(),
            replaying: tab.replay.is_some(),
            needs_attention: tab.needs_attention(),
        })
        .collect();
    if let Some(state) = title.requests.take() {
        send_state(ctx, state);
    }
    title.controls.clear();
    ctx.input(|input| {
        title.maximized = input.viewport().maximized.unwrap_or(false);
        title.minimized = input.viewport().minimized.unwrap_or(false);
    });
    let mut action = None;
    egui::TopBottomPanel::top("market_title_bar")
        .exact_height(HEIGHT)
        .frame(
            egui::Frame::none()
                .fill(theme::INSET)
                .inner_margin(egui::Margin {
                    left: 4.0,
                    right: 4.0,
                    top: 4.0,
                    bottom: 0.0,
                }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let reserve_drag = if ui.available_width() >= 600.0 {
                    DRAG_WIDTH
                } else {
                    0.0
                };
                let tab_width =
                    (ui.available_width() - 3.0 * WINDOW_BUTTON_WIDTH - reserve_drag - 8.0)
                        .max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(tab_width, HEIGHT - 4.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                        let surface = &title.surface;
                        action =
                            crate::tabstrip::draw(ui, &chips, active, surface, &mut title.strip);
                    },
                );
                let drag_width = (ui.available_width()
                    - 3.0 * (WINDOW_BUTTON_WIDTH + ui.spacing().item_spacing.x))
                    .max(0.0);
                let (_, drag) = ui.allocate_exact_size(
                    egui::vec2(drag_width, HEIGHT - 4.0),
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
    action
}

/// Vector strokes keep native controls crisp and equally legible at every DPI.
fn window_button(ui: &mut egui::Ui, id: &str, hint: &str, maximized: bool) -> egui::Response {
    let response = ui.add_sized(
        [WINDOW_BUTTON_WIDTH, HEIGHT - 4.0],
        egui::Button::new("").frame(false),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), hint));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            response.rect,
            4.0,
            if id == "close" {
                theme::WARN
            } else {
                theme::BARE_HOVER
            },
        );
    }
    let ink = if id == "close" && (response.hovered() || response.has_focus()) {
        theme::ink_on(theme::WARN)
    } else {
        theme::TEXT_PRIMARY
    };
    let stroke = egui::Stroke::new(1.5_f32, ink);
    let icon = egui::Rect::from_center_size(response.rect.center(), egui::vec2(14.0, 14.0));
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
