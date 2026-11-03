//! The `⋮` window menu as data: its groups, its rows and what each row is.
//!
//! The brief is `docs/ux/window-menu-and-workspace-tabs.md`. The app paints this
//! table and carries out what a row asks; nothing here knows a pixel. Every row
//! names the behaviour it reaches, so a row without a named command cannot be
//! written and a menu cannot quietly become mouse-only.

/// One command row of the window menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RowId {
    SaveToWorkspace,
    WorkspaceSaveAsNew,
    WorkspaceRename,
    WorkspaceRevert,
    WorkspaceDelete,
    SaveDesk,
    TogglePanels,
    MarketReplay,
    CloseReplay,
    CommandPalette,
    Settings,
    DockL2,
    DockBubbles,
    DockSession,
    DockPaperTrading,
    DockTrades,
    ToolbarLeft,
    ToolbarTop,
    ToolbarBottom,
    ToolbarHidden,
    ExportDesk,
    ImportDesk,
    RevealDesk,
    ResetStartupDesk,
    ReopenLastTabs,
    KeyboardShortcuts,
    ReplayFileFormat,
    AboutWorkspaces,
    AboutQuantick,
}

/// A submenu of the window menu. They all open to the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SubmenuId {
    Workspace,
    Panels,
    Desk,
    Help,
    Layouts,
    Layout,
    MoveChart,
    OpenRecent,
}

/// Every submenu, for whoever has to walk the labels.
pub const SUBMENUS: &[SubmenuId] = &[
    SubmenuId::Workspace,
    SubmenuId::Panels,
    SubmenuId::Desk,
    SubmenuId::Help,
    SubmenuId::Layouts,
    SubmenuId::Layout,
    SubmenuId::MoveChart,
    SubmenuId::OpenRecent,
];

/// A stretch of a menu whose rows are computed from live state by the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRows {
    /// The layout book's tabs and the three edits the strip's own menu holds.
    LayoutBook,
    /// The canvas arrangements, from the layout registry.
    LayoutPresets,
    /// Show or hide the context charts, while the layout has any.
    ContextCharts,
    /// Fold the focused chart's indicator legend.
    Legend,
    /// `Chart N up` and `Chart N down`.
    ChartMoves,
    /// Workspace files opened before.
    RecentFiles,
    /// The saved workspaces, each with the tabs on it.
    WorkspaceList,
}

/// One line of a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Row(RowId),
    Submenu(SubmenuId),
    Live(LiveRows),
    Caption(&'static str),
    Separator,
}

/// What a command row says and which named behaviour it reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub id: RowId,
    pub label: &'static str,
    /// The label while the row's switch is on, when it reads differently.
    pub label_when_on: Option<&'static str>,
    /// The behaviour's id in the operability table.
    pub behaviour: &'static str,
    pub hover: Option<&'static str>,
    pub disabled_hover: Option<&'static str>,
}

const fn row(id: RowId, label: &'static str, behaviour: &'static str) -> Row {
    Row {
        id,
        label,
        label_when_on: None,
        behaviour,
        hover: None,
        disabled_hover: None,
    }
}

const DOCK_BEHAVIOUR: &str = "dock.tab.open";
const TOOLBAR_BEHAVIOUR: &str = "toolrail.dock.set";
const HELP_BEHAVIOUR: &str = "window.help.open";
const NO_EDITS_HOVER: &str = "This tab holds no edits";
const DEFAULT_PROTECTED_HOVER: &str = "Default cannot be deleted or renamed";

/// Every command row of the window menu, in no particular order.
pub const ROWS: &[Row] = &[
    Row {
        disabled_hover: Some(NO_EDITS_HOVER),
        ..row(
            RowId::SaveToWorkspace,
            "Save to workspace",
            "workspace.save_to",
        )
    },
    row(
        RowId::WorkspaceSaveAsNew,
        "Save as new\u{2026}",
        "workspace.save_as",
    ),
    Row {
        disabled_hover: Some(DEFAULT_PROTECTED_HOVER),
        ..row(
            RowId::WorkspaceRename,
            "Rename workspace\u{2026}",
            "workspace.rename",
        )
    },
    Row {
        disabled_hover: Some(NO_EDITS_HOVER),
        ..row(RowId::WorkspaceRevert, "Revert edits", "workspace.revert")
    },
    Row {
        disabled_hover: Some(DEFAULT_PROTECTED_HOVER),
        ..row(
            RowId::WorkspaceDelete,
            "Delete workspace",
            "workspace.delete",
        )
    },
    Row {
        hover: Some(
            "Remember this arrangement — the tabs, the charts on each, the panels, the \
             timezone and the window — as what quantick opens on",
        ),
        ..row(RowId::SaveDesk, "Save desk", "desk.save")
    },
    Row {
        label_when_on: Some("Hide panels"),
        ..row(RowId::TogglePanels, "Show panels", "dock.toggle")
    },
    row(RowId::MarketReplay, "Market Replay…", "replay.browser.open"),
    row(RowId::CloseReplay, "Close Replay", "replay.close"),
    Row {
        disabled_hover: Some("Not built yet: it arrives with the market palette"),
        ..row(
            RowId::CommandPalette,
            "Command palette…",
            "window.command_palette.open",
        )
    },
    row(RowId::Settings, "Settings…", "settings.open"),
    row(RowId::DockL2, "L2", DOCK_BEHAVIOUR),
    row(RowId::DockBubbles, "Bubbles", DOCK_BEHAVIOUR),
    row(RowId::DockSession, "Session", DOCK_BEHAVIOUR),
    row(RowId::DockPaperTrading, "Paper trading", DOCK_BEHAVIOUR),
    row(RowId::DockTrades, "Trades", DOCK_BEHAVIOUR),
    row(RowId::ToolbarLeft, "Left", TOOLBAR_BEHAVIOUR),
    row(RowId::ToolbarTop, "Top", TOOLBAR_BEHAVIOUR),
    row(RowId::ToolbarBottom, "Bottom", TOOLBAR_BEHAVIOUR),
    row(RowId::ToolbarHidden, "Hidden", "toolrail.visible.toggle"),
    Row {
        hover: Some(
            "Save the whole cockpit — tabs, indicators, layers, drawing colours, footprint \
             and added symbols — as one file in your documents",
        ),
        ..row(RowId::ExportDesk, "Export desk to file…", "desk.export")
    },
    Row {
        hover: Some(
            "Open a workspace file. It replaces the cockpit on screen; a file that cannot \
             be read changes nothing.",
        ),
        ..row(RowId::ImportDesk, "Import desk from file…", "desk.import")
    },
    Row {
        hover: Some(
            "Open the folder quantick keeps your cockpit in, so you can see it and back it up",
        ),
        ..row(
            RowId::RevealDesk,
            "Show where it's saved",
            "workspace.reveal",
        )
    },
    Row {
        hover: Some(
            "Forget the saved workspace; the next launch opens on the configured default. \
             The charts on screen are left alone.",
        ),
        disabled_hover: Some(
            "Nothing saved yet — quantick already opens on the configured default",
        ),
        ..row(RowId::ResetStartupDesk, "Reset startup desk", "desk.forget")
    },
    Row {
        hover: Some(
            "Keep the arrangement automatically when the window closes. Off, only Save desk \
             changes what quantick opens on.",
        ),
        ..row(
            RowId::ReopenLastTabs,
            "Reopen last tabs on launch",
            "desk.reopen_on_launch.set",
        )
    },
    row(
        RowId::KeyboardShortcuts,
        "Keyboard shortcuts…",
        HELP_BEHAVIOUR,
    ),
    row(
        RowId::ReplayFileFormat,
        "Replay file format…",
        "replay.format_help.open",
    ),
    row(RowId::AboutWorkspaces, "About workspaces…", HELP_BEHAVIOUR),
    row(RowId::AboutQuantick, "About quantick", HELP_BEHAVIOUR),
];

/// The menu the `⋮` button opens.
pub const ROOT: &[Item] = &[
    Item::Row(RowId::SaveToWorkspace),
    Item::Row(RowId::SaveDesk),
    Item::Row(RowId::TogglePanels),
    Item::Row(RowId::MarketReplay),
    Item::Row(RowId::CloseReplay),
    Item::Separator,
    Item::Submenu(SubmenuId::Workspace),
    Item::Submenu(SubmenuId::Panels),
    Item::Submenu(SubmenuId::Desk),
    Item::Separator,
    Item::Row(RowId::CommandPalette),
    Item::Row(RowId::Settings),
    Item::Submenu(SubmenuId::Help),
];

/// A submenu's label and lines.
#[must_use]
pub const fn submenu(id: SubmenuId) -> (&'static str, &'static [Item]) {
    match id {
        SubmenuId::Workspace => (
            "Workspace",
            &[
                Item::Live(LiveRows::WorkspaceList),
                Item::Row(RowId::WorkspaceSaveAsNew),
                Item::Row(RowId::WorkspaceRename),
                Item::Row(RowId::WorkspaceRevert),
                Item::Row(RowId::WorkspaceDelete),
                Item::Separator,
                Item::Submenu(SubmenuId::Layouts),
                Item::Submenu(SubmenuId::Layout),
                Item::Live(LiveRows::ContextCharts),
                Item::Submenu(SubmenuId::MoveChart),
                Item::Live(LiveRows::Legend),
                Item::Separator,
                Item::Row(RowId::AboutWorkspaces),
            ],
        ),
        SubmenuId::Panels => (
            "Panels",
            &[
                Item::Row(RowId::DockL2),
                Item::Row(RowId::DockBubbles),
                Item::Row(RowId::DockSession),
                Item::Row(RowId::DockPaperTrading),
                Item::Row(RowId::DockTrades),
                Item::Separator,
                Item::Caption("Drawing toolbar"),
                Item::Row(RowId::ToolbarLeft),
                Item::Row(RowId::ToolbarTop),
                Item::Row(RowId::ToolbarBottom),
                Item::Row(RowId::ToolbarHidden),
            ],
        ),
        SubmenuId::Desk => (
            "Desk",
            &[
                Item::Row(RowId::ExportDesk),
                Item::Row(RowId::ImportDesk),
                Item::Submenu(SubmenuId::OpenRecent),
                Item::Row(RowId::RevealDesk),
                Item::Separator,
                Item::Row(RowId::ResetStartupDesk),
                Item::Row(RowId::ReopenLastTabs),
            ],
        ),
        SubmenuId::Help => (
            "Help",
            &[
                Item::Row(RowId::KeyboardShortcuts),
                Item::Row(RowId::ReplayFileFormat),
                Item::Row(RowId::AboutWorkspaces),
                Item::Row(RowId::AboutQuantick),
            ],
        ),
        SubmenuId::Layouts => ("Layouts", &[Item::Live(LiveRows::LayoutBook)]),
        SubmenuId::Layout => ("Layout", &[Item::Live(LiveRows::LayoutPresets)]),
        SubmenuId::MoveChart => ("Move chart", &[Item::Live(LiveRows::ChartMoves)]),
        SubmenuId::OpenRecent => ("Open recent", &[Item::Live(LiveRows::RecentFiles)]),
    }
}

/// The row with this id.
///
/// # Panics
/// Only if [`ROWS`] misses an id, which a test here rules out.
#[must_use]
pub fn row_of(id: RowId) -> &'static Row {
    ROWS.iter()
        .find(|row| row.id == id)
        .expect("every row id has a row")
}

/// A page of the help window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpPage {
    KeyboardShortcuts,
    AboutWorkspaces,
    AboutQuantick,
}

impl HelpPage {
    /// The window's title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::KeyboardShortcuts => "Keyboard shortcuts",
            Self::AboutWorkspaces => "About workspaces",
            Self::AboutQuantick => "About quantick",
        }
    }
}

/// The legend row's label: while two charts are open it says which one, because
/// the trader cannot otherwise know which corner is about to change.
#[must_use]
pub fn legend_label(collapsed: bool, split: Option<&str>) -> String {
    let verb = if collapsed { "Show" } else { "Collapse" };
    match split {
        Some(pane) => format!("{verb} indicator legend ({pane})"),
        None => format!("{verb} indicator legend"),
    }
}

/// The name dialog's title when it makes a new workspace.
pub const SAVE_AS_DIALOG_TITLE: &str = "Save as new workspace";
/// The name dialog's title when it renames one.
pub const RENAME_DIALOG_TITLE: &str = "Rename workspace";
pub const DIALOG_SAVE: &str = "Save";
pub const DIALOG_CANCEL: &str = "Cancel";
/// The toolbar's Revert, beside its Save to.
pub const TOOLBAR_REVERT: &str = "Revert";

fn named(symbols: &[String]) -> String {
    symbols.join(", ")
}

/// The root row: `Save WINV26 to Scalp`.
#[must_use]
pub fn save_to_label(symbol: &str, workspace: &str) -> String {
    format!("Save {symbol} to {workspace}")
}

/// The toolbar's shorter form, where the symbol stands beside it.
#[must_use]
pub fn save_to_short_label(workspace: &str) -> String {
    format!("Save to {workspace}")
}

/// The first line of the chip's menu: `WINV26 uses`.
#[must_use]
pub fn uses_caption(symbol: &str) -> String {
    format!("{symbol} uses")
}

/// Save to names the tabs it will change before the click.
#[must_use]
pub fn save_to_hover(followers: &[String]) -> String {
    if followers.is_empty() {
        "No other tab changes".to_owned()
    } else {
        format!("Also changes {}", named(followers))
    }
}

/// The armed delete row, stating what the second click does.
#[must_use]
pub fn delete_armed_label(workspace: &str, moved: &[String]) -> String {
    if moved.is_empty() {
        format!("Delete {workspace}? No tab uses it")
    } else {
        format!("Delete {workspace}? {} move to Default", named(moved))
    }
}

/// The toast after a switch, which says when edits were dropped.
#[must_use]
pub fn switched_toast(symbol: &str, workspace: &str, dropped_edits: bool) -> String {
    if dropped_edits {
        format!("{symbol} now uses {workspace}; its unsaved edits were dropped")
    } else {
        format!("{symbol} now uses {workspace}")
    }
}

/// The toast after Save to, naming the tabs that changed.
#[must_use]
pub fn saved_toast(workspace: &str, followers: &[String]) -> String {
    if followers.is_empty() {
        format!("Saved to {workspace}")
    } else {
        format!("Saved to {workspace}; {} changed", named(followers))
    }
}

#[must_use]
pub fn saved_as_toast(symbol: &str, workspace: &str) -> String {
    format!("{symbol} now uses the new workspace {workspace}")
}

#[must_use]
pub fn reverted_toast(symbol: &str) -> String {
    format!("{symbol} is back on its saved workspace")
}

#[must_use]
pub fn renamed_toast(from: &str, to: &str) -> String {
    format!("{from} is now {}", to.trim())
}

/// The toast after a delete, naming each tab and what became of it.
#[must_use]
pub fn deleted_toast(workspace: &str, adopted: &[String], kept_edits: &[String]) -> String {
    let mut toast = format!("Deleted {workspace}");
    if !adopted.is_empty() {
        toast.push_str(&format!("; {} moved to Default", named(adopted)));
    }
    if !kept_edits.is_empty() {
        let verb = if kept_edits.len() == 1 {
            "keeps its"
        } else {
            "keep their"
        };
        toast.push_str(&format!(
            "; {} {verb} edits as Default \u{2022}",
            named(kept_edits)
        ));
    }
    toast
}

/// What `About workspaces…` says. Written for what ships: an About page that
/// describes a control the trader cannot find is a false statement.
pub const ABOUT_WORKSPACES: &[&str] = &[
    "A workspace is a saved way of reading a market: which charts the tab shows (Flow, Time \
     or both), the context panes and their intervals, the indicators on each pane, the layer \
     toggles and the footprint style. Keep one per kind of strategy and switch the tab between \
     them: Scalp for the open, Tape for a quiet range, Swing for the daily picture.",
    "Every tab is one market and uses one workspace; a new tab opens on Default. Any number of \
     tabs can share a workspace. Editing a tab marks only that tab (•); nothing reaches the \
     other tabs until you choose Save to, and that step names the tabs it will change.",
    "Drawings, anchored studies, bar size and the paper position belong to the market, never \
     to the workspace: switching WINV26 from Scalp to Tape keeps its levels.",
    "Deleting a workspace moves the tabs on it to Default. A tab with unsaved edits keeps them, \
     marked Default •, so nothing is lost. Default cannot be deleted.",
];

/// A Settings switch's tooltip.
pub const PROGRESSIVE_HISTORY_HOVER: &str = "Build the venue's candle history from now backwards, a week at \
             a time, so the chart fills in while the rest arrives. Off asks \
             for the whole span in one request: fewer calls, nothing on \
             screen until all of it lands.";

/// A Settings switch's tooltip.
pub const VENUE_LEAD_IN_HOVER: &str = "A tick, volume, dollar or imbalance chart cannot fold venue \
             candles into its own bars, so it opens holding only the prints \
             this session saw. Switch this on to put the venue's 1-minute \
             candles in front of them anyway — counted apart from built bars \
             on the status bar — so yesterday is on screen to compare \
             against. They stay candles: a minute never becomes a tick bar, \
             and an indicator running across the seam is averaging both \
             kinds.";

/// The legend row's tooltip.
pub const LEGEND_HOVER: &str = "Folds the healthy rows to a count on the focused chart. Errored \
     and stale indicators stay on it.";

/// The window's menu button, in the scene's list of title-strip controls.
pub const KEBAB_CONTROL: &str = "kebab";
/// The local-agent access pill, in the same list.
pub const AGENT_PILL_CONTROL: &str = "agent";
/// What the pill reads: an exposure, not a health state, so it wears amber and
/// no status dot.
pub const AGENT_PILL_LABEL: &str = "Agent on";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn walk(items: &[Item], depth: usize, rows: &mut Vec<RowId>, deepest: &mut usize) {
        *deepest = (*deepest).max(depth);
        for item in items {
            match item {
                Item::Row(id) => rows.push(*id),
                Item::Submenu(id) => walk(submenu(*id).1, depth + 1, rows, deepest),
                Item::Live(_) | Item::Caption(_) | Item::Separator => {}
            }
        }
    }

    #[test]
    fn every_row_is_reachable_and_submenus_stop_at_two_levels() {
        let (mut rows, mut deepest) = (Vec::new(), 0);
        walk(ROOT, 0, &mut rows, &mut deepest);
        let about = rows
            .iter()
            .filter(|id| **id == RowId::AboutWorkspaces)
            .count();
        assert_eq!(about, 2, "at the end of Workspace and again under Help");
        let reachable: BTreeSet<_> = rows.into_iter().collect();
        let declared: BTreeSet<_> = ROWS.iter().map(|row| row.id).collect();
        assert_eq!(reachable, declared);
        assert_eq!(declared.len(), ROWS.len(), "one row per id");
        assert_eq!(deepest, 2);
    }

    #[test]
    fn the_root_groups_read_in_the_order_of_the_brief() {
        let labels: Vec<_> = ROOT
            .iter()
            .map(|item| match item {
                Item::Row(id) => row_of(*id).label_when_on.unwrap_or(row_of(*id).label),
                Item::Submenu(id) => submenu(*id).0,
                Item::Separator => "-",
                Item::Live(_) | Item::Caption(_) => unreachable!("the root has neither"),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "Save to workspace",
                "Save desk",
                "Hide panels",
                "Market Replay…",
                "Close Replay",
                "-",
                "Workspace",
                "Panels",
                "Desk",
                "-",
                "Command palette…",
                "Settings…",
                "Help",
            ]
        );
    }

    #[test]
    fn the_legend_row_names_the_chart_only_while_two_are_open() {
        assert_eq!(legend_label(true, None), "Show indicator legend");
        assert_eq!(
            legend_label(false, Some("Flow")),
            "Collapse indicator legend (Flow)"
        );
    }

    #[test]
    fn about_workspaces_reads_the_four_paragraphs_verbatim() {
        assert_eq!(
            ABOUT_WORKSPACES,
            [
                "A workspace is a saved way of reading a market: which charts the tab shows (Flow, Time or both), the context panes and their intervals, the indicators on each pane, the layer toggles and the footprint style. Keep one per kind of strategy and switch the tab between them: Scalp for the open, Tape for a quiet range, Swing for the daily picture.",
                "Every tab is one market and uses one workspace; a new tab opens on Default. Any number of tabs can share a workspace. Editing a tab marks only that tab (•); nothing reaches the other tabs until you choose Save to, and that step names the tabs it will change.",
                "Drawings, anchored studies, bar size and the paper position belong to the market, never to the workspace: switching WINV26 from Scalp to Tape keeps its levels.",
                "Deleting a workspace moves the tabs on it to Default. A tab with unsaved edits keeps them, marked Default •, so nothing is lost. Default cannot be deleted.",
            ]
        );
    }

    #[test]
    fn the_save_words_name_the_tab_and_the_tabs_it_changes() {
        assert_eq!(save_to_label("WINV26", "Scalp"), "Save WINV26 to Scalp");
        assert_eq!(save_to_hover(&[]), "No other tab changes");
        let tabs = ["BTCUSDT".to_owned(), "WDOV26".to_owned()];
        assert_eq!(save_to_hover(&tabs), "Also changes BTCUSDT, WDOV26");
        assert_eq!(
            saved_toast("Scalp", &tabs),
            "Saved to Scalp; BTCUSDT, WDOV26 changed"
        );
    }

    #[test]
    fn a_delete_says_what_became_of_each_tab() {
        let (moved, kept) = (["BTCUSDT".to_owned()], ["WDOV26".to_owned()]);
        assert_eq!(
            deleted_toast("Scalp", &moved, &kept),
            "Deleted Scalp; BTCUSDT moved to Default; WDOV26 keeps its edits as Default \u{2022}"
        );
        let kept = ["WINV26".to_owned(), "WDOV26".to_owned()];
        assert_eq!(
            deleted_toast("Scalp", &[], &kept),
            "Deleted Scalp; WINV26, WDOV26 keep their edits as Default \u{2022}"
        );
        assert_eq!(deleted_toast("Scalp", &[], &[]), "Deleted Scalp");
    }

    #[test]
    fn exit_is_not_a_row() {
        assert!(ROWS.iter().all(|row| !row.label.contains("Exit")));
    }
}
