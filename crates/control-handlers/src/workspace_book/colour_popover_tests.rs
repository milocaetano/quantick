//! The colour popover's presses and its two calls, on the mock's eight
//! workspaces with tab 1 on Flow: what a pick does, the question, Custom and
//! the palette read, all through the one path `workspace.colour.set` takes.

use quantick_control::wire::ActorContext;
use quantick_control_schema::workspace_colour::WorkspaceColourPaletteResult;
use quantick_session::colour_popover::ColourIntent;
use quantick_session::workspace_menu::{WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::palette::palette;
use quantick_stores::workspaces::{WorkspaceBook, WorkspaceColour, parse_book};
use serde_json::{Value, json};

use super::*;
use crate::tabs::TabDirectory;
use crate::test_support::{FakeAccess, FakeWindow};
use crate::workspace_book::{WorkspaceBookHost, apply_row_command};

struct Host {
    book: WorkspaceBook,
    menu: WorkspaceMenuState,
    writes: usize,
}

impl TabDirectory for Host {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (tab_id == 1).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        1
    }
}

impl WorkspaceBookHost for Host {
    fn workspace_book(&mut self) -> Option<&mut WorkspaceBook> {
        Some(&mut self.book)
    }
    fn workspace_book_ref(&self) -> Option<&WorkspaceBook> {
        Some(&self.book)
    }
    fn persist_workspace_book(&mut self) -> Result<(), String> {
        self.writes += 1;
        Ok(())
    }
    fn workspace_menu(&mut self) -> Option<&mut WorkspaceMenuState> {
        Some(&mut self.menu)
    }
}

const NAMES: [(&str, u8); 8] = [
    ("Default", 4),
    ("Flow", 3),
    ("Macro", 14),
    ("Night", 15),
    ("Opening", 0),
    ("Scalp", 2),
    ("Swing", 5),
    ("Tape", 1),
];

/// The mock's book, tab 1 on Flow with the menu and its popover open.
fn host() -> (Host, FakeAccess, ActorContext) {
    let mut text = String::from("version = 1\nlast_custom = \"#3A7BD5\"\n");
    for (name, _) in NAMES {
        text.push_str(&format!("[recipes.{name}]\nlayout = \"flow\"\n"));
    }
    text.push_str("[colours]\n");
    for (name, index) in NAMES {
        text.push_str(&format!("{name} = {index}\n"));
    }
    let mut book = parse_book(&text).unwrap();
    book.open_tab(1);
    book.set(1, "Flow").unwrap();
    let mut menu = WorkspaceMenuState::default();
    menu.toggle_chip_menu(1);
    menu.toggle_colour();
    let host = Host {
        book,
        menu,
        writes: 0,
    };
    (host, FakeAccess::with_budget(0), FakeWindow::assistant())
}

fn preset(index: u8) -> WorkspaceColour {
    WorkspaceColour::preset(index).unwrap()
}

/// A press in the popover, as the window carries it out.
fn press(
    host: &mut Host,
    access: &mut FakeAccess,
    actor: &ActorContext,
    intent: ColourIntent,
) -> Option<String> {
    let mut menu = std::mem::take(&mut host.menu);
    let command = WorkspaceCommand::Colour(intent);
    let note = apply_row_command(host, Some((access, actor)), &mut menu, 1, "WIN", command);
    host.menu = menu;
    note
}

/// A free colour applies at once through the colour handler, journaled,
/// and closes the popover; the menu stays open.
#[test]
fn a_free_colour_applies_at_once_and_closes_the_popover() {
    let (mut host, mut access, actor) = host();
    let note = press(
        &mut host,
        &mut access,
        &actor,
        ColourIntent::Pick("petrol".to_owned()),
    );
    assert_eq!(note, None);
    assert_eq!(host.book.colour("Flow"), Some(preset(6)));
    assert_eq!(host.writes, 1);
    assert_eq!(access.events.len(), 1, "journaled as workspace.colour_set");
    assert!(host.menu.colour.is_none() && host.menu.chip_menu_on(1));
    host.menu.toggle_colour();
    press(
        &mut host,
        &mut access,
        &actor,
        ColourIntent::Pick("last_custom".to_owned()),
    );
    assert_eq!(
        host.book.colour("Flow"),
        Some(WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]))
    );
}

/// A held colour asks first and changes nothing; Cancel changes nothing;
/// Swap reaches the colour handler with swap, closes the popover and tells
/// which workspace changed.
#[test]
fn a_held_colour_asks_and_swap_tells_which_workspace_changed() {
    let (mut host, mut access, actor) = host();
    press(
        &mut host,
        &mut access,
        &actor,
        ColourIntent::Pick("sky".to_owned()),
    );
    let popover = host.menu.colour.as_ref().unwrap();
    assert_eq!(popover.asked.as_deref(), Some("sky"));
    assert_eq!(
        (host.writes, host.book.colour("Flow")),
        (0, Some(preset(3)))
    );
    press(&mut host, &mut access, &actor, ColourIntent::Answer(false));
    assert_eq!(
        host.menu.colour.as_ref().unwrap().asked,
        None,
        "back to the grid"
    );
    assert_eq!(
        (host.writes, access.events.len()),
        (0, 0),
        "Cancel changes nothing"
    );
    press(
        &mut host,
        &mut access,
        &actor,
        ColourIntent::Pick("sky".to_owned()),
    );
    let note = press(&mut host, &mut access, &actor, ColourIntent::Answer(true));
    assert_eq!(note.as_deref(), Some("Scalp is now Violet"));
    assert_eq!(host.book.colour("Flow"), Some(preset(2)));
    assert_eq!(host.book.colour("Scalp"), Some(preset(3)));
    assert_eq!(access.events[0].payload["swapped_with"], json!("Scalp"));
    assert!(host.menu.colour.is_none() && host.menu.chip_menu_on(1));
}

/// Custom opens on the current colour as the theme paints it; the square
/// and the strip write the field; Apply takes the field's colour through the
/// same path, an identical one asks, and a malformed one does nothing.
#[test]
fn custom_applies_its_field_and_asks_on_an_identical_colour() {
    let (mut host, mut access, actor) = host();
    press(
        &mut host,
        &mut access,
        &actor,
        ColourIntent::Custom { dark: true },
    );
    let popover = host.menu.colour.as_ref().unwrap();
    assert!(popover.custom);
    assert_eq!(popover.hex, "#A887F2", "Violet as Dark paints it");
    press(
        &mut host,
        &mut access,
        &actor,
        ColourIntent::Square { x: 0, y: 0 },
    );
    assert_eq!(
        host.menu.colour.as_ref().unwrap().hex,
        "#FFFFFF",
        "the square's white corner"
    );
    press(
        &mut host,
        &mut access,
        &actor,
        ColourIntent::Square { x: 1000, y: 0 },
    );
    press(&mut host, &mut access, &actor, ColourIntent::Hue(0));
    assert_eq!(host.menu.colour.as_ref().unwrap().hex, "#FF0000");
    host.menu.colour.as_mut().unwrap().hex = "#3A7BD".to_owned();
    assert_eq!(
        press(&mut host, &mut access, &actor, ColourIntent::Apply),
        None
    );
    assert_eq!(host.writes, 0, "a malformed field applies nothing");
    host.menu.colour.as_mut().unwrap().hex = "5dbdeb".to_owned();
    press(&mut host, &mut access, &actor, ColourIntent::Apply);
    let popover = host.menu.colour.as_ref().unwrap();
    assert_eq!(
        popover.asked.as_deref(),
        Some("#5DBDEB"),
        "Sky's Dark value is Scalp's"
    );
    press(&mut host, &mut access, &actor, ColourIntent::Answer(false));
    assert!(
        host.menu.colour.as_ref().unwrap().custom,
        "Cancel returns to Custom"
    );
    host.menu.colour.as_mut().unwrap().hex = "#62B8EA".to_owned();
    press(&mut host, &mut access, &actor, ColourIntent::Apply);
    assert_eq!(
        host.book.colour("Flow"),
        Some(WorkspaceColour::Custom([0x62, 0xB8, 0xEA]))
    );
    assert_eq!(host.book.last_custom(), Some([0x62, 0xB8, 0xEA]));
    assert!(host.menu.colour.is_none());
}

fn call(
    host: &mut Host,
    access: &mut FakeAccess,
    actor: &ActorContext,
    id: &str,
    value: Value,
) -> Value {
    let handler = match id {
        "palette" => palette_read::<Host, FakeAccess>,
        _ => popover_call::<Host, FakeAccess>,
    };
    handler(host, access, actor, &value).unwrap()
}

/// `workspace.colour.palette` answers the model the popover is drawn from.
#[test]
fn the_palette_read_equals_the_popover_model() {
    let (mut host, mut access, actor) = host();
    let read = call(&mut host, &mut access, &actor, "palette", json!({}));
    let model = palette(&host.book, "Flow").unwrap();
    assert_eq!(read, json!(WorkspaceColourPaletteResult::from(&model)));
    assert_eq!(read["presets"][2]["initials"], json!("Sc"));
    assert_eq!(read["last_custom"]["key"], json!("last_custom"));
    assert_eq!(read["next_free"]["preset"], json!("petrol"));
    let scalp = call(
        &mut host,
        &mut access,
        &actor,
        "palette",
        json!({"name": "Scalp"}),
    );
    assert_eq!(scalp["workspace"], json!("Scalp"));
    let unknown =
        palette_read::<Host, FakeAccess>(&mut host, &mut access, &actor, &json!({"name": "Nope"}));
    assert!(unknown.is_err());
}

/// The popover call opens it, raises the question, reads Custom's
/// warnings and hint, and answers the question, as the presses do.
#[test]
fn the_popover_call_drives_every_face_without_a_hand() {
    let (mut host, mut access, actor) = host();
    host.menu = WorkspaceMenuState::default();
    let refused =
        popover_call::<Host, FakeAccess>(&mut host, &mut access, &actor, &json!({"act": "apply"}));
    assert!(refused.is_err(), "closed: only open acts");
    let open = call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "open"}),
    );
    assert_eq!(
        (open["open"].clone(), open["face"].clone()),
        (json!(true), json!("grid"))
    );
    assert!(host.menu.chip_menu_on(1), "the menu opens with it");
    let asked = call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "pick", "colour": "sky"}),
    );
    assert_eq!(asked["face"], json!("question"));
    assert_eq!(
        asked["question"],
        json!("Sky is used by Scalp. Swap? Scalp becomes Violet.")
    );
    call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "answer", "swap": false}),
    );
    let custom = call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "custom", "hex": "#62B8EA"}),
    );
    assert_eq!(custom["face"], json!("custom"));
    assert_eq!(
        custom["warnings"],
        json!([{"kind": "close", "workspace": "Scalp"}])
    );
    let bad = call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "custom", "hex": "#3A7BD"}),
    );
    assert_eq!(bad["hex_hint"], json!("Use #RRGGBB, for example #3A7BD5"));
    call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "custom", "hex": "#2BA597"}),
    );
    let applied = call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "apply"}),
    );
    assert_eq!(applied["open"], json!(false));
    assert_eq!(
        host.book.colour("Flow"),
        Some(WorkspaceColour::Custom([0x2B, 0xA5, 0x97]))
    );
    call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "open"}),
    );
    call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "pick", "colour": "sky"}),
    );
    let swapped = call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "answer", "swap": true}),
    );
    assert_eq!(swapped["note"], json!("Scalp is now Custom #2BA597"));
    call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "open"}),
    );
    let closed = call(
        &mut host,
        &mut access,
        &actor,
        "popover",
        json!({"act": "close"}),
    );
    assert_eq!(
        (closed["open"].clone(), closed["face"].clone()),
        (json!(false), json!("closed"))
    );
    assert!(host.menu.chip_menu_on(1), "the menu stays");
}

/// `workspace.colour.set` answers what its colour warns, and blocks none.
#[test]
fn the_colour_call_answers_its_warnings() {
    let (mut host, mut access, actor) = host();
    let value = json!({"name": "Flow", "colour": "#2BA597"});
    let set = crate::workspace_book::colour_set(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(set["warnings"], json!([{"kind": "near_buy"}]));
    assert_eq!(set["changed"], json!(true));
    let value = json!({"name": "Flow", "colour": "petrol"});
    let set = crate::workspace_book::colour_set(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(set["warnings"], json!([]));
}
