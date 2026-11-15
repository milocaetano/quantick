//! The menu below the capsule's workspace name (designer B, "workspace
//! first"), apart from its pixels: the header on the tab's workspace with
//! what acts on it, the list in its stored order with each row's summary
//! and status, the filter and the footer, the words of each, what a press
//! asks and the call that does the same, so the window only draws and the
//! scene reads the whole menu. Its size, scroll, filter and focus are
//! `quantick_session::workspace_menu`'s.
//!
//! A row view-model sits among the handler families because what a row does
//! is the workspace calls' own business: each row names the call that does
//! the same, under `workspace_book`, which carries the rows' commands out,
//! and this is the highest headless crate that sees both the session's
//! commands and the control plane's words and scene.

use quantick_control_host::wire::{available, unavailable};
use quantick_control_schema::scene::{
    SceneBoundsSnapshot, SceneControlSnapshot, SceneOwnerKindDto, SceneOwnerSnapshot, SceneRoleDto,
};
use quantick_control_schema::window_menu::{RowId, row_of};
use quantick_control_schema::workspace_book as calls;
use quantick_session::workspace_menu::{CHIP_MENU_FILTER_FROM, WorkspaceCommand, filter_matches};
use quantick_stores::workspaces::{
    DEFAULT_WORKSPACE, Rgb, WorkspaceBook, WorkspaceColour, WorkspaceListing, WorkspaceRecipe,
};
use quantick_workspace::canvas::PaneTree;
use quantick_workspace::canvas_layout::recipe_canvas;

/// Between the parts of a summary.
const SEPARATOR: &str = " \u{b7} ";

/// The open menu's id among the toolbar's scene controls; a row's is
/// `<this>.<key>`.
pub const SCENE_ID: &str = "workspace_menu";
/// The call that reads what the menu lists, named by the menu and by the
/// capsule's workspace name that opens it.
pub const MENU_CAPABILITY_ID: &str = calls::WORKSPACE_LIST_CAPABILITY_ID;
/// Why Save is greyed while auto save is on.
pub const AUTOSAVE_SAVES_REASON: &str = "autosave_saves_every_change";
/// Why Save is greyed on a tab with nothing held.
pub const NO_EDITS_REASON: &str = "the_tab_holds_no_edits";
/// Why Rename is greyed on `Default`.
pub const DEFAULT_PROTECTED_REASON: &str = "the_default_workspace_cannot_be_renamed";
/// The caption over the list, beside its count.
pub const CAPTION: &str = "WORKSPACES";
/// What Save says while autosave saves every change.
pub const QUIET_SAVE: &str = "Saved automatically";
/// The empty filter field's words.
pub const FILTER_PLACEHOLDER: &str = "Type to filter";
/// How a row ends while only the tab whose menu it is holds its edit.
pub const UNSAVED_HERE: &str = "unsaved";
/// The header's mark while the tab holds edits.
pub const UNSAVED: &str = "Unsaved";

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
    /// The header's line on the tab's workspace: its miniature, its name over
    /// its summary, and the Unsaved mark.
    Current,
    /// A button.
    Press,
    /// The quiet line Save turns into while autosave saves.
    Quiet,
    /// The autosave switch, its state words at its end.
    Switch,
    /// A workspace in the list: miniature, name over summary, status.
    Workspace,
    /// The filter field over a long list.
    Filter,
}

/// A workspace's colour as one theme paints it: the value its slot fills
/// and the words the scene ends its row with (`Sky #5DBDEB`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowColour {
    pub rgb: Rgb,
    pub words: String,
}

impl RowColour {
    fn of(colour: WorkspaceColour, dark: bool) -> Self {
        Self {
            rgb: colour.rgb(dark),
            words: colour.describe(dark),
        }
    }
}

/// One row of the menu.
#[derive(Debug, Clone, PartialEq)]
pub struct CellRow {
    pub kind: RowKind,
    /// Its scene key; empty for the menu itself.
    pub key: String,
    pub label: String,
    /// The line under a workspace's name, or the switch's state words.
    pub summary: String,
    /// A press's shortcut words at its right end.
    pub beside: String,
    /// Switched on, or the tab's own workspace.
    pub on: bool,
    /// Why a press would do nothing now, as a stable code; `None` while it
    /// would.
    pub unavailable: Option<&'static str>,
    /// Unsaved edits: the header's mark, a row's dot, Save standing out.
    pub dot: bool,
    pub hover: Option<&'static str>,
    pub disabled_hover: Option<&'static str>,
    /// What a press asks; `None` on the tab's own row, which only closes.
    pub command: Option<WorkspaceCommand>,
    /// The call that does what a press does.
    pub capability: Option<&'static str>,
    /// The layout its miniature pictures.
    pub tree: Option<PaneTree>,
    /// A workspace's colour, on its row and the header.
    pub colour: Option<RowColour>,
}

impl CellRow {
    fn new(kind: RowKind, key: &str, label: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.to_owned(),
            label: label.into(),
            summary: String::new(),
            beside: String::new(),
            on: false,
            unavailable: None,
            dot: false,
            hover: None,
            disabled_hover: None,
            command: None,
            capability: None,
            tree: None,
            colour: None,
        }
    }

    fn press(
        key: &str,
        label: &str,
        id: RowId,
        command: WorkspaceCommand,
        call: &'static str,
    ) -> Self {
        let row = row_of(id);
        Self {
            hover: row.hover,
            disabled_hover: row.disabled_hover,
            command: Some(command),
            capability: Some(call),
            ..Self::new(RowKind::Press, key, label)
        }
    }

    /// The open menu itself, holding `workspaces`, as the scene lists it.
    #[must_use]
    pub fn menu(workspaces: usize) -> Self {
        Self {
            on: true,
            summary: format!("{workspaces} workspaces"),
            capability: Some(MENU_CAPABILITY_ID),
            ..Self::new(RowKind::Current, "", "Workspace menu")
        }
    }

    /// The row's id among the toolbar's scene controls.
    #[must_use]
    pub fn scene_id(&self) -> String {
        scene_id(&self.key)
    }

    /// What hovering the row says: why it is greyed while it is, else what it
    /// does, else its whole label when `cut` short.
    #[must_use]
    pub fn tip(&self, cut: bool) -> Option<&str> {
        if self.unavailable.is_some() {
            return self.disabled_hover;
        }
        self.hover.or_else(|| cut.then_some(self.label.as_str()))
    }

    /// The row as the scene lists it under the toolbar owner `owner`, at
    /// `bounds` when its rectangle is reportable: its summary as its
    /// description, unavailable with its reason while a press would do
    /// nothing.
    #[must_use]
    pub fn scene_control(
        &self,
        owner: &str,
        bounds: Option<SceneBoundsSnapshot>,
    ) -> SceneControlSnapshot {
        let marked = self.kind == RowKind::Current && self.dot;
        let mut description = match (self.summary.is_empty(), marked) {
            (true, _) => None,
            (false, false) => Some(self.summary.clone()),
            (false, true) => Some(format!("{}{SEPARATOR}{UNSAVED}", self.summary)),
        };
        if let (Some(said), Some(colour)) = (description.as_mut(), &self.colour) {
            *said = format!("{said}{SEPARATOR}{}", colour.words);
        }
        SceneControlSnapshot {
            control_id: format!("{owner}.{}", self.scene_id()),
            label: self.label.clone(),
            description,
            role: SceneRoleDto::Action,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::Toolbar,
                id: owner.to_owned(),
            },
            selected: self.on,
            availability: self.unavailable.map_or_else(available, unavailable),
            bounds_availability: match bounds {
                Some(_) => available(),
                None => unavailable("the_control_rectangle_is_not_reportable"),
            },
            bounds,
            capability_id: self.capability.map(str::to_owned),
        }
    }
}

/// The menu: the header on the tab's workspace with what acts on it, the
/// list, which alone scrolls, and the footer.
#[derive(Debug, Clone, PartialEq)]
pub struct CellMenu {
    pub current: CellRow,
    /// Save (or its quiet line), Save as, Rename.
    pub head: Vec<CellRow>,
    /// Every workspace, in the stored order.
    pub list: Vec<CellRow>,
    /// Autosave, New, Manage.
    pub foot: Vec<CellRow>,
}

impl CellMenu {
    /// Whether a filter field stands over the list.
    #[must_use]
    pub fn filters(&self) -> bool {
        self.list.len() >= CHIP_MENU_FILTER_FROM
    }

    /// The list rows `filter` keeps, in the stored order.
    #[must_use]
    pub fn shown(&self, filter: &str) -> Vec<&CellRow> {
        let kept = |row: &&CellRow| filter_matches(filter, &row.label);
        self.list.iter().filter(kept).collect()
    }

    /// The filter field holding `filter`, while the list has one.
    #[must_use]
    pub fn filter_row(&self, filter: &str) -> Option<CellRow> {
        let label = if filter.is_empty() {
            FILTER_PLACEHOLDER
        } else {
            filter
        };
        self.filters().then(|| CellRow {
            on: !filter.is_empty(),
            capability: Some(MENU_CAPABILITY_ID),
            ..CellRow::new(RowKind::Filter, "filter", label)
        })
    }
}

/// The workspace `tab` is on, and whether it holds edits.
#[must_use]
pub fn state_of(book: &WorkspaceBook, tab: u64) -> (&str, bool) {
    book.tab_state(tab)
        .map_or((DEFAULT_WORKSPACE, false), |state| {
            (state.workspace, state.modified)
        })
}

/// The layout workspace `name` restores and its summary, as `workspace.list`
/// says it.
fn described(book: &WorkspaceBook, name: &str) -> (PaneTree, String) {
    let factory = WorkspaceRecipe::default();
    let recipe = book.recipe(name).unwrap_or(&factory);
    let tree = recipe_canvas(recipe).tree().clone();
    let summary = recipe.summary_of(&tree);
    (tree, summary)
}

/// `1 tab`, `2 tabs`.
fn tabs_said(count: usize) -> String {
    if count == 1 {
        "1 tab".to_owned()
    } else {
        format!("{count} tabs")
    }
}

/// Where workspace `row`'s unsaved edit is held, as its summary ends: just
/// `unsaved` when only `tab`, whose menu it is, holds it; else each holder's
/// label once, or how many tabs hold it when two labels read alike. `None`
/// while no tab holds one.
fn held_in(
    book: &WorkspaceBook,
    row: &WorkspaceListing,
    tab: u64,
    label: &dyn Fn(u64) -> String,
) -> Option<String> {
    let holders: Vec<u64> = (row.tabs.iter().copied())
        .filter(|&other| state_of(book, other).1)
        .collect();
    let mut labels: Vec<String> = holders.iter().map(|&holder| label(holder)).collect();
    labels.sort();
    labels.dedup();
    match holders.as_slice() {
        [] => None,
        [only] if *only == tab => Some(UNSAVED_HERE.to_owned()),
        _ if labels.len() < holders.len() => {
            Some(format!("unsaved in {}", tabs_said(holders.len())))
        }
        _ => Some(format!("unsaved in {}", labels.join(", "))),
    }
}

/// The menu as `book` stands for `tab`: `keys` the save shortcut's words,
/// `live` the layout the tab shows, `dark` the theme its colours paint in,
/// `label` a tab's words where an edit is held.
#[must_use]
pub fn cell_menu(
    book: &WorkspaceBook,
    tab: u64,
    keys: &str,
    live: &PaneTree,
    dark: bool,
    label: impl Fn(u64) -> String,
) -> CellMenu {
    let (workspace, modified) = state_of(book, tab);
    let auto = book.autosave();
    let list = (book.list().into_iter())
        .map(|row| {
            let (tree, mut summary) = described(book, &row.name);
            let held = held_in(book, &row, tab, &label).filter(|_| !auto);
            if let Some(held) = &held {
                summary = format!("{summary}{SEPARATOR}{held}");
            }
            let on = row.name == workspace;
            CellRow {
                summary,
                on,
                dot: held.is_some(),
                command: (!on).then(|| WorkspaceCommand::Set(row.name.clone())),
                capability: Some(calls::TAB_WORKSPACE_SET_CAPABILITY_ID),
                tree: Some(tree),
                colour: Some(RowColour::of(row.colour, dark)),
                ..CellRow::new(
                    RowKind::Workspace,
                    &format!("workspace.{}", row.name),
                    &row.name,
                )
            }
        })
        .collect::<Vec<_>>();
    let tabs = tabs_said(book.tabs_on(workspace).len().max(1));
    let shown = book.tab_copy(tab).or_else(|| book.recipe(workspace));
    let shown = shown.map_or_else(
        || described(book, workspace).1,
        |copy| copy.summary_of(live),
    );
    let current = CellRow {
        summary: format!("{shown}{SEPARATOR}{tabs}"),
        on: true,
        dot: modified && !auto,
        capability: Some(MENU_CAPABILITY_ID),
        tree: Some(live.clone()),
        colour: book
            .colour(workspace)
            .map(|colour| RowColour::of(colour, dark)),
        ..CellRow::new(RowKind::Current, "current", workspace)
    };
    CellMenu {
        current,
        head: head(workspace, modified, auto, keys),
        list,
        foot: foot(auto, keys),
    }
}

/// Save, or its quiet line while autosave saves; Save as; Rename.
fn head(workspace: &str, modified: bool, auto: bool, keys: &str) -> Vec<CellRow> {
    let save_call = calls::WORKSPACE_SAVE_TO_CAPABILITY_ID;
    let save = if auto {
        CellRow {
            unavailable: Some(AUTOSAVE_SAVES_REASON),
            capability: Some(save_call),
            ..CellRow::new(RowKind::Quiet, "save", QUIET_SAVE)
        }
    } else {
        CellRow {
            beside: keys.to_owned(),
            unavailable: (!modified).then_some(NO_EDITS_REASON),
            dot: modified,
            ..CellRow::press(
                "save",
                "Save",
                RowId::SaveToWorkspace,
                WorkspaceCommand::SaveTo,
                save_call,
            )
        }
    };
    let rename = WorkspaceCommand::BeginRename(workspace.to_owned());
    vec![
        save,
        CellRow::press(
            "save_as",
            "Save as\u{2026}",
            RowId::WorkspaceSaveAsNew,
            WorkspaceCommand::BeginSaveAs,
            calls::WORKSPACE_SAVE_AS_CAPABILITY_ID,
        ),
        CellRow {
            unavailable: (workspace == DEFAULT_WORKSPACE).then_some(DEFAULT_PROTECTED_REASON),
            ..CellRow::press(
                "rename",
                "Rename\u{2026}",
                RowId::WorkspaceRename,
                rename,
                calls::WORKSPACE_RENAME_CAPABILITY_ID,
            )
        },
    ]
}

/// The autosave switch with its state words, New and Manage.
fn foot(auto: bool, keys: &str) -> Vec<CellRow> {
    let summary = if auto {
        format!("On{SEPARATOR}every change is saved")
    } else {
        format!("Off{SEPARATOR}{keys} saves")
    };
    let switch = CellRow::press(
        "autosave",
        "Autosave",
        RowId::WorkspaceAutosave,
        WorkspaceCommand::SetAutosave(!auto),
        calls::WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID,
    );
    vec![
        CellRow {
            kind: RowKind::Switch,
            summary,
            on: auto,
            ..switch
        },
        CellRow::press(
            "new",
            "New workspace\u{2026}",
            RowId::WorkspaceNew,
            WorkspaceCommand::BeginNew,
            calls::WORKSPACE_NEW_CAPABILITY_ID,
        ),
        CellRow::press(
            "manage",
            "Manage\u{2026}",
            RowId::ManageWorkspaces,
            WorkspaceCommand::BeginManage,
            MENU_CAPABILITY_ID,
        ),
    ]
}

/// What the rows were built from: the book, the tab, the shortcut's words,
/// the live layout, the theme and the tabs' labels.
type CellMenuRead = (WorkspaceBook, u64, String, PaneTree, bool, Vec<String>);

/// The open menu's rows, built once and again only when what they read
/// changes: the book, the tab, the shortcut's words, the live layout, the
/// theme or the tabs' labels. A quiet frame borrows the last build.
#[derive(Debug, Default)]
pub struct CellMenuCache {
    read: Option<CellMenuRead>,
    menu: Option<CellMenu>,
    /// How many times the rows were built.
    pub builds: usize,
}

impl CellMenuCache {
    /// [`cell_menu`] for these words, built only when they changed.
    pub fn menu(
        &mut self,
        book: &WorkspaceBook,
        tab: u64,
        keys: &str,
        live: &PaneTree,
        dark: bool,
        label: impl Fn(u64) -> String,
    ) -> &CellMenu {
        let labels: Vec<String> = book.tab_workspaces().map(|(held, _)| label(held)).collect();
        let read = (book, tab, keys, live, dark, &labels);
        let same = (self.read.as_ref())
            .is_some_and(|(b, t, k, l, d, ls)| (b, *t, k.as_str(), l, *d, ls) == read);
        if !same {
            let owned = (
                book.clone(),
                tab,
                keys.to_owned(),
                live.clone(),
                dark,
                labels,
            );
            (self.read, self.menu) = (Some(owned), None);
        }
        let builds = &mut self.builds;
        self.menu.get_or_insert_with(|| {
            *builds += 1;
            cell_menu(book, tab, keys, live, dark, label)
        })
    }
}

#[cfg(test)]
#[path = "cell_menu_tests.rs"]
mod tests;
