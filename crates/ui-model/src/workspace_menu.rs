//! The capsule's workspace menu (designer B, "workspace first") as marks to
//! paint: where each part of the header, a list row, the filter field and
//! the footer stands, its words, sizes and tokens. The rows are
//! `cell_menu`'s, the menu's size `workspace_menu`'s; the window only turns
//! these marks into pixels and presses.

use quantick_appearance::Token;
use quantick_appearance::workspace_menu::{MenuChrome, RowTint, RowWords, chrome};
use quantick_control_handlers::workspace_book::cell_menu::{
    CAPTION, CellMenu, CellRow, RowKind, UNSAVED,
};
use quantick_session::workspace_menu::{
    CHIP_MENU_BORDER_PT, CHIP_MENU_CAPTION_PT, CHIP_MENU_FILTER_PT, CHIP_MENU_FOOTER_PT,
    CHIP_MENU_HEADER_PT, CHIP_MENU_LIST_PAD_PT as PAD, CHIP_MENU_ROW_PT, ChipMenuLayout,
};
use quantick_workspace::canvas::PaneTree;

/// The header block with the caption under it, in points.
pub const HEAD_PT: f32 = CHIP_MENU_HEADER_PT + CHIP_MENU_CAPTION_PT;
/// A button's and a row's corner radius.
const RADIUS: f32 = 4.0;
/// The blocks' outer corners, inside the menu's radius-6 border.
const BLOCK_RADIUS: f32 = 5.0;
/// Words: a name and a button, a header name, a summary or hint, the caption.
const BODY_PT: f32 = 12.5;
const HEADER_NAME_PT: f32 = 13.0;
const SMALL_PT: f32 = 11.0;
const CAPTION_PT: f32 = 10.0;
/// The colour slot before a row's miniature, its height and corner radius;
/// the gap after it, and after the header's swatch.
const SLOT_HEIGHT_PT: f32 = 22.0;
const SLOT_GAP_PT: f32 = 8.0;
const SLOT_WIDTH_PT: f32 = 3.0;
/// The panel showing between a row's colour slot and its highlight.
const HIGHLIGHT_GAP_PT: f32 = 1.0;
const SLOT_RADIUS_PT: f32 = 1.5;
/// The header's colour swatch, its press and its corner radius.
pub const SWATCH_PT: f32 = 16.0;
/// Where the header's swatch ends, from the header block's top: centred on
/// its 34 pt line 8 pt down.
pub const SWATCH_BOTTOM_PT: f32 = 8.0 + (34.0 + SWATCH_PT) / 2.0;
const SWATCH_HIT_PT: f32 = 24.0;
const SWATCH_RADIUS_PT: f32 = 4.0;
/// The miniatures: the header's as large as the capsule's, a row's smaller.
const MINI_HEADER: [f32; 2] = [40.0, 14.0];
const MINI_ROW: [f32; 2] = [26.0, 14.0];
/// The switch's track and the gaps either side of it.
const SWITCH_PT: f32 = 6.0 + 26.0 + 8.0;
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

pub(crate) const fn area(x: f32, y: f32, w: f32, h: f32) -> Area {
    Area { x, y, w, h }
}

/// A rounded box, filled and edged; `None` leaves either clear.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    pub at: Area,
    pub radius: f32,
    pub fill: Option<Token>,
    /// The fill's opacity, out of 255.
    pub alpha: u8,
    pub edge: Option<Token>,
    /// A colour and the share of the way the fill moves toward it: a
    /// workspace row's tint over the ground the theme paints.
    pub toward: Option<([u8; 3], f32)>,
}

/// Words starting at `x`, centred on `y`, cut short to `width`, `tracking`
/// points between letters; `names` marks a row's name, whose cut the hover
/// tells whole.
#[derive(Debug, Clone, PartialEq)]
pub struct Words {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub size: f32,
    pub ink: Token,
    pub width: f32,
    pub tracking: f32,
    pub names: bool,
    /// A colour of its own instead of `ink`'s: black or white on a swatch.
    pub rgb: Option<[u8; 3]>,
    /// On a workspace row's tint, where the painter chooses the ink.
    pub on: Option<RowWords>,
}

/// One thing to paint.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Box(Plate),
    Text(Words),
    /// A layout miniature, as the capsule paints it.
    Tree(Area, PaneTree),
    /// A workspace's colour in its slot, as the theme resolves it: a custom
    /// colour is no theme token.
    Colour {
        at: Area,
        radius: f32,
        rgb: [u8; 3],
    },
    /// A box shaded between its corners, each a point and its colour: top
    /// left, top right, bottom right, bottom left (Custom's square and strip).
    Shade([([f32; 2], [u8; 3]); 4]),
    /// Diagonal stripes in `ink` at `alpha` across a box: a colour another
    /// workspace holds, locked in the create dialogs.
    Hatch {
        at: Area,
        ink: Token,
        alpha: u8,
    },
}

/// A part of the menu: its box, the row the scene lists it as, whether a
/// press reaches it, the fill a hover lays over its first mark, its marks.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub at: Area,
    pub row: Option<CellRow>,
    pub press: bool,
    /// A press or drag reports where it is ([`crate::colour_palette::dragged`]).
    pub drag: bool,
    /// Only a double click presses it (a Manage row's name).
    pub double: bool,
    /// What a hover lays over its first mark.
    pub hover: Option<Plate>,
    pub marks: Vec<Mark>,
}

/// A surface to paint with one painter (the menu, Manage, the name
/// dialogs, the palette popover): the fixed head and foot, the list in its
/// own coordinates scrolled in `list_view`, and what the window lays over it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub size: [f32; 2],
    pub head: Vec<Piece>,
    /// Where the list starts, and the height it shows at once.
    pub list_top: f32,
    pub list_view: f32,
    /// Every row's pieces, from the list's top.
    pub list: Vec<Piece>,
    /// The list's whole height.
    pub list_height: f32,
    /// The chosen row, which the list scrolls into view.
    pub focus: Option<Area>,
    /// The text field's box, in the list's coordinates.
    pub field: Option<Area>,
    /// The open palette and where it stands in the list.
    pub palette: Option<([f32; 2], crate::colour_palette::Popover)>,
    pub foot: Vec<Piece>,
    /// Manage's rows: each name and whether rename and delete refuse it.
    pub names: Vec<(String, bool)>,
    /// A name dialog's Save would save: a name, and a free colour.
    pub saves: bool,
    /// The open palette hangs over the list and a click outside it closes
    /// it (Manage's rows); else it stands in the list.
    pub closes_out: bool,
}

impl Sheet {
    /// The sheet, its list brought to its focus only when that `moved`.
    #[must_use]
    pub fn revealed(mut self, moved: bool) -> Self {
        self.focus = self.focus.filter(|_| moved);
        self
    }

    /// A sheet `size` large of `list` alone, which never scrolls.
    #[must_use]
    pub fn of(size: [f32; 2], list: Vec<Piece>) -> Self {
        Self {
            size,
            head: Vec::new(),
            list_top: 0.0,
            list_view: size[1],
            list,
            list_height: size[1],
            focus: None,
            field: None,
            palette: None,
            foot: Vec::new(),
            names: Vec::new(),
            saves: false,
            closes_out: false,
        }
    }
}

impl Piece {
    /// The piece `dy` points lower, its marks and hover with it.
    #[must_use]
    pub fn shifted(mut self, dy: f32) -> Self {
        self.at.y += dy;
        if let Some(hover) = &mut self.hover {
            hover.at.y += dy;
        }
        for mark in &mut self.marks {
            match mark {
                Mark::Box(Plate { at, .. })
                | Mark::Tree(at, _)
                | Mark::Colour { at, .. }
                | Mark::Hatch { at, .. } => at.y += dy,
                Mark::Text(words) => words.y += dy,
                Mark::Shade(corners) => corners.iter_mut().for_each(|(at, _)| at[1] += dy),
            }
        }
        self
    }

    pub(crate) fn plain(at: Area, marks: Vec<Mark>) -> Self {
        Self {
            at,
            row: None,
            press: false,
            drag: false,
            double: false,
            hover: None,
            marks,
        }
    }
}

fn plate(at: Area, radius: f32, fill: Option<Token>, edge: Option<Token>) -> Mark {
    Mark::Box(Plate {
        toward: None,
        at,
        radius,
        fill,
        alpha: u8::MAX,
        edge,
    })
}

/// A row's fill over `at`: a workspace row's own colour at `tint`'s share
/// over the ground, mixed as the theme paints it, the one fill menu B and
/// Manage both paint; a row without a colour keeps the menu's hover and
/// current tokens.
pub(crate) fn tinted(at: Area, row: &CellRow, tint: RowTint, dark: bool) -> Plate {
    let ink = chrome(dark);
    let toward = (row.colour.as_ref()).map(|colour| (colour.rgb, tint.share()));
    let fill = match tint {
        _ if toward.is_some() => Some(ink.ground),
        RowTint::Rest => None,
        RowTint::Hover => Some(ink.hover),
        RowTint::Current => Some(ink.current),
    };
    Plate {
        toward,
        at,
        radius: RADIUS,
        fill,
        alpha: u8::MAX,
        edge: None,
    }
}

/// `marks`' words in the menu's text and support inks on `row`'s tint: the
/// painter chooses their ink ([`RowWords::ink`]) for every fill the row
/// wears. One rule for menu B and Manage.
pub(crate) fn legible(marks: &mut [Mark], row: &CellRow, dark: bool) {
    let (Some(colour), ink) = (row.colour.as_ref(), chrome(dark)) else {
        return;
    };
    let on = RowWords {
        ground: ink.ground,
        colour: colour.rgb,
        current: row.on,
    };
    for mark in marks {
        if let Mark::Text(words) = mark
            && words.rgb.is_none()
            && [ink.ink, ink.support].contains(&words.ink)
        {
            words.on = Some(on);
        }
    }
}

/// The hover fill over `at`, as a button's corners round it.
fn hover(at: Area, ink: Token) -> Option<Plate> {
    let (radius, fill, alpha, edge) = (RADIUS, Some(ink), u8::MAX, None);
    Some(Plate {
        toward: None,
        at,
        radius,
        fill,
        alpha,
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
        tracking: 0.0,
        names: false,
        rgb: None,
        on: None,
    })
}

/// `mark` spaced as the caption is: 0.08 em between letters.
fn tracked(mut mark: Mark) -> Mark {
    if let Mark::Text(words) = &mut mark {
        words.tracking = 0.08 * words.size;
    }
    mark
}

/// `mark`, a row's name.
fn named(mut mark: Mark) -> Mark {
    if let Mark::Text(words) = &mut mark {
        words.names = true;
    }
    mark
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

/// A workspace's line in `at`: `slot` points of colour before its `mini`
/// miniature (painted here on a row, by the swatch in the header), its
/// `name_pt` name over its summary, `status` points left at the right.
fn entry(
    at: Area,
    row: &CellRow,
    (slot, mini): (f32, [f32; 2]),
    name_pt: f32,
    status: f32,
    ink: &MenuChrome,
) -> Vec<Mark> {
    let lead = slot + SLOT_GAP_PT;
    let left = at.x + lead + mini[0] + 8.0;
    let width = at.x + at.w - status - left;
    let top = at.y + (at.h - 30.0) / 2.0;
    let mut marks = vec![
        named(text(left, top + 7.5, &row.label, name_pt, ink.ink, width)),
        text(left, top + 23.5, &row.summary, SMALL_PT, ink.support, width),
    ];
    let face = area(at.x + lead, at.y + (at.h - mini[1]) / 2.0, mini[0], mini[1]);
    marks.extend(row.tree.clone().map(|tree| Mark::Tree(face, tree)));
    let slot_at = area(
        at.x,
        at.y + (at.h - SLOT_HEIGHT_PT) / 2.0,
        SLOT_WIDTH_PT,
        SLOT_HEIGHT_PT,
    );
    let slot_mark = row.colour.as_ref().map(|colour| Mark::Colour {
        at: slot_at,
        radius: SLOT_RADIUS_PT,
        rgb: colour.rgb,
    });
    marks.extend(slot_mark.filter(|_| row.kind != RowKind::Current));
    marks
}

/// The header's swatch on the line `line`: the colour 16 x 16, pressed over
/// 24 x 24, ringed in the accent on hover; it opens the palette popover.
fn swatch(line: Area, row: &CellRow, ink: &MenuChrome) -> Piece {
    let at = area(
        line.x,
        line.y + (line.h - SWATCH_PT) / 2.0,
        SWATCH_PT,
        SWATCH_PT,
    );
    let grow = (SWATCH_HIT_PT - SWATCH_PT) / 2.0;
    let hit = area(at.x - grow, at.y - grow, SWATCH_HIT_PT, SWATCH_HIT_PT);
    let ring = area(at.x - 2.0, at.y - 2.0, at.w + 4.0, at.h + 4.0);
    let colour = row.colour.as_ref().map(|colour| Mark::Colour {
        at,
        radius: SWATCH_RADIUS_PT,
        rgb: colour.rgb,
    });
    Piece {
        row: Some(row.clone()),
        press: true,
        hover: Some(Plate {
            toward: None,
            at: ring,
            radius: SWATCH_RADIUS_PT + 2.0,
            fill: None,
            alpha: u8::MAX,
            edge: Some(ink.accent),
        }),
        ..Piece::plain(hit, colour.into_iter().collect())
    }
}

/// A header or footer press of `row` at `at`, `plain` without a resting
/// fill: Save stands out while it holds edits and turns quiet while
/// autosave saves; the switch shows its state words.
fn press(at: Area, row: &CellRow, plain: bool, ink: &MenuChrome, measure: Measure<'_>) -> Piece {
    let quiet = row.kind == RowKind::Quiet;
    let enabled = row.unavailable.is_none() && !quiet;
    let (back, edge) = match (row.dot, plain || quiet, enabled) {
        (true, ..) => (Some(ink.accent), Some(ink.accent)),
        (false, true, _) => (None, None),
        (false, false, true) => (Some(ink.control), Some(ink.edge)),
        (false, false, false) => (None, Some(ink.block_edge)),
    };
    let (y, mut x) = (at.y + at.h / 2.0, at.x + 8.0);
    let mut marks = vec![plate(at, RADIUS, back, edge)];
    if let (true, Mark::Box(save)) = (row.dot, &mut marks[0]) {
        save.alpha = ink.tint;
    }
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
        drag: false,
        double: false,
        hover: hover(at, ink.hover).filter(|_| enabled && !row.dot),
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
        (SWATCH_PT, MINI_HEADER),
        HEADER_NAME_PT,
        state,
        &ink,
    ));
    pieces.push(Piece {
        row: Some(menu.current.clone()),
        ..Piece::plain(line, marks)
    });
    pieces.push(swatch(line, &menu.swatch, &ink));
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
        tracked(text(11.0, y, CAPTION, CAPTION_PT, ink.faint, width)),
        ending(measure, width - 10.0, y, &count, SMALL_PT, ink.faint),
    ];
    let at = area(0.0, CHIP_MENU_HEADER_PT, width, CHIP_MENU_CAPTION_PT);
    pieces.push(Piece::plain(at, caption));
    pieces
}

/// The filter field across a list `width` wide, on its rows' extents, holding `row`'s words: the box the window's
/// text field sits in.
#[must_use]
pub fn filter(row: CellRow, width: f32, dark: bool) -> Piece {
    let ink = chrome(dark);
    let at = area(PAD, 0.0, width - 2.0 * PAD, 24.0);
    let field = plate(at, RADIUS, Some(ink.control), Some(ink.edge));
    Piece {
        row: Some(row),
        ..Piece::plain(at, vec![field])
    }
}

/// Where the filter field's words stand, in a list `width` wide.
#[must_use]
pub fn filter_text(width: f32) -> Area {
    area(PAD + 8.0, 4.0, width - 2.0 * PAD - 16.0, 16.0)
}

/// A list row in a list `width` wide, inside its padding: the tab's own on the selection fill with its
/// check, a row whose edit a tab holds with its dot, the focused one ringed.
#[must_use]
pub fn row(row: &CellRow, width: f32, focused: bool, dark: bool) -> Piece {
    let ink = chrome(dark);
    let at = area(PAD, 0.0, width - 2.0 * PAD, CHIP_MENU_ROW_PT);
    let ring = if focused { Some(ink.accent) } else { None };
    // The selection, hover and focus start past the colour slot, so the slot
    // stands on the panel and never sinks into the highlight.
    let from = 2.0 * PAD + SLOT_WIDTH_PT + HIGHLIGHT_GAP_PT;
    let lit = area(from, 0.0, at.x + at.w - from, CHIP_MENU_ROW_PT);
    let tint = match (row.on, focused) {
        (true, _) => RowTint::Current,
        (false, true) => RowTint::Hover,
        (false, false) => RowTint::Rest,
    };
    let mut marks = vec![
        Mark::Box(tinted(lit, row, tint, dark)),
        plate(lit, RADIUS, None, ring),
    ];
    let (status, y) = (width - PAD - 8.0 - STATUS_PT, CHIP_MENU_ROW_PT / 2.0);
    if row.on {
        marks.push(text(status + 1.0, y, CHECK, 12.0, ink.accent, STATUS_PT));
    } else if row.dot {
        marks.push(dot(status + 7.0, y, DOT_PT, ink.accent));
    }
    let line = area(2.0 * PAD, 0.0, width - 3.0 * PAD - 8.0, CHIP_MENU_ROW_PT);
    let line = entry(
        line,
        row,
        (SLOT_WIDTH_PT, MINI_ROW),
        BODY_PT,
        STATUS_PT + 8.0,
        &ink,
    );
    marks.extend(line);
    legible(&mut marks, row, dark);
    Piece {
        at,
        row: Some(row.clone()),
        press: true,
        drag: false,
        double: false,
        hover: (!row.on).then(|| tinted(lit, row, RowTint::Hover, dark)),
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
    // Only the switch and its label take a press; the state words do not.
    if let (Some(switch), Some(row)) = (pieces.get_mut(1), menu.foot.first()) {
        switch.at.w = SWITCH_PT + measure(&row.label, BODY_PT) + 16.0;
        switch.hover = hover(switch.at, ink.hover);
    }
    pieces
}

/// The menu as one sheet at `layout`'s size: the header, the filter field
/// while the list has one, the rows `filter` keeps (`focus` ringed) in a
/// list `layout.list_height` tall, and the footer.
#[must_use]
pub fn sheet(
    menu: &CellMenu,
    (filter, focus): (&str, Option<usize>),
    layout: &ChipMenuLayout,
    dark: bool,
    measure: Measure<'_>,
) -> Sheet {
    let width = layout.width - 2.0 * CHIP_MENU_BORDER_PT;
    let mut view = Sheet::of([width, 0.0], Vec::new());
    view.head = head(menu, width, dark, measure);
    view.list_top = HEAD_PT;
    if let Some(row) = menu.filter_row(filter) {
        view.head
            .push(self::filter(row, width, dark).shifted(HEAD_PT));
        view.list_top = HEAD_PT + CHIP_MENU_FILTER_PT; // The window lays the field.
    }
    let shown = menu.shown(filter);
    let rows = shown.iter().enumerate();
    let place = |(at, shown): (usize, &&CellRow)| {
        row(shown, width, focus == Some(at), dark).shifted(at as f32 * CHIP_MENU_ROW_PT)
    };
    view.list = rows.map(place).collect();
    view.list_height = shown.len() as f32 * CHIP_MENU_ROW_PT + PAD;
    view.list_view = layout.list_height;
    let foot_y = view.list_top + view.list_view;
    view.foot = (foot(menu, width, dark, measure).into_iter())
        .map(|piece| piece.shifted(foot_y))
        .collect();
    view.size[1] = foot_y + CHIP_MENU_FOOTER_PT;
    view
}

#[cfg(test)]
#[path = "workspace_menu_tests.rs"]
mod tests;
