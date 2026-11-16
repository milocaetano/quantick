//! The layout picker (alternative A, picked 5 to 0): sixteen equal cells on
//! one 4 x 4 grid, WITH FLOW in the left two columns and TIMEFRAMES in the
//! right two, under menu B's header naming the tab's workspace and whether
//! its layout is saved.
//!
//! Every size, place, word and colour is decided here, in fixed geometry: the
//! window only paints the rectangles and words it is handed, inside a frame
//! of [`SIZE`], and reports what the pointer and keys did ([`Seen`]); what
//! that does to the picker is [`State::step`]'s.

use quantick_appearance::layout_picker::{self as look, ColourInks, colour_inks};
use quantick_appearance::{PremultipliedRgba, ResolvedTheme};
use quantick_control_schema::canvas::{LayoutGroupDto, LayoutPickerCell, LayoutPickerResult};
use quantick_workspace::canvas::{
    CanvasRect, LAYOUT_PRESETS, LayoutMarks, LayoutPreset, PaneKind, PaneTree, PresetGroup,
};
use quantick_workspace::workspace_colour::WorkspaceColour;

/// The grid: two columns per group, four rows.
pub const COLUMNS: usize = 4;
pub const ROWS: usize = 4;
/// A cell, and the gap between two.
pub const CELL: [f32; 2] = [84.0, 60.0];
const GAP_PT: f32 = 4.0;
/// Between the two groups, the divider in its middle.
const GROUP_GAP_PT: f32 = 16.0;
/// Around the grid.
const PAD_PT: f32 = 8.0;
/// The header block, its rule included, and its two lines' middles.
const HEADER_PT: f32 = 48.0;
const LINE_ONE_Y: f32 = 15.0;
const LINE_TWO_Y: f32 = 33.0;
/// The group captions' middle, and where the grid starts.
const CAPTION_Y: f32 = HEADER_PT + 12.0;
const GRID_TOP_PT: f32 = HEADER_PT + 22.0;
/// The content inside the 1 pt border: the grid's width, the header and the
/// grid's height. Never anything else, in any state.
pub const SIZE: [f32; 2] = [
    2.0 * PAD_PT + 4.0 * CELL[0] + 2.0 * GAP_PT + GROUP_GAP_PT,
    GRID_TOP_PT + 4.0 * CELL[1] + 3.0 * GAP_PT + PAD_PT,
];
/// Menu B's border and corners.
pub const BORDER_PT: f32 = 1.0;
pub const RADIUS_PT: f32 = 6.0;
const BLOCK_RADIUS_PT: f32 = 5.0;
/// The frame, its border included.
pub const OUTER: [f32; 2] = [SIZE[0] + 2.0 * BORDER_PT, SIZE[1] + 2.0 * BORDER_PT];
/// How far below its button the popover hangs, and the least room it keeps
/// from the window's edges.
pub const DROP_PT: f32 = 4.0;
const MARGIN_PT: f32 = 4.0;
/// The least height shown in a window too short for the whole.
const MIN_SHOWN_PT: f32 = 80.0;
/// A cell's thumbnail, from the cell's top left, and its size.
const THUMB_AT: [f32; 2] = [15.0, 6.0];
const THUMB: [f32; 2] = [54.0, 30.0];
/// A cell's label: its size and its middle from the cell's top.
pub const LABEL_PT: f32 = 11.0;
const LABEL_Y: f32 = 47.0;
/// The current ring, the focus ring's gap outside the cell and its width,
/// its dash and space, and how far a pressed cell sinks.
pub const RING_PT: f32 = 2.0;
pub const FOCUS_GAP_PT: f32 = 2.0;
const FOCUS_PT: f32 = 2.0;
const DASH: [f32; 2] = [4.0, 3.0];
pub const SINK_PT: f32 = 1.0;
const CELL_RADIUS_PT: f32 = 5.0;
/// The ✓ badge's size and the gap to the cell's top right corner.
const BADGE_PT: f32 = 12.0;
const BADGE_INSET_PT: f32 = 3.0;
/// Words: the workspace name, the status and hint, the captions.
const NAME_PT: f32 = 13.0;
pub const SMALL_PT: f32 = 11.0;
const CAPTION_PT: f32 = 10.0;
const TRACKING: f32 = 0.08;
/// The header's swatch, and where its words start.
const SWATCH_PT: f32 = 12.0;
const TITLE_X: f32 = 28.0;
const NAME_X: f32 = 92.0;
/// The status slot at the header's right: fixed, so nothing reflows; the
/// floppy or the unsaved dot stands at its left.
pub const STATUS_W: f32 = 140.0;
pub const STATUS_ICON_W: f32 = 10.0;
const DOT_PT: f32 = 5.0;
/// How long a pick shows its confirmation before the popover closes.
pub const CONFIRM_MS: u64 = 800;

/// The header's words.
pub const TITLE: &str = "LAYOUT OF";
pub const SAVED: &str = "Saved:";
pub const NOT_SAVED: &str = "Not saved \u{b7} Ctrl+S";
pub const CHANGED: &str = "Changed to";
pub const HINT: &str = "Arrows move \u{b7} Enter picks \u{b7} Esc closes";
/// Phosphor's check and floppy disk, in the bundled icon font.
pub const CHECK: &str = "\u{e182}";
pub const FLOPPY: &str = "\u{e248}";

/// A box, in points from the picker's top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

/// Where words stand on their `x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Left,
    Centre,
    Right,
}

/// A box and the colour it is filled or edged in, as premultiplied bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plate {
    pub at: Rect,
    pub radius: f32,
    pub rgba: [u8; 4],
}

/// Words centred on `y`, standing on `x` as `anchor` says, cut short to
/// `width`, `tracking` points between letters.
#[derive(Debug, Clone, PartialEq)]
pub struct Words {
    pub x: f32,
    pub y: f32,
    pub anchor: Anchor,
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub tracking: f32,
    pub width: f32,
    pub rgba: [u8; 4],
}

/// One thing to paint.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// The plate filled; square ones are pixel exact.
    Fill(Plate),
    /// The plate's edge, this wide, inside it.
    Edge(Plate, f32),
    Words(Words),
}

fn fill(at: Rect, radius: f32, rgba: PremultipliedRgba) -> Mark {
    let rgba = rgba.to_array();
    Mark::Fill(Plate { at, radius, rgba })
}

fn words(x: f32, y: f32, anchor: Anchor, text: &str, size: f32, rgba: PremultipliedRgba) -> Mark {
    Mark::Words(Words {
        x,
        y,
        anchor,
        text: text.to_owned(),
        size,
        bold: false,
        tracking: 0.0,
        width: SIZE[0],
        rgba: rgba.to_array(),
    })
}

/// `mark` cut at `width`, bold, or tracked as a caption.
fn shaped(mut mark: Mark, cut: Option<f32>, heavy: bool, caption: bool) -> Mark {
    if let Mark::Words(words) = &mut mark {
        words.width = cut.unwrap_or(words.width);
        words.bold = heavy;
        words.tracking = if caption { TRACKING * words.size } else { 0.0 };
    }
    mark
}

/// What a press can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Cell(&'static LayoutPreset),
    /// The header's "was <layout>" words.
    Undo,
}

/// A pick being confirmed: the layout it set, the one it replaced, when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    pub to: &'static LayoutPreset,
    pub was: Option<&'static LayoutPreset>,
    pub at_ms: u64,
}

/// The picker's own state while open; closing forgets it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    pub hover: Option<Target>,
    pub pressed: Option<Target>,
    /// The keyboard's cell; none until an arrow is pressed.
    pub focus: Option<&'static LayoutPreset>,
    pub changed: Option<Changed>,
    /// Kept open past the confirmation: a capture's staging.
    pub held: bool,
}

/// The keys the picker takes, by the window's key names, with Ctrl or not,
/// in [`Keys::from_pressed`]'s order.
pub const KEY_NAMES: [(&str, bool); 7] = [
    ("ArrowLeft", false),
    ("ArrowRight", false),
    ("ArrowUp", false),
    ("ArrowDown", false),
    ("Enter", false),
    ("Escape", false),
    ("Z", true),
];

/// The keys the picker takes while open.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub enter: bool,
    pub escape: bool,
    /// Ctrl+Z.
    pub undo: bool,
}

impl Keys {
    /// [`KEY_NAMES`], each pressed or not this frame.
    #[must_use]
    pub const fn from_pressed(pressed: [bool; 7]) -> Self {
        let [left, right, up, down, enter, escape, undo] = pressed;
        Self {
            left,
            right,
            up,
            down,
            enter,
            escape,
            undo,
        }
    }
}

/// What the window saw this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub now_ms: u64,
    /// The layout the canvas draws.
    pub current: Option<&'static LayoutPreset>,
    /// What the pointer is over.
    pub over: Option<Target>,
    /// The primary button went down, or up, this frame.
    pub press: bool,
    pub release: bool,
    /// A press landed outside the popover and its button.
    pub outside: bool,
    pub keys: Keys,
}

impl Seen {
    /// A frame: the clock, the canvas's layout, what the pointer is over, the
    /// primary press and release and a press outside, and [`KEY_NAMES`] each
    /// pressed or not.
    #[must_use]
    pub const fn of(
        now_ms: u64,
        current: Option<&'static LayoutPreset>,
        over: Option<Target>,
        [press, release, outside]: [bool; 3],
        keys: [bool; 7],
    ) -> Self {
        let keys = Keys::from_pressed(keys);
        Self {
            now_ms,
            current,
            over,
            press,
            release,
            outside,
            keys,
        }
    }
}

/// What a frame asks of the window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Lay the canvas out as this preset.
    pub pick: Option<&'static LayoutPreset>,
    /// Close the popover.
    pub close: bool,
}

impl State {
    /// Take one frame of input: a press commits on release inside the cell
    /// it began on, Enter picks the focused cell, Ctrl+Z or the "was" words
    /// restore the replaced layout; Esc, a press outside, picking the current
    /// layout and the end of a confirmation close.
    pub fn step(&mut self, seen: Seen) -> Step {
        let mut step = Step::default();
        self.hover = seen.over;
        let mut chosen = None;
        if seen.press {
            self.pressed = seen.over;
        }
        if seen.release {
            chosen = self
                .pressed
                .take()
                .filter(|target| Some(*target) == seen.over);
        }
        self.focus = moved(self.focus, seen.current, seen.keys);
        if seen.keys.enter {
            chosen = chosen.or(self.focus.map(Target::Cell));
        }
        if seen.keys.undo {
            chosen = chosen.or(Some(Target::Undo));
        }
        let to = match chosen {
            Some(Target::Cell(preset)) => Some(preset),
            Some(Target::Undo) => self.changed.and_then(|changed| changed.was),
            None => None,
        };
        if let Some(to) = to {
            if Some(to) == seen.current {
                step.close = true;
            } else {
                let was = seen.current;
                self.changed = Some(Changed {
                    to,
                    was,
                    at_ms: seen.now_ms,
                });
                step.pick = Some(to);
            }
        }
        let confirmed = self.changed.is_some_and(|changed| {
            !self.held && seen.now_ms.saturating_sub(changed.at_ms) >= CONFIRM_MS
        });
        step.close |= seen.keys.escape || seen.outside || (confirmed && step.pick.is_none());
        if step.close {
            *self = Self::default();
        }
        step
    }

    /// When the window should paint again after a frame that began as
    /// `before`: at once if the frame moved the picker, else when the
    /// confirmation ends.
    #[must_use]
    pub fn repaint_ms(&self, before: &Self, now_ms: u64) -> Option<u64> {
        if self != before {
            return Some(0);
        }
        self.remaining_ms(now_ms)
    }

    /// How long until the confirmation ends.
    #[must_use]
    pub fn remaining_ms(&self, now_ms: u64) -> Option<u64> {
        let changed = self.changed.filter(|_| !self.held)?;
        Some(CONFIRM_MS.saturating_sub(now_ms.saturating_sub(changed.at_ms)))
    }
}

/// The preset at a place on the grid, if one stands there.
#[must_use]
pub fn preset_at(row: usize, column: usize) -> Option<&'static LayoutPreset> {
    let (group, _) = PresetGroup::COLUMNS.get(column / 2)?;
    let index = row * 2 + column % 2;
    (row < ROWS).then(|| group.presets().get(index).copied())?
}

/// Where `preset` stands on the grid: its row and column.
#[must_use]
pub fn place(preset: &LayoutPreset) -> Option<(usize, usize)> {
    (0..ROWS)
        .flat_map(|row| (0..COLUMNS).map(move |column| (row, column)))
        .find(|(row, column)| preset_at(*row, *column) == Some(preset))
}

/// The focus after this frame's arrows: the first lands on the current cell,
/// each next one moves a place, held at the grid's edges and across the gap.
fn moved(
    focus: Option<&'static LayoutPreset>,
    current: Option<&'static LayoutPreset>,
    keys: Keys,
) -> Option<&'static LayoutPreset> {
    let arrows = [keys.left, keys.right, keys.up, keys.down];
    if !arrows.contains(&true) {
        return focus;
    }
    let Some(from) = focus else {
        return current.or_else(|| preset_at(0, 0));
    };
    let (row, column) = place(from)?;
    let step = |at: usize, back: bool, ahead: bool, last: usize| match (back, ahead) {
        (true, false) => at.saturating_sub(1),
        (false, true) => (at + 1).min(last),
        _ => at,
    };
    let column = step(column, keys.left, keys.right, COLUMNS - 1);
    let row = step(row, keys.up, keys.down, ROWS - 1);
    preset_at(row, column).or(Some(from))
}

/// Where the popover's frame stands: below its button, right edges flush,
/// shifted right in a narrow window, never up; in a short window only
/// `height` shows and the rest scrolls inside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub scrolls: bool,
}

/// The frame under `button` in `screen`, both in window points.
#[must_use]
pub fn place_under(button: Rect, screen: Rect) -> Placement {
    let flush = button.x + button.w - OUTER[0];
    let x = flush.max(screen.x + MARGIN_PT).round();
    let y = (button.y + button.h + DROP_PT).round();
    let room = screen.y + screen.h - MARGIN_PT - y;
    let height = OUTER[1].min(room.max(MIN_SHOWN_PT)).floor();
    Placement {
        x,
        y,
        height,
        scrolls: height < OUTER[1],
    }
}

/// What the picker shows: the tab's workspace, its colour in this theme, the
/// layout marks and the picker's state.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub workspace: &'a str,
    pub colour: Option<WorkspaceColour>,
    pub marks: LayoutMarks,
    pub state: &'a State,
}

impl<'a> View<'a> {
    #[must_use]
    pub const fn new(
        workspace: &'a str,
        colour: Option<WorkspaceColour>,
        marks: LayoutMarks,
        state: &'a State,
    ) -> Self {
        Self {
            workspace,
            colour,
            marks,
            state,
        }
    }
}

/// One cell as drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub preset: &'static LayoutPreset,
    /// Its box, the press's reach.
    pub at: Rect,
    pub current: bool,
    pub saved: bool,
    pub marks: Vec<Mark>,
}

/// The header's words, for painting and the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub at: Rect,
    pub workspace: String,
    /// `Saved: <layout>` or `Not saved · Ctrl+S`.
    pub status: String,
    /// The hint, or `Changed to <layout>` while a pick confirms.
    pub line: String,
    /// The "was <layout>" words and their reach, while a pick confirms.
    pub undo: Option<(Rect, String)>,
}

impl Header {
    /// What the scene calls the header.
    #[must_use]
    pub fn label(&self) -> String {
        format!(
            "{TITLE} {} \u{b7} {} \u{b7} {}",
            self.workspace, self.status, self.line
        )
    }
}

/// The picker in `theme`: the frame's colours, the marks behind the cells,
/// the cells and the header.
#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub size: [f32; 2],
    pub ground: [u8; 4],
    pub edge: [u8; 4],
    pub marks: Vec<Mark>,
    pub cells: Vec<Cell>,
    pub header: Header,
}

/// The workspace colour's inks on `ground`; the accent stands in without one.
fn inks(colour: Option<[u8; 3]>, theme: &ResolvedTheme, ground: PremultipliedRgba) -> ColourInks {
    let accent = theme.color(quantick_appearance::Token::Accent).to_array();
    let rgb = colour.unwrap_or([accent[0], accent[1], accent[2]]);
    colour_inks(rgb, ground, theme.dark)
}

/// The save state's words.
fn status(marks: &LayoutMarks) -> String {
    match marks.saved.filter(|_| !marks.layout_unsaved) {
        Some(saved) => format!("{SAVED} {}", saved.label),
        None => NOT_SAVED.to_owned(),
    }
}

/// The picker as `view` shows it in `theme`.
#[must_use]
pub fn picker(view: &View<'_>, theme: &ResolvedTheme) -> Picker {
    let look = look::chrome(theme.dark);
    let ground = theme.color(look.ground);
    let colour = view.colour.map(|colour| colour.rgb(theme.dark));
    let inks = inks(colour, theme, ground);
    let ink = |token| theme.color(token);
    let mut marks = Vec::new();
    let header = header(view, theme, &inks, &mut marks);
    for (index, (group, caption)) in PresetGroup::COLUMNS.iter().enumerate() {
        let x = column_x(2 * index) + 2.0;
        let words = words(
            x,
            CAPTION_Y,
            Anchor::Left,
            caption,
            CAPTION_PT,
            ink(look.support),
        );
        marks.push(shaped(words, None, false, true));
        debug_assert!(group.presets().len() <= 2 * ROWS, "{group:?} fits the grid");
    }
    let divider_x = column_x(1) + CELL[0] + GROUP_GAP_PT / 2.0;
    let grid_h = 4.0 * CELL[1] + 3.0 * GAP_PT;
    marks.push(fill(
        Rect::new(divider_x, GRID_TOP_PT, 1.0, grid_h),
        0.0,
        ink(look.block_edge),
    ));
    let cells = (LAYOUT_PRESETS.iter())
        .filter_map(|preset| Some((preset, place(preset)?)))
        .map(|(preset, (row, column))| cell(view, theme, &inks, preset, row, column))
        .collect();
    Picker {
        size: SIZE,
        ground: ground.to_array(),
        edge: ink(look.edge).to_array(),
        marks,
        cells,
        header,
    }
}

fn column_x(column: usize) -> f32 {
    let group = if column >= 2 {
        2.0 * CELL[0] + GAP_PT + GROUP_GAP_PT
    } else {
        0.0
    };
    PAD_PT + group + (column % 2) as f32 * (CELL[0] + GAP_PT)
}

fn row_y(row: usize) -> f32 {
    GRID_TOP_PT + row as f32 * (CELL[1] + GAP_PT)
}

/// The header block, menu B's: the swatch, LAYOUT OF and the name, the save
/// state in its fixed slot; under them the hint, or the confirmation with
/// the "was" words.
fn header(
    view: &View<'_>,
    theme: &ResolvedTheme,
    inks: &ColourInks,
    marks: &mut Vec<Mark>,
) -> Header {
    let look = look::chrome(theme.dark);
    let ink = |token| theme.color(token);
    let (block, width) = (ink(look.block), SIZE[0]);
    marks.push(fill(
        Rect::new(0.0, 0.0, width, HEADER_PT),
        BLOCK_RADIUS_PT,
        block,
    ));
    let square = Rect::new(0.0, BLOCK_RADIUS_PT, width, HEADER_PT - BLOCK_RADIUS_PT);
    marks.push(fill(square, 0.0, block));
    marks.push(fill(
        Rect::new(0.0, HEADER_PT - 1.0, width, 1.0),
        0.0,
        ink(look.block_edge),
    ));
    let swatch = Rect::new(10.0, LINE_ONE_Y - SWATCH_PT / 2.0, SWATCH_PT, SWATCH_PT);
    marks.push(fill(swatch, 3.0, inks.ring));
    let caption = words(
        TITLE_X,
        LINE_ONE_Y,
        Anchor::Left,
        TITLE,
        CAPTION_PT,
        ink(look.support),
    );
    marks.push(shaped(caption, None, false, true));
    let slot = width - 10.0 - STATUS_W;
    let name = words(
        NAME_X,
        LINE_ONE_Y,
        Anchor::Left,
        view.workspace,
        NAME_PT,
        ink(look.ink),
    );
    marks.push(shaped(name, Some(slot - 8.0 - NAME_X), false, false));
    let status = status(&view.marks);
    if view.marks.saved.is_some() && !view.marks.layout_unsaved {
        let icon = words(
            slot,
            LINE_ONE_Y,
            Anchor::Left,
            FLOPPY,
            SMALL_PT,
            ink(look.support),
        );
        marks.push(icon);
    } else {
        let dot = Rect::new(slot + 2.0, LINE_ONE_Y - DOT_PT / 2.0, DOT_PT, DOT_PT);
        marks.push(fill(dot, DOT_PT / 2.0, inks.ring));
    }
    let text_x = slot + STATUS_ICON_W + 4.0;
    let said = words(
        text_x,
        LINE_ONE_Y,
        Anchor::Left,
        &status,
        SMALL_PT,
        ink(look.support),
    );
    marks.push(shaped(
        said,
        Some(STATUS_W - STATUS_ICON_W - 4.0),
        false,
        false,
    ));
    let changed = view.state.changed;
    let line = match changed {
        Some(changed) => format!("{CHANGED} {}", changed.to.label),
        None => HINT.to_owned(),
    };
    let mut undo = None;
    if let Some(changed) = changed {
        marks.push(words(
            10.0,
            LINE_TWO_Y,
            Anchor::Left,
            CHECK,
            SMALL_PT,
            inks.ring,
        ));
        marks.push(words(
            24.0,
            LINE_TWO_Y,
            Anchor::Left,
            &line,
            SMALL_PT,
            ink(look.ink),
        ));
        if let Some(was) = changed.was {
            let said = format!("was {} \u{b7} Ctrl+Z", was.label);
            let lit = view.state.hover == Some(Target::Undo);
            let tone = if lit { look.ink } else { look.support };
            let right = width - 10.0;
            let reach = Rect::new(right - STATUS_W, LINE_TWO_Y - 8.0, STATUS_W + 4.0, 16.0);
            if lit {
                marks.push(fill(reach, 4.0, ink(look.hover)));
            }
            marks.push(words(
                right,
                LINE_TWO_Y,
                Anchor::Right,
                &said,
                SMALL_PT,
                ink(tone),
            ));
            undo = Some((reach, said));
        }
    } else {
        marks.push(words(
            10.0,
            LINE_TWO_Y,
            Anchor::Left,
            &line,
            SMALL_PT,
            ink(look.support),
        ));
    }
    Header {
        at: Rect::new(0.0, 0.0, width, HEADER_PT),
        workspace: view.workspace.to_owned(),
        status,
        line,
        undo,
    }
}

/// One cell at `row`, `column`: its plate for hover, press or current, the
/// current ring and ✓, the thumbnail, the name, and the focus ring outside.
fn cell(
    view: &View<'_>,
    theme: &ResolvedTheme,
    inks: &ColourInks,
    preset: &'static LayoutPreset,
    row: usize,
    column: usize,
) -> Cell {
    let look = look::chrome(theme.dark);
    let ink = |token| theme.color(token);
    let at = Rect::new(column_x(column), row_y(row), CELL[0], CELL[1]);
    let state = view.state;
    let me = Some(Target::Cell(preset));
    let current = view.marks.current == Some(preset);
    let pressed = state.pressed == me && state.hover == me;
    let hovered = state.hover == me && (state.pressed.is_none() || pressed);
    let mut marks = Vec::new();
    let plate = match (pressed, hovered, current) {
        (true, ..) => Some(look::pressed(theme)),
        (false, true, _) => Some(ink(look.hover)),
        (false, false, true) => Some(inks.plate),
        _ => None,
    };
    marks.extend(plate.map(|plate| fill(at, CELL_RADIUS_PT, plate)));
    let sink = if pressed { SINK_PT } else { 0.0 };
    let thumb = Rect::new(
        at.x + THUMB_AT[0],
        at.y + THUMB_AT[1] + sink,
        THUMB[0],
        THUMB[1],
    );
    marks.extend(thumbnail(preset.tree, thumb, inks));
    let label = words(
        at.x + at.w / 2.0,
        at.y + LABEL_Y + sink,
        Anchor::Centre,
        preset.label,
        LABEL_PT,
        ink(look.ink),
    );
    marks.push(shaped(label, Some(at.w - 2.0 * RING_PT), current, false));
    if current {
        let ring = Plate {
            at,
            radius: CELL_RADIUS_PT,
            rgba: inks.ring.to_array(),
        };
        marks.push(Mark::Edge(ring, RING_PT));
        let badge = Rect::new(
            at.x + at.w - BADGE_INSET_PT - BADGE_PT,
            at.y + BADGE_INSET_PT,
            BADGE_PT,
            BADGE_PT,
        );
        marks.push(fill(badge, BADGE_PT / 2.0, inks.ring));
        let centre = (badge.x + BADGE_PT / 2.0, badge.y + BADGE_PT / 2.0);
        marks.push(words(
            centre.0,
            centre.1,
            Anchor::Centre,
            CHECK,
            9.0,
            inks.check,
        ));
    }
    if state.focus == Some(preset) {
        marks.extend(dashed(at, ink(look.focus)));
    }
    Cell {
        preset,
        at,
        current,
        saved: view.marks.saved == Some(preset),
        marks,
    }
}

/// A dashed ring [`FOCUS_PT`] wide, [`FOCUS_GAP_PT`] outside `cell`.
fn dashed(cell: Rect, rgba: PremultipliedRgba) -> Vec<Mark> {
    let out = FOCUS_GAP_PT + FOCUS_PT;
    let ring = Rect::new(
        cell.x - out,
        cell.y - out,
        cell.w + 2.0 * out,
        cell.h + 2.0 * out,
    );
    let mut dashes = Vec::new();
    let mut run = |from: f32, length: f32, along: &dyn Fn(f32, f32) -> Rect| {
        let mut at = 0.0;
        while at < length {
            let dash = DASH[0].min(length - at);
            dashes.push(fill(along(from + at, dash), 0.0, rgba));
            at += DASH[0] + DASH[1];
        }
    };
    let (right, bottom) = (ring.x + ring.w - FOCUS_PT, ring.y + ring.h - FOCUS_PT);
    run(ring.x, ring.w, &|x, w| Rect::new(x, ring.y, w, FOCUS_PT));
    run(ring.x, ring.w, &|x, w| Rect::new(x, bottom, w, FOCUS_PT));
    let side = ring.h - 2.0 * FOCUS_PT;
    run(ring.y + FOCUS_PT, side, &|y, h| {
        Rect::new(ring.x, y, FOCUS_PT, h)
    });
    run(ring.y + FOCUS_PT, side, &|y, h| {
        Rect::new(right, y, FOCUS_PT, h)
    });
    dashes
}

/// `tree`'s panes inside `at`, as the canvas lays them out, on whole points:
/// the flow pane filled with the colour's tint and edged in the colour (or
/// edged wider where no tint stands off the ground), a timeframe pane edged
/// in the colour at 60%.
#[must_use]
pub fn thumbnail(tree: &PaneTree, at: Rect, inks: &ColourInks) -> Vec<Mark> {
    let kinds = tree.panes();
    let area = CanvasRect::new(at.x, at.y, at.w, at.h);
    let mut marks = Vec::new();
    for (slot, pane) in tree.rects(area) {
        let (x0, y0) = (pane.x.round(), pane.y.round());
        let (x1, y1) = ((pane.x + pane.w).round(), (pane.y + pane.h).round());
        let flow = (kinds.iter()).any(|(held, kind)| *held == slot && *kind == PaneKind::Flow);
        let (edge, width) = if flow {
            (inks.ring, inks.flow_edge)
        } else {
            (inks.time_edge, 1.0)
        };
        if let Some(tint) = inks.fill.filter(|_| flow) {
            let inner = Rect::new(
                x0 + width,
                y0 + width,
                x1 - x0 - 2.0 * width,
                y1 - y0 - 2.0 * width,
            );
            marks.push(fill(inner, 0.0, tint));
        }
        let (w, h) = (x1 - x0, y1 - y0);
        for side in [
            Rect::new(x0, y0, w, width),
            Rect::new(x0, y1 - width, w, width),
            Rect::new(x0, y0 + width, width, h - 2.0 * width),
            Rect::new(x1 - width, y0 + width, width, h - 2.0 * width),
        ] {
            marks.push(fill(side, 0.0, edge));
        }
    }
    marks
}

/// The toolbar button's miniature of `tree` in `at`, in the workspace colour
/// on the button's `ground`; grey without one.
#[must_use]
pub fn miniature(
    tree: &PaneTree,
    at: Rect,
    colour: Option<WorkspaceColour>,
    ground: [u8; 4],
    theme: &ResolvedTheme,
) -> Vec<Mark> {
    let grey = (theme.color(quantick_appearance::Token::CapsuleMiniature)).to_array();
    let rgb = colour.map(|colour| colour.rgb(theme.dark));
    let rgb = rgb.unwrap_or([grey[0], grey[1], grey[2]]);
    let ground = PremultipliedRgba::from_premultiplied(ground);
    thumbnail(tree, at, &colour_inks(rgb, ground, theme.dark))
}

/// The picker as the `canvas.layout.picker` read says it.
#[must_use]
pub fn read(workspace: &str, marks: LayoutMarks, state: &State, open: bool) -> LayoutPickerResult {
    let view = View::new(workspace, None, marks, state);
    let id = |preset: Option<&'static LayoutPreset>| preset.map(|preset| preset.id.to_owned());
    let changed = view.state.changed;
    let cells = (LAYOUT_PRESETS.iter())
        .filter_map(|preset| Some((preset, place(preset)?)))
        .map(|(preset, (row, column))| LayoutPickerCell {
            preset_id: preset.id.to_owned(),
            name: preset.label.to_owned(),
            hint: preset.hint.to_owned(),
            group: match preset.group {
                PresetGroup::WithFlow => LayoutGroupDto::WithFlow,
                PresetGroup::Timeframes => LayoutGroupDto::Timeframes,
            },
            row: row as u32,
            column: column as u32,
            current: marks.current == Some(preset),
            saved: marks.saved == Some(preset),
        })
        .collect();
    LayoutPickerResult {
        open,
        workspace: view.workspace.to_owned(),
        status: status(&marks),
        unsaved: marks.layout_unsaved,
        current_id: id(marks.current),
        saved_id: id(marks.saved),
        changed: changed.map(|changed| format!("{CHANGED} {}", changed.to.label)),
        undo_id: id(changed.and_then(|changed| changed.was)),
        cells,
    }
}

#[cfg(test)]
#[path = "layout_picker_tests.rs"]
mod tests;
