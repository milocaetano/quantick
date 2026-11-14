//! Paints the menu below the capsule's workspace name and the name dialog; the
//! `⋮` menu's workspace rows are `chrome_menu_bar`'s. The words are `window_menu`'s, the state is
//! `workspace_menu`'s and what a row does is [`WorkspaceCommand`]'s.

use crate::surfaces::chrome_menu_bar::{metrics, scrolled};
use eframe::egui;
use quantick_control_handlers::workspace_book::cell_menu::{CellRow, RowKind, cell_menu};
use quantick_control_schema::window_menu as text;
use quantick_session::workspace_menu::{self as model, ChipSizing, MenuBounds};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::WorkspaceBook;

use super::MenuCommand;
use crate::app::QuantickApp;

const NAME_FIELD_WIDTH_PT: f32 = 220.0;

/// What the workspace rows read this frame.
#[derive(Clone, Copy)]
pub(super) struct WorkspaceRows<'a> {
    pub(super) book: &'a WorkspaceBook,
    pub(super) tab: u64,
}

impl<'a> WorkspaceRows<'a> {
    /// The rows of the window's presented tab.
    pub(super) fn of(app: &'a QuantickApp) -> Self {
        Self {
            book: app.runtime.workspace.book.book(),
            tab: app.runtime.tabs.active_id(),
        }
    }

    /// The capsule's workspace menu: `cell_menu`'s rows within `chip_menu_budget`.
    pub(super) fn chip_menu(
        self,
        ctx: &egui::Context,
        anchor: egui::Rect,
        menu: &mut WorkspaceMenuState,
        keys: String,
        commands: &mut Vec<MenuCommand>,
    ) -> Vec<(CellRow, egui::Rect)> {
        let mut drawn = Vec::new();
        if !menu.chip_menu_on(self.tab) {
            return drawn;
        }
        let rows = cell_menu(self.book, self.tab, &keys);
        let (recorded, (measure, show)) =
            (commands.len(), menu.chip_sizing.frame(rows.signature()));
        let area = egui::Area::new(egui::Id::new(("tab_workspace_menu", self.tab)))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(anchor.right_bottom() + egui::vec2(0.0, model::CHIP_MENU_DROP_PT))
            .constrain(true)
            .show(ctx, |ui| {
                super::menus::flat_rows(ui);
                let at = |r: egui::Rect| MenuBounds::new(r.left(), r.top(), r.right(), r.bottom());
                let m = metrics(ui);
                let n = rows.list.len();
                let budget = model::chip_menu_budget(at(ctx.screen_rect()), at(anchor), n, m);
                ui.set_max_width(ui.available_width().min(budget.width(measure)));
                if !show {
                    ui.set_invisible();
                }
                if !measure {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                }
                let layout = egui::Layout::default().with_cross_justify(!measure);
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(budget.min_width);
                    ui.with_layout(layout, |ui| {
                        let mut draw = |ui: &mut egui::Ui, rows: &[CellRow]| {
                            for row in rows {
                                let asked = cell_row(ui, row, &mut menu.chip_sizing, &mut drawn);
                                commands.extend(asked.map(MenuCommand::Workspace));
                            }
                        };
                        draw(ui, &rows.head);
                        scrolled(budget.list_max(), ui, |ui| draw(ui, &rows.list));
                        draw(ui, &rows.foot);
                    });
                });
            });
        let rect = area.response.rect;
        let outside = |at: egui::Pos2| !anchor.contains(at) && !rect.contains(at);
        let dismissed = ctx.input_mut(|input| {
            let pressed =
                input.pointer.any_pressed() && input.pointer.interact_pos().is_some_and(outside);
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) || pressed
        });
        if commands.len() > recorded || dismissed {
            menu.close_chip_menu();
        }
        drawn.insert(0, (CellRow::menu(), rect));
        drawn.retain(|_| show); // An unseen frame shows nothing to press.
        drawn
    }
}

/// One cell-menu row as its kind draws it; answers what a press on it asked.
fn cell_row(
    ui: &mut egui::Ui,
    row: &CellRow,
    sizing: &mut ChipSizing,
    drawn: &mut Vec<(CellRow, egui::Rect)>,
) -> Option<WorkspaceCommand> {
    let (label, mut on) = (row.label.as_str(), row.on);
    let mut response = match row.kind {
        RowKind::Separator => ui.separator(),
        RowKind::Caption => ui.label(crate::theme::supporting_text(ui, label)),
        RowKind::Switch => ui.checkbox(&mut on, label),
        RowKind::Press => {
            let beside = crate::theme::supporting_text(ui, row.beside.as_str());
            let button = egui::Button::new(label).selected(on).shortcut_text(beside);
            ui.add_enabled(row.unavailable.is_none(), button)
        }
    };
    let asked = response.intrinsic_size.map_or(0.0, |size| size.x);
    let cut = row.kind == RowKind::Press && sizing.cut(&row.key, asked, ui.available_width());
    if let Some(text) = row.tip(cut) {
        response = response.on_hover_text(text).on_disabled_hover_text(text);
    }
    if response.gained_focus() || (row.lit() && sizing.measuring()) {
        response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
    }
    if row.dot {
        let at = response.rect.left_center() + egui::vec2(model::CHIP_MENU_DOT_INSET_PT, 0.0);
        let accent = crate::theme::get(ui.ctx()).accent();
        ui.painter().circle_filled(at, 2.5, accent);
    }
    let seen = response.rect.intersect(ui.clip_rect());
    if !row.key.is_empty() && seen.height() * 2.0 >= response.rect.height() {
        drawn.push((row.clone(), seen));
    }
    response.clicked().then(|| row.command.clone()).flatten()
}

/// The workspace dialogs: the one asking a name over the Manage one.
pub(super) fn dialogs(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    name_dialog(app, ctx, commands); // First, so its Esc is not the Manage one's.
    super::workspace_manage::manage_dialog(app, ctx, commands);
}

/// The dialog that asks for a name: Enter saves, Esc cancels, and a refusal stands
/// under the field with the name still in it.
fn name_dialog(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    let palette = crate::theme::get(ctx);
    let tab = app.runtime.tabs.active_id();
    let state = &mut app.chrome.window_menu.tab_workspace;
    let Some(dialog) = state.dialog.as_mut().filter(|dialog| dialog.tab == tab) else {
        return;
    };
    let title = match &dialog.mode {
        NameDialogMode::SaveAsNew => text::SAVE_AS_DIALOG_TITLE,
        NameDialogMode::New => text::NEW_DIALOG_TITLE,
        NameDialogMode::Rename { .. } => text::RENAME_DIALOG_TITLE,
    };
    let mut cancelled = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut dialog.name).desired_width(NAME_FIELD_WIDTH_PT),
            );
            if std::mem::take(&mut dialog.wants_focus) {
                field.request_focus();
            }
            if field.changed() {
                dialog.refusal = None;
            }
            if let Some(reason) = &dialog.refusal {
                ui.colored_label(palette.warn(), reason);
            }
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            cancelled =
                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            ui.horizontal(|ui| {
                if ui.button(text::DIALOG_SAVE).clicked() || (enter && !cancelled) {
                    commands.push(MenuCommand::Workspace(WorkspaceCommand::SubmitDialog));
                }
                cancelled |= ui.button(text::DIALOG_CANCEL).clicked();
            });
        });
    if cancelled {
        state.close_dialog();
    }
}
