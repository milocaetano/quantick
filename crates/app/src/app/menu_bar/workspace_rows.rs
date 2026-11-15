//! Paints the menu below the capsule's workspace name and the name dialog; the
//! `⋮` menu's workspace rows are `chrome_menu_bar`'s. The rows are `cell_menu`'s,
//! the size, filter, focus and scroll `workspace_menu`'s and the marks
//! `quantick_ui_model::workspace_menu`'s: this turns them into pixels and presses.

use eframe::egui::{self, Color32, Key, Modifiers, Rect, Sense, TextWrapMode, vec2};
use quantick_appearance::Token;
use quantick_control_handlers::workspace_book::cell_menu::{self as cells, CellRow};
use quantick_control_schema::window_menu as text;
use quantick_session::workspace_menu::{self as model, MenuBounds};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_ui_model::workspace_menu::{self as marks, Mark, Piece};

use super::MenuCommand;
use crate::app::QuantickApp;
use crate::toolbar::capsule::paint_tree;

const NAME_FIELD_WIDTH_PT: f32 = 220.0;
/// A press's answer: the command of the row pressed, `None` on the tab's own.
type Asked = Option<Option<WorkspaceCommand>>;

/// The capsule's workspace menu on the presented tab: `cell_menu`'s rows as
/// `marks` lays them at `chip_menu_layout`'s size; answers the rows drawn.
pub(super) fn chip_menu(
    app: &QuantickApp,
    ctx: &egui::Context,
    anchor: Rect,
    menu: &mut WorkspaceMenuState,
    keys: String,
    commands: &mut Vec<MenuCommand>,
) -> Vec<(CellRow, Rect)> {
    let (tab, tabs, mut drawn) = (app.runtime.tabs.active_id(), &app.runtime.tabs, Vec::new());
    if !menu.chip_menu_on(tab) {
        return drawn;
    }
    let label = |id| tabs.by_id(id).map(|t| t.symbol.clone()).unwrap_or_default();
    let book = app.runtime.workspace.book.book();
    let live = app.active_tab().canvas.state.tree();
    let rows = cells::cell_menu(book, tab, &keys, live, label);
    let shown = rows.shown(&menu.filter);
    let current = shown.iter().position(|row| row.on);
    let keys = [Key::ArrowDown, Key::ArrowUp, Key::Enter, Key::Escape];
    let none = Modifiers::NONE;
    let [down, up, enter, escape] = ctx.input_mut(|i| keys.map(|key| i.consume_key(none, key)));
    let chosen = menu.keys([down, up, enter], shown.len(), current);
    let mut asked: Asked = chosen.map(|at| shown[at].command.clone());
    let at = |r: Rect| MenuBounds::new(r.left(), r.top(), r.right(), r.bottom());
    let n = rows.list.len();
    let layout = model::chip_menu_layout(at(ctx.screen_rect()), at(anchor), n, shown.len());
    let (palette, border) = (crate::theme::get(ctx), model::CHIP_MENU_BORDER_PT);
    let (w, dark) = (layout.width - 2.0 * border, palette.resolved.dark);
    let ink = |t: Option<Token>| t.map_or(Color32::TRANSPARENT, |t| *palette.get(t));
    let chrome = quantick_appearance::workspace_menu::chrome(dark);
    let edge = egui::Stroke::new(border, ink(Some(chrome.edge)));
    let frame = egui::Frame::popup(&ctx.style()).stroke(edge).rounding(6.0);
    let frame = frame.inner_margin(border).fill(ink(Some(chrome.ground)));
    let id = egui::Id::new(("tab_workspace_menu", tab));
    let fresh = ctx.memory(|memory| memory.area_rect(id).is_none());
    let area = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(anchor.right_bottom() + vec2(0.0, model::CHIP_MENU_DROP_PT))
        .constrain(true);
    let area = area.show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.set_width(w);
            ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
            let measure = |words: &str, size: f32| galley(ui, words, size, f32::INFINITY).size().x;
            let head = marks::head(&rows, w, dark, &measure);
            let foot = marks::foot(&rows, w, dark, &measure);
            let mut block = |ui: &mut egui::Ui, pieces: &[Piece], height: f32| {
                let size = vec2(ui.available_width(), height);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                for piece in pieces {
                    let got = paint(ui, rect.min, piece, &ink, &mut drawn);
                    asked = asked.take().or(got);
                }
                rect
            };
            let top = model::CHIP_MENU_HEADER_PT + model::CHIP_MENU_CAPTION_PT;
            block(ui, &head, top);
            let field = rows.filter_row(&menu.filter);
            if let Some(field) = field.map(|row| marks::filter(row, w, dark)) {
                let rect = block(ui, std::slice::from_ref(&field), model::CHIP_MENU_FILTER_PT);
                let at = rect.min + vec2(field.at.x + 8.0, 4.0);
                let place = Rect::from_min_size(at, vec2(field.at.w - 16.0, 16.0));
                let edit = egui::TextEdit::singleline(&mut menu.filter).frame(false);
                let edit = ui.put(place, edit.hint_text(cells::FILTER_PLACEHOLDER));
                edit.request_focus();
                if edit.changed() {
                    menu.filter_changed();
                }
            }
            let mut list = egui::ScrollArea::vertical().max_height(layout.list_height);
            if !fresh && let Some(offset) = menu.reveal(current, layout.list_height) {
                list = list.vertical_scroll_offset(offset);
            }
            let focus = menu.focus;
            let list = list.auto_shrink([false, true]).show(ui, |ui| {
                for (index, row) in shown.iter().enumerate() {
                    let piece = marks::row(row, ui.available_width(), focus == Some(index), dark);
                    block(ui, std::slice::from_ref(&piece), model::CHIP_MENU_ROW_PT);
                }
                ui.add_space(model::CHIP_MENU_LIST_PAD_PT);
            });
            menu.scrolled(list.state.offset.y);
            block(ui, &foot, model::CHIP_MENU_FOOTER_PT);
        });
    });
    let rect = area.response.rect;
    let outside = |at: egui::Pos2| !anchor.contains(at) && !rect.contains(at);
    let pointer = |i: &egui::InputState| i.pointer.interact_pos().is_some_and(outside);
    let out = ctx.input(|i| i.pointer.any_pressed() && pointer(i));
    commands.extend(asked.clone().flatten().map(MenuCommand::Workspace));
    if asked.is_some() || escape || out {
        menu.close_chip_menu();
    }
    drawn.insert(0, (CellRow::menu(n), rect));
    drawn.retain(|_| !fresh); // egui lays a new area out unseen first.
    drawn
}

/// `text` at `size` points, cut short to `width`.
fn galley(ui: &egui::Ui, text: &str, size: f32, width: f32) -> std::sync::Arc<egui::Galley> {
    let (words, cut) = (
        egui::RichText::new(text).size(size),
        Some(TextWrapMode::Truncate),
    );
    egui::WidgetText::from(words).into_galley(ui, cut, width, egui::TextStyle::Button)
}

/// `piece` from `at`, its hover fill over its first mark, its row in `drawn`
/// while half of it shows, its tip naming it whole while cut short.
fn paint(ui: &egui::Ui, at: egui::Pos2, piece: &Piece, ink: &Ink<'_>, drawn: &mut Drawn) -> Asked {
    let rect = |a: marks::Area| Rect::from_min_size(at + vec2(a.x, a.y), vec2(a.w, a.h));
    let (whole, painter, mut cut) = (rect(piece.at), ui.painter(), false);
    let key = piece.row.as_ref().map_or("", |row| row.key.as_str());
    let sense = [Sense::hover(), Sense::click()][usize::from(piece.press)];
    let mut response = ui.interact(whole, ui.id().with(("workspace_menu", key)), sense);
    for (index, mark) in piece.marks.iter().enumerate() {
        match mark {
            Mark::Box(b) => {
                let edge = egui::Stroke::new(1.0_f32, ink(b.edge));
                painter.rect(rect(b.at), b.radius, ink(b.fill), edge);
            }
            Mark::Text(t) => {
                let words = galley(ui, &t.text, t.size, t.width);
                let place = at + vec2(t.x, t.y - words.size().y / 2.0);
                cut |= words.elided;
                painter.galley(place, words, ink(Some(t.ink)));
            }
            Mark::Tree(area, tree) => paint_tree(painter, rect(*area), tree),
        }
        if index == 0 && response.hovered() && piece.hover.is_some() {
            painter.rect_filled(whole, 4.0, ink(piece.hover));
        }
    }
    let row = piece.row.as_ref()?;
    let seen = whole.intersect(ui.clip_rect());
    if seen.height() * 2.0 >= whole.height() {
        drawn.push((row.clone(), seen));
    }
    if let Some(tip) = row.tip(cut) {
        response = response.on_hover_text(tip);
    }
    response.clicked().then(|| row.command.clone())
}

/// A token's colour, clear for none; the rows drawn, for the scene.
type Ink<'a> = dyn Fn(Option<Token>) -> Color32 + 'a;
type Drawn = Vec<(CellRow, Rect)>;

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
