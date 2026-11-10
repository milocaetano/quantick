//! The dialog before a saved chart leaves the canvas: the window dimmed, the
//! doomed chart outlined in red behind it, the words
//! `quantick_workspace::canvas::remove_prompt` wrote, and three answers.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_control_handlers::canvas::answer_capability;
use quantick_workspace::canvas::{RemoveAnswer, RemovePrompt};

use crate::pane_menu::{PaneMenus, danger_ink};
use crate::theme;

/// The dialog's width, where it stands, its padding, and where its words start.
const WIDTH_PX: f32 = 440.0;
const TOP_SHARE: f32 = 0.35;
const PADDING_PX: f32 = 18.0;
const WORDS_X_PX: f32 = 26.0;
const BUTTON_HEIGHT_PX: f32 = 26.0;

/// Draw the dialog over `doomed`; answers the button pressed, Cancel for Esc.
pub(crate) fn draw(
    ctx: &egui::Context,
    prompt: &RemovePrompt,
    doomed: egui::Rect,
    menus: &mut PaneMenus,
) -> Option<RemoveAnswer> {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("remove_chart_scrim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            ui.painter()
                .rect_filled(screen, 0.0, theme::get(ctx).modal_scrim());
            let red = (2.0, danger_ink(ui));
            ui.painter().rect_stroke(doomed.shrink(1.0), 0.0, red);
        });
    let mut answer = None;
    let at = egui::pos2(
        screen.center().x,
        screen.top() + screen.height() * TOP_SHARE,
    );
    egui::Area::new(egui::Id::new("remove_chart_dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .pivot(egui::Align2::CENTER_CENTER)
        .show(ctx, |ui| {
            ctx.move_to_top(ui.layer_id());
            let palette = theme::get(ctx);
            let frame = egui::Frame::popup(ui.style()).rounding(8.0);
            frame.inner_margin(PADDING_PX).show(ui, |ui| {
                ui.set_width(WIDTH_PX - 2.0 * PADDING_PX);
                ui.horizontal(|ui| {
                    let red = danger_ink(ui);
                    ui.label(egui::RichText::new(icons::WARNING).size(18.0).color(red));
                    let title = egui::RichText::new(&prompt.title).size(15.0);
                    ui.label(title.color(palette.text_primary()));
                });
                ui.add_space(8.0);
                for line in &prompt.lines {
                    ui.horizontal(|ui| {
                        ui.add_space(WORDS_X_PX);
                        let words = egui::RichText::new(line).size(12.5);
                        ui.label(words.color(palette.text_muted()));
                    });
                }
                ui.add_space(24.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for choice in RemoveAnswer::ALL.into_iter().rev() {
                        if button(ui, choice, menus) {
                            answer = Some(choice);
                        }
                    }
                });
            });
        });
    let escaped = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    answer.or(escaped.then_some(RemoveAnswer::Cancel))
}

/// One answer: Remove filled red, the others outlined.
fn button(ui: &mut egui::Ui, choice: RemoveAnswer, menus: &mut PaneMenus) -> bool {
    let palette = theme::get(ui.ctx());
    let words = egui::RichText::new(choice.label()).size(12.5);
    let red = choice == RemoveAnswer::Remove;
    let widget = if red {
        egui::Button::new(words.color(egui::Color32::WHITE)).fill(danger_ink(ui))
    } else {
        let edge = (0.75, palette.text_muted());
        egui::Button::new(words.color(palette.text_primary())).stroke(edge)
    };
    let widget = widget
        .min_size(egui::vec2(0.0, BUTTON_HEIGHT_PX))
        .rounding(4.0);
    let response = ui.add(widget);
    let id = format!("remove_chart.{}", choice.id());
    menus.publish(id, choice.label(), response.rect, answer_capability(choice));
    response.clicked()
}
