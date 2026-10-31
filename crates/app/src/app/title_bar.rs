//! Native window chrome; market commands still use the existing arrangement owner.
use crate::theme;
use eframe::egui;

use quantick_control_schema::native_window::WindowState;

#[derive(Default)]
pub(crate) struct TitleBarState {
    pub(crate) maximized: bool,
    pub(crate) minimized: bool,
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

const HEIGHT: f32 = 32.0;
const WINDOW_BUTTON_WIDTH: f32 = 30.0;
const DRAG_WIDTH: f32 = 24.0;

pub(crate) fn draw(
    ctx: &egui::Context,
    tabs: &super::arrangement_host::ArrangementHost,
    title: &mut TitleBarState,
) -> Option<crate::tabstrip::TabAction> {
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
                .fill(theme::CHROME)
                .inner_margin(egui::Margin::symmetric(4.0, 2.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let tab_width =
                    (ui.available_width() - 3.0 * WINDOW_BUTTON_WIDTH - DRAG_WIDTH - 20.0).max(0.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(tab_width, 26.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                        action = crate::tabstrip::draw(ui, &chips, active, &mut title.strip);
                    },
                );
                let drag_width = (ui.available_width()
                    - 3.0 * (WINDOW_BUTTON_WIDTH + ui.spacing().item_spacing.x))
                    .max(0.0);
                let (_, drag) = ui.allocate_exact_size(
                    egui::vec2(drag_width, 26.0),
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
                for (id, label, hint, state) in [
                    (
                        "minimize",
                        "\u{2212}",
                        "Minimize window",
                        Some(WindowState::Minimized),
                    ),
                    (
                        "maximize",
                        if maximized { "\u{2750}" } else { "\u{25a1}" },
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
                    ("close", "\u{00d7}", "Close window", None),
                ] {
                    let button = ui
                        .add_sized(
                            [WINDOW_BUTTON_WIDTH, 26.0],
                            egui::Button::new(label).frame(false),
                        )
                        .on_hover_text(hint);
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
