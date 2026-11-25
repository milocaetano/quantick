//! The Strategies tab's paint: per asset a header over its instances.

use eframe::egui;
use egui::RichText;
use egui_phosphor::regular as icons;
use quantick_ui_model::strategy_desk::{ActionDraft, EMPTY, Group, Header, Ink, Line, Page, Sent};
use quantick_ui_model::strategy_desk::{StrategyWindow, Toggle, verb};

use crate::theme;

/// The groups the tab paints this frame.
pub(crate) struct Rail(pub(super) Vec<Group>);

impl Rail {
    pub(crate) fn show(&self, ui: &mut egui::Ui) {
        if self.0.is_empty() {
            ui.label(RichText::new(EMPTY).color(ink(ui, Ink::Muted)));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for group in &self.0 {
                header(ui, &group.header);
                group.lines.iter().for_each(|line| row(ui, line));
                ui.separator();
            }
        });
    }
}

fn header(ui: &mut egui::Ui, header: &Header) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&header.label).monospace().strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            send_on(&toggle(ui, header.toggle, &header.hover), &header.send);
            ui.label(RichText::new(&header.count).color(ink(ui, Ink::Muted)));
        });
    });
}

fn row(ui: &mut egui::Ui, line: &Line) {
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        ui.label(RichText::new(&line.row.name).strong())
            .on_hover_text(&line.status);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(word(ui, line.state))
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        ui.label(word(ui, (&line.link, Ink::Muted)));
        match &line.draft {
            Some(draft) => action_chip(ui, line, draft),
            None => _ = ui.label(word(ui, line.action)),
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let open = ui.small_button(icons::ARROW_SQUARE_OUT);
            if open.on_hover_text("open the strategy window").clicked() {
                let window = StrategyWindow::open(&line.row, line.asset.clone(), Page::Summary);
                super::window::open(ui.ctx(), window);
            }
            let remove = ui
                .small_button(icons::X)
                .on_hover_text("remove this strategy");
            send_on(&remove, &line.remove);
            if let Some((glyph, sent)) = &line.toggle {
                send_on(&toggle(ui, *glyph, verb(*glyph)), sent);
            }
        });
    });
}

/// A script's order settings behind its action word, drafted in egui memory.
fn action_chip(ui: &mut egui::Ui, line: &Line, start: &ActionDraft) {
    let id = egui::Id::new(start);
    ui.menu_button(word(ui, (&line.chip, line.action.1)), |ui| {
        let mut draft = ui.data(|data| data.get_temp(id).unwrap_or_else(|| start.clone()));
        for (label, on) in draft.switches() {
            ui.checkbox(on, label);
        }
        for (label, text) in draft.fields() {
            ui.horizontal(|ui| (ui.label(label), ui.text_edit_singleline(text)));
        }
        let sent = draft.sent();
        let apply = ui.add_enabled(sent.is_ok(), egui::Button::new("Apply"));
        let apply = apply.on_disabled_hover_text(sent.as_ref().err().map_or("", String::as_str));
        sent.iter().for_each(|sent| send_on(&apply, sent));
        ui.data_mut(|data| data.insert_temp(id, draft));
        apply.clicked().then(|| ui.close_menu());
    });
}

fn toggle(ui: &mut egui::Ui, toggle: Toggle, hover: &str) -> egui::Response {
    let glyph = match toggle {
        Toggle::Pause => icons::PAUSE,
        Toggle::Resume => icons::PLAY,
    };
    ui.small_button(glyph).on_hover_text(hover)
}

/// Keep `sent` for the next frame's settle when `response` was clicked.
fn send_on(response: &egui::Response, sent: &Sent) {
    if response.clicked() {
        response
            .ctx
            .data_mut(|data| data.insert_temp(key(), sent.clone()));
        response.ctx.request_repaint();
    }
}

/// The command a click kept last frame, taken.
pub(super) fn take(ctx: &egui::Context) -> Option<Sent> {
    ctx.data_mut(|data| {
        let sent = data.get_temp::<Sent>(key());
        data.remove::<Sent>(key());
        sent
    })
}

fn key() -> egui::Id {
    egui::Id::new("strategy_rail_sent")
}

fn word(ui: &egui::Ui, (text, tone): (&str, Ink)) -> RichText {
    RichText::new(text).small().color(ink(ui, tone))
}

fn ink(ui: &egui::Ui, ink: Ink) -> egui::Color32 {
    let palette = theme::get(ui.ctx());
    match ink {
        Ink::Muted => palette.text_muted(),
        Ink::Buy => palette.buy(),
        Ink::Sell => palette.sell(),
        Ink::Violet => palette.draw_violet(),
        Ink::Accent => palette.accent(),
    }
}

/// How far an instance row sits in from its asset header, in points.
const INDENT: f32 = 12.0;
