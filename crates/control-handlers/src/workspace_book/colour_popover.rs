//! What a press in the header swatch's palette popover does, and the two
//! calls that read the palette and drive the popover without a hand. A pick
//! reaches the same handler `workspace.colour.set` uses, journaled alike;
//! the palette, the question's words and the warnings are
//! `quantick_stores::workspaces::palette`'s.

use quantick_control::error::ControlError;
use quantick_control::wire::ActorContext;
use quantick_control_schema::workspace_colour::{
    ColourWarningDto, POPOVER_CLOSED_CODE, WorkspaceColourPaletteInput,
    WorkspaceColourPaletteResult, WorkspaceColourPopoverInput, WorkspaceColourPopoverResult,
};
use quantick_session::colour_popover::{ColourIntent, ColourPopover};
use quantick_session::workspace_menu::WorkspaceMenuState;
use quantick_stores::workspaces::palette::{
    HEX_HINT, Palette, Pick, SwapQuestion, palette, said, warnings,
};
use quantick_stores::workspaces::{
    WorkspaceBook, WorkspaceColour, hex, hsv_rgb, parse_typed_hex, rgb_hsv,
};
use serde_json::{Value, json};

use super::cell_menu::state_of;
use super::{Journal, WorkspaceBookHost, answer, colour_journaled, input};
use crate::notify::NotifyAccess;
use crate::tabs::TabDirectory;

/// Where the picker stands: the field's colour while it holds a valid one
/// the picker does not already stand on, else the last valid colour.
#[must_use]
pub fn shown_hsv(popover: &ColourPopover) -> [f32; 3] {
    match (parse_typed_hex(&popover.hex), popover.hsv) {
        (Some(rgb), Some(hsv)) if hsv_rgb(hsv) == rgb => hsv,
        (Some(rgb), _) => rgb_hsv(rgb),
        (None, hsv) => hsv.unwrap_or_default(),
    }
}

/// Bring `popover` in step with the book and its field, wherever it is read:
/// a question whose colour is no longer held is left for the grid, and the
/// picker keeps the last valid colour (the current one as `dark` paints it,
/// before any).
pub fn settle(model: &Palette, popover: &mut ColourPopover, dark: bool) {
    leave_freed(model, popover);
    let known = parse_typed_hex(&popover.hex).is_some() || popover.hsv.is_some();
    let current = rgb_hsv(model.current.rgb(dark));
    popover.hsv = Some(if known { shown_hsv(popover) } else { current });
}

/// Leave a question whose colour is no longer held, for the grid.
pub fn leave_freed(model: &Palette, popover: &mut ColourPopover) {
    if popover.asked.is_some() && question(model, popover).is_none() {
        popover.back();
    }
}

/// The colour a swatch's or the question's word names: a swatch key, a
/// preset's name or a hex.
fn named(model: &Palette, word: &str) -> Option<WorkspaceColour> {
    let swatch = model.swatch(word).map(|swatch| swatch.colour);
    swatch.or_else(|| WorkspaceColour::parse(word))
}

/// The swap question standing in `popover`.
#[must_use]
pub fn question(model: &Palette, popover: &ColourPopover) -> Option<SwapQuestion> {
    let word = popover.asked.as_deref()?;
    match model.pick(named(model, word)?, word.starts_with('#')) {
        Pick::Ask(question) => Some(question),
        _ => None,
    }
}

/// The palette of the workspace `tab` is on, as the book stands.
fn model_of<H: WorkspaceBookHost + ?Sized>(host: &H, tab: u64) -> Option<Palette> {
    let book = host.workspace_book_ref()?;
    palette(book, state_of(book, tab).0)
}

/// Carry out a press in the popover on the menu of `tab`; answers what to
/// tell the trader.
pub fn act<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    host: &mut H,
    to: &mut Journal<'_, A>,
    menu: &mut WorkspaceMenuState,
    tab: u64,
    intent: ColourIntent,
) -> Option<String> {
    let model = model_of(host, tab)?;
    if intent == ColourIntent::Toggle {
        menu.toggle_colour();
        return None;
    }
    let popover = menu.colour.as_mut()?;
    leave_freed(&model, popover);
    let (colour, word, typed, swap) = match intent {
        ColourIntent::Toggle | ColourIntent::Close => {
            menu.colour = None;
            return None;
        }
        ColourIntent::Custom { dark } => {
            let rgb = model.current.rgb(dark);
            popover.open_custom(hex(rgb), rgb_hsv(rgb));
            return None;
        }
        ColourIntent::Square { x, y } => return pressed(popover, None, Some((x, y))),
        ColourIntent::Hue(x) => return pressed(popover, Some(x), None),
        ColourIntent::Answer(false) => {
            popover.back();
            return None;
        }
        ColourIntent::Pick(word) => (named(&model, &word)?, word, false, false),
        ColourIntent::Apply => {
            let rgb = parse_typed_hex(&popover.hex)?;
            (WorkspaceColour::Custom(rgb), hex(rgb), true, false)
        }
        ColourIntent::Answer(true) => {
            let asked = question(&model, popover)?.colour; // None once it freed.
            (asked, String::new(), false, true)
        }
    };
    if !swap {
        match model.pick(colour, typed) {
            Pick::Apply(_) => {}
            Pick::Ask(_) => {
                popover.ask(&word);
                return None;
            }
            Pick::Refused(reason) => return Some(reason),
        }
    }
    match colour_journaled(host, to, &model.workspace, (colour, swap)) {
        Ok(outcome) => {
            menu.colour = None;
            let swapped = outcome.swapped_with.zip(outcome.holder_colour);
            swapped.map(|(owner, now)| format!("{owner} is now {}", said(now)))
        }
        Err(error) => Some(error.message),
    }
}

/// A press on the hue strip (`hue`) or the square (`square`), in
/// thousandths: the picker moves there and the field follows.
fn pressed(
    popover: &mut ColourPopover,
    hue: Option<u16>,
    square: Option<(u16, u16)>,
) -> Option<String> {
    let share = |at: u16| f32::from(at.min(1000)) / 1000.0;
    let mut hsv = shown_hsv(popover);
    if let Some(at) = hue {
        hsv[0] = share(at);
    }
    if let Some((x, y)) = square {
        (hsv[1], hsv[2]) = (share(x), 1.0 - share(y));
    }
    (popover.hsv, popover.hex) = (Some(hsv), hex(hsv_rgb(hsv)));
    None
}

/// The popover as the call answers it.
fn read_back(
    book: &WorkspaceBook,
    menu: &WorkspaceMenuState,
    tab: u64,
    note: Option<String>,
) -> WorkspaceColourPopoverResult {
    let workspace = state_of(book, tab).0.to_owned();
    let model = palette(book, &workspace);
    let popover = menu.colour.as_ref().filter(|_| menu.chip_menu_on(tab));
    let asked = popover
        .zip(model.as_ref())
        .and_then(|(p, m)| question(m, p));
    let asked = asked.map(|question| question.words);
    let custom = popover.filter(|popover| popover.custom);
    let typed = custom.and_then(|popover| parse_typed_hex(&popover.hex));
    let found = typed.map(|rgb| warnings(book, &workspace, WorkspaceColour::Custom(rgb)));
    let face = match (popover, &asked, custom) {
        (None, ..) => "closed",
        (Some(_), Some(_), _) => "question",
        (Some(_), None, Some(_)) => "custom",
        (Some(_), None, None) => "grid",
    };
    WorkspaceColourPopoverResult {
        open: popover.is_some(),
        workspace,
        face: face.to_owned(),
        question: asked,
        hex: custom.map(|popover| popover.hex.clone()),
        hex_hint: custom
            .filter(|_| typed.is_none())
            .map(|_| HEX_HINT.to_owned()),
        warnings: found.iter().flatten().map(ColourWarningDto::from).collect(),
        note,
    }
}

fn closed() -> ControlError {
    let mut error = ControlError::invalid_request("the colour popover is closed: open it first");
    error.context.details = Some(json!({ "code": POPOVER_CLOSED_CODE }));
    error
}

/// `workspace.colour.palette`.
pub(super) fn palette_read<H: WorkspaceBookHost + TabDirectory + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceColourPaletteInput { name } = input(value)?;
    let tab = app.tab_id_at(app.active_tab_index());
    let book = app.workspace_book_ref().ok_or_else(super::unavailable)?;
    let name = name.unwrap_or_else(|| state_of(book, tab).0.to_owned());
    let unknown = || ControlError::invalid_request(format!("no workspace is called {name:?}"));
    let model = palette(book, &name).ok_or_else(unknown)?;
    answer(&WorkspaceColourPaletteResult::from(&model))
}

/// `workspace.colour.popover`: the act on the tab on screen's popover. The
/// menu's state is put back whatever the act answers.
pub(super) fn popover_call<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let asked: WorkspaceColourPopoverInput = input(value)?;
    let tab = app.tab_id_at(app.active_tab_index());
    let (held, dark) = app.workspace_menu().ok_or_else(super::unavailable)?;
    let mut menu = std::mem::take(held);
    let result = drive(app, (access, actor, dark), &mut menu, tab, asked);
    if let Some((held, _)) = app.workspace_menu() {
        *held = menu;
    }
    answer(&result?)
}

/// One act on `menu`, taken out of the window, and the popover after it.
fn drive<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    (access, actor, dark): (&mut A, &ActorContext, bool),
    menu: &mut WorkspaceMenuState,
    tab: u64,
    asked: WorkspaceColourPopoverInput,
) -> Result<WorkspaceColourPopoverResult, ControlError> {
    let mut to = Some((access, actor));
    let open = menu.chip_menu_on(tab) && menu.colour.is_some();
    let intent = match asked {
        WorkspaceColourPopoverInput::Open => {
            if !menu.chip_menu_on(tab) {
                menu.toggle_chip_menu(tab);
            }
            if menu.colour.is_none() {
                menu.toggle_colour();
            }
            None
        }
        _ if !open => return Err(closed()),
        WorkspaceColourPopoverInput::Close => Some(ColourIntent::Close),
        WorkspaceColourPopoverInput::Pick { colour } => Some(ColourIntent::Pick(colour)),
        WorkspaceColourPopoverInput::Custom { hex: typed } => {
            if !menu.colour.as_ref().is_some_and(|popover| popover.custom) {
                act(app, &mut to, menu, tab, ColourIntent::Custom { dark });
            }
            if let (Some(popover), Some(typed)) = (menu.colour.as_mut(), typed) {
                popover.hex = typed;
            }
            None
        }
        WorkspaceColourPopoverInput::Apply => Some(ColourIntent::Apply),
        WorkspaceColourPopoverInput::Answer { swap } => Some(ColourIntent::Answer(swap)),
    };
    let note = intent.and_then(|intent| act(app, &mut to, menu, tab, intent));
    let book = app.workspace_book_ref().ok_or_else(super::unavailable)?;
    if let (Some(model), Some(popover)) = (palette(book, state_of(book, tab).0), &mut menu.colour) {
        settle(&model, popover, dark);
    }
    Ok(read_back(book, menu, tab, note))
}

#[cfg(test)]
#[path = "colour_popover_tests.rs"]
mod tests;
