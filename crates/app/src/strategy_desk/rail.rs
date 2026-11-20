//! The Strategies tab: per asset a header (symbol and bar type, how many,
//! pause or resume them all) over its instances (name and state; drawing or
//! chart, alarm or order; pause or resume, remove, open).

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_control_handlers::strategy_desk::{self, StrategyDeskHost};
use quantick_drawings::DrawingId;
use quantick_session::strategy_desk::{
    DeskAction, DeskGroup, DeskLink, DeskRow, DeskState, StrategyDeskCommand as Command,
};

use super::Sent;
use crate::theme;

/// The groups the tab paints this frame, and the one command a click sent.
pub(crate) struct Rail {
    groups: Vec<DeskGroup<DrawingId>>,
    pub(super) sent: Option<Sent>,
}

impl Rail {
    pub(crate) fn read<H: StrategyDeskHost + ?Sized>(host: &mut H) -> Self {
        let groups = strategy_desk::groups(host);
        Self { groups, sent: None }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let muted = theme::get(ui.ctx()).text_muted();
        if self.groups.is_empty() {
            ui.label(egui::RichText::new(EMPTY).color(muted));
        }
        let Self { groups, sent } = self;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for group in groups.iter() {
                let clicked = asset(ui, group);
                *sent = sent.take().or(clicked);
            }
        });
    }
}

const EMPTY: &str = "No strategy is armed. Arm one from a drawing's menu or the script library.";

fn asset(ui: &mut egui::Ui, group: &DeskGroup<DrawingId>) -> Option<Sent> {
    let muted = theme::get(ui.ctx()).text_muted();
    let label = super::window::asset_label(&group.asset);
    let mut sent = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&label).monospace().strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (glyph, verb) = if group.paused {
                (icons::PLAY, "resume")
            } else {
                (icons::PAUSE, "pause")
            };
            let hover = format!("{verb} every strategy on {label}");
            if ui.small_button(glyph).on_hover_text(hover).clicked() {
                let asset = group.asset.clone();
                let command = if group.paused {
                    Command::ResumeAsset(asset)
                } else {
                    Command::PauseAsset(asset)
                };
                sent = Some((group.rows[0].tab_id, command));
            }
            ui.label(egui::RichText::new(group.rows.len().to_string()).color(muted));
        });
    });
    for row in &group.rows {
        let clicked = instance(ui, row, &label);
        sent = sent.or(clicked);
    }
    ui.separator();
    sent
}

fn instance(ui: &mut egui::Ui, row: &DeskRow<DrawingId>, asset: &str) -> Option<Sent> {
    let palette = theme::get(ui.ctx());
    let (state, ink) = match &row.state {
        DeskState::Watching => ("WATCHING", palette.buy()),
        DeskState::Paused => ("PAUSED", palette.text_muted()),
        DeskState::Ended { .. } => ("ENDED", palette.sell()),
    };
    let (action, action_ink) = match row.action {
        DeskAction::Alarm => ("alarm", palette.draw_violet()),
        DeskAction::Order => ("order", palette.accent()),
    };
    let mut sent = None;
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        ui.label(egui::RichText::new(&row.name).strong())
            .on_hover_text(&row.status);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(state).small().color(ink));
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        ui.label(
            egui::RichText::new(link(&row.link))
                .small()
                .color(palette.text_muted()),
        );
        ui.label(egui::RichText::new(action).small().color(action_ink));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let open = ui.small_button(icons::ARROW_SQUARE_OUT);
            if open.on_hover_text("open the strategy window").clicked() {
                super::window::open(ui.ctx(), row, asset.to_owned(), None);
            }
            let remove = ui
                .small_button(icons::X)
                .on_hover_text("remove this strategy");
            if remove.clicked() {
                sent = Some(Command::Remove(row.id));
            }
            let toggle = match row.state {
                DeskState::Watching => Some((icons::PAUSE, "pause", Command::Pause(row.id))),
                DeskState::Paused => Some((icons::PLAY, "resume", Command::Resume(row.id))),
                DeskState::Ended { .. } => None,
            };
            if let Some((glyph, hover, command)) = toggle
                && ui.small_button(glyph).on_hover_text(hover).clicked()
            {
                sent = Some(command);
            }
        });
    });
    sent.map(|command| (row.tab_id, command))
}

/// What an instance rides: its drawing's name, or the chart.
pub(super) fn link(link: &DeskLink<DrawingId>) -> String {
    match link {
        DeskLink::Drawing {
            name: Some(name), ..
        } => name.clone(),
        DeskLink::Drawing { drawing, .. } => format!("drawing {}", drawing.0),
        DeskLink::Chart => "chart".to_owned(),
    }
}

/// How far an instance row sits in from its asset header, in points.
const INDENT: f32 = 12.0;
