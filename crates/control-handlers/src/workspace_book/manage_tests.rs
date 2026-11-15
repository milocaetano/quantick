//! Manage workspaces' presses and the create dialogs' palette, on the mock's
//! eight workspaces with tab 1 on Flow and tabs 2 and 3 on Scalp, and the
//! calls that open the dialogs, save in a colour and undo a delete.

use quantick_control::wire::ActorContext;
use quantick_session::colour_popover::ColourIntent;
use quantick_session::manage_workspaces::ManageIntent;
use quantick_session::workspace_menu::{WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::{WorkspaceBook, WorkspaceColour, parse_book};
use serde_json::{Value, json};

use crate::tabs::TabDirectory;
use crate::test_support::{FakeAccess, FakeWindow};
use crate::workspace_book::{WorkspaceBookHost, apply_row_command};

struct Host {
    book: WorkspaceBook,
    menu: WorkspaceMenuState,
    writes: usize,
    now_ms: i64,
}

impl TabDirectory for Host {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (1..=3).contains(&tab_id).then(|| tab_id as usize - 1)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        index as u64 + 1
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
    fn tab_symbols(&self, tabs: &[u64]) -> Vec<String> {
        let symbol = |tab: &u64| ["WINZ26", "WIN$N", "WDO$N"][*tab as usize - 1].to_owned();
        tabs.iter().map(symbol).collect()
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

/// The mock's book: tab 1 on Flow, tabs 2 and 3 on Scalp; Manage open.
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
    for (tab, name) in [(1, "Flow"), (2, "Scalp"), (3, "Scalp")] {
        book.open_tab(tab);
        book.set(tab, name).unwrap();
    }
    let mut menu = WorkspaceMenuState::default();
    menu.open_manage(1);
    let host = Host {
        book,
        menu,
        writes: 0,
        now_ms: 1_000,
    };
    (host, FakeAccess::with_budget(0), FakeWindow::assistant())
}

fn preset(index: u8) -> WorkspaceColour {
    WorkspaceColour::preset(index).unwrap()
}

/// A command as the window carries it out.
fn press(
    host: &mut Host,
    access: &mut FakeAccess,
    actor: &ActorContext,
    command: WorkspaceCommand,
) -> Option<String> {
    let mut menu = std::mem::take(&mut host.menu);
    let note = apply_row_command(host, Some((access, actor)), &mut menu, 1, "WINZ26", command);
    host.menu = menu;
    note
}

fn manage(
    host: &mut Host,
    access: &mut FakeAccess,
    actor: &ActorContext,
    intent: ManageIntent,
) -> Option<String> {
    press(host, access, actor, WorkspaceCommand::Manage(intent))
}

fn call(
    host: &mut Host,
    access: &mut FakeAccess,
    actor: &ActorContext,
    handler: fn(
        &mut Host,
        &mut FakeAccess,
        &ActorContext,
        &Value,
    ) -> Result<Value, quantick_control::error::ControlError>,
    input: Value,
) -> Result<Value, String> {
    let actor = ActorContext {
        requested_at_unix_ms: host.now_ms,
        ..actor.clone()
    };
    handler(host, access, &actor, &input).map_err(|error| {
        let code = error
            .context
            .details
            .as_ref()
            .and_then(|d| d.get("code").cloned());
        code.map_or(error.message, |code| {
            code.as_str().unwrap_or_default().to_owned()
        })
    })
}

/// The pencil, then Enter, reaches `workspace.rename`; a refusal keeps the
/// field open with today's reason; Esc cancels and renames nothing.
#[test]
fn an_inline_rename_commits_refuses_and_cancels() {
    let (mut host, mut access, actor) = host();
    manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::BeginRename("Scalp".to_owned()),
    );
    host.menu
        .manage
        .as_mut()
        .unwrap()
        .rename
        .as_mut()
        .unwrap()
        .name = "Tape".to_owned();
    manage(&mut host, &mut access, &actor, ManageIntent::CommitRename);
    let rename = host.menu.manage.as_ref().unwrap().rename.as_ref().unwrap();
    assert_eq!(
        rename.refusal.as_deref(),
        Some("a workspace is already called Tape")
    );
    assert_eq!(host.writes, 0);
    host.menu
        .manage
        .as_mut()
        .unwrap()
        .rename
        .as_mut()
        .unwrap()
        .name = "Scalper".to_owned();
    let note = manage(&mut host, &mut access, &actor, ManageIntent::CommitRename);
    assert_eq!(note.as_deref(), Some("Scalp is now Scalper"));
    assert!(host.book.recipe("Scalper").is_some());
    assert_eq!(host.menu.manage.as_ref().unwrap().rename, None);
    assert_eq!(access.events.len(), 1, "journaled as workspace.renamed");
    manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::BeginRename("Tape".to_owned()),
    );
    manage(&mut host, &mut access, &actor, ManageIntent::Escape);
    assert_eq!(host.menu.manage.as_ref().unwrap().rename, None);
    assert!(
        host.menu.manage.is_some(),
        "Esc closed the field, not the dialog"
    );
    manage(&mut host, &mut access, &actor, ManageIntent::Escape);
    assert!(host.menu.manage.is_none(), "then the dialog");
}

/// Arm, Keep, arm again and Delete: the tabs go to Default, the row turns
/// into its Undo row naming them, and Undo inside ten seconds brings the
/// workspace, its colour and its tabs back.
#[test]
fn delete_in_the_row_then_undo_restores_the_workspace_and_its_tabs() {
    let (mut host, mut access, actor) = host();
    let before = host.book.clone();
    manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::Arm("Scalp".to_owned()),
    );
    manage(&mut host, &mut access, &actor, ManageIntent::Keep);
    assert_eq!(host.writes, 0, "Keep deletes nothing");
    manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::Arm("Scalp".to_owned()),
    );
    let toast = manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::Delete { now_ms: 1_000 },
    );
    assert!(toast.unwrap().starts_with("Deleted Scalp"));
    assert_eq!(host.book.tab_workspace(2), "Default");
    let dialog = host.menu.manage.as_ref().unwrap();
    let row = dialog.undo_row(1_000).unwrap();
    assert_eq!(
        row.words,
        "Scalp deleted \u{b7} WIN$N and WDO$N back on Default"
    );
    assert_eq!(row.until_ms, 11_000);
    assert_eq!(access.events.len(), 1, "journaled as workspace.deleted");
    let note = manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::Undo { now_ms: 10_999 },
    );
    assert_eq!(note.as_deref(), Some("Scalp is back"));
    assert_eq!(host.book.list(), before.list());
    assert_eq!(host.book.tab_state(2), before.tab_state(2));
    assert!(host.menu.manage.as_ref().unwrap().undo.is_empty());
    assert_eq!(access.events[1].kind.as_str(), "workspace.restored");
}

/// A chip opens that row's palette; a free colour applies through the
/// colour handler and closes it; a held one asks; Swap says which
/// workspace changed, in the footer and the toast.
#[test]
fn recolour_and_swap_in_manage_go_through_the_colour_handler() {
    let (mut host, mut access, actor) = host();
    manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::Chip("Tape".to_owned()),
    );
    let colour = |intent| WorkspaceCommand::Manage(ManageIntent::Colour(intent));
    press(
        &mut host,
        &mut access,
        &actor,
        colour(ColourIntent::Pick("petrol".to_owned())),
    );
    assert_eq!(host.book.colour("Tape"), Some(preset(6)));
    assert_eq!(
        host.menu.manage.as_ref().unwrap().palette,
        None,
        "closed at once"
    );
    manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::Chip("Flow".to_owned()),
    );
    press(
        &mut host,
        &mut access,
        &actor,
        colour(ColourIntent::Pick("sky".to_owned())),
    );
    let open = host.menu.manage.as_ref().unwrap().palette.as_ref().unwrap();
    assert_eq!(
        open.popover.asked.as_deref(),
        Some("sky"),
        "the swap question"
    );
    let note = press(
        &mut host,
        &mut access,
        &actor,
        colour(ColourIntent::Answer(true)),
    );
    assert_eq!(note.as_deref(), Some("Scalp is now Violet"));
    assert_eq!(
        host.menu.manage.as_ref().unwrap().note.as_deref(),
        Some("Scalp is now Violet")
    );
    assert_eq!(host.book.colour("Flow"), Some(preset(2)));
    assert_eq!(access.events.len(), 2, "two workspace.colour_set rows");
    manage(
        &mut host,
        &mut access,
        &actor,
        ManageIntent::Chip("Default".to_owned()),
    );
    press(
        &mut host,
        &mut access,
        &actor,
        colour(ColourIntent::Pick("lavender".to_owned())),
    );
    assert_eq!(
        host.book.colour("Default"),
        Some(preset(7)),
        "Default's colour changes"
    );
}

/// Save as opens on `Flow 2` and the next free colour (Petrol); a held
/// swatch is locked; a free one is chosen; Custom identical to a held colour
/// is refused at Save; Enter saves with the chosen colour.
#[test]
fn the_create_dialog_chooses_and_enter_saves_with_the_colour() {
    let (mut host, mut access, actor) = host();
    press(
        &mut host,
        &mut access,
        &actor,
        WorkspaceCommand::BeginSaveAs,
    );
    let dialog = host.menu.dialog.as_ref().unwrap();
    assert_eq!(dialog.name, "Flow 2");
    assert_eq!(dialog.colour.as_deref(), Some("petrol"));
    let colour = |intent| WorkspaceCommand::NameColour(intent);
    press(
        &mut host,
        &mut access,
        &actor,
        colour(ColourIntent::Pick("sky".to_owned())),
    );
    assert_eq!(
        host.menu.dialog.as_ref().unwrap().colour.as_deref(),
        Some("petrol"),
        "locked"
    );
    press(
        &mut host,
        &mut access,
        &actor,
        colour(ColourIntent::Pick("lavender".to_owned())),
    );
    assert_eq!(
        host.menu.dialog.as_ref().unwrap().colour.as_deref(),
        Some("lavender")
    );
    press(
        &mut host,
        &mut access,
        &actor,
        colour(ColourIntent::Custom { dark: true }),
    );
    host.menu.dialog.as_mut().unwrap().palette.hex = "#5DBDEB".to_owned();
    press(
        &mut host,
        &mut access,
        &actor,
        WorkspaceCommand::SubmitDialog,
    );
    let dialog = host.menu.dialog.as_ref().unwrap();
    assert_eq!(
        dialog.refusal.as_deref(),
        Some("Used by Scalp. Pick another colour.")
    );
    assert_eq!(host.writes, 0, "nothing saved");
    host.menu.dialog.as_mut().unwrap().palette.custom = false;
    let toast = press(
        &mut host,
        &mut access,
        &actor,
        WorkspaceCommand::SubmitDialog,
    );
    assert_eq!(
        toast.as_deref(),
        Some("WINZ26 now uses the new workspace Flow 2")
    );
    assert_eq!(host.book.colour("Flow 2"), Some(preset(7)));
    assert!(host.menu.dialog.is_none());
}

/// New workspace opens on `Workspace 9` and the next free colour, and
/// Enter makes it in that colour.
#[test]
fn new_workspace_suggests_its_name_and_saves_the_preselected_colour() {
    let (mut host, mut access, actor) = host();
    let recipe = quantick_stores::workspaces::WorkspaceRecipe {
        layout: quantick_workspace::arrangement_document::DeclaredLayout::TimeAndFlow,
        ..Default::default()
    };
    host.book.observe(1, &recipe).unwrap();
    press(&mut host, &mut access, &actor, WorkspaceCommand::BeginNew);
    assert_eq!(host.menu.dialog.as_ref().unwrap().name, "Workspace 9");
    let toast = press(
        &mut host,
        &mut access,
        &actor,
        WorkspaceCommand::SubmitDialog,
    );
    assert!(toast.unwrap().ends_with("Workspace 9"));
    assert_eq!(host.book.colour("Workspace 9"), Some(preset(6)), "Petrol");
}

/// `workspace.dialog.open` opens each dialog on the tab and names it; the
/// create dialogs answer their suggested name and colour; nothing is written.
#[test]
fn the_dialog_call_opens_each_dialog() {
    let (mut host, mut access, actor) = host();
    host.menu.close_manage();
    let open = |host: &mut Host, access: &mut FakeAccess, mode: &str| {
        call(
            host,
            access,
            &actor,
            super::dialog_open,
            json!({ "mode": mode }),
        )
        .unwrap()
    };
    let saved = open(&mut host, &mut access, "save_as");
    assert_eq!(saved["dialog"], "save_as");
    assert_eq!(saved["title"], "Save as new workspace");
    assert_eq!(saved["name"], "Flow 2");
    assert_eq!(saved["colour"]["preset"], "petrol");
    assert!(host.menu.dialog.is_some());
    let new = open(&mut host, &mut access, "new");
    assert_eq!(
        (new["title"].as_str(), new["name"].as_str()),
        (Some("New workspace"), Some("Workspace 9"))
    );
    let managed = open(&mut host, &mut access, "manage");
    assert_eq!(managed["dialog"], "manage");
    assert!(managed["name"].is_null());
    assert!(host.menu.manage.is_some());
    assert_eq!(host.writes, 0, "nothing written");
    assert_eq!(access.events.len(), 3, "workspace.dialog_opened each");
    let stranger = call(
        &mut host,
        &mut access,
        &actor,
        super::dialog_open,
        json!({"mode": "manage", "tab_id": "9"}),
    );
    assert_eq!(stranger.unwrap_err(), "workspace.unknown_tab");
}

/// `workspace.save_as` and `workspace.new` take a colour; without one the
/// next free; a held one is refused with `workspace.colour_taken`, unswapped.
#[test]
fn save_as_and_new_take_a_colour_and_refuse_a_held_one() {
    let (mut host, mut access, actor) = host();
    let save_as = super::super::save_as::<Host, FakeAccess>;
    let made = call(
        &mut host,
        &mut access,
        &actor,
        save_as,
        json!({"name": "Flow 2", "colour": "lavender"}),
    );
    assert_eq!(made.unwrap()["workspace"], "Flow 2");
    assert_eq!(host.book.colour("Flow 2"), Some(preset(7)));
    let held = call(
        &mut host,
        &mut access,
        &actor,
        save_as,
        json!({"name": "Flow 3", "colour": "sky"}),
    );
    assert_eq!(held.unwrap_err(), "workspace.colour_taken");
    assert_eq!(
        host.book.colour("Scalp"),
        Some(preset(2)),
        "no swap on create"
    );
    assert!(host.book.recipe("Flow 3").is_none());
    let bad = call(
        &mut host,
        &mut access,
        &actor,
        save_as,
        json!({"name": "Flow 3", "colour": "gold"}),
    );
    assert_eq!(bad.unwrap_err(), "workspace.colour_invalid");
    call(
        &mut host,
        &mut access,
        &actor,
        save_as,
        json!({"name": "Flow 3"}),
    )
    .unwrap();
    assert_eq!(
        host.book.colour("Flow 3"),
        Some(preset(6)),
        "the next free: Petrol"
    );
    let new = super::super::new::<Host, FakeAccess>;
    let taken = call(
        &mut host,
        &mut access,
        &actor,
        new,
        json!({"name": "Blank", "colour": "#5DBDEB"}),
    );
    assert_eq!(taken.unwrap_err(), "workspace.colour_taken");
}

/// `workspace.delete` answers until when it can be undone and, with Manage
/// open, leaves the Undo row; `workspace.delete.undo` brings it back inside
/// the ten seconds, is refused after, and a repeat answers no change.
#[test]
fn the_undo_call_restores_inside_ten_seconds_and_refuses_after() {
    let (mut host, mut access, actor) = host();
    let delete = super::super::delete::<Host, FakeAccess>;
    let undo = super::undo_call::<Host, FakeAccess>;
    let gone = call(
        &mut host,
        &mut access,
        &actor,
        delete,
        json!({"name": "Scalp"}),
    )
    .unwrap();
    assert_eq!(gone["undo_until_unix_ms"], 11_000);
    assert!(host.menu.manage.as_ref().unwrap().undo_row(1_000).is_some());
    host.now_ms = 10_999;
    let back = call(
        &mut host,
        &mut access,
        &actor,
        undo,
        json!({"name": "Scalp"}),
    )
    .unwrap();
    assert_eq!(
        (back["changed"].as_bool(), back["recoloured"].as_bool()),
        (Some(true), Some(false))
    );
    assert_eq!(back["tabs"], json!(["2", "3"]));
    assert!(host.menu.manage.as_ref().unwrap().undo.is_empty());
    let again = call(
        &mut host,
        &mut access,
        &actor,
        undo,
        json!({"name": "Scalp"}),
    )
    .unwrap();
    assert_eq!(again["changed"], false);
    call(
        &mut host,
        &mut access,
        &actor,
        delete,
        json!({"name": "Tape"}),
    )
    .unwrap();
    host.now_ms += 10_000;
    let late = call(
        &mut host,
        &mut access,
        &actor,
        undo,
        json!({"name": "Tape"}),
    );
    assert_eq!(late.unwrap_err(), "workspace.undo_expired");
    let never = call(
        &mut host,
        &mut access,
        &actor,
        undo,
        json!({"name": "Macro"}),
    );
    assert_eq!(never.unwrap_err(), "workspace.unknown");
}
