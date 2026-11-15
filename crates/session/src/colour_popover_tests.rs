//! The colour popover's state: its faces, its focus and what a key does.
use super::*;
use crate::workspace_menu::{WorkspaceCommand, WorkspaceMenuState};

fn keys(pressed: &[Key]) -> Keys {
    let mut keys = Keys::default();
    for key in pressed {
        keys.press(*key);
    }
    keys
}

/// Sixteen presets, the seventeenth swatch and Custom… make eighteen cells
/// in a four-wide grid: arrows move over it and stop at its edges, the last
/// row's two cells under the first two columns.
#[test]
fn arrows_move_over_the_four_wide_grid_and_stop_at_its_edges() {
    let mut popover = ColourPopover::default();
    let mut press = |key| popover.keys(keys(&[key]), 18, Some(3));
    assert_eq!(press(Key::Right), KeyAct::None);
    assert_eq!(popover.focus, Some(3), "the first arrow shows the current swatch");
    let mut step = |key| {
        popover.keys(keys(&[key]), 18, Some(3));
        popover.focus
    };
    assert_eq!(step(Key::Right), Some(3), "the row's end");
    assert_eq!(step(Key::Down), Some(7));
    assert_eq!(step(Key::Down), Some(11));
    assert_eq!(step(Key::Down), Some(15));
    assert_eq!(step(Key::Down), Some(17), "under the second column: Custom…");
    assert_eq!(step(Key::Down), Some(17), "the grid's bottom");
    assert_eq!(step(Key::Left), Some(16), "the seventeenth swatch");
    assert_eq!(step(Key::Up), Some(12));
    assert_eq!(step(Key::Left), Some(12), "the row's start");
    let mut short = ColourPopover::default();
    short.keys(keys(&[Key::Down]), 17, None);
    assert_eq!(short.focus, Some(0), "no current swatch: the first");
    for _ in 0..5 {
        short.keys(keys(&[Key::Down]), 17, None);
    }
    assert_eq!(short.focus, Some(16), "without a seventeenth, Custom… is first");
}

/// Enter and Space press the focused cell; nothing is pressed before an
/// arrow moved the focus; Esc closes the popover from any face.
#[test]
fn enter_presses_the_focused_cell_and_esc_closes() {
    let mut popover = ColourPopover::default();
    assert_eq!(popover.keys(keys(&[Key::Enter]), 18, Some(3)), KeyAct::None);
    popover.keys(keys(&[Key::Down]), 18, Some(3));
    assert_eq!(popover.keys(keys(&[Key::Enter]), 18, Some(3)), KeyAct::Press(3));
    assert_eq!(popover.keys(keys(&[Key::Space]), 18, Some(3)), KeyAct::Press(3));
    assert_eq!(popover.keys(keys(&[Key::Escape]), 18, Some(3)), KeyAct::Close);
    popover.custom = true;
    assert_eq!(popover.keys(keys(&[Key::Enter]), 18, Some(3)), KeyAct::Apply, "Apply is Enter's");
    assert_eq!(popover.keys(keys(&[Key::Escape]), 18, Some(3)), KeyAct::Close);
}

/// The swap question: Enter alone never swaps, nor answers at all; Tab
/// reaches Swap, then Cancel, and Enter or Space presses the one reached.
#[test]
fn the_swap_question_takes_no_reflex_enter() {
    let mut popover = ColourPopover::default();
    popover.ask("sky");
    assert_eq!(popover.asked.as_deref(), Some("sky"));
    assert_eq!(popover.focus, None);
    assert_eq!(popover.keys(keys(&[Key::Enter]), 18, Some(3)), KeyAct::None);
    assert_eq!(popover.keys(keys(&[Key::Space]), 18, Some(3)), KeyAct::None);
    assert_eq!(popover.keys(keys(&[Key::Down]), 18, Some(3)), KeyAct::None);
    assert_eq!(popover.focus, None, "arrows do not reach the buttons");
    popover.keys(keys(&[Key::Tab]), 18, Some(3));
    assert_eq!(popover.keys(keys(&[Key::Enter]), 18, Some(3)), KeyAct::Swap);
    popover.keys(keys(&[Key::Tab]), 18, Some(3));
    assert_eq!(popover.keys(keys(&[Key::Space]), 18, Some(3)), KeyAct::Cancel);
    assert_eq!(popover.keys(keys(&[Key::Escape]), 18, Some(3)), KeyAct::Close);
    popover.back();
    assert_eq!((popover.asked.as_deref(), popover.focus), (None, None));
}

/// The header swatch opens the popover over the open menu and closes it
/// again; reopening the menu forgets it.
#[test]
fn the_header_swatch_toggles_the_popover_over_the_open_menu() {
    let mut menu = WorkspaceMenuState::default();
    menu.toggle_chip_menu(7);
    menu.toggle_colour();
    assert!(menu.colour.is_some());
    menu.toggle_colour();
    assert!(menu.colour.is_none(), "a second click closes it");
    menu.toggle_colour();
    menu.toggle_chip_menu(7);
    menu.toggle_chip_menu(7);
    assert!(menu.colour.is_none(), "a new opening starts closed");
}

/// Esc and a click outside close only the popover while it is open; a
/// colour press keeps the menu; without the popover any press, Esc or a
/// click outside closes the menu as before.
#[test]
fn esc_and_a_click_outside_close_only_the_popover() {
    let colour = Some(WorkspaceCommand::Colour(ColourIntent::Pick("sky".to_owned())));
    let mut menu = WorkspaceMenuState::default();
    menu.toggle_chip_menu(7);
    menu.toggle_colour();
    menu.settle(None, true, false);
    assert!(menu.chip_menu_on(7) && menu.colour.is_none(), "Esc");
    menu.toggle_colour();
    menu.settle(None, false, true);
    assert!(menu.chip_menu_on(7) && menu.colour.is_none(), "a click outside");
    menu.toggle_colour();
    menu.settle(Some(&colour), false, false);
    assert!(menu.chip_menu_on(7) && menu.colour.is_some(), "a colour press");
    menu.toggle_colour();
    menu.settle(Some(&colour), false, false);
    assert!(menu.chip_menu_on(7), "a colour press never closes the menu");
    menu.settle(Some(&Some(WorkspaceCommand::SaveTo)), false, false);
    assert!(!menu.chip_menu_on(7), "any other press closes it");
    menu.toggle_chip_menu(7);
    menu.settle(None, true, false);
    assert!(!menu.chip_menu_on(7), "Esc closes the menu");
}

/// Custom opens under the grid on the field's words; the question from its
/// Apply goes back to it.
#[test]
fn custom_opens_on_its_words_and_the_question_returns_to_it() {
    let mut popover = ColourPopover::default();
    popover.open_custom("#A887F2".to_owned(), [0.7, 0.4, 0.9]);
    assert!(popover.custom);
    assert_eq!((popover.hex.as_str(), popover.hsv), ("#A887F2", [0.7, 0.4, 0.9]));
    popover.ask("#5DBDEB");
    popover.back();
    assert!(popover.custom, "Cancel goes back to Custom");
    popover.open_custom("#000000".to_owned(), [0.0; 3]);
    assert!(!popover.custom, "Custom… again folds it away");
}
