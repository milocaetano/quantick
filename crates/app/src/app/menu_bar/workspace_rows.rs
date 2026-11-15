//! Paints the menu below the capsule's workspace name and the name dialog; the
//! `⋮` menu's workspace rows are `chrome_menu_bar`'s. The rows are `cell_menu`'s,
//! the size, filter, focus and scroll `workspace_menu`'s and the marks
//! `quantick_ui_model::workspace_menu`'s: this turns them into pixels and presses.

use eframe::egui::scroll_area::ScrollBarVisibility::AlwaysHidden;
use eframe::egui::{self, Color32, Key, Modifiers, Rect, RichText, Sense, TextWrapMode, vec2};
use quantick_appearance::Token;
use quantick_control_handlers::workspace_book::cell_menu::{self as cells, CellMenuCache, CellRow};
use quantick_control_schema::window_menu as text;
use quantick_session::workspace_menu::{self as model, CHIP_MENU_RADIUS_PT as RADIUS, MenuBounds};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_ui_model::workspace_menu::{self as marks, Mark, Piece};

use super::MenuCommand;
use crate::app::QuantickApp;
use crate::toolbar::capsule::paint_tree;

const NAME_FIELD_WIDTH_PT: f32 = 220.0;
type Asked = Option<Option<WorkspaceCommand>>; // A press's command; None on the tab's own row.

/// The capsule's workspace menu on the presented tab: `cell_menu`'s rows as
/// `marks` lays them at `chip_menu_layout`'s size; answers the rows drawn.
pub(super) fn chip_menu(
    app: &QuantickApp,
    ctx: &egui::Context,
    anchor: Rect,
    (menu, cache): (&mut WorkspaceMenuState, &mut CellMenuCache),
    keys: String,
    commands: &mut Vec<MenuCommand>,
) -> Vec<(CellRow, Rect)> {
    let (tab, tabs, mut drawn) = (app.runtime.tabs.active_id(), &app.runtime.tabs, Vec::new());
    if !menu.chip_menu_on(tab) {
        return drawn;
    }
    let label = |id| tabs.by_id(id).map(|t| t.symbol.clone()).unwrap_or_default();
    let live = app.active_tab().canvas.state.tree();
    let (palette, border) = (crate::theme::get(ctx), model::CHIP_MENU_BORDER_PT);
    let dark = palette.resolved.dark;
    let rows = cache.menu(app.runtime.workspace.book.book(), tab, &keys, live, dark, label);
    let keys = [Key::ArrowDown, Key::ArrowUp, Key::Enter, Key::Escape];
    let [down, up, enter, escape] =
        ctx.input_mut(|i| keys.map(|k| i.consume_key(Modifiers::NONE, k)));
    let at = |r: Rect| MenuBounds::new(r.left(), r.top(), r.right(), r.bottom());
    let (n, screen, anchored) = (rows.list.len(), at(ctx.screen_rect()), at(anchor));
    let layout = |shown| model::chip_menu_layout(screen, anchored, n, shown);
    let w = layout(n).width - 2.0 * border;
    let ink = |t: Option<Token>| t.map_or(Color32::TRANSPARENT, |t| *palette.get(t));
    let chrome = quantick_appearance::workspace_menu::chrome(dark);
    let edge = egui::Stroke::new(border, ink(Some(chrome.edge)));
    let frame = egui::Frame::popup(&ctx.style()).stroke(egui::Stroke::NONE);
    let frame = frame.inner_margin(border).rounding(RADIUS);
    let frame = frame.fill(ink(Some(chrome.ground)));
    let id = egui::Id::new(("tab_workspace_menu", tab));
    let fresh = ctx.memory(|memory| memory.area_rect(id).is_none());
    let area = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos((anchor.right_bottom() + vec2(0.0, model::CHIP_MENU_DROP_PT)).round())
        .constrain(true);
    let (mut asked, mut keyed): (Asked, Asked) = (None, None);
    let area = area.show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.set_width(w);
            let measure =
                |s: &str, pt: f32| galley(ui, RichText::new(s).size(pt), f32::MAX).size().x;
            let head = marks::head(rows, w, dark, &measure);
            let foot = marks::foot(rows, w, dark, &measure);
            let mut block = |ui: &mut egui::Ui, pieces: &[Piece], height: f32| {
                let size = vec2(ui.available_width(), height);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                for piece in pieces {
                    let got = paint(ui, rect.min, piece, &ink, &mut drawn);
                    asked = asked.take().or(got);
                }
                rect
            };
            block(ui, &head, marks::HEAD_PT);
            let field = rows.filter_row(&menu.filter);
            if let Some(field) = field.map(|row| marks::filter(row, w, dark)) {
                let rect = block(ui, std::slice::from_ref(&field), model::CHIP_MENU_FILTER_PT);
                let words = marks::filter_text(w);
                let place =
                    Rect::from_min_size(rect.min + vec2(words.x, words.y), vec2(words.w, words.h));
                let edit = egui::TextEdit::singleline(&mut menu.filter).frame(false);
                let mut field = ui.new_child(egui::UiBuilder::new().max_rect(place)); // Off the layout.
                let edit = field.add(edit.hint_text(cells::FILTER_PLACEHOLDER));
                edit.request_focus();
                if edit.changed() {
                    menu.filter_changed();
                }
            }
            let shown = rows.shown(&menu.filter); // Narrowed by this frame's keystroke.
            let current = shown.iter().position(|row| row.on);
            let chosen = menu.keys([down, up, enter], shown.len(), current);
            keyed = chosen.map(|at| shown[at].command.clone());
            let height = layout(shown.len()).list_height;
            let list = egui::ScrollArea::vertical().max_height(height);
            let list = list.vertical_scroll_offset(menu.offset(current, height, !fresh));
            let list = list.scroll_bar_visibility(AlwaysHidden); // The cut row says more.
            let list = list.auto_shrink([false, true]).show(ui, |ui| {
                for (index, row) in shown.iter().enumerate() {
                    let piece =
                        marks::row(row, ui.available_width(), menu.focus == Some(index), dark);
                    block(ui, std::slice::from_ref(&piece), model::CHIP_MENU_ROW_PT);
                }
                ui.add_space(model::CHIP_MENU_LIST_PAD_PT);
            });
            menu.scrolled(list.state.offset.y);
            block(ui, &foot, model::CHIP_MENU_FOOTER_PT);
            let rim = ui.min_rect().expand(0.5); // The border, painted over the blocks.
            ui.painter().rect_stroke(rim, RADIUS - 0.5, edge);
        });
    });
    let rect = area.response.rect;
    let cover = ctx.screen_rect();
    let shield = egui::Area::new(id.with("shield")).order(egui::Order::Middle); // Eats a click out.
    let shield = shield.fixed_pos(cover.min);
    let out = shield.show(ctx, |ui| ui.allocate_rect(cover, Sense::click()));
    let out = out.inner.clicked();
    let asked = keyed.or(asked);
    commands.extend(asked.clone().flatten().map(MenuCommand::Workspace));
    if asked.is_some() || escape || out {
        menu.close_chip_menu();
    }
    drawn.insert(0, (CellRow::menu(n), rect));
    drawn.retain(|_| !fresh); // egui lays a new area out unseen first.
    drawn
}

/// `words`, cut short to `width`.
fn galley(ui: &egui::Ui, words: RichText, width: f32) -> std::sync::Arc<egui::Galley> {
    let (words, cut) = (egui::WidgetText::from(words), Some(TextWrapMode::Truncate));
    words.into_galley(ui, cut, width, egui::TextStyle::Button)
}

/// `piece` from `at`, its hover fill over its first mark, its row in `drawn`
/// while half of it shows, its tip naming it whole while its name is cut.
fn paint(ui: &egui::Ui, at: egui::Pos2, piece: &Piece, ink: &Ink<'_>, drawn: &mut Drawn) -> Asked {
    let rect = |a: marks::Area| Rect::from_min_size(at + vec2(a.x, a.y), vec2(a.w, a.h));
    let (whole, painter, mut cut) = (rect(piece.at), ui.painter(), false);
    let key = piece.row.as_ref().map_or("", |row| row.key.as_str());
    let sense = [Sense::hover(), Sense::click()][usize::from(piece.press)];
    let mut response = ui.interact(whole, ui.id().with(("workspace_menu", key)), sense);
    let plate = |b: &marks::Plate| {
        let fill = ink(b.fill).gamma_multiply(f32::from(b.alpha) / 255.0);
        let stroke = egui::Stroke::new(1.0_f32, ink(b.edge));
        painter.rect(rect(b.at), b.radius, fill, stroke);
    };
    for (index, mark) in piece.marks.iter().enumerate() {
        match mark {
            Mark::Box(b) => plate(b),
            Mark::Text(t) => {
                let words = RichText::new(&t.text).size(t.size);
                let words = galley(ui, words.extra_letter_spacing(t.tracking), t.width);
                let place = at + vec2(t.x, t.y - words.size().y / 2.0);
                cut |= words.elided && t.names;
                painter.galley(place, words, ink(Some(t.ink)));
            }
            Mark::Tree(area, tree) => paint_tree(painter, rect(*area), tree),
            Mark::Colour { at, radius, rgb } => {
                painter.rect_filled(rect(*at), *radius, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
            }
        }
        if let Some(hover) = piece.hover.filter(|_| index == 0 && response.hovered()) {
            plate(&hover);
        }
    }
    let row = piece.row.as_ref()?;
    let seen = whole.intersect(ui.clip_rect());
    drawn.extend((seen.height() * 2.0 >= whole.height()).then(|| (row.clone(), seen)));
    if let Some(tip) = row.tip(cut) {
        response = response.on_hover_text(tip);
    }
    response.clicked().then(|| row.command.clone())
}

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
