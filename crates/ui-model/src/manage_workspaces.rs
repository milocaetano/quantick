//! Manage workspaces (designer A, the compact table) and the dialogs that ask
//! a workspace's name, as marks to paint. Manage: a title bar, the WORKSPACE
//! and TABS captions, one 36-point row per workspace (its colour chip, name,
//! CURRENT pill, summary, tab count, pencil and trash), Default's line, the
//! one row whose palette, rename field, armed delete or Undo stands in
//! place, and the footer. The name dialogs: the field, the palette on the
//! chosen colour (Save as and New only) and Save. The state is
//! `quantick_session`'s, what a press does `quantick_control_handlers`'; the
//! window turns both [`Sheet`]s into pixels and presses with one painter,
//! scrolls the list in its fixed height and lays the text field and the
//! palette where the sheet says.

use quantick_appearance::Token;
use quantick_appearance::workspace_menu::{FilledButton, MenuChrome, chrome};
use quantick_control_handlers::workspace_book::cell_menu::{
    CellRow, RowColour, RowKind, described,
};
use quantick_control_handlers::workspace_book::manage::chosen;
use quantick_control_schema::window_menu as text;
use quantick_control_schema::workspace_book as calls;
use quantick_control_schema::workspace_manage::{
    WORKSPACE_DELETE_UNDO_CAPABILITY_ID as UNDO_CALL, WORKSPACE_DIALOG_OPEN_CAPABILITY_ID as OPEN,
};
use quantick_session::colour_popover::ColourIntent;
use quantick_session::manage_workspaces::{ManageDialog, ManageIntent};
use quantick_session::workspace_menu::{NameDialog, NameDialogMode, WorkspaceCommand};
use quantick_stores::workspaces::palette::{Pick, choosing, said};
use quantick_stores::workspaces::{
    DEFAULT_WORKSPACE, WorkspaceBook, WorkspaceColour, parse_typed_hex,
};

use crate::colour_palette;
pub use crate::workspace_menu::Sheet;
use crate::workspace_menu::{Area, Mark, Measure, Piece, Plate, Words, area, legible, tinted};
use quantick_appearance::workspace_menu::RowTint;

/// The scene ids' first word: Manage's, and the name dialogs'.
pub const SCOPE: &str = "manage_workspaces";
pub const NAME_SCOPE: &str = "name_dialog";
/// Manage's width, its title bar, the captions, a row, the footer.
pub const WIDTH_PT: f32 = 520.0;
pub const TITLE_PT: f32 = 32.0;
pub const CAPTION_PT: f32 = 22.0;
pub const ROW_PT: f32 = 36.0;
pub const FOOT_PT: f32 = 44.0;
/// Default's two-line note, and a refusal's line under a row.
const NOTE_PT: f32 = 30.0;
const LINE_PT: f32 = 18.0;
/// The list's fixed height: eight rows and Default's note show whole; more
/// scroll, and nothing open in a row ever resizes the dialog.
pub const LIST_PT: f32 = 8.0 * ROW_PT + NOTE_PT;
/// The name dialogs' width.
pub const NAME_WIDTH_PT: f32 = 360.0;
const PAD: f32 = 12.0;
/// The chip, its press, and where the name column starts.
const CHIP: f32 = 16.0;
const HIT: f32 = 24.0;
const NAME_X: f32 = 44.0;
/// The pencil and trash presses and the gap between them.
const ICON_GAP: f32 = 8.0;
const RADIUS: f32 = 4.0;
const BODY_PT: f32 = 12.5;
const SMALL_PT: f32 = 11.0;
const CAPTION_TEXT_PT: f32 = 10.0;
const TITLE_TEXT_PT: f32 = 13.0;
/// The rename field's width.
const FIELD_W: f32 = 168.0;
/// The armed row's tint and the CURRENT pill's, out of 255.
const ARMED_ALPHA: u8 = 28;
const PILL_ALPHA: u8 = 48;
/// Phosphor's pencil, trash, plus and close, in the bundled icon font.
const PENCIL: &str = "\u{e3b4}";
const TRASH: &str = "\u{e4a6}";
const PLUS: &str = "\u{e3d4}";
const CLOSE: &str = "\u{e4f6}";

#[path = "workspace_creation.rs"]
mod creation;

fn words(x: f32, y: f32, said: &str, size: f32, ink: Token, width: f32) -> Mark {
    Mark::Text(Words {
        x,
        y,
        text: said.to_owned(),
        size,
        ink,
        width,
        tracking: 0.0,
        names: false,
        rgb: None,
        on: None,
    })
}

fn plate(at: Area, fill: Option<Token>, edge: Option<Token>, alpha: u8) -> Mark {
    Mark::Box(Plate {
        toward: None,
        at,
        radius: RADIUS,
        fill,
        alpha,
        edge,
    })
}

fn ring(at: Area, ink: Token) -> Option<Plate> {
    Some(Plate {
        toward: None,
        at,
        radius: RADIUS,
        fill: None,
        alpha: u8::MAX,
        edge: Some(ink),
    })
}

/// A press: `marks` over `at`, the row the scene lists it as, ringed in the
/// accent on hover while it presses.
fn press(at: Area, row: CellRow, enabled: bool, marks: Vec<Mark>, ink: &MenuChrome) -> Piece {
    Piece {
        hover: ring(at, ink.accent).filter(|_| enabled),
        row: Some(row),
        press: enabled,
        ..Piece::plain(at, marks)
    }
}

/// A scene row of Manage under `key`.
fn scene(key: &str, label: &str, command: Option<ManageIntent>, call: &'static str) -> CellRow {
    CellRow {
        scope: SCOPE,
        command: command.map(WorkspaceCommand::Manage),
        capability: Some(call),
        ..CellRow::new(RowKind::Press, key, label)
    }
}

/// A button worded `label` at `x`, `y`, `fill` its filled colours or the
/// control's; `None` greys it.
fn button(
    (x, y): (f32, f32),
    row: CellRow,
    fill: Option<FilledButton>,
    ink: &MenuChrome,
    measure: Measure<'_>,
) -> Piece {
    let label = row.label.clone();
    let at = area(x, y, measure(&label, BODY_PT) + 20.0, HIT);
    let enabled = row.unavailable.is_none();
    let (back, edge, said) = fill.map_or((ink.control, ink.edge, ink.ink), |filled| {
        (filled.fill, filled.fill, filled.ink)
    });
    let alpha = if enabled { u8::MAX } else { u8::MAX / 2 };
    let marks = vec![
        plate(at, Some(back), Some(edge), alpha),
        words(at.x + 10.0, at.y + HIT / 2.0, &label, BODY_PT, said, at.w),
    ];
    press(at, row, enabled, marks, ink)
}

/// An icon press 24 x 24 at `x`, greyed with `refusal`'s hover when refused.
fn icon(x: f32, top: f32, glyph: &str, row: CellRow, ink: &MenuChrome) -> Piece {
    let at = area(x, top + (ROW_PT - HIT) / 2.0, HIT, HIT);
    let enabled = row.unavailable.is_none();
    let said = if enabled {
        ink.support
    } else {
        Token::RestingIcon
    };
    let marks = vec![words(at.x + 5.0, at.y + HIT / 2.0, glyph, 13.0, said, HIT)];
    press(at, row, enabled, marks, ink)
}

/// `said`'s width at `size`, ending at `right`.
fn ending(measure: Measure<'_>, right: f32, y: f32, said: &str, size: f32, ink: Token) -> Mark {
    let width = measure(said, size);
    words(right - width, y, said, size, ink, width + 1.0)
}

/// Manage for `dialog` over `book`, on `current` (the tab's workspace) at
/// `now_ms`: `symbols` names tabs, `measure` sizes words.
#[must_use]
pub fn manage(
    book: &WorkspaceBook,
    dialog: &ManageDialog,
    (current, now_ms, dark): (&str, i64, bool),
    symbols: &dyn Fn(&[u64]) -> Vec<String>,
    measure: Measure<'_>,
) -> Sheet {
    let ink = chrome(dark);
    let size = [WIDTH_PT, TITLE_PT + CAPTION_PT + LIST_PT + FOOT_PT];
    let mut view = Sheet::of(size, Vec::new());
    (view.head, view.list_top) = (head(&ink, measure), TITLE_PT + CAPTION_PT);
    (view.list_view, view.list_height) = (LIST_PT, 0.0);
    view.closes_out = true;
    let undo = dialog.undo_row(now_ms);
    let mut undo_left = undo.is_some();
    let listed = book.list();
    for (at, row) in listed.iter().enumerate() {
        if undo_left && at > 0 && undo.is_some_and(|undo| undo.workspace < row.name) {
            undo_left = false;
            undo_row(&mut view, undo, now_ms, &ink, measure);
        }
        let protected = row.name == DEFAULT_WORKSPACE;
        let focused = dialog.focus == Some(view.names.len());
        view.names.push((row.name.clone(), protected));
        let top = view.list_height;
        // Default's band and rule span its note too.
        let tall = ROW_PT + if protected { NOTE_PT } else { 0.0 };
        if focused {
            view.focus = Some(area(0.0, top, WIDTH_PT, tall));
        }
        let tabs = symbols(&row.tabs);
        let line = Line {
            name: &row.name,
            top,
            colour: RowColour::of(row.colour, dark),
            tabs: &tabs,
            protected,
            now_ms,
            focused,
            dark,
        };
        if dialog.armed.as_deref() == Some(row.name.as_str()) {
            let (adopted, kept) = book.delete_split(&row.name);
            let said = (symbols(&adopted), symbols(&kept));
            armed(&mut view, &line, (&said.0, &said.1), (&ink, measure));
        } else {
            plain(&mut view, book, (&line, current, dialog), &ink, measure);
        }
        if let Some(open) = dialog
            .palette
            .as_ref()
            .filter(|open| open.workspace == row.name)
        {
            let state = &mut open.popover.clone(); // Settled for this frame only.
            let mut pop = colour_palette::popover(book, &row.name, state, dark, measure);
            if let Some(pop) = pop.as_mut() {
                pop.scoped(SCOPE, &format!("{}.", row.name));
            }
            // Over the rows below, hung from the chip: every row stays put.
            view.palette = pop.map(|pop| ([PAD - 2.0, top + ROW_PT - 4.0], pop));
            view.focus = Some(area(0.0, top, WIDTH_PT, ROW_PT));
        }
    }
    if undo_left && let Some(undo) = undo {
        undo_row(&mut view, Some(undo), now_ms, &ink, measure);
    }
    view.foot = foot(dialog, &ink, view.list_top + LIST_PT, measure);
    view
}

/// Manage's rows: each name, and whether rename and delete refuse it, as
/// the keys read them.
#[must_use]
pub fn names(book: &WorkspaceBook) -> Vec<(String, bool)> {
    let row = |row: quantick_stores::workspaces::WorkspaceListing| {
        let protected = row.name == DEFAULT_WORKSPACE;
        (row.name, protected)
    };
    book.list().into_iter().map(row).collect()
}

/// One row as it is built.
struct Line<'a> {
    name: &'a str,
    top: f32,
    colour: RowColour,
    tabs: &'a [String],
    protected: bool,
    now_ms: i64,
    focused: bool,
    dark: bool,
}

fn head(ink: &MenuChrome, measure: Measure<'_>) -> Vec<Piece> {
    let bar = area(0.0, 0.0, WIDTH_PT, TITLE_PT);
    let title = words(
        PAD,
        TITLE_PT / 2.0,
        text::MANAGE_DIALOG_TITLE,
        TITLE_TEXT_PT,
        ink.ink,
        WIDTH_PT - 60.0,
    );
    let rule = plate(
        area(0.0, TITLE_PT - 1.0, WIDTH_PT, 1.0),
        Some(ink.block_edge),
        None,
        u8::MAX,
    );
    let mut head = vec![Piece {
        row: Some(CellRow {
            scope: SCOPE,
            on: true,
            capability: Some(OPEN),
            ..CellRow::new(RowKind::Current, "", text::MANAGE_DIALOG_TITLE)
        }),
        ..Piece::plain(
            bar,
            vec![plate(bar, Some(ink.block), None, u8::MAX), rule, title],
        )
    }];
    let x = area(WIDTH_PT - PAD - HIT, (TITLE_PT - HIT) / 2.0, HIT, HIT);
    let close = scene(
        "dismiss",
        text::MANAGE_CLOSE,
        Some(ManageIntent::Close),
        OPEN,
    );
    let glyph = words(x.x + 6.0, x.y + HIT / 2.0, CLOSE, 12.0, ink.support, HIT);
    head.push(press(x, close, true, vec![glyph], ink));
    let y = TITLE_PT + CAPTION_PT / 2.0;
    let captions = vec![
        caption(words(
            NAME_X,
            y,
            text::MANAGE_WORKSPACE_CAPTION,
            CAPTION_TEXT_PT,
            ink.faint,
            200.0,
        )),
        flush_right(caption(ending(
            measure,
            count_right(),
            y,
            text::MANAGE_TABS_CAPTION,
            CAPTION_TEXT_PT,
            ink.faint,
        ))),
    ];
    head.push(Piece::plain(
        area(0.0, TITLE_PT, WIDTH_PT, CAPTION_PT),
        captions,
    ));
    head
}

/// `mark` spaced as a caption is.
fn caption(mut mark: Mark) -> Mark {
    if let Mark::Text(said) = &mut mark {
        said.tracking = 0.08 * said.size;
        // The letters spread: as much more room, so none is cut.
        said.width += said.tracking * said.text.chars().count() as f32;
    }
    mark
}

/// A tracked caption ending where its column does: its spread taken from
/// the left, so TABS ends with the counts.
fn flush_right(mut mark: Mark) -> Mark {
    if let Mark::Text(said) = &mut mark {
        said.x -= said.tracking * said.text.chars().count() as f32;
    }
    mark
}

/// Where the tab count ends: left of the pencil.
fn count_right() -> f32 {
    WIDTH_PT - PAD - 2.0 * HIT - ICON_GAP - 12.0
}

/// The chip 16 x 16 in a 24 x 24 press that opens the row's palette.
fn chip(line: &Line<'_>, ink: &MenuChrome) -> Piece {
    let at = area(PAD + 4.0, line.top + (ROW_PT - CHIP) / 2.0, CHIP, CHIP);
    let hit = area(PAD, line.top + (ROW_PT - HIT) / 2.0, HIT, HIT);
    let row = CellRow {
        tip: Some(format!("{}{}", line.colour.words, " \u{b7} change colour")),
        unavailable: line.protected.then_some("workspace.default_protected"),
        colour: Some(line.colour.clone()),
        hover: Some(text::MANAGE_CHIP_HOVER),
        ..scene(
            &format!("{}.chip", line.name),
            "Colour",
            Some(ManageIntent::Chip(line.name.to_owned())),
            calls::WORKSPACE_COLOUR_SET_CAPABILITY_ID,
        )
    };
    let colour = Mark::Colour {
        at,
        radius: RADIUS,
        rgb: line.colour.rgb,
    };
    press(hit, row, !line.protected, vec![colour], ink)
}

/// The tab count at the right, its hover naming the tabs.
fn count(line: &Line<'_>, ink: &MenuChrome, measure: Measure<'_>) -> Piece {
    let said = text::tab_count_label(line.tabs.len());
    let width = measure(&said, SMALL_PT) + 1.0;
    let at = area(count_right() - width, line.top + 8.0, width, ROW_PT - 16.0);
    let mark = words(
        at.x,
        line.top + ROW_PT / 2.0,
        &said,
        SMALL_PT,
        ink.support,
        width,
    );
    Piece {
        row: Some(CellRow {
            scope: SCOPE,
            tip: Some(text::tab_names_hover(line.tabs)),
            summary: text::tab_names_hover(line.tabs),
            capability: Some(calls::WORKSPACE_LIST_CAPABILITY_ID),
            ..CellRow::new(RowKind::Press, &format!("{}.tabs", line.name), said.clone())
        }),
        ..Piece::plain(at, vec![mark])
    }
}

/// The pencil and the trash, greyed on Default with its reason.
fn icons(line: &Line<'_>, renaming: bool, ink: &MenuChrome) -> [Piece; 2] {
    let (refused, why) = match (line.protected, renaming) {
        (true, _) => (
            Some("the_default_workspace_cannot_be_renamed_or_deleted"),
            Some(text::DEFAULT_PROTECTED_HOVER),
        ),
        (false, true) => (
            Some("finish_renaming_first"),
            Some(text::MANAGE_RENAMING_HOVER),
        ),
        (false, false) => (None, None),
    };
    let x = WIDTH_PT - PAD - HIT;
    let rename = CellRow {
        unavailable: refused,
        hover: Some(text::MANAGE_RENAME_HOVER),
        disabled_hover: why,
        ..scene(
            &format!("{}.rename", line.name),
            "Rename",
            Some(ManageIntent::BeginRename(line.name.to_owned())),
            calls::WORKSPACE_RENAME_CAPABILITY_ID,
        )
    };
    let delete = CellRow {
        unavailable: refused,
        hover: Some(text::MANAGE_DELETE_HOVER),
        disabled_hover: why,
        ..scene(
            &format!("{}.delete", line.name),
            text::MANAGE_DELETE,
            Some(ManageIntent::Arm(line.name.to_owned())),
            calls::WORKSPACE_DELETE_CAPABILITY_ID,
        )
    };
    [
        icon(x - HIT - ICON_GAP, line.top, PENCIL, rename, ink),
        icon(x, line.top, TRASH, delete, ink),
    ]
}

/// A row at rest, or with its name turned into a field.
fn plain(
    view: &mut Sheet,
    book: &WorkspaceBook,
    (line, current, dialog): (&Line<'_>, &str, &ManageDialog),
    ink: &MenuChrome,
    measure: Measure<'_>,
) {
    let (top, y) = (line.top, line.top + ROW_PT / 2.0);
    let rename = dialog
        .rename
        .as_ref()
        .filter(|rename| rename.from == line.name);
    let summary = described(book, line.name).1;
    let rule = top + ROW_PT + if line.protected { NOTE_PT } else { 0.0 } - 1.0;
    let mut marks = vec![plate(
        area(PAD, rule, WIDTH_PT - 2.0 * PAD, 1.0),
        Some(ink.block_edge),
        None,
        96,
    )];
    let row = CellRow {
        scope: SCOPE,
        summary: summary.clone(),
        on: line.name == current,
        colour: Some(line.colour.clone()),
        capability: Some(calls::WORKSPACE_LIST_CAPABILITY_ID),
        ..CellRow::new(RowKind::Workspace, line.name, line.name)
    };
    // Its own colour, as menu B tints it: stronger on the tab's own.
    let band = area(4.0, top + 1.0, WIDTH_PT - 8.0, rule - top - 1.0);
    let tint = match (row.on, line.focused) {
        (true, _) => RowTint::Current,
        (false, true) => RowTint::Hover,
        (false, false) => RowTint::Rest,
    };
    marks.insert(0, Mark::Box(tinted(band, &row, tint, line.dark)));
    let (first, legible_on) = (view.list.len(), row.clone());
    view.list.push(Piece {
        hover: (!row.on).then(|| tinted(band, &row, RowTint::Hover, line.dark)),
        row: Some(row),
        ..Piece::plain(
            area(0.0, top, WIDTH_PT, rule - top),
            std::mem::take(&mut marks),
        )
    });
    view.list.push(chip(line, ink));
    let room = count_right() - 80.0 - NAME_X;
    if let Some(rename) = rename {
        let field = area(NAME_X, top + 6.0, FIELD_W, ROW_PT - 12.0);
        marks.push(plate(field, Some(ink.inset), Some(ink.accent), u8::MAX));
        marks.push(words(
            field.x + field.w + 10.0,
            y,
            text::MANAGE_RENAME_HINT,
            SMALL_PT,
            Token::KeyHint,
            room - FIELD_W,
        ));
        view.field = Some(area(
            field.x + 6.0,
            field.y + 3.0,
            field.w - 12.0,
            field.h - 6.0,
        ));
        let name_row = scene(
            &format!("{}.name", line.name),
            &rename.name,
            Some(ManageIntent::CommitRename),
            calls::WORKSPACE_RENAME_CAPABILITY_ID,
        );
        view.list.push(Piece {
            row: Some(name_row),
            ..Piece::plain(field, std::mem::take(&mut marks))
        });
    } else {
        let width = measure(line.name, BODY_PT) + 1.0;
        let name_at = area(NAME_X, top + 6.0, width.min(room), ROW_PT - 12.0);
        marks.push(words(NAME_X, y, line.name, BODY_PT, ink.ink, room));
        let mut x = NAME_X + width + 8.0;
        if line.name == current {
            let pill_w = measure(text::MANAGE_CURRENT_PILL, 9.0) + 10.0;
            let pill = area(x, y - 7.0, pill_w, 14.0);
            marks.push(plate(pill, Some(ink.accent), None, PILL_ALPHA));
            marks.push(caption(words(
                x + 5.0,
                y,
                text::MANAGE_CURRENT_PILL,
                9.0,
                ink.accent,
                pill_w,
            )));
            x += pill_w + 8.0;
        }
        let summary_mark = words(
            x,
            y,
            &summary,
            SMALL_PT,
            ink.support,
            (count_right() - 70.0 - x).max(0.0),
        );
        let begin = Some(ManageIntent::BeginRename(line.name.to_owned()));
        let name_row = CellRow {
            unavailable: line
                .protected
                .then_some("the_default_workspace_cannot_be_renamed"),
            hover: Some(text::MANAGE_RENAME_HOVER),
            ..scene(
                &format!("{}.name", line.name),
                line.name,
                begin,
                calls::WORKSPACE_RENAME_CAPABILITY_ID,
            )
        };
        view.list.push(Piece {
            double: !line.protected,
            press: !line.protected,
            row: Some(name_row),
            ..Piece::plain(name_at, std::mem::take(&mut marks))
        });
        view.list.push(Piece::plain(name_at, vec![summary_mark]));
    }
    view.list.push(count(line, ink, measure));
    view.list.extend(icons(line, rename.is_some(), ink));
    view.list_height += ROW_PT;
    if let Some(reason) = rename.and_then(|rename| rename.refusal.as_deref()) {
        let at = area(NAME_X, view.list_height, WIDTH_PT - NAME_X - PAD, LINE_PT);
        let said = words(
            NAME_X,
            at.y + LINE_PT / 2.0,
            reason,
            SMALL_PT,
            Token::Warn,
            at.w,
        );
        view.list.push(Piece {
            row: Some(scene(
                &format!("{}.refusal", line.name),
                reason,
                None,
                calls::WORKSPACE_RENAME_CAPABILITY_ID,
            )),
            ..Piece::plain(at, vec![said])
        });
        view.list_height += LINE_PT;
    }
    if line.protected {
        let at = area(
            NAME_X,
            view.list_height - 6.0,
            WIDTH_PT - NAME_X - PAD,
            NOTE_PT,
        );
        let (first, second) = text::MANAGE_DEFAULT_NOTE
            .split_once(". ")
            .unwrap_or((text::MANAGE_DEFAULT_NOTE, ""));
        let lines = vec![
            words(
                NAME_X,
                at.y + 8.0,
                &format!("{first}."),
                SMALL_PT,
                ink.support,
                at.w,
            ),
            words(NAME_X, at.y + 22.0, second, SMALL_PT, ink.support, at.w),
        ];
        view.list.push(Piece {
            row: Some(scene(
                &format!("{}.note", line.name),
                text::MANAGE_DEFAULT_NOTE,
                None,
                calls::WORKSPACE_LIST_CAPABILITY_ID,
            )),
            ..Piece::plain(at, lines)
        });
        view.list_height += NOTE_PT;
    }
    for piece in &mut view.list[first..] {
        legible(&mut piece.marks, &legible_on, line.dark); // As menu B's.
    }
}

/// The armed row, at the same height: the question naming the tabs that
/// go back to Default, Delete (the Enter default) and Keep.
fn armed(
    view: &mut Sheet,
    line: &Line<'_>,
    (adopted, kept): (&[String], &[String]),
    (ink, measure): (&MenuChrome, Measure<'_>),
) {
    let (top, y) = (line.top, line.top + ROW_PT / 2.0);
    let band = area(0.0, top, WIDTH_PT, ROW_PT);
    let marks = vec![
        plate(band, Some(Token::ErrorText), None, ARMED_ALPHA),
        plate(
            area(0.0, top, 3.0, ROW_PT),
            Some(Token::ErrorText),
            None,
            u8::MAX,
        ),
    ];
    let asked = text::delete_armed_label(line.name, adopted, kept);
    let keep_w = measure(text::MANAGE_KEEP, BODY_PT) + 20.0;
    let delete_w = measure(text::MANAGE_DELETE, BODY_PT) + 20.0;
    let keep_x = WIDTH_PT - PAD - keep_w;
    let delete_x = keep_x - 8.0 - delete_w;
    let row = CellRow {
        scope: SCOPE,
        summary: asked.clone(),
        colour: Some(line.colour.clone()),
        capability: Some(calls::WORKSPACE_DELETE_CAPABILITY_ID),
        ..CellRow::new(RowKind::Workspace, line.name, line.name)
    };
    view.list.push(Piece {
        row: Some(row),
        ..Piece::plain(band, marks)
    });
    view.list.push(chip(line, ink));
    let question = words(NAME_X, y, &asked, BODY_PT, ink.ink, delete_x - NAME_X - 8.0);
    view.list.push(Piece::plain(
        area(NAME_X, top, delete_x - NAME_X, ROW_PT),
        vec![question],
    ));
    let at = (delete_x, top + (ROW_PT - HIT) / 2.0);
    let delete = ManageIntent::Delete {
        now_ms: line.now_ms,
    };
    let confirm = scene(
        &format!("{}.confirm", line.name),
        text::MANAGE_DELETE,
        Some(delete),
        calls::WORKSPACE_DELETE_CAPABILITY_ID,
    );
    view.list.push(button(
        at,
        confirm,
        Some(FilledButton::danger()),
        ink,
        measure,
    ));
    let keep = scene(
        &format!("{}.keep", line.name),
        text::MANAGE_KEEP,
        Some(ManageIntent::Keep),
        calls::WORKSPACE_DELETE_CAPABILITY_ID,
    );
    view.list
        .push(button((keep_x, at.1), keep, None, ink, measure));
    view.list_height += ROW_PT;
}

/// The row a delete left: what it did, Ctrl+Z and Undo, and the thin line
/// emptying over the ten seconds.
fn undo_row(
    view: &mut Sheet,
    undo: Option<&quantick_session::manage_workspaces::UndoRow>,
    now_ms: i64,
    ink: &MenuChrome,
    measure: Measure<'_>,
) {
    let Some(undo) = undo else { return };
    let (top, y) = (view.list_height, view.list_height + ROW_PT / 2.0);
    let undo_w = measure(text::MANAGE_UNDO, BODY_PT) + 20.0;
    let undo_x = WIDTH_PT - PAD - undo_w;
    let keys_w = measure(text::MANAGE_UNDO_KEYS, SMALL_PT);
    let left = (undo.until_ms - now_ms).clamp(0, quantick_stores::workspaces::UNDO_WINDOW_MS);
    let share = left as f32 / quantick_stores::workspaces::UNDO_WINDOW_MS as f32;
    let marks = vec![
        plate(
            area(PAD + 4.0, y - CHIP / 2.0, CHIP, CHIP),
            Some(ink.faint),
            None,
            96,
        ),
        words(
            NAME_X,
            y,
            &undo.words,
            BODY_PT,
            ink.support,
            undo_x - keys_w - 16.0 - NAME_X,
        ),
        words(
            undo_x - keys_w - 8.0,
            y,
            text::MANAGE_UNDO_KEYS,
            SMALL_PT,
            Token::KeyHint,
            keys_w + 1.0,
        ),
        plate(
            area(0.0, top + ROW_PT - 2.0, (WIDTH_PT * share).round(), 2.0),
            Some(ink.accent),
            None,
            u8::MAX,
        ),
    ];
    view.list.push(Piece {
        row: Some(scene(&undo.workspace, &undo.words, None, UNDO_CALL)),
        ..Piece::plain(area(0.0, top, WIDTH_PT, ROW_PT), marks)
    });
    let row = scene(
        &format!("{}.undo", undo.workspace),
        text::MANAGE_UNDO,
        Some(ManageIntent::Undo { now_ms }),
        UNDO_CALL,
    );
    view.list.push(button(
        (undo_x, top + (ROW_PT - HIT) / 2.0),
        row,
        None,
        ink,
        measure,
    ));
    view.list_height += ROW_PT;
}

/// New workspace… at the left; the keys, or the last swap's or Undo's
/// words, and Close at the right.
fn foot(dialog: &ManageDialog, ink: &MenuChrome, top: f32, measure: Measure<'_>) -> Vec<Piece> {
    let block = area(0.0, top, WIDTH_PT, FOOT_PT);
    let rule = plate(
        area(0.0, top, WIDTH_PT, 1.0),
        Some(ink.block_edge),
        None,
        u8::MAX,
    );
    let mut pieces = vec![Piece::plain(
        block,
        vec![plate(block, Some(ink.block), None, u8::MAX), rule],
    )];
    let y = top + (FOOT_PT - HIT) / 2.0;
    let label = format!("{PLUS}  {}", text::MANAGE_NEW);
    let new = CellRow {
        command: Some(WorkspaceCommand::BeginNew),
        ..scene("new", &label, None, OPEN)
    };
    let new_at = area(PAD, y, measure(&label, BODY_PT) + 16.0, HIT);
    let new_mark = words(
        new_at.x + 8.0,
        new_at.y + HIT / 2.0,
        &label,
        BODY_PT,
        ink.ink,
        new_at.w,
    );
    pieces.push(press(new_at, new, true, vec![new_mark], ink));
    let close_w = measure(text::MANAGE_CLOSE, BODY_PT) + 20.0;
    let close_x = WIDTH_PT - PAD - close_w;
    let close = scene("close", text::MANAGE_CLOSE, Some(ManageIntent::Close), OPEN);
    pieces.push(button((close_x, y), close, None, ink, measure));
    let (said, said_ink) = match &dialog.note {
        Some(note) => (note.as_str(), ink.ink),
        None => (text::MANAGE_KEYS_HINT, Token::KeyHint),
    };
    let hint = ending(
        measure,
        close_x - 10.0,
        y + HIT / 2.0,
        said,
        SMALL_PT,
        said_ink,
    );
    let row = scene("hint", said, None, OPEN);
    pieces.push(Piece {
        row: Some(row),
        ..Piece::plain(
            area(
                close_x - 10.0 - measure(said, SMALL_PT),
                y,
                measure(said, SMALL_PT),
                HIT,
            ),
            vec![hint],
        )
    });
    pieces
}

/// The dialog asking a name for `dialog`'s mode over `book`: the field with
/// its refusal, then for Save as and New the "Colour · Petrol" caption and
/// the palette on the chosen colour, every held one locked; Save (the Enter
/// default), Cancel and "Enter saves".
#[must_use]
pub fn name_dialog(
    book: &WorkspaceBook,
    dialog: &NameDialog,
    dark: bool,
    measure: Measure<'_>,
) -> Sheet {
    let ink = chrome(dark);
    let width = NAME_WIDTH_PT;
    let title = match dialog.mode {
        NameDialogMode::SaveAsNew => text::SAVE_AS_DIALOG_TITLE,
        NameDialogMode::New => text::NEW_DIALOG_TITLE,
        NameDialogMode::Rename { .. } => text::RENAME_DIALOG_TITLE,
    };
    let bar = area(0.0, 0.0, width, TITLE_PT);
    let rule = plate(
        area(0.0, TITLE_PT - 1.0, width, 1.0),
        Some(ink.block_edge),
        None,
        u8::MAX,
    );
    let named =
        |key: &str, label: &str, command: Option<WorkspaceCommand>, call: &'static str| CellRow {
            scope: NAME_SCOPE,
            command,
            capability: Some(call),
            ..CellRow::new(RowKind::Press, key, label)
        };
    let call = match dialog.mode {
        NameDialogMode::New => calls::WORKSPACE_NEW_CAPABILITY_ID,
        NameDialogMode::SaveAsNew => calls::WORKSPACE_SAVE_AS_CAPABILITY_ID,
        NameDialogMode::Rename { .. } => calls::WORKSPACE_RENAME_CAPABILITY_ID,
    };
    let mut pieces = vec![Piece {
        row: Some(CellRow {
            on: true,
            ..named("", title, None, OPEN)
        }),
        ..Piece::plain(
            bar,
            vec![
                plate(bar, Some(ink.block), None, u8::MAX),
                rule,
                words(
                    PAD,
                    TITLE_PT / 2.0,
                    title,
                    TITLE_TEXT_PT,
                    ink.ink,
                    width - 60.0,
                ),
            ],
        )
    }];
    let x = area(width - PAD - HIT, (TITLE_PT - HIT) / 2.0, HIT, HIT);
    let glyph = words(x.x + 6.0, x.y + HIT / 2.0, CLOSE, 12.0, ink.support, HIT);
    pieces.push(press(
        x,
        named(
            "dismiss",
            text::DIALOG_CANCEL,
            Some(WorkspaceCommand::CloseDialog),
            OPEN,
        ),
        true,
        vec![glyph],
        &ink,
    ));
    let field = area(PAD, TITLE_PT + 12.0, width - 2.0 * PAD, 26.0);
    pieces.push(Piece {
        row: Some(named(
            "name",
            &dialog.name,
            Some(WorkspaceCommand::SubmitDialog),
            call,
        )),
        ..Piece::plain(
            field,
            vec![plate(field, Some(ink.inset), Some(ink.accent), u8::MAX)],
        )
    });
    let mut y = field.y + field.h + 8.0;
    if let Some(reason) = &dialog.refusal {
        let at = area(PAD, y, width - 2.0 * PAD, LINE_PT);
        let said = words(PAD, y + LINE_PT / 2.0, reason, SMALL_PT, Token::Warn, at.w);
        pieces.push(Piece {
            row: Some(named("refusal", reason, None, call)),
            ..Piece::plain(at, vec![said])
        });
        y += LINE_PT;
    }
    creation::controls(&mut pieces, &mut y, dialog, dark, measure);
    let colour = chosen(dialog);
    let mut saves = dialog.mode == NameDialogMode::New || !dialog.name.trim().is_empty();
    let mut palette = None;
    if dialog.colour.is_some() {
        let model = choosing(book, colour);
        let free = |colour| matches!(model.pick(colour, true), Pick::Apply(_));
        let typed = (dialog.palette.custom).then(|| parse_typed_hex(&dialog.palette.hex));
        let typed = typed.flatten().map(WorkspaceColour::Custom);
        saves &= colour.is_some_and(free) && typed.is_none_or(free);
        let caption_said = text::colour_caption(&colour.map(said).unwrap_or_default());
        let at = area(PAD, y, width - 2.0 * PAD, LINE_PT);
        pieces.push(Piece {
            row: Some(named("colour", &caption_said, None, call)),
            ..Piece::plain(
                at,
                vec![words(
                    PAD,
                    y + LINE_PT / 2.0,
                    &caption_said,
                    SMALL_PT,
                    ink.support,
                    at.w,
                )],
            )
        });
        y += LINE_PT - 4.0;
        let state = &mut dialog.palette.clone(); // Settled for this frame only.
        let mut pop = colour_palette::choosing(book, colour, state, dark, measure);
        if dialog.mode == NameDialogMode::New {
            pop.pieces.retain(|piece| {
                !piece
                    .row
                    .as_ref()
                    .is_some_and(|row| row.key.ends_with("apply") || row.key.ends_with("cancel"))
            });
            creation::colour(&mut pieces, &mut y, colour, dark, measure);
        }
        pop.scoped(NAME_SCOPE, "");
        let height = pop.size[1];
        palette = Some(([0.0, y], pop));
        y += height;
    }
    y += 4.0;
    let foot = area(0.0, y, width, FOOT_PT);
    pieces.push(Piece::plain(
        foot,
        vec![
            plate(foot, Some(ink.block), None, u8::MAX),
            plate(
                area(0.0, y, width, 1.0),
                Some(ink.block_edge),
                None,
                u8::MAX,
            ),
        ],
    ));
    let top = y + (FOOT_PT - HIT) / 2.0;
    let save = CellRow {
        unavailable: (!saves).then_some("the_name_or_the_colour_is_refused"),
        ..named(
            "save",
            if dialog.mode == NameDialogMode::New {
                "Create workspace"
            } else {
                text::DIALOG_SAVE
            },
            Some(WorkspaceCommand::SubmitDialog),
            call,
        )
    };
    let save = button(
        (PAD, top),
        save,
        Some(FilledButton::primary()),
        &ink,
        measure,
    );
    let cancel_x = save.at.x + save.at.w + 8.0;
    pieces.push(save);
    let cancel = named(
        "cancel",
        text::DIALOG_CANCEL,
        Some(WorkspaceCommand::CloseDialog),
        OPEN,
    );
    pieces.push(button((cancel_x, top), cancel, None, &ink, measure));
    pieces.push(Piece::plain(
        foot,
        vec![ending(
            measure,
            width - PAD,
            top + HIT / 2.0,
            if dialog.mode == NameDialogMode::New {
                "Enter creates"
            } else {
                text::DIALOG_ENTER_SAVES
            },
            SMALL_PT,
            Token::KeyHint,
        )],
    ));
    let mut view = Sheet::of([width, y + FOOT_PT], pieces);
    view.field = Some(area(
        field.x + 6.0,
        field.y + 4.0,
        field.w - 12.0,
        field.h - 8.0,
    ));
    (view.palette, view.saves) = (palette, saves);
    view
}

/// The name dialog's height at rest, Custom folded: where its top stays
/// while Custom grows it downward.
#[must_use]
pub fn name_rest_height(book: &WorkspaceBook, dialog: &NameDialog) -> f32 {
    let mut rest = dialog.clone();
    (rest.palette.custom, rest.refusal) = (false, None);
    name_dialog(book, &rest, false, &|_, _| 0.0).size[1]
}

/// The name dialog's distance from the window's edges.
pub const DIALOG_MARGIN_PT: f32 = 8.0;

/// Where the name dialog stands in a window from `top` to `bottom`: its
/// top, centred at rest and kept while Custom grows it downward, and the
/// room below it, which [`capped`] holds it to.
#[must_use]
pub fn name_place(
    book: &WorkspaceBook,
    dialog: &NameDialog,
    [top, bottom]: [f32; 2],
) -> (f32, f32) {
    let free = bottom - top - name_rest_height(book, dialog);
    let top = top + (free / 2.0).max(DIALOG_MARGIN_PT);
    (top, bottom - DIALOG_MARGIN_PT - top)
}

/// The dialog `view` capped at `room` points: what does not fit scrolls.
#[must_use]
pub fn capped(mut view: Sheet, room: f32) -> Sheet {
    let height = view.size[1].min(room.max(TITLE_PT + FOOT_PT));
    (view.size[1], view.list_view) = (height, height);
    view
}

/// What a name dialog's frame asks: a press (its palette's as the
/// dialog's), then Enter on a valid form. Save as may apply its custom
/// swatch first. A typed change clears the refusal.
pub fn name_answer(
    dialog: &mut NameDialog,
    typed: &str,
    asked: Option<WorkspaceCommand>,
    ([field, anywhere], saves, escape): ([bool; 2], bool, bool),
) -> Option<WorkspaceCommand> {
    if dialog.name != typed {
        dialog.refusal = None;
    }
    let submit = match (
        field || anywhere,
        dialog.palette.custom && dialog.mode != NameDialogMode::New,
    ) {
        (true, true) => Some(WorkspaceCommand::NameColour(ColourIntent::Apply)),
        (true, false) => saves.then_some(WorkspaceCommand::SubmitDialog),
        (false, _) => None,
    };
    let escape = escape.then(|| name_escape(dialog));
    asked.map(name_colour).or(submit).or(escape)
}

/// What Manage's frame asks: a key's, else a press (a row palette's as
/// Manage's), else Enter in the rename field.
#[must_use]
pub fn manage_answer(
    keyed: Option<ManageIntent>,
    asked: Option<WorkspaceCommand>,
    enter: bool,
) -> Option<WorkspaceCommand> {
    let commit = enter.then_some(ManageIntent::CommitRename);
    let keyed = keyed.or_else(|| asked.is_none().then_some(commit).flatten());
    keyed
        .map(WorkspaceCommand::Manage)
        .or(asked.map(row_colour))
}

/// What the create dialog's palette asks as a name dialog's command.
#[must_use]
pub fn name_colour(command: WorkspaceCommand) -> WorkspaceCommand {
    match command {
        WorkspaceCommand::Colour(intent) => WorkspaceCommand::NameColour(intent),
        other => other,
    }
}

/// What a row palette asks as Manage's command.
#[must_use]
pub fn row_colour(command: WorkspaceCommand) -> WorkspaceCommand {
    match command {
        WorkspaceCommand::Colour(intent) => WorkspaceCommand::Manage(ManageIntent::Colour(intent)),
        other => other,
    }
}

/// The intent Esc in a create dialog asks: Custom folds first.
#[must_use]
pub fn name_escape(dialog: &NameDialog) -> WorkspaceCommand {
    if dialog.palette.custom {
        WorkspaceCommand::NameColour(ColourIntent::Close)
    } else {
        WorkspaceCommand::CloseDialog
    }
}

#[cfg(test)]
#[path = "manage_workspaces_tests.rs"]
mod tests;
