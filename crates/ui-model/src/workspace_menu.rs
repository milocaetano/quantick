//! The capsule's workspace menu (designer B, "workspace first") as marks to
//! paint: where each part of the header, a list row, the filter field and
//! the footer stands, its words, sizes and tokens. The rows are
//! `cell_menu`'s, the menu's size `workspace_menu`'s; the window only turns
//! these marks into pixels and presses.

use quantick_appearance::Token;
use quantick_appearance::workspace_menu::{MenuChrome, chrome};
use quantick_control_handlers::workspace_book::cell_menu::{
    CAPTION, CellMenu, CellRow, RowKind, UNSAVED,
};
use quantick_session::workspace_menu::{
    CHIP_MENU_CAPTION_PT, CHIP_MENU_FOOTER_PT, CHIP_MENU_HEADER_PT, CHIP_MENU_LIST_PAD_PT as PAD,
    CHIP_MENU_ROW_PT,
};
use quantick_workspace::canvas::PaneTree;

/// A button's and a row's corner radius.
const RADIUS: f32 = 4.0;
/// The blocks' outer corners, inside the menu's radius-6 border.
const BLOCK_RADIUS: f32 = 5.0;
/// Words: a name and a button, a header name, a summary or hint, the caption.
const BODY_PT: f32 = 12.5;
const HEADER_NAME_PT: f32 = 13.0;
const SMALL_PT: f32 = 11.0;
const CAPTION_PT: f32 = 10.0;
/// The empty colour slot before a miniature, and the gap after it.
const SLOT_PT: f32 = 3.0 + 8.0;
/// The miniatures: the header's as large as the capsule's, a row's smaller.
const MINI_HEADER: [f32; 2] = [40.0, 14.0];
const MINI_ROW: [f32; 2] = [26.0, 14.0];
/// A row's status column: the check or the unsaved dot.
const STATUS_PT: f32 = 14.0;
/// The unsaved dot's radius.
const DOT_PT: f32 = 2.5;
/// Phosphor's floppy disk, plus and check, in the bundled icon font.
const FLOPPY: &str = "\u{e248}";
const PLUS: &str = "\u{e3d4}";
const CHECK: &str = "\u{e182}";

/// A box, in points from its piece's origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const fn area(x: f32, y: f32, w: f32, h: f32) -> Area {
    Area { x, y, w, h }
}

/// A rounded box, filled and edged; `None` leaves either clear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    pub at: Area,
    pub radius: f32,
    pub fill: Option<Token>,
    pub edge: Option<Token>,
}

/// Words starting at `x`, centred on `y`, cut short to `width`.
#[derive(Debug, Clone, PartialEq)]
pub struct Words {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub size: f32,
    pub ink: Token,
    pub width: f32,
}

/// One thing to paint.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Box(Plate),
    Text(Words),
    /// A layout miniature, as the capsule paints it.
    Tree(Area, PaneTree),
}

/// A part of the menu: its box, the row the scene lists it as, whether a
/// press reaches it, the fill a hover lays over its first mark, its marks.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub at: Area,
    pub row: Option<CellRow>,
    pub press: bool,
    pub hover: Option<Token>,
    pub marks: Vec<Mark>,
}

impl Piece {
    fn plain(at: Area, marks: Vec<Mark>) -> Self {
        Self {
            at,
            row: None,
            press: false,
            hover: None,
            marks,
        }
    }
}

fn plate(at: Area, radius: f32, fill: Option<Token>, edge: Option<Token>) -> Mark {
    Mark::Box(Plate {
        at,
        radius,
        fill,
        edge,
    })
}

fn dot(x: f32, y: f32, radius: f32, ink: Token) -> Mark {
    let at = area(x - radius, y - radius, 2.0 * radius, 2.0 * radius);
    plate(at, radius, Some(ink), None)
}

fn text(x: f32, y: f32, words: &str, size: f32, ink: Token, width: f32) -> Mark {
    let text = words.to_owned();
    Mark::Text(Words {
        x,
        y,
        text,
        size,
        ink,
        width,
    })
}

/// What measures words: their width at a size, in points.
pub type Measure<'a> = &'a dyn Fn(&str, f32) -> f32;

/// `words` ending at `right`, centred on `y`.
fn ending(measure: Measure<'_>, right: f32, y: f32, words: &str, size: f32, ink: Token) -> Mark {
    let width = measure(words, size);
    text(right - width, y, words, size, ink, width + 1.0)
}

/// A block `height` tall in `ink`'s block colour, rounded at its `top` or
/// bottom only, its rule on the side that meets the list.
fn block(width: f32, height: f32, top: bool, ink: &MenuChrome) -> Piece {
    let (fill, rule) = (Some(ink.block), Some(ink.block_edge));
    let square = if top { BLOCK_RADIUS } else { 0.0 };
    let rule_y = if top { height - 1.0 } else { 0.0 };
    let marks = vec![
        plate(area(0.0, 0.0, width, height), BLOCK_RADIUS, fill, None),
        plate(
            area(0.0, square, width, height - BLOCK_RADIUS),
            0.0,
            fill,
            None,
        ),
        plate(area(0.0, rule_y, width, 1.0), 0.0, rule, None),
    ];
    Piece::plain(area(0.0, 0.0, width, height), marks)
}

/// A workspace's line in `at`: the empty colour slot, its `mini` miniature,
/// its `name_pt` name over its summary, `status` points left at the right.
fn entry(
    at: Area,
    row: &CellRow,
    mini: [f32; 2],
    name_pt: f32,
    status: f32,
    ink: &MenuChrome,
) -> Vec<Mark> {
    let left = at.x + SLOT_PT + mini[0] + 8.0;
    let width = at.x + at.w - status - left;
    let top = at.y + (at.h - 30.0) / 2.0;
    let mut marks = vec![
        text(left, top + 7.5, &row.label, name_pt, ink.ink, width),
        text(left, top + 23.5, &row.summary, SMALL_PT, ink.support, width),
    ];
    let face = area(
        at.x + SLOT_PT,
        at.y + (at.h - mini[1]) / 2.0,
        mini[0],
        mini[1],
    );
    marks.extend(row.tree.clone().map(|tree| Mark::Tree(face, tree)));
    marks
}

/// A header or footer press of `row` at `at`, `plain` without a resting
/// fill: Save stands out while it holds edits and turns quiet while
/// autosave saves; the switch shows its state words.
fn press(at: Area, row: &CellRow, plain: bool, ink: &MenuChrome, measure: Measure<'_>) -> Piece {
    let quiet = row.kind == RowKind::Quiet;
    let enabled = row.unavailable.is_none() && !quiet;
    let (back, edge) = match (row.dot, plain || quiet) {
        (true, _) => (Some(ink.current), Some(ink.accent)),
        (false, true) => (None, None),
        (false, false) => (Some(ink.control), Some(ink.control_edge)),
    };
    let (y, mut x) = (at.y + at.h / 2.0, at.x + 8.0);
    let mut marks = vec![plate(at, RADIUS, back, edge)];
    if row.kind == RowKind::Switch {
        let track = area(x - 2.0, y - 7.0, 26.0, 14.0);
        let (knob, knob_ink) = match row.on {
            true => (track.x + 19.0, ink.accent),
            false => (track.x + 7.0, ink.faint),
        };
        marks.push(plate(track, 7.0, Some(ink.control), Some(ink.edge)));
        marks.push(dot(knob, y, 5.0, knob_ink));
        x = track.x + track.w + 8.0;
    }
    let icon = match (row.key.as_str(), row.kind) {
        ("save", RowKind::Press) => FLOPPY,
        ("new", _) => PLUS,
        _ => "",
    };
    let words = [ink.faint, ink.ink, ink.support][usize::from(enabled) + 2 * usize::from(quiet)];
    let gap = if icon.is_empty() { "" } else { "  " };
    let label = format!("{icon}{gap}{}", row.label);
    marks.push(text(x, y, &label, BODY_PT, words, at.x + at.w - x - 8.0));
    let (end, end_ink) = match row.kind {
        RowKind::Switch => (&row.summary, ink.support),
        _ => (&row.beside, ink.faint),
    };
    marks.push(ending(
        measure,
        at.x + at.w - 6.0,
        y,
        end,
        SMALL_PT,
        end_ink,
    ));
    Piece {
        at,
        row: Some(row.clone()),
        press: enabled,
        hover: (enabled && !row.dot).then_some(ink.hover),
        marks,
    }
}

/// The header block `width` wide with the caption under it: the tab's
/// workspace, its Unsaved mark, Save, Save as and Rename, then WORKSPACES and
/// the count. `measure` sizes the buttons that fit their words.
#[must_use]
pub fn head(menu: &CellMenu, width: f32, dark: bool, measure: Measure<'_>) -> Vec<Piece> {
    let ink = chrome(dark);
    let mut pieces = vec![block(width, CHIP_MENU_HEADER_PT, true, &ink)];
    let line = area(8.0, 8.0, width - 18.0, 34.0);
    let (right, y) = (line.x + line.w, line.y + line.h / 2.0);
    let mut state = 0.0;
    let mut marks = Vec::new();
    if menu.current.dot {
        state = measure(UNSAVED, SMALL_PT) + 12.0;
        marks.push(dot(right - state + 2.5, y, DOT_PT, ink.accent));
        marks.push(ending(measure, right, y, UNSAVED, SMALL_PT, ink.support));
    }
    marks.extend(entry(
        line,
        &menu.current,
        MINI_HEADER,
        HEADER_NAME_PT,
        state,
        &ink,
    ));
    pieces.push(Piece {
        row: Some(menu.current.clone()),
        ..Piece::plain(line, marks)
    });
    let (left, right) = (19.0, width - 10.0);
    let wide = |row: &CellRow| measure(&row.label, BODY_PT) + 16.0;
    let fixed: f32 = menu.head.iter().skip(1).map(|row| wide(row) + 4.0).sum();
    let mut x = left;
    for (index, row) in menu.head.iter().enumerate() {
        let w = if index == 0 {
            right - left - fixed
        } else {
            wide(row)
        };
        pieces.push(press(area(x, 48.0, w, 24.0), row, false, &ink, measure));
        x += w + 4.0;
    }
    let y = CHIP_MENU_HEADER_PT + CHIP_MENU_CAPTION_PT / 2.0;
    let count = menu.list.len().to_string();
    let caption = vec![
        text(11.0, y, CAPTION, CAPTION_PT, ink.faint, width),
        ending(measure, width - 10.0, y, &count, SMALL_PT, ink.faint),
    ];
    let at = area(0.0, CHIP_MENU_HEADER_PT, width, CHIP_MENU_CAPTION_PT);
    pieces.push(Piece::plain(at, caption));
    pieces
}

/// The filter field `width` wide holding `row`'s words: the box the window's
/// text field sits in.
#[must_use]
pub fn filter(row: CellRow, width: f32, dark: bool) -> Piece {
    let ink = chrome(dark);
    let at = area(8.0, 0.0, width - 16.0, 24.0);
    let field = plate(at, RADIUS, Some(ink.inset), Some(ink.edge));
    Piece {
        row: Some(row),
        ..Piece::plain(at, vec![field])
    }
}

/// A list row in a list `width` wide, inside its padding: the tab's own on the selection fill with its
/// check, a row whose edit a tab holds with its dot, the focused one ringed.
#[must_use]
pub fn row(row: &CellRow, width: f32, focused: bool, dark: bool) -> Piece {
    let ink = chrome(dark);
    let at = area(PAD, 0.0, width - 2.0 * PAD, CHIP_MENU_ROW_PT);
    let back = match (row.on, focused) {
        (true, _) => Some(ink.current),
        (false, true) => Some(ink.hover),
        (false, false) => None,
    };
    let ring = if focused { Some(ink.accent) } else { None };
    let mut marks = vec![plate(at, RADIUS, back, ring)];
    let (status, y) = (width - PAD - 8.0 - STATUS_PT, CHIP_MENU_ROW_PT / 2.0);
    if row.on {
        marks.push(text(status + 1.0, y, CHECK, 12.0, ink.accent, STATUS_PT));
    } else if row.dot {
        marks.push(dot(status + 7.0, y, DOT_PT, ink.accent));
    }
    let line = area(2.0 * PAD, 0.0, width - 3.0 * PAD - 8.0, CHIP_MENU_ROW_PT);
    marks.extend(entry(line, row, MINI_ROW, BODY_PT, STATUS_PT + 8.0, &ink));
    Piece {
        at,
        row: Some(row.clone()),
        press: true,
        hover: (!row.on).then_some(ink.hover),
        marks,
    }
}

/// The footer block `width` wide: Autosave with its state words, then New
/// workspace and Manage.
#[must_use]
pub fn foot(menu: &CellMenu, width: f32, dark: bool, measure: Measure<'_>) -> Vec<Piece> {
    let ink = chrome(dark);
    let mut pieces = vec![block(width, CHIP_MENU_FOOTER_PT, false, &ink)];
    let manage = menu
        .foot
        .get(2)
        .map_or(0.0, |row| measure(&row.label, BODY_PT) + 16.0);
    let places = [
        area(4.0, 5.0, width - 8.0, 28.0),
        area(6.0, 33.0, width - 16.0 - manage, 24.0),
        area(width - 6.0 - manage, 33.0, manage, 24.0),
    ];
    let rows = menu.foot.iter().zip(places);
    pieces.extend(rows.map(|(row, at)| press(at, row, true, &ink, measure)));
    pieces
}

#[cfg(test)]
#[path = "workspace_menu_tests.rs"]
mod tests;
