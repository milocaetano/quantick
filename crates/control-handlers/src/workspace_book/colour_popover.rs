//! What a press in a workspace's palette does, under the menu's header
//! swatch or under a Manage row, and the two calls that read the palette and
//! drive the header's popover without a hand. A pick reaches the same
//! handler `workspace.colour.set` uses, journaled alike; the palette, the
//! question's words and the warnings are
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

/// Where the picker stands: the last press on the square or the strip,
/// unless the field holds another valid colour, which moves it.
#[must_use]
pub fn shown_hsv(popover: &ColourPopover) -> [f32; 3] {
    match parse_typed_hex(&popover.hex) {
        Some(rgb) if hsv_rgb(popover.hsv) != rgb => rgb_hsv(rgb),
        _ => popover.hsv,
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

/// Carry out a press in the popover on the menu of `tab`, on the workspace
/// the tab is on; answers what to tell the trader.
pub fn act<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    host: &mut H,
    to: &mut Journal<'_, A>,
    menu: &mut WorkspaceMenuState,
    tab: u64,
    intent: ColourIntent,
) -> Option<String> {
    let book = host.workspace_book_ref()?;
    let workspace = state_of(book, tab).0.to_owned();
    act_on(host, to, &mut menu.colour, &workspace, intent)
}

/// Carry out a press in `open`, the palette of `workspace` (the menu's
/// header popover or a Manage row's): a free colour applies at once and
/// closes it, a held one asks to swap. Answers what to tell the trader.
pub fn act_on<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    host: &mut H,
    to: &mut Journal<'_, A>,
    open: &mut Option<ColourPopover>,
    workspace: &str,
    intent: ColourIntent,
) -> Option<String> {
    let model = palette(host.workspace_book_ref()?, workspace)?;
    if intent == ColourIntent::Toggle {
        *open = match open.take() {
            Some(_) => None,
            None => Some(ColourPopover::default()),
        };
        return None;
    }
    let popover = open.as_mut()?;
    let (word, typed, swap) = match intent {
        ColourIntent::Toggle | ColourIntent::Close => {
            *open = None;
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
        ColourIntent::Pick(word) => (word, false, false),
        ColourIntent::Apply => (hex(parse_typed_hex(&popover.hex)?), true, false),
        ColourIntent::Answer(true) => (popover.asked.clone()?, false, true),
    };
    let colour = named(&model, &word)?;
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
            *open = None;
            let swapped = outcome.swapped_with.zip(outcome.holder_colour);
            swapped.map(|(owner, now)| format!("{owner} is now {}", said(now)))
        }
        Err(error) => Some(error.message),
    }
}

/// A press on the hue strip (`hue`) or the square (`square`), in
/// thousandths: the picker moves there and the field follows.
pub(super) fn pressed(
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
    (popover.hsv, popover.hex) = (hsv, hex(hsv_rgb(hsv)));
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

/// `workspace.colour.popover`: the act on the tab on screen's popover.
pub(super) fn popover_call<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let asked: WorkspaceColourPopoverInput = input(value)?;
    let tab = app.tab_id_at(app.active_tab_index());
    let mut menu = std::mem::take(app.workspace_menu().ok_or_else(super::unavailable)?);
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
        _ if !open => {
            *app.workspace_menu().ok_or_else(super::unavailable)? = menu;
            return Err(closed());
        }
        WorkspaceColourPopoverInput::Close => Some(ColourIntent::Close),
        WorkspaceColourPopoverInput::Pick { colour } => Some(ColourIntent::Pick(colour)),
        WorkspaceColourPopoverInput::Custom { hex: typed } => {
            if !menu.colour.as_ref().is_some_and(|popover| popover.custom) {
                let custom = ColourIntent::Custom { dark: true };
                act(app, &mut to, &mut menu, tab, custom);
            }
            if let (Some(popover), Some(typed)) = (menu.colour.as_mut(), typed) {
                popover.hex = typed;
            }
            None
        }
        WorkspaceColourPopoverInput::Apply => Some(ColourIntent::Apply),
        WorkspaceColourPopoverInput::Answer { swap } => Some(ColourIntent::Answer(swap)),
    };
    let note = intent.and_then(|intent| act(app, &mut to, &mut menu, tab, intent));
    let book = app.workspace_book_ref().ok_or_else(super::unavailable)?;
    let result = read_back(book, &menu, tab, note);
    *app.workspace_menu().ok_or_else(super::unavailable)? = menu;
    answer(&result)
}

#[cfg(test)]
#[path = "colour_popover_tests.rs"]
mod tests;
