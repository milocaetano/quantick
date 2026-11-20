//! The strategy window's paint: Summary, Code and History pages over one
//! instance, with Arm, Save as and Duplicate. Which instance is open is
//! egui's view memory.

use eframe::egui;
use quantick_control_handlers::strategy_desk::StrategyDeskHost;
use quantick_ui_model::strategy_desk::{NO_CODE, Page, Sent, StrategyWindow};

use crate::theme;

fn key() -> egui::Id {
    egui::Id::new("strategy_window")
}

pub(super) fn open(ctx: &egui::Context, window: StrategyWindow) {
    ctx.data_mut(|data| data.insert_temp(key(), window));
}

/// Draw the open window, read again when `stale`; what it sent.
pub(super) fn show<H>(host: &mut H, ctx: &egui::Context, stale: bool) -> Option<Sent>
where
    H: StrategyDeskHost + ?Sized,
{
    #[cfg(any(feature = "scenario-harness", test))]
    hooked(host, ctx);
    let mut window: StrategyWindow = ctx.data(|data| data.get_temp(key()))?;
    if stale {
        window.forget();
    }
    window.read(host);
    let (mut keep, mut sent) = (true, None);
    egui::Window::new(window.title().to_owned())
        .id(key())
        .open(&mut keep)
        .default_width(420.0)
        .show(ctx, |ui| sent = body(ui, &mut window));
    if sent.is_some() {
        window.forget();
    }
    ctx.data_mut(|data| match keep {
        true => data.insert_temp(key(), window),
        false => data.remove::<StrategyWindow>(key()),
    });
    sent
}

fn body(ui: &mut egui::Ui, window: &mut StrategyWindow) -> Option<Sent> {
    let muted = theme::get(ui.ctx()).text_muted();
    ui.label(egui::RichText::new(window.description()).italics());
    if !window.readable() {
        return None;
    }
    ui.horizontal(|ui| {
        Page::ALL.map(|page| ui.selectable_value(&mut window.page, page, page.title()))
    });
    ui.separator();
    match (window.page, window.code()) {
        (Page::Summary, _) => {
            _ = egui::Grid::new("strategy_summary").show(ui, |ui| {
                for (name, value) in window.summary() {
                    ui.label(egui::RichText::new(name).color(muted));
                    ui.monospace(value);
                    ui.end_row();
                }
            })
        }
        (Page::Code, Some(code)) => {
            _ = egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| ui.monospace(code));
        }
        (Page::Code, None) => _ = ui.label(egui::RichText::new(NO_CODE).color(muted)),
        (Page::History, _) => history(ui, window.history()),
    }
    ui.separator();
    ui.horizontal(|ui| {
        let arm = ui.add_enabled(window.arms(), egui::Button::new("Arm"));
        let arm = arm.on_disabled_hover_text("only an ended strategy arms again");
        let duplicate = ui
            .button("Duplicate")
            .on_hover_text("arm a paused copy beside it");
        let saved = window.saves() && {
            ui.add(egui::TextEdit::singleline(&mut window.save_name).desired_width(140.0));
            ui.button("Save as…")
                .on_hover_text("keep this script in the library")
                .clicked()
        };
        (arm.clicked().then(|| window.arm()))
            .or_else(|| duplicate.clicked().then(|| window.duplicate()))
            .or_else(|| saved.then(|| window.save_as()))
    })
    .inner
}

/// A strip across the loaded bars with a mark at each fire, oldest left.
fn history(ui: &mut egui::Ui, (caption, marks): (String, Vec<f32>)) {
    let palette = theme::get(ui.ctx());
    ui.label(caption);
    let size = egui::vec2(ui.available_width(), 28.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.hline(rect.x_range(), rect.center().y, (1.0, palette.border()));
    for mark in marks {
        let x = egui::lerp(rect.x_range(), mark);
        painter.vline(x, rect.y_range().shrink(4.0), (2.0, palette.draw_violet()));
    }
}

/// `QUANTICK_STRATEGY_WINDOW=<summary|code|history>`: the window open on
/// that page over the first armed instance, once.
#[cfg(any(feature = "scenario-harness", test))]
fn hooked<H: StrategyDeskHost + ?Sized>(host: &mut H, ctx: &egui::Context) {
    let done = egui::Id::new("strategy_window_hooked");
    if let Some(page) = crate::hooks::captured::var("QUANTICK_STRATEGY_WINDOW")
        && ctx.data(|data| data.get_temp::<bool>(done)).is_none()
        && let Some(window) = StrategyWindow::first(host, Page::parse(&page))
    {
        ctx.data_mut(|data| data.insert_temp(done, true));
        open(ctx, window);
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_STRATEGY_WINDOW"];
