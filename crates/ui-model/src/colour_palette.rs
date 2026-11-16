//! The palette popover under the workspace menu's header swatch (designer
//! B: four by four named swatches) as marks to paint: the grid, Custom open
//! under it, or the swap question in its place, each part's words, sizes and
//! tokens, and the row the scene lists it as. The palette and the warnings
//! are `quantick_stores::workspaces::palette`'s, the state and the keys
//! `quantick_session::colour_popover`'s; the window turns these marks into
//! pixels and presses, and lays its text field where [`Popover::field`] says.
//! Manage draws the same popover under a row; the create dialogs draw it
//! from [`choosing`], every held colour locked: hatched, faded, initialled,
//! no press.

use quantick_appearance::workspace_menu::{FilledButton, chrome};
use quantick_appearance::{PremultipliedRgba, ResolvedTheme, Token, contrast::contrast_ratio};
use quantick_control_handlers::workspace_book::cell_menu::{CellRow, RowColour, RowKind};
use quantick_control_handlers::workspace_book::colour_popover::{question, settle, shown_hsv};
use quantick_control_schema::workspace_book::WORKSPACE_COLOUR_SET_CAPABILITY_ID as SET;
use quantick_control_schema::workspace_colour::WORKSPACE_COLOUR_POPOVER_CAPABILITY_ID as POPOVER;
use quantick_session::colour_popover::{ColourIntent, ColourPopover, KeyAct, Keys};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_stores::workspaces::palette::{
    self, ColourWarning, HEX_HINT, Palette, Pick, SwapQuestion, Swatch, palette, warnings,
};
use quantick_stores::workspaces::{Rgb, WorkspaceBook, WorkspaceColour, hsv_rgb, parse_typed_hex};

use crate::workspace_menu::{Area, HEAD_PT, Mark, Piece, Plate, SWATCH_BOTTOM_PT, Words, area};

/// The popover's width and padding, and how far below the swatch it hangs.
pub const WIDTH_PT: f32 = 272.0;
const PAD: f32 = 12.0;
/// It clears the header's words below the swatch.
pub const DROP_PT: f32 = 8.0;
/// A swatch, the gap between two, and one grid row with its name under it.
const SWATCH: [f32; 2] = [56.0, 26.0];
const GAP: f32 = 8.0;
const PITCH: f32 = 51.0;
/// Where a swatch's name sits under it, centred.
const NAME_Y: f32 = 38.0;
const RADIUS: f32 = 5.0;
/// Words: a button and the question, a swatch's name.
const BODY_PT: f32 = 12.5;
const NAME_PT: f32 = 11.0;
/// Custom: the square, the strip, the row of the field and its buttons, and
/// one line per warning.
const SQUARE_H: f32 = 104.0;
const STRIP_H: f32 = 10.0;
const FIELD_W: f32 = 96.0;
const CONTROL_H: f32 = 24.0;
const LINE_PT: f32 = 18.0;
/// The square's strips: enough that its shading reads smooth.
const STRIPS: usize = 16;
/// Phosphor's check and warning, in the bundled icon font.
const CHECK: &str = "\u{e182}";
const WARNING: &str = "\u{e4e0}";
/// A locked swatch's veil: its colour shows at 45%.
const VEIL_ALPHA: u8 = 140;

/// The popover to paint.
#[derive(Debug, Clone, PartialEq)]
pub struct Popover {
    /// Width and height, in points.
    pub size: [f32; 2],
    pub pieces: Vec<Piece>,
    /// Where the window lays Custom's text field, while it is open.
    pub field: Option<Area>,
    /// What Enter or Space presses on each grid cell: the swatches in
    /// order, then Custom….
    pub cells: Vec<WorkspaceCommand>,
    /// The workspace's own swatch among [`Self::cells`].
    pub current: Option<usize>,
    /// Apply would apply: the field holds a colour.
    pub applies: bool,
}

impl Popover {
    /// The popover as a sheet of its own, under the menu's header swatch.
    #[must_use]
    pub fn sheet(self) -> crate::workspace_menu::Sheet {
        let mut sheet = crate::workspace_menu::Sheet::of(self.size, Vec::new());
        sheet.palette = Some(([0.0, 0.0], self));
        sheet
    }

    /// Every row's scene id under `scope`, its key after `prefix` (a Manage
    /// row's `Scalp.`): the same popover, listed where it stands.
    pub fn scoped(&mut self, scope: &'static str, prefix: &str) {
        for row in self
            .pieces
            .iter_mut()
            .filter_map(|piece| piece.row.as_mut())
        {
            row.scope = scope;
            row.key = format!("{prefix}{}", row.key);
        }
    }

    /// What `pressed` (one flag per `colour_popover::Key::ALL`) presses on
    /// `state`, its focus moved.
    pub fn keyed(&self, state: &mut ColourPopover, pressed: [bool; 8]) -> Option<WorkspaceCommand> {
        self.command(state.keys(Keys::of(pressed), self.cells.len(), self.current))
    }

    /// What a frame's keys press.
    #[must_use]
    pub fn command(&self, act: KeyAct) -> Option<WorkspaceCommand> {
        let colour = |intent| Some(WorkspaceCommand::Colour(intent));
        match act {
            KeyAct::None => None,
            KeyAct::Close => colour(ColourIntent::Close),
            KeyAct::Press(at) => self.cells.get(at).cloned(),
            KeyAct::Apply => self
                .applies
                .then_some(WorkspaceCommand::Colour(ColourIntent::Apply)),
            KeyAct::Swap => colour(ColourIntent::Answer(true)),
            KeyAct::Cancel => colour(ColourIntent::Answer(false)),
        }
    }
}

/// What a drag on `key`'s piece asks, at `at` (its fraction of the box from
/// the left and the top): the square's or the strip's new place.
#[must_use]
pub fn dragged(key: &str, at: [f32; 2]) -> Option<WorkspaceCommand> {
    let share = |f: f32| (f.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let intent = match key {
        _ if key.ends_with("colour.square") => ColourIntent::Square {
            x: share(at[0]),
            y: share(at[1]),
        },
        _ if key.ends_with("colour.hue") => ColourIntent::Hue(share(at[0])),
        _ => return None,
    };
    Some(WorkspaceCommand::Colour(intent))
}

/// The ink that reads best on `rgb`: the chip's or the text's, as `dark`'s
/// bundled theme paints them.
fn ink_on(rgb: Rgb, dark: bool) -> Token {
    let theme = ResolvedTheme::bundled(dark);
    let fill = PremultipliedRgba::from_rgb(rgb[0], rgb[1], rgb[2]);
    let reads = |token| contrast_ratio(theme.color(token), fill);
    if reads(Token::ChipInk) >= reads(Token::TextPrimary) {
        Token::ChipInk
    } else {
        Token::TextPrimary
    }
}

/// Pure black or pure white, whichever reads more on `rgb`: a swatch's
/// initials and check, 4.58:1 at the least on any colour.
pub(crate) fn mono_ink(rgb: Rgb) -> Rgb {
    let fill = PremultipliedRgba::from_rgb(rgb[0], rgb[1], rgb[2]);
    let reads = |v: u8| contrast_ratio(PremultipliedRgba::from_rgb(v, v, v), fill);
    if reads(0) >= reads(255) {
        [0; 3]
    } else {
        [255; 3]
    }
}

/// The stripes over a locked swatch at `at`: Light's in its white control
/// ink at about 35%, Dark's in the support ink.
fn hatch(at: Area, dark: bool) -> Mark {
    match dark {
        true => Mark::Hatch {
            at,
            ink: Token::TextSupport,
            alpha: u8::MAX,
        },
        false => Mark::Hatch {
            at,
            ink: chrome(false).control,
            alpha: 89,
        },
    }
}

/// `rgb` under the veil a locked swatch wears: the ground at
/// [`VEIL_ALPHA`] over it, as `dark`'s bundled theme paints the ground.
pub(crate) fn veiled(rgb: Rgb, dark: bool) -> Rgb {
    let ground = ResolvedTheme::bundled(dark)
        .color(chrome(dark).ground)
        .to_array();
    let share = f32::from(VEIL_ALPHA) / 255.0;
    let mix = |at: usize| {
        (f32::from(ground[at]) * share + f32::from(rgb[at]) * (1.0 - share)).round() as u8
    };
    [mix(0), mix(1), mix(2)]
}

/// `mark`, words in `rgb` of their own.
fn tinted(mut mark: Mark, rgb: Rgb) -> Mark {
    if let Mark::Text(words) = &mut mark {
        words.rgb = Some(rgb);
    }
    mark
}

fn text(x: f32, y: f32, words: &str, size: f32, ink: Token, width: f32) -> Mark {
    Mark::Text(Words {
        x,
        y,
        text: words.to_owned(),
        size,
        ink,
        width,
        tracking: 0.0,
        names: false,
        rgb: None,
        on: None,
    })
}

fn plate(at: Area, radius: f32, fill: Option<Token>, edge: Option<Token>) -> Plate {
    Plate {
        toward: None,
        at,
        radius,
        fill,
        alpha: u8::MAX,
        edge,
    }
}

/// `at` shaded between `colours`: top left, top right, bottom right, bottom left.
fn shade(at: Area, colours: [Rgb; 4]) -> Mark {
    let (right, bottom) = (at.x + at.w, at.y + at.h);
    let corners = [[at.x, at.y], [right, at.y], [right, bottom], [at.x, bottom]];
    Mark::Shade([0, 1, 2, 3].map(|corner| (corners[corner], colours[corner])))
}

fn ring(at: Area, grow: f32, ink: Token) -> Mark {
    let at = area(
        at.x - grow,
        at.y - grow,
        at.w + 2.0 * grow,
        at.h + 2.0 * grow,
    );
    Mark::Box(plate(at, RADIUS + grow, None, Some(ink)))
}

/// A scene row under `colour.<key>`.
fn row(key: &str, label: &str, command: Option<ColourIntent>, call: &'static str) -> CellRow {
    CellRow {
        command: command.map(WorkspaceCommand::Colour),
        capability: Some(call),
        ..CellRow::new(RowKind::Press, &format!("colour.{key}"), label)
    }
}

/// `words` centred in `at`.
fn centred(
    measure: crate::workspace_menu::Measure<'_>,
    at: Area,
    words: &str,
    size: f32,
    ink: Token,
) -> Mark {
    let width = measure(words, size);
    text(
        at.x + (at.w - width) / 2.0,
        at.y + at.h / 2.0,
        words,
        size,
        ink,
        width + 1.0,
    )
}

/// One swatch at `at`: its colour, the current one ringed and checked, a
/// held one carrying its owner's initials, its name under it, its focus.
fn swatch(
    swatch: &Swatch,
    at: Area,
    focused: bool,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) -> Piece {
    let ink = chrome(dark);
    let rgb = swatch.colour.rgb(dark);
    let mut marks = vec![Mark::Colour {
        at,
        radius: RADIUS,
        rgb,
    }];
    // A locked swatch reads as its veiled colour: what its words stand on.
    let rgb = if swatch.locked {
        let mut veil = plate(at, RADIUS, Some(ink.ground), None);
        veil.alpha = VEIL_ALPHA;
        marks.extend([Mark::Box(veil), hatch(at, dark)]);
        veiled(rgb, dark)
    } else {
        rgb
    };
    if swatch.current {
        marks.extend([ring(at, 3.0, ink.ink), ring(at, 2.0, ink.ink)]);
        let check = centred(measure, at, CHECK, 12.0, ink.ink);
        marks.push(tinted(check, mono_ink(rgb)));
    } else if let Some(initials) = &swatch.initials {
        let initials = centred(measure, at, initials, NAME_PT, ink.ink);
        marks.push(tinted(initials, mono_ink(rgb)));
    }
    if focused {
        marks.push(ring(at, 2.0, ink.accent));
    }
    let named = if swatch.current { ink.ink } else { ink.support };
    let width = SWATCH[0] + GAP;
    marks.push(text(
        at.x,
        at.y + NAME_Y,
        &swatch.name,
        NAME_PT,
        named,
        width,
    ));
    let pick = ColourIntent::Pick(swatch.key.clone());
    Piece {
        at,
        row: Some(CellRow {
            summary: swatch.tip(),
            tip: Some(swatch.tip()),
            on: swatch.current,
            unavailable: swatch
                .locked
                .then_some("another_workspace_holds_this_colour"),
            colour: Some(RowColour::of(swatch.colour, dark)),
            ..row(&swatch.key, &swatch.name, Some(pick), SET)
        }),
        press: !swatch.locked,
        drag: false,
        double: false,
        hover: (!swatch.locked).then(|| {
            plate(
                area(at.x - 2.0, at.y - 2.0, at.w + 4.0, at.h + 4.0),
                RADIUS + 2.0,
                None,
                Some(ink.accent),
            )
        }),
        marks,
    }
}

/// The create dialogs' popover: [`palette::choosing`] on `chosen`, every
/// held colour locked.
#[must_use]
pub fn choosing(
    book: &WorkspaceBook,
    chosen: Option<WorkspaceColour>,
    state: &mut ColourPopover,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) -> Popover {
    popover_of(book, &palette::choosing(book, chosen), state, dark, measure)
}

/// A button at `at` worded `label`: filled for the Enter default, else the
/// menu's control; `enabled` or faded.
fn button(
    at: Area,
    row: CellRow,
    filled: bool,
    enabled: bool,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) -> Piece {
    let ink = chrome(dark);
    let primary = FilledButton::primary();
    let (fill, edge, words) = match (filled, enabled) {
        (true, true) => (primary.fill, primary.fill, primary.ink),
        (_, false) => (ink.control, ink.control, ink.faint), // No edge to read as a ring.
        (false, true) => (ink.control, ink.edge, ink.ink),
    };
    let back = plate(at, 4.0, Some(fill), Some(edge));
    let label = row.label.clone();
    Piece {
        at,
        hover: enabled.then(|| plate(at, 4.0, None, Some(ink.accent))),
        row: Some(row),
        press: enabled,
        drag: false,
        double: false,
        marks: vec![
            Mark::Box(back),
            centred(measure, at, &label, BODY_PT, words),
        ],
    }
}

/// The popover for workspace `name` in `book`, as `state` stands: the grid,
/// with Custom under it while it is open, or the swap question in its
/// place. `None` for a name the book lacks.
#[must_use]
pub fn popover(
    book: &WorkspaceBook,
    name: &str,
    state: &mut ColourPopover,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) -> Option<Popover> {
    Some(popover_of(
        book,
        &palette(book, name)?,
        state,
        dark,
        measure,
    ))
}

/// The popover of `model`, as `state` stands once settled.
fn popover_of(
    book: &WorkspaceBook,
    model: &Palette,
    state: &mut ColourPopover,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) -> Popover {
    settle(model, state, dark); // A freed question is left; the picker keeps its colour.
    if let Some(question) = question(model, state) {
        return asked(model, &question, state.focus, dark, measure);
    }
    let mut marks = grid(model, state, dark, measure);
    if state.custom {
        custom(&mut marks, book, model, state, dark, measure);
    }
    marks
}

/// The sixteen swatches four by four, then the seventeenth and Custom….
fn grid(
    model: &Palette,
    state: &ColourPopover,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) -> Popover {
    let mut pieces = Vec::new();
    let mut cells = Vec::new();
    let place = |at: usize| {
        let (row, column) = (at / 4, at % 4);
        area(
            PAD + column as f32 * (SWATCH[0] + GAP),
            PAD + row as f32 * PITCH,
            SWATCH[0],
            SWATCH[1],
        )
    };
    for (at, each) in model.swatches().enumerate() {
        pieces.push(swatch(
            each,
            place(at),
            state.focus == Some(at),
            dark,
            measure,
        ));
        cells.push(WorkspaceCommand::Colour(ColourIntent::Pick(
            each.key.clone(),
        )));
    }
    let open = cells.len();
    let custom_at = place(open);
    let label = "Custom\u{2026}";
    let at = area(
        custom_at.x,
        custom_at.y,
        measure(label, BODY_PT) + 16.0,
        SWATCH[1],
    );
    let intent = ColourIntent::Custom { dark };
    let mut custom = button(
        at,
        row("custom", label, Some(intent.clone()), POPOVER),
        false,
        true,
        dark,
        measure,
    );
    if state.custom || state.focus == Some(open) {
        custom
            .marks
            .push(Mark::Box(plate(at, 4.0, None, Some(chrome(dark).accent))));
    }
    pieces.push(custom);
    cells.push(WorkspaceCommand::Colour(intent));
    let current = model.swatches().position(|each| each.current);
    let height = PAD + 5.0 * PITCH - 7.0 + PAD;
    Popover {
        size: [WIDTH_PT, height],
        pieces,
        field: None,
        cells,
        current,
        applies: false,
    }
}

/// Custom under the grid: the square and the strip at the field's colour,
/// the field, Apply and Cancel, then one line per warning, or the hint.
fn custom(
    marks: &mut Popover,
    book: &WorkspaceBook,
    model: &Palette,
    state: &ColourPopover,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) {
    let ink = chrome(dark);
    let top = marks.size[1] - PAD + 4.0;
    let rule = plate(
        area(PAD, top, WIDTH_PT - 2.0 * PAD, 1.0),
        0.0,
        Some(ink.block_edge),
        None,
    );
    marks
        .pieces
        .push(Piece::plain(rule.at, vec![Mark::Box(rule)]));
    let [hue, saturation, value] = shown_hsv(state);
    let square = area(PAD, top + 12.0, WIDTH_PT - 2.0 * PAD, SQUARE_H);
    let mut shades = Vec::new();
    for strip in 0..STRIPS {
        let (from, to) = (
            strip as f32 / STRIPS as f32,
            (strip + 1) as f32 / STRIPS as f32,
        );
        let at = area(
            square.x + from * square.w,
            square.y,
            (to - from) * square.w,
            square.h,
        );
        let top = |s: f32| hsv_rgb([hue, s, 1.0]);
        shades.push(shade(at, [top(from), top(to), [0; 3], [0; 3]]));
    }
    let typed = parse_typed_hex(&state.hex);
    let shown = typed.unwrap_or_else(|| hsv_rgb([hue, saturation, value]));
    let handle = area(
        square.x + saturation * square.w - 5.0,
        square.y + (1.0 - value) * square.h - 5.0,
        10.0,
        10.0,
    );
    shades.push(Mark::Box(plate(
        handle,
        5.0,
        None,
        Some(ink_on(shown, dark)),
    )));
    let square_row = row("square", "Saturation and brightness", None, POPOVER);
    marks.pieces.push(Piece {
        drag: true,
        press: true,
        row: Some(square_row),
        ..Piece::plain(square, shades)
    });
    let strip = area(PAD, square.y + SQUARE_H + 8.0, square.w, STRIP_H);
    let mut hues = Vec::new();
    for sixth in 0..6 {
        let at = area(
            strip.x + sixth as f32 * strip.w / 6.0,
            strip.y,
            strip.w / 6.0,
            strip.h,
        );
        let (left, right) = (
            hsv_rgb([sixth as f32 / 6.0, 1.0, 1.0]),
            hsv_rgb([(sixth + 1) as f32 / 6.0, 1.0, 1.0]),
        );
        hues.push(shade(at, [left, right, right, left]));
    }
    let knob = area(
        strip.x + hue * strip.w - 2.0,
        strip.y - 2.0,
        4.0,
        strip.h + 4.0,
    );
    hues.push(Mark::Box(plate(
        knob,
        1.0,
        Some(Token::TextPrimary),
        Some(Token::ChipInk),
    )));
    marks.pieces.push(Piece {
        drag: true,
        press: true,
        row: Some(row("hue", "Hue", None, POPOVER)),
        ..Piece::plain(strip, hues)
    });
    let line = strip.y + strip.h + 10.0;
    let field = area(PAD, line, FIELD_W, CONTROL_H);
    let hex_row = row("hex", &state.hex, None, POPOVER);
    let boxed = plate(field, 4.0, Some(ink.control), Some(ink.accent)); // It holds the focus.
    marks.pieces.push(Piece {
        row: Some(hex_row),
        ..Piece::plain(field, vec![Mark::Box(boxed)])
    });
    marks.field = Some(area(
        field.x + 6.0,
        field.y + 4.0,
        field.w - 12.0,
        field.h - 8.0,
    ));
    let preview = area(field.x + field.w + 6.0, line, CONTROL_H, CONTROL_H);
    let chip = typed.map(|rgb| Mark::Colour {
        at: preview,
        radius: 4.0,
        rgb,
    });
    let edge = Mark::Box(plate(preview, 4.0, None, Some(ink.edge)));
    marks.pieces.push(Piece {
        row: Some(row("preview", &state.hex, None, POPOVER)),
        ..Piece::plain(preview, chip.into_iter().chain([edge]).collect())
    });
    let apply_label = "Apply";
    let apply_at = area(
        preview.x + preview.w + 8.0,
        line,
        measure(apply_label, BODY_PT) + 16.0,
        CONTROL_H,
    );
    // Creating, an identical colour blocks Apply and says whose it is.
    let blocked = typed.filter(|_| model.choosing).and_then(|rgb| {
        match model.pick(WorkspaceColour::Custom(rgb), true) {
            Pick::Refused(words) => Some(words),
            _ => None,
        }
    });
    let mut apply_row = row("apply", apply_label, Some(ColourIntent::Apply), SET);
    apply_row.unavailable = match (typed, &blocked) {
        (None, _) => Some("the_field_holds_no_colour"),
        (Some(_), Some(_)) => Some("another_workspace_holds_this_colour"),
        _ => None,
    };
    let applies = apply_row.unavailable.is_none();
    marks
        .pieces
        .push(button(apply_at, apply_row, true, applies, dark, measure));
    let cancel_label = "Cancel";
    let cancel_at = area(
        apply_at.x + apply_at.w + 4.0,
        line,
        measure(cancel_label, BODY_PT) + 16.0,
        CONTROL_H,
    );
    let fold = ColourIntent::Custom { dark };
    marks.pieces.push(button(
        cancel_at,
        row("cancel", cancel_label, Some(fold), POPOVER),
        false,
        true,
        dark,
        measure,
    ));
    let found: Vec<ColourWarning> = match (typed, &blocked) {
        (Some(rgb), None) => warnings(book, &model.workspace, WorkspaceColour::Custom(rgb)),
        _ => Vec::new(),
    };
    let mut y = line + CONTROL_H;
    let refusal = if typed.is_none() {
        Some(HEX_HINT.to_owned())
    } else {
        blocked
    };
    if let Some(words) = refusal {
        let at = area(PAD, y, WIDTH_PT - 2.0 * PAD, LINE_PT);
        let hint = text(PAD, y + LINE_PT / 2.0, &words, NAME_PT, Token::Warn, at.w);
        marks.pieces.push(Piece {
            row: Some(row("hint", &words, None, POPOVER)),
            ..Piece::plain(at, vec![hint])
        });
        y += LINE_PT;
    }
    for (index, warning) in found.iter().enumerate() {
        let words = warning.words();
        let at = area(PAD, y, WIDTH_PT - 2.0 * PAD, LINE_PT);
        let lines = vec![
            text(PAD, y + LINE_PT / 2.0, WARNING, NAME_PT, ink.ink, 14.0),
            text(
                PAD + 18.0,
                y + LINE_PT / 2.0,
                &words,
                NAME_PT,
                ink.ink,
                at.w - 18.0,
            ),
        ];
        let key = format!("warning.{index}");
        marks.pieces.push(Piece {
            row: Some(row(&key, &words, None, POPOVER)),
            ..Piece::plain(at, lines)
        });
        y += LINE_PT;
    }
    marks.size[1] = y + PAD;
    marks.applies = applies;
}

/// The swap question in the grid's place: its two lines, the two
/// workspaces with the colours a swap gives them, then Swap and Cancel.
fn asked(
    model: &Palette,
    question: &SwapQuestion,
    focus: Option<usize>,
    dark: bool,
    measure: crate::workspace_menu::Measure<'_>,
) -> Popover {
    let ink = chrome(dark);
    let words = question.words.as_str();
    let (first, second) = words.split_once("? ").map_or((words, ""), |(a, b)| (a, b));
    let first = format!("{first}?");
    let width = WIDTH_PT - 2.0 * PAD;
    let mut lines = vec![
        text(PAD, PAD + 9.0, &first, BODY_PT, ink.ink, width),
        text(PAD, PAD + 28.0, second, BODY_PT, ink.ink, width),
    ];
    let mut x = PAD;
    let after = [
        (question.colour, model.workspace.as_str()),
        (question.owner_gets, question.owner.as_str()),
    ];
    for (colour, name) in after {
        let chip = area(x, PAD + 44.0, 12.0, 12.0);
        lines.push(Mark::Colour {
            at: chip,
            radius: 2.0,
            rgb: colour.rgb(dark),
        });
        lines.push(text(
            x + 18.0,
            chip.y + 6.0,
            name,
            NAME_PT,
            ink.support,
            measure(name, NAME_PT) + 1.0,
        ));
        x += 18.0 + measure(name, NAME_PT) + 20.0;
    }
    let at = area(0.0, 0.0, WIDTH_PT, 60.0);
    let mut pieces = vec![Piece {
        row: Some(row("question", words, None, POPOVER)),
        ..Piece::plain(at, lines)
    }];
    let swap_at = area(PAD, PAD + 64.0, measure("Swap", BODY_PT) + 16.0, CONTROL_H);
    let swap = row("swap", "Swap", Some(ColourIntent::Answer(true)), SET);
    let mut swap = button(swap_at, swap, true, true, dark, measure);
    let cancel_at = area(
        swap_at.x + swap_at.w + 6.0,
        swap_at.y,
        measure("Cancel", BODY_PT) + 16.0,
        CONTROL_H,
    );
    let cancel = row(
        "cancel",
        "Cancel",
        Some(ColourIntent::Answer(false)),
        POPOVER,
    );
    let mut cancel = button(cancel_at, cancel, false, true, dark, measure);
    match focus {
        Some(0) => swap.marks.push(ring(swap_at, 2.0, ink.accent)),
        Some(_) => cancel.marks.push(ring(cancel_at, 2.0, ink.accent)),
        None => {}
    }
    pieces.extend([swap, cancel]);
    Popover {
        size: [WIDTH_PT, on_a_row(swap_at.y + CONTROL_H + PAD)],
        pieces,
        field: None,
        cells: Vec::new(),
        current: None,
        applies: false,
    }
}

/// The least height from `content` up at which the popover, hung under the
/// header swatch, ends on a boundary of the list rows behind it.
fn on_a_row(content: f32) -> f32 {
    use quantick_session::workspace_menu::{CHIP_MENU_BORDER_PT, CHIP_MENU_ROW_PT};
    let (frame, top) = (2.0 * CHIP_MENU_BORDER_PT, SWATCH_BOTTOM_PT + DROP_PT);
    let rows = ((top + content + frame - HEAD_PT) / CHIP_MENU_ROW_PT).ceil();
    HEAD_PT + rows * CHIP_MENU_ROW_PT - top - frame
}

#[cfg(test)]
#[path = "colour_palette_tests.rs"]
mod tests;
