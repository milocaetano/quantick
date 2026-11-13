//! The menu below the capsule's workspace name, apart from its pixels: which
//! rows it shows and in what order, their words, what a press on each asks
//! and the call that does the same, so the window only draws and the scene
//! names every row. The size it keeps to is
//! `quantick_session::workspace_menu::chip_menu_budget`'s.

use quantick_control_schema::window_menu::{RowId, row_of};
use quantick_control_schema::workspace_book as calls;
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_stores::workspaces::{DEFAULT_WORKSPACE, WorkspaceBook};

/// The open menu's id among the toolbar's scene controls; a row's is
/// `<this>.<key>`.
pub const SCENE_ID: &str = "workspace_menu";
/// The call that reads what the menu lists, named by the menu and by the
/// capsule's workspace name that opens it.
pub const MENU_CAPABILITY_ID: &str = calls::WORKSPACE_LIST_CAPABILITY_ID;

/// The scene id of the row under `key`, or of the menu for an empty one.
#[must_use]
pub fn scene_id(key: &str) -> String {
    if key.is_empty() {
        SCENE_ID.to_owned()
    } else {
        format!("{SCENE_ID}.{key}")
    }
}

/// How a row draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Separator,
    /// A line of supporting text.
    Caption,
    /// A tick box.
    Switch,
    /// A button, with its supporting text beside it.
    Press,
}

/// One row of the menu.
#[derive(Debug, Clone, PartialEq)]
pub struct CellRow {
    pub kind: RowKind,
    /// Its scene key; empty for a row the scene does not list.
    pub key: String,
    pub label: String,
    /// The supporting text at the row's right end.
    pub beside: String,
    /// Ticked, or lit as the tab's own.
    pub on: bool,
    pub enabled: bool,
    /// The unsaved dot, inside the row before its text.
    pub dot: bool,
    pub hover: Option<&'static str>,
    pub disabled_hover: Option<&'static str>,
    /// What a press asks.
    pub command: Option<WorkspaceCommand>,
    /// The call that does what a press does.
    pub capability: Option<&'static str>,
}

impl CellRow {
    fn new(kind: RowKind, key: &str, label: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.to_owned(),
            label: label.into(),
            beside: String::new(),
            on: false,
            enabled: true,
            dot: false,
            hover: None,
            disabled_hover: None,
            command: None,
            capability: None,
        }
    }

    fn line(kind: RowKind, label: impl Into<String>) -> Self {
        Self::new(kind, "", label)
    }

    fn press(key: &str, id: RowId, command: WorkspaceCommand, call: &'static str) -> Self {
        let row = row_of(id);
        Self {
            hover: row.hover,
            disabled_hover: row.disabled_hover,
            command: Some(command),
            capability: Some(call),
            ..Self::new(RowKind::Press, key, row.label)
        }
    }

    /// The open menu itself, as the scene lists it.
    #[must_use]
    pub fn menu() -> Self {
        Self {
            on: true,
            capability: Some(MENU_CAPABILITY_ID),
            ..Self::line(RowKind::Caption, "Workspace menu")
        }
    }

    /// The row's id among the toolbar's scene controls.
    #[must_use]
    pub fn scene_id(&self) -> String {
        scene_id(&self.key)
    }
}

/// The menu's rows: the fixed ones above and below the workspaces, which
/// alone scroll.
#[derive(Debug, Clone, PartialEq)]
pub struct CellMenu {
    pub head: Vec<CellRow>,
    pub list: Vec<CellRow>,
    pub foot: Vec<CellRow>,
}

/// The workspace `tab` is on, and whether it holds edits.
fn state_of(book: &WorkspaceBook, tab: u64) -> (&str, bool) {
    book.tab_state(tab)
        .map_or((DEFAULT_WORKSPACE, false), |state| {
            (state.workspace, state.modified)
        })
}

/// The menu as `book` stands for `tab`, `keys` being the save shortcut's
/// words (designer 01): auto save and its line, Save, Save as, Rename, the
/// workspaces with their one-line summaries, New and Manage.
#[must_use]
pub fn cell_menu(book: &WorkspaceBook, tab: u64, keys: &str) -> CellMenu {
    let (workspace, _) = state_of(book, tab);
    let list = (book.list().into_iter())
        .map(|row| CellRow {
            beside: book.summary(&row.name),
            on: row.name == workspace,
            command: Some(WorkspaceCommand::Set(row.name.clone())),
            capability: Some(calls::TAB_WORKSPACE_SET_CAPABILITY_ID),
            ..CellRow::new(
                RowKind::Press,
                &format!("workspace.{}", row.name),
                &row.name,
            )
        })
        .collect();
    CellMenu {
        list,
        ..fixed(book, tab, keys)
    }
}

/// The row the `⋮` menu draws for `id`, the same as the cell menu's; `None`
/// for a row neither holds.
#[must_use]
pub fn action_row(book: &WorkspaceBook, tab: u64, id: RowId) -> Option<CellRow> {
    let key = match id {
        RowId::WorkspaceAutosave => "autosave",
        RowId::WorkspaceSaveAsNew => "save_as",
        RowId::WorkspaceRename => "rename",
        RowId::WorkspaceNew => "new",
        RowId::ManageWorkspaces => "manage",
        _ => return None,
    };
    let CellMenu { head, foot, .. } = fixed(book, tab, "");
    head.into_iter().chain(foot).find(|row| row.key == key)
}

/// The rows that never scroll, around an empty list.
fn fixed(book: &WorkspaceBook, tab: u64, keys: &str) -> CellMenu {
    let (workspace, modified) = state_of(book, tab);
    let auto = book.autosave();
    let autosave_call = calls::WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID;
    let label = row_of(RowId::WorkspaceAutosave).label;
    let line = if auto {
        "Every change is saved".to_owned()
    } else {
        format!("Save with {keys}")
    };
    let rename = WorkspaceCommand::BeginRename(workspace.to_owned());
    let rename_call = calls::WORKSPACE_RENAME_CAPABILITY_ID;
    let head = vec![
        CellRow {
            on: auto,
            command: Some(WorkspaceCommand::SetAutosave(!auto)),
            capability: Some(autosave_call),
            ..CellRow::new(RowKind::Switch, "autosave", label)
        },
        CellRow::line(RowKind::Caption, line),
        CellRow::line(RowKind::Separator, ""),
        CellRow {
            beside: keys.to_owned(),
            enabled: modified && !auto,
            dot: modified && !auto,
            command: Some(WorkspaceCommand::SaveTo),
            capability: Some(calls::WORKSPACE_SAVE_TO_CAPABILITY_ID),
            ..CellRow::new(
                RowKind::Press,
                "save",
                format!("Save \u{201c}{workspace}\u{201d}"),
            )
        },
        CellRow::press(
            "save_as",
            RowId::WorkspaceSaveAsNew,
            WorkspaceCommand::BeginSaveAs,
            calls::WORKSPACE_SAVE_AS_CAPABILITY_ID,
        ),
        CellRow {
            enabled: workspace != DEFAULT_WORKSPACE,
            ..CellRow::press("rename", RowId::WorkspaceRename, rename, rename_call)
        },
        CellRow::line(RowKind::Separator, ""),
        CellRow::line(RowKind::Caption, "WORKSPACES"),
    ];
    let foot = vec![
        CellRow::line(RowKind::Separator, ""),
        CellRow::press(
            "new",
            RowId::WorkspaceNew,
            WorkspaceCommand::BeginNew,
            calls::WORKSPACE_NEW_CAPABILITY_ID,
        ),
        CellRow::press(
            "manage",
            RowId::ManageWorkspaces,
            WorkspaceCommand::BeginManage,
            MENU_CAPABILITY_ID,
        ),
    ];
    CellMenu {
        head,
        list: Vec::new(),
        foot,
    }
}

#[cfg(test)]
#[path = "workspace_cell_menu_tests.rs"]
mod tests;
