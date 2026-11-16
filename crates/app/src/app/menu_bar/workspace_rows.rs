//! Paints the workspace surfaces' marks: the menu below the capsule's
//! workspace name, its palette popover, and through [`sheet`] Manage and the
//! name dialogs too; the `⋮` menu's workspace rows are `chrome_menu_bar`'s.
//! The rows are `cell_menu`'s, the size, filter, focus and scroll
//! `workspace_menu`'s and the marks `quantick_ui_model`'s: this turns them
//! into pixels and presses.

use eframe::egui::scroll_area::ScrollBarVisibility::{AlwaysHidden, VisibleWhenNeeded};
use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{self, Color32, Key, Modifiers, Rect, RichText, Sense, TextWrapMode, vec2};
use quantick_appearance::Token;
use quantick_control_handlers::workspace_book::cell_menu::{self as cells, CellMenuCache, CellRow};
use quantick_control_handlers::workspace_book::colour_popover;
use quantick_session::colour_popover::{ColourIntent, ColourPopover, Key as PopoverKey};
use quantick_session::workspace_menu::{self as model, MenuBounds};
use quantick_session::workspace_menu::{WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::WorkspaceBook;
use quantick_ui_model::colour_palette as palette;
use quantick_ui_model::workspace_menu::{self as marks, Mark, Piece, Sheet};

use super::MenuCommand;
use crate::app::QuantickApp;
use crate::toolbar::capsule::paint_tree;

type Asked = Option<Option<WorkspaceCommand>>; // A press's command; None on the tab's own row.
pub(super) type Pen<'a, 'b> = (&'a Ink<'a>, &'b mut Drawn, bool);
/// What builds a sheet, given what measures words.
pub(super) type View<'a> = &'a dyn Fn(&dyn Fn(&str, f32) -> f32) -> Sheet;
/// A sheet's text field and whether it selects its words, its palette's state
/// and keys, and the list's scroll offset when the caller keeps it.
pub(super) type Parts<'a> = (
    Option<(&'a mut String, &'a mut bool)>,
    Option<(&'a mut ColourPopover, [bool; 8])>,
    Option<f32>,
);

/// What a sheet's frame did: its press, Enter leaving its field, whether
/// Save would save, a typed change, the list's offset, its rectangle and a
/// click outside it.
#[derive(Default)]
pub(super) struct Painted {
    pub asked: Asked,
    pub enter: bool,
    pub saves: bool,
    pub offset: f32,
    pub rect: Option<Rect>,
    pub out: bool,
}

/// The capsule's workspace menu on the presented tab: `cell_menu`'s rows as
/// one sheet at `chip_menu_layout`'s size; answers the rows drawn.
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
    let (live, theme) = (app.active_tab().canvas.state.tree(), crate::theme::get(ctx));
    let (book, dark) = (app.runtime.workspace.book.book(), theme.resolved.dark);
    let rows = cache.menu(book, tab, &keys, live, dark, label);
    let inert = menu.colour.is_some(); // The popover takes the keys and presses.
    let keys = [Key::ArrowDown, Key::ArrowUp, Key::Enter, Key::Escape];
    let [down, up, enter, escape] =
        ctx.input_mut(|i| keys.map(|k| !inert && i.consume_key(Modifiers::NONE, k)));
    let at = |r: Rect| MenuBounds::new(r.left(), r.top(), r.right(), r.bottom());
    let (n, screen, anchored) = (rows.list.len(), at(ctx.screen_rect()), at(anchor));
    let id = egui::Id::new(("tab_workspace_menu", tab));
    let laid = ctx.memory(|m| m.area_rect(id)).filter(|_| rows.filters());
    if let Some(shown) = laid {
        filter(ctx, id, shown, menu); // This frame's keystroke, before its keys and size.
    }
    let shown = rows.shown(&menu.filter);
    let current = shown.iter().position(|row| row.on);
    let keyed = menu.keys([down, up, enter], shown.len(), current);
    let keyed = keyed.map(|at| shown[at].command.clone());
    let layout = model::chip_menu_layout(screen, anchored, n, shown.len());
    let ink = |t: Option<Token>| t.map_or(Color32::TRANSPARENT, |t| *theme.get(t));
    let fresh = ctx.memory(|memory| memory.area_rect(id).is_none());
    let drop = (anchor.right_bottom() + vec2(0.0, model::CHIP_MENU_DROP_PT)).round();
    let area = egui::Area::new(id).order(egui::Order::Foreground);
    let area = area.fixed_pos(drop).pivot(egui::Align2::RIGHT_TOP);
    let area = area.constrain(true);
    let (typed, focus) = (menu.filter.clone(), menu.focus);
    let view = |m: &dyn Fn(&str, f32) -> f32| marks::sheet(rows, (&typed, focus), &layout, dark, m);
    let offset = Some(menu.offset(current, layout.list_height, !fresh));
    let parts = (None, None, offset);
    let painted = sheet(ctx, area, &view, parts, (&ink, &mut drawn, dark));
    menu.scrolled(painted.offset);
    let swatch = cells::drawn(&drawn, "colour").map(|at| at.shrink(4.0));
    let held = (book, rows.current.label.as_str(), dark);
    let popover = swatch.map(|at| colour_popover(ctx, held, at, menu, (&ink, &mut drawn, dark)));
    let (picked, off) = popover.unwrap_or_default();
    let cover = ctx.screen_rect();
    let shield = egui::Area::new(id.with("shield")).order(egui::Order::Middle); // Eats a click out.
    let shield = shield.fixed_pos(cover.min);
    let out = shield.show(ctx, |ui| ui.allocate_rect(cover, Sense::click()));
    let asked = picked.or(keyed).or(painted.asked.filter(|_| !inert)); // Under the popover.
    commands.extend(asked.clone().flatten().map(MenuCommand::Workspace));
    menu.settle(asked.as_ref(), escape, out.inner.clicked() || off);
    drawn.insert(0, (CellRow::menu(n), painted.rect.unwrap_or(anchor)));
    drawn.retain(|_| !fresh); // egui lays a new area out unseen first.
    drawn
}

/// The filter field over the menu laid at `shown`, above its marks: typed
/// into before the menu's keys and size are read.
fn filter(ctx: &egui::Context, id: egui::Id, shown: Rect, menu: &mut WorkspaceMenuState) {
    let border = model::CHIP_MENU_BORDER_PT;
    let words = marks::filter_text(shown.width() - 2.0 * border);
    let at = shown.min + vec2(border, border + marks::HEAD_PT);
    let edit = egui::TextEdit::singleline(&mut menu.filter).frame(false);
    let edit = edit.desired_width(words.w);
    let edit = edit.hint_text(cells::FILTER_PLACEHOLDER);
    let area = egui::Area::new(id.with("filter")).order(egui::Order::Tooltip);
    let area = area.fixed_pos(place(at, words).min);
    let edit = area.show(ctx, |ui| ui.add(edit)).inner;
    edit.request_focus();
    if edit.changed() {
        menu.filter_changed();
    }
}

/// The palette popover under the header `swatch` while `menu` holds it open,
/// as a sheet of its own. Its press or key, and whether a click landed
/// outside it.
fn colour_popover(
    ctx: &egui::Context,
    (book, workspace, dark): (&WorkspaceBook, &str, bool),
    swatch: Rect,
    menu: &mut WorkspaceMenuState,
    pen: Pen<'_, '_>,
) -> (Asked, bool) {
    let Some(state) = menu.colour.as_mut() else {
        return (None, false);
    };
    let named = |k: PopoverKey| Key::from_name(k.name()).filter(|_| state.takes(k));
    let pressed = ctx.input_mut(|i| {
        PopoverKey::ALL.map(|k| named(k).is_some_and(|k| i.consume_key(Modifiers::NONE, k)))
    });
    let area = egui::Area::new(egui::Id::new("colour_popover")).order(egui::Order::Tooltip);
    let area = area.fixed_pos((swatch.left_bottom() + vec2(0.0, palette::DROP_PT)).round());
    let model = quantick_stores::workspaces::palette::palette(book, workspace);
    model.inspect(|model| colour_popover::settle(model, state, dark)); // Kept frame to frame.
    let held = state.clone();
    let view = |m: &dyn Fn(&str, f32) -> f32| {
        let pop = palette::popover(book, workspace, &mut held.clone(), dark, m);
        pop.map_or_else(|| Sheet::of([0.0; 2], Vec::new()), palette::Popover::sheet)
    };
    let parts = (None, Some((state, pressed)), None);
    let painted = sheet(ctx, area.constrain(true), &view, parts, pen);
    (painted.asked, painted.out)
}

/// One sheet in `area` on the menu's ground with its rim over it: its head
/// and foot, its list in a scroll area (at the given offset, its bar hidden,
/// else shown and brought to the sheet's focus), the text field and the
/// palette where the sheet says. One painter for every workspace surface.
pub(super) fn sheet(
    ctx: &egui::Context,
    area: egui::Area,
    view: View<'_>,
    (text, pop, offset): Parts<'_>,
    (ink, drawn, dark): Pen<'_, '_>,
) -> Painted {
    let chrome = quantick_appearance::workspace_menu::chrome(dark);
    let frame = egui::Frame::popup(&ctx.style()).stroke(egui::Stroke::NONE);
    let frame = frame.inner_margin(model::CHIP_MENU_BORDER_PT);
    let frame = frame.rounding(RADIUS).fill(ink(Some(chrome.ground)));
    let (mut painted, kept) = (Painted::default(), drawn.len());
    let fresh = ctx.memory(|m| m.area_rect(area.layer().id).is_none());
    let shown = area.show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let view = view(&measure(ui));
            painted.saves = view.saves;
            let (rect, _) =
                ui.allocate_exact_size(vec2(view.size[0], view.size[1]), Sense::hover());
            painted.asked = put(ui, rect.min, &view.head, (ink, drawn));
            let foot = put(ui, rect.min, &view.foot, (ink, drawn));
            let list = rect.min + vec2(0.0, view.list_top);
            let list = Rect::from_min_size(list, vec2(view.size[0], view.list_view));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
            child.spacing_mut().scroll = egui::style::ScrollStyle::solid(); // Seen when it scrolls.
            let bar = [VisibleWhenNeeded, AlwaysHidden][usize::from(offset.is_some())];
            let scroll = egui::ScrollArea::vertical().max_height(list.height());
            let scroll = scroll.scroll_bar_visibility(bar).auto_shrink([false; 2]);
            let scroll = (offset.into_iter()).fold(scroll, |s, o| s.vertical_scroll_offset(o));
            let scrolled = scroll.show(&mut child, |ui| {
                ui.set_clip_rect(list.intersect(ui.clip_rect())); // Never over the head.
                let size = vec2(view.size[0], view.list_height);
                let body = ui.allocate_exact_size(size, Sense::hover()).0.min;
                if let Some(at) = view.focus {
                    ui.scroll_to_rect(place(body, at), None);
                }
                put(ui, body, &view.list, (ink, drawn))
            });
            let body = list.min - vec2(0.0, scrolled.state.offset.y);
            if let (Some((at, pane)), Some((state, keys))) = (&view.palette, pop) {
                let origin = body + vec2(at[0], at[1]);
                let mut lay = |ui: &mut egui::Ui| {
                    let got = palette_at(ui, origin, pane, state, (ink, drawn));
                    let inside = Rect::from_min_size(origin, vec2(pane.size[0], pane.size[1]));
                    ui.allocate_rect(inside, Sense::hover()); // Its frame's size.
                    pane.keyed(state, keys).map(Some).or(got)
                };
                // Manage's hangs over the rows below its chip, a click out
                // closing it; a dialog's stands in its list.
                let got = if view.closes_out {
                    let edge = egui::Stroke::new(1.0_f32, ink(Some(chrome.edge)));
                    let area = egui::Area::new(ui.id().with("row_palette")).fixed_pos(origin);
                    let framed = |ui: &mut egui::Ui| frame.stroke(edge).show(ui, lay).inner;
                    let shown = area.order(egui::Order::Tooltip).show(ui.ctx(), framed);
                    let close = Some(WorkspaceCommand::Colour(ColourIntent::Close));
                    let out = shown.response.clicked_elsewhere();
                    shown.inner.or(out.then_some(close))
                } else {
                    let mut inline = ui.new_child(egui::UiBuilder::new().max_rect(list));
                    inline.set_clip_rect(list.intersect(inline.clip_rect()));
                    lay(&mut inline)
                };
                painted.asked = painted.asked.take().or(got);
            }
            if let (Some(at), Some(field)) = (view.field, text) {
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(place(body, at)));
                ui.set_clip_rect(list.intersect(ui.clip_rect()));
                field_at(&mut ui, field, &mut painted);
            }
            painted.asked = painted.asked.take().or(foot).or(scrolled.inner);
            painted.offset = scrolled.state.offset.y;
            let rim = ui.min_rect().expand(0.5); // The border, painted over the blocks.
            let edge = egui::Stroke::new(model::CHIP_MENU_BORDER_PT, ink(Some(chrome.edge)));
            ui.painter().rect_stroke(rim, RADIUS - 0.5, edge);
        });
    });
    painted.rect = Some(shown.response.rect);
    painted.out = shown.response.clicked_elsewhere();
    drawn.truncate(if fresh { kept } else { drawn.len() }); // Laid out unseen first.
    painted
}

const RADIUS: f32 = model::CHIP_MENU_RADIUS_PT;

/// A sheet's text field: its words selected when it first takes the
/// keyboard; whether Enter left it.
fn field_at(ui: &mut egui::Ui, (text, select): (&mut String, &mut bool), painted: &mut Painted) {
    let id = ui.make_persistent_id("workspace_sheet_field");
    let edit = egui::TextEdit::singleline(text).frame(false);
    let mut out = edit.id(id).show(ui);
    if std::mem::take(select) {
        let end = CCursor::new(out.galley.text().chars().count());
        let all = CCursorRange::two(CCursor::new(0), end);
        out.state.cursor.set_char_range(Some(all));
        out.state.store(ui.ctx(), id);
        out.response.request_focus();
    }
    painted.enter = out.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
}

/// The palette `pop` from `origin`, `state` its popover, egui's field over
/// Custom's: the one painter of the header's, Manage's and the create
/// dialogs' palette. Its press.
pub(super) fn palette_at(
    ui: &mut egui::Ui,
    origin: egui::Pos2,
    pop: &palette::Popover,
    state: &mut ColourPopover,
    (ink, drawn): (&Ink<'_>, &mut Drawn),
) -> Asked {
    let asked = put(ui, origin, &pop.pieces, (ink, drawn));
    if let Some(f) = pop.field {
        let mut field = ui.new_child(egui::UiBuilder::new().max_rect(place(origin, f)));
        field.visuals_mut().text_cursor.stroke.color = ink(Some(Token::TextPrimary));
        let edit = egui::TextEdit::singleline(&mut state.hex).frame(false);
        field.add(edit).request_focus();
    }
    asked
}

/// `pieces` from `origin`; the first press.
pub(super) fn put(
    ui: &egui::Ui,
    origin: egui::Pos2,
    pieces: &[Piece],
    (ink, drawn): (&Ink<'_>, &mut Drawn),
) -> Asked {
    (pieces.iter()).fold(None, |asked, piece| {
        asked.or(paint(ui, origin, piece, ink, drawn))
    })
}

/// `area` from `origin`, in points.
pub(super) fn place(origin: egui::Pos2, area: marks::Area) -> Rect {
    Rect::from_min_size(origin + vec2(area.x, area.y), vec2(area.w, area.h))
}

/// What measures words in `ui`: their width at a size.
pub(super) fn measure(ui: &egui::Ui) -> impl Fn(&str, f32) -> f32 + '_ {
    |s: &str, pt: f32| galley(ui, RichText::new(s).size(pt), f32::MAX).size().x
}

/// `words`, cut short to `width`.
pub(super) fn galley(ui: &egui::Ui, words: RichText, width: f32) -> std::sync::Arc<egui::Galley> {
    let (words, cut) = (egui::WidgetText::from(words), Some(TextWrapMode::Truncate));
    words.into_galley(ui, cut, width, egui::TextStyle::Button)
}

/// `piece` from `at`, its hover fill over its first mark, its row in `drawn`
/// while half of it shows, its tip naming it whole while its name is cut; a
/// drag piece answers where it is pressed.
pub(super) fn paint(
    ui: &egui::Ui,
    at: egui::Pos2,
    piece: &Piece,
    ink: &Ink<'_>,
    drawn: &mut Drawn,
) -> Asked {
    let rect = |a: marks::Area| place(at, a);
    let (whole, painter, mut cut) = (rect(piece.at), ui.painter(), false);
    let key = piece.row.as_ref().map_or("", |row| row.key.as_str());
    let senses = [Sense::hover(), Sense::click(), Sense::click_and_drag()];
    let sense = senses[usize::from(piece.press) + usize::from(piece.drag)];
    let mut response = ui.interact(whole, ui.id().with(("workspace_menu", key)), sense);
    let plate = |b: &marks::Plate| {
        let own = b.rgb.map(|[r, g, b]| Color32::from_rgb(r, g, b));
        let fill = own
            .unwrap_or_else(|| ink(b.fill))
            .gamma_multiply(f32::from(b.alpha) / 255.0);
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
                let own = t.rgb.map(|[r, g, b]| Color32::from_rgb(r, g, b));
                painter.galley(place, words, own.unwrap_or_else(|| ink(Some(t.ink))));
            }
            Mark::Tree(area, tree) => paint_tree(painter, rect(*area), tree, Token::Accent),
            Mark::Colour { at, radius, rgb } => {
                let [r, g, b] = *rgb;
                painter.rect_filled(rect(*at), *radius, Color32::from_rgb(r, g, b));
            }
            Mark::Shade(corners) => {
                let mut mesh = egui::Mesh::default();
                for ([x, y], [r, g, b]) in corners {
                    mesh.colored_vertex(at + vec2(*x, *y), Color32::from_rgb(*r, *g, *b));
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(mesh);
            }
            Mark::Hatch {
                at,
                rgba: [r, g, b, a],
            } => {
                let line = Color32::from_rgba_unmultiplied(*r, *g, *b, *a);
                let (area, stroke) = (rect(*at), egui::Stroke::new(1.0_f32, line));
                for step in (0..(area.width() + area.height()) as i32).step_by(6) {
                    let from = area.left_bottom() + vec2(step as f32 - area.height(), 0.0);
                    let line = [from, from + vec2(area.height(), -area.height())];
                    painter.with_clip_rect(area).line_segment(line, stroke);
                }
            }
        }
        if let Some(hover) = piece.hover.filter(|_| index == 0 && response.hovered()) {
            plate(&hover);
        }
    }
    let row = piece.row.as_ref()?;
    let pressed = response.dragged() || response.clicked();
    if let Some(p) = response
        .interact_pointer_pos()
        .filter(|_| piece.drag && pressed)
    {
        let at = (p - whole.min) / whole.size();
        return palette::dragged(key, [at.x, at.y]).map(Some);
    }
    let seen = whole.intersect(ui.clip_rect());
    drawn.extend((seen.height() * 2.0 >= whole.height()).then(|| (row.clone(), seen)));
    if let Some(tip) = row.tip(cut) {
        response = response.on_hover_text(tip);
    }
    let pressed = [response.clicked(), response.double_clicked()][usize::from(piece.double)];
    pressed.then(|| row.command.clone())
}

pub(super) type Ink<'a> = dyn Fn(Option<Token>) -> Color32 + 'a;
pub(super) type Drawn = Vec<(CellRow, Rect)>;
