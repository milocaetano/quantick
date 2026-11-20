//! The strategy window: one instance in full on three pages — Summary, Code
//! (read-only) and History (the loaded bars a fresh copy fires on) — with
//! Arm, Save as and Duplicate. Which instance and page are open is egui's
//! view memory; the detail is read again after every command.

use eframe::egui;
use quantick_control_handlers::strategy_desk::{self, StrategyDeskHost};
use quantick_drawings::DrawingId;
use quantick_session::script_strategies::ScriptPlacement;
use quantick_session::strategy_desk::{
    Asset, DeskAction, DeskRow, DeskState, InstanceRef, StrategyDeskCommand as Command,
    StrategyDetail,
};

use super::Sent;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Summary,
    Code,
    History,
}

#[derive(Clone)]
struct Open {
    tab_id: u64,
    id: InstanceRef<DrawingId>,
    asset: String,
    page: Page,
    save_name: String,
    detail: Option<Result<StrategyDetail<DrawingId>, String>>,
}

fn key() -> egui::Id {
    egui::Id::new("strategy_window")
}

pub(super) fn asset_label(asset: &Asset) -> String {
    format!("{} · {}", asset.symbol, asset.bar_type)
}

/// Open the window on `row`, at `page` or the summary.
pub(super) fn open(
    ctx: &egui::Context,
    row: &DeskRow<DrawingId>,
    asset: String,
    page: Option<Page>,
) {
    let (tab_id, id, detail) = (row.tab_id, row.id, None);
    let page = page.unwrap_or(Page::Summary);
    let save_name = format!("{} copy", row.name.trim_end_matches(".pine"));
    let open = Open {
        tab_id,
        id,
        asset,
        page,
        save_name,
        detail,
    };
    ctx.data_mut(|data| data.insert_temp(key(), open));
}

pub(super) fn show<H: StrategyDeskHost + ?Sized>(
    host: &mut H,
    ctx: &egui::Context,
) -> Option<Sent> {
    #[cfg(any(feature = "scenario-harness", test))]
    hooked(host, ctx);
    let mut open: Open = ctx.data(|data| data.get_temp(key()))?;
    let (tab_id, id) = (open.tab_id, open.id);
    let detail = (open.detail.take())
        .unwrap_or_else(|| strategy_desk::instance_detail(host, tab_id, id).map_err(|e| e.message));
    let (mut keep, mut sent) = (true, None);
    let title = detail
        .as_ref()
        .map_or("Strategy", |detail| detail.row.name.as_str());
    egui::Window::new(title)
        .id(key())
        .open(&mut keep)
        .default_width(420.0)
        .show(ctx, |ui| match &detail {
            Ok(detail) => sent = body(ui, &mut open, detail),
            Err(refusal) => _ = ui.label(refusal),
        });
    open.detail = Some(detail);
    ctx.data_mut(|data| match keep {
        true => data.insert_temp(key(), open),
        false => data.remove::<Open>(key()),
    });
    sent.map(|command| (tab_id, command))
}

/// Read the open instance again next frame: a command may have changed it.
pub(super) fn refresh(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        if let Some(mut open) = data.get_temp::<Open>(key()) {
            open.detail = None;
            data.insert_temp(key(), open);
        }
    });
}

fn body(
    ui: &mut egui::Ui,
    open: &mut Open,
    detail: &StrategyDetail<DrawingId>,
) -> Option<Command<DrawingId>> {
    ui.label(egui::RichText::new(&detail.description).italics());
    ui.horizontal(|ui| {
        for (page, word) in [
            (Page::Summary, "Summary"),
            (Page::Code, "Code"),
            (Page::History, "History"),
        ] {
            ui.selectable_value(&mut open.page, page, word);
        }
    });
    ui.separator();
    let muted = theme::get(ui.ctx()).text_muted();
    match open.page {
        Page::Summary => {
            egui::Grid::new("strategy_summary")
                .num_columns(2)
                .show(ui, |ui| {
                    for (name, value) in summary(detail, &open.asset) {
                        ui.label(egui::RichText::new(name).color(muted));
                        ui.monospace(value);
                        ui.end_row();
                    }
                })
                .inner
        }
        Page::Code => match &detail.code {
            Some(code) => {
                _ = egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| ui.monospace(code))
            }
            None => _ = ui.label(egui::RichText::new(NO_CODE).color(muted)),
        },
        Page::History => history(ui, &detail.history),
    }
    ui.separator();
    let id = detail.row.id;
    let ended = matches!(detail.row.state, DeskState::Ended { .. });
    ui.horizontal(|ui| {
        let arm = ui.add_enabled(ended, egui::Button::new("Arm"));
        let arm = arm.on_disabled_hover_text("only an ended strategy arms again");
        let duplicate = ui
            .button("Duplicate")
            .on_hover_text("arm a paused copy beside it");
        let save = detail.code.is_some().then(|| {
            ui.add(egui::TextEdit::singleline(&mut open.save_name).desired_width(140.0));
            ui.button("Save as…")
                .on_hover_text("keep this script in the library under that name")
        });
        let saved = save.is_some_and(|save| save.clicked());
        let name = open.save_name.clone();
        (arm.clicked().then_some(Command::Arm(id)))
            .or_else(|| duplicate.clicked().then_some(Command::Duplicate(id)))
            .or_else(|| saved.then_some(Command::SaveAs { instance: id, name }))
    })
    .inner
}

const NO_CODE: &str = "A strategy on a drawing runs a preset; it has no script.";

fn summary(detail: &StrategyDetail<DrawingId>, asset: &str) -> Vec<(String, String)> {
    let place = match detail.placement {
        ScriptPlacement::Region(_) => super::rail::link(&detail.row.link),
        ScriptPlacement::Range { a, b } => format!("range {a}–{b}"),
        ScriptPlacement::Line { level, rising } => {
            format!("cross {level} {}", if rising { "↑" } else { "↓" })
        }
        ScriptPlacement::Anywhere => "whole chart".to_owned(),
    };
    let action = match detail.row.action {
        DeskAction::Alarm => "alarm",
        DeskAction::Order => "paper order",
    };
    let mut rows = vec![
        ("Where".to_owned(), place),
        (
            "Action".to_owned(),
            format!("{action} · {:?}", detail.side).to_lowercase(),
        ),
    ];
    rows.extend(detail.inputs.iter().cloned());
    rows.push(("Asset and bar".to_owned(), asset.to_owned()));
    rows.push(("Fires today".to_owned(), detail.fires_today.to_string()));
    rows
}

/// A strip across the loaded bars with a mark at each bar a fresh copy fires
/// on, oldest left.
fn history(ui: &mut egui::Ui, fired: &[usize]) {
    let palette = theme::get(ui.ctx());
    ui.label(format!("{} fires on the loaded bars", fired.len()));
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.hline(rect.x_range(), rect.center().y, (1.0, palette.border()));
    let last = fired.last().copied().unwrap_or(0).max(1) as f32;
    for slot in fired {
        let x = egui::lerp(rect.x_range(), *slot as f32 / last);
        painter.vline(x, rect.y_range().shrink(4.0), (2.0, palette.draw_violet()));
    }
}

/// `QUANTICK_STRATEGY_WINDOW=<summary|code|history>`: the window open on that
/// page over the first listed instance, once one is armed.
#[cfg(any(feature = "scenario-harness", test))]
fn hooked<H: StrategyDeskHost + ?Sized>(host: &mut H, ctx: &egui::Context) {
    let done = egui::Id::new("strategy_window_hook");
    let Some(page) = crate::hooks::captured::var("QUANTICK_STRATEGY_WINDOW") else {
        return;
    };
    if ctx.data(|data| data.get_temp::<bool>(done)).is_some() {
        return;
    }
    let groups = strategy_desk::groups(host);
    let Some((group, row)) = groups
        .iter()
        .find_map(|group| Some((group, group.rows.first()?)))
    else {
        return;
    };
    let page = match page.trim() {
        "code" => Page::Code,
        "history" => Page::History,
        _ => Page::Summary,
    };
    open(ctx, row, asset_label(&group.asset), Some(page));
    ctx.data_mut(|data| data.insert_temp(done, true));
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_STRATEGY_WINDOW"];
