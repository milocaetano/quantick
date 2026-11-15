//! Cached window-menu projection and interaction policy.
use quantick_control_schema::window_menu::{self as menu, Item, LiveRows, RowId, SubmenuId};
use quantick_session::workspace_menu::WorkspaceCommand;
use quantick_workspace::layout_document::LayoutId;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Door {
    #[default]
    Kebab,
    Toolbar,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Row(RowId),
    Workspace(WorkspaceCommand),
    SaveOnExit(bool),
    SwitchLayout(LayoutId),
    CreateLayout,
    RenameLayout(LayoutId),
    DeleteLayout(LayoutId),
    Preset(&'static str),
    ContextCharts,
    MoveChart { from: usize, to: usize },
    Legend,
    Recent(PathBuf),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Row(RowId),
    Layout(usize),
    Preset(usize),
    Context,
    Legend,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub hover: String,
    pub disabled_hover: String,
    pub enabled: bool,
    pub selected: bool,
    pub checkbox: bool,
    pub shortcut: Option<Shortcut>,
    pub beside: String,
    pub action: Action,
}
impl Row {
    fn new(label: impl Into<String>, action: Action) -> Self {
        Self {
            label: label.into(),
            hover: String::new(),
            disabled_hover: String::new(),
            enabled: true,
            selected: false,
            checkbox: false,
            shortcut: None,
            beside: String::new(),
            action,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Row(Row),
    Separator,
    Caption(&'static str),
    Submenu {
        id: SubmenuId,
        enabled: bool,
        hover: &'static str,
        nodes: Vec<Node>,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    pub name: String,
    pub symbol: String,
    pub modified: bool,
    pub autosave: bool,
    pub followers: Vec<String>,
    pub choices: Vec<(String, String)>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub open: bool,
    pub door: Door,
    pub keyboard: bool,
    pub chart_shown: bool,
    pub replaying: bool,
    pub startup_saved: bool,
    pub save_on_exit: bool,
    pub dock_visible: bool,
    pub selected_rows: Vec<RowId>,
    pub flow_pane_shown: bool,
    pub toolbox_dock: Option<quantick_chart_interaction::tool_rail::ToolboxDock>,
    pub dock_tab: Option<quantick_workspace::dock::DockTab>,
    pub workspace: Workspace,
    pub layouts: Vec<(LayoutId, String)>,
    pub active_layout: Option<LayoutId>,
    pub preset: String,
    pub context_column: bool,
    pub context_collapsed: bool,
    pub context_panes: usize,
    pub legend_collapsed: bool,
    pub legend_side: Option<String>,
    pub has_legend: bool,
    pub recent: Vec<(PathBuf, String)>,
}
#[derive(Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
    pub nodes: Vec<Node>,
    pub path: [Option<SubmenuId>; 2],
    pub focus_pane: Option<usize>,
}
pub enum Event {
    OpenSubmenu(usize, SubmenuId, bool),
    LeaveSubmenu(usize),
    FocusTaken(usize),
    Activate(Action),
    Dismiss,
}
pub enum Intent {
    Refresh(Box<Snapshot>),
    Events(Vec<Event>),
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub actions: Vec<Action>,
    pub closed: bool,
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        let mut effects = Effects::default();
        match intent {
            Intent::Refresh(snapshot) => {
                if state.snapshot.as_ref() != Some(&snapshot) {
                    if !snapshot.open
                        || state
                            .snapshot
                            .as_ref()
                            .is_none_or(|old| !old.open || old.door != snapshot.door)
                    {
                        state.path = [None; 2];
                        state.focus_pane = (snapshot.open && snapshot.keyboard).then_some(0);
                    }
                    state.nodes = if snapshot.open {
                        project(&snapshot, menu::ROOT)
                    } else {
                        Vec::new()
                    };
                    state.snapshot = Some(*snapshot);
                }
            }
            Intent::Events(events) => {
                for event in events {
                    if !state.snapshot.as_ref().is_some_and(|s| s.open) {
                        break;
                    }
                    match event {
                        Event::OpenSubmenu(level, id, keyboard)
                            if level < 2 && submenu_enabled(&state.nodes, id) =>
                        {
                            if state.path[level] != Some(id) && level + 1 < 2 {
                                state.path[level + 1] = None;
                            }
                            state.path[level] = Some(id);
                            if keyboard {
                                state.focus_pane = Some(level + 1);
                            }
                        }
                        Event::LeaveSubmenu(level) if level < 2 => state.path[level] = None,
                        Event::FocusTaken(level) if state.focus_pane == Some(level) => {
                            state.focus_pane = None
                        }
                        Event::Activate(action) if available(&state.nodes, &action) => {
                            let stays_open = matches!(action, Action::SaveOnExit(_));
                            let visible = state
                                .snapshot
                                .as_ref()
                                .is_some_and(|s| s.toolbox_dock.is_some());
                            let show_tools = matches!(
                                action,
                                Action::Row(
                                    RowId::ToolbarLeft | RowId::ToolbarTop | RowId::ToolbarBottom
                                )
                            ) && !visible;
                            if !matches!(action, Action::Row(RowId::ToolbarHidden)) || visible {
                                effects.actions.push(action);
                            }
                            if show_tools {
                                effects.actions.push(Action::Row(RowId::ToolbarHidden));
                            }
                            if !stays_open {
                                close(state);
                                effects.closed = true;
                            }
                        }
                        Event::Dismiss => {
                            close(state);
                            effects.closed = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        effects
    }
}
fn close(state: &mut State) {
    if let Some(snapshot) = &mut state.snapshot {
        snapshot.open = false;
    }
    state.path = [None; 2];
    state.focus_pane = None;
}
fn submenu_enabled(nodes: &[Node], wanted: SubmenuId) -> bool {
    nodes.iter().any(|node| matches!(node, Node::Submenu { id, enabled: true, nodes, .. } if *id == wanted || submenu_enabled(nodes, wanted)))
}
fn available(nodes: &[Node], action: &Action) -> bool {
    nodes.iter().any(|node| match node {
        Node::Row(row) => row.enabled && &row.action == action,
        Node::Submenu {
            enabled: true,
            nodes,
            ..
        } => available(nodes, action),
        _ => false,
    })
}
fn project(s: &Snapshot, items: &[Item]) -> Vec<Node> {
    let mut nodes = Vec::new();
    for item in items {
        match *item {
            Item::Separator => nodes.push(Node::Separator),
            Item::Caption(text) => nodes.push(Node::Caption(text)),
            Item::Row(id) => {
                if let Some(row) = row(s, id) {
                    nodes.push(Node::Row(row));
                }
            }
            Item::Live(rows) => nodes.extend(live(s, rows)),
            Item::Submenu(id) => {
                if id == SubmenuId::MoveChart && s.context_panes < 2 {
                    continue;
                }
                let empty = id == SubmenuId::OpenRecent && s.recent.is_empty();
                let chart = matches!(
                    id,
                    SubmenuId::Layouts | SubmenuId::Layout | SubmenuId::MoveChart
                );
                nodes.push(Node::Submenu {
                    id,
                    enabled: !empty && (!chart || s.chart_shown),
                    hover: if empty {
                        "No workspace files opened yet"
                    } else {
                        menu::HOME_SHOWN_HOVER
                    },
                    nodes: project(s, menu::submenu(id).1),
                });
            }
        }
    }
    nodes
}
fn row(s: &Snapshot, id: RowId) -> Option<Row> {
    if id == RowId::CloseReplay && !s.replaying {
        return None;
    }
    let text = menu::row_of(id);
    let mut row = Row::new(text.label, Action::Row(id));
    row.hover = text.hover.unwrap_or_default().into();
    row.disabled_hover = text.disabled_hover.unwrap_or_default().into();
    row.shortcut = Some(Shortcut::Row(id));
    use quantick_chart_interaction::tool_rail::ToolboxDock;
    use quantick_workspace::dock::DockTab;
    row.selected = match id {
        RowId::DockSession => s.dock_visible && s.dock_tab == Some(DockTab::Session),
        RowId::DockPaperTrading => s.dock_visible && s.dock_tab == Some(DockTab::Trading),
        RowId::DockTrades => s.dock_visible && s.dock_tab == Some(DockTab::Trades),
        RowId::ToolbarLeft => s.toolbox_dock == Some(ToolboxDock::Left),
        RowId::ToolbarTop => s.toolbox_dock == Some(ToolboxDock::Top),
        RowId::ToolbarBottom => s.toolbox_dock == Some(ToolboxDock::Bottom),
        RowId::ToolbarHidden => s.toolbox_dock.is_none(),
        _ => s.selected_rows.contains(&id),
    };
    let workspace = &s.workspace;
    match id {
        RowId::SaveToWorkspace => {
            row.label = menu::save_to_label(&workspace.symbol, &workspace.name);
            row.hover = menu::save_to_hover(&workspace.followers);
            row.enabled = workspace.modified;
            row.action = Action::Workspace(WorkspaceCommand::SaveTo);
        }
        RowId::WorkspaceSaveAsNew => row.action = Action::Workspace(WorkspaceCommand::BeginSaveAs),
        RowId::WorkspaceNew => row.action = Action::Workspace(WorkspaceCommand::BeginNew),
        RowId::WorkspaceRename => {
            row.enabled = workspace.name != quantick_stores::workspaces::DEFAULT_WORKSPACE;
            row.action = Action::Workspace(WorkspaceCommand::BeginRename(workspace.name.clone()));
        }
        RowId::WorkspaceAutosave => {
            row.checkbox = true;
            row.selected = workspace.autosave;
            row.action = Action::Workspace(WorkspaceCommand::SetAutosave(!workspace.autosave));
        }
        RowId::ManageWorkspaces => row.action = Action::Workspace(WorkspaceCommand::BeginManage),
        RowId::CommandPalette => row.enabled = false,
        RowId::ResetStartupDesk => row.enabled = s.startup_saved,
        RowId::ReopenLastTabs => {
            row.checkbox = true;
            row.selected = s.save_on_exit;
            row.action = Action::SaveOnExit(!s.save_on_exit);
        }
        RowId::TogglePanels if s.dock_visible => {
            row.label = text.label_when_on.unwrap_or(text.label).into()
        }
        _ => {}
    }
    if menu::panel_section_of(id).is_some()
        && let Some(blocked) = menu::panel_row_blocked(s.chart_shown, s.flow_pane_shown)
    {
        row.enabled = false;
        row.selected = false;
        row.disabled_hover = blocked.into();
    }
    Some(row)
}
fn live(s: &Snapshot, rows: LiveRows) -> Vec<Node> {
    let mut result = Vec::new();
    match rows {
        LiveRows::LayoutBook => {
            for (index, (id, name)) in s.layouts.iter().enumerate() {
                let mut row = Row::new(name, Action::SwitchLayout(*id));
                row.selected = Some(*id) == s.active_layout;
                row.shortcut = Some(Shortcut::Layout(index));
                result.push(Node::Row(row));
            }
            result.push(Node::Separator);
            let mut row = Row::new("New layout", Action::CreateLayout);
            row.enabled = s.layouts.len() < quantick_workspace::layout_document::MAX_LAYOUTS;
            result.push(Node::Row(row));
            if let Some(active) = s.active_layout {
                result.push(Node::Row(Row::new(
                    "Rename layout…",
                    Action::RenameLayout(active),
                )));
                let mut row = Row::new("Delete layout", Action::DeleteLayout(active));
                row.enabled = s.layouts.len() > 1;
                result.push(Node::Row(row));
            }
        }
        LiveRows::LayoutPresets => {
            for (index, preset) in quantick_workspace::canvas::LAYOUT_PRESETS
                .iter()
                .enumerate()
            {
                let mut row = Row::new(preset.label, Action::Preset(preset.id));
                row.selected = s.preset == preset.id;
                row.shortcut = Some(Shortcut::Preset(index));
                result.push(Node::Row(row));
            }
        }
        LiveRows::ContextCharts if s.context_column => {
            let mut row = Row::new(
                if s.context_collapsed {
                    "Show context charts"
                } else {
                    "Hide context charts"
                },
                Action::ContextCharts,
            );
            row.enabled = s.chart_shown;
            row.shortcut = Some(Shortcut::Context);
            result.push(Node::Row(row));
        }
        LiveRows::ChartMoves => {
            for slot in 1..=s.context_panes {
                let mut up = Row::new(
                    format!("Chart {slot} up"),
                    Action::MoveChart {
                        from: slot,
                        to: slot - 1,
                    },
                );
                up.enabled = slot > 1;
                up.disabled_hover = "already the top chart".into();
                result.push(Node::Row(up));
                let mut down = Row::new(
                    format!("Chart {slot} down"),
                    Action::MoveChart {
                        from: slot,
                        to: slot + 1,
                    },
                );
                down.enabled = slot < s.context_panes;
                down.disabled_hover = "already the bottom chart".into();
                result.push(Node::Row(down));
            }
        }
        LiveRows::Legend => {
            let mut row = Row::new(
                menu::legend_label(s.legend_collapsed, s.legend_side.as_deref()),
                Action::Legend,
            );
            row.enabled = s.chart_shown && s.has_legend;
            row.hover = menu::LEGEND_HOVER.into();
            row.disabled_hover =
                "This chart has no indicators, so there is no legend to fold".into();
            row.shortcut = Some(Shortcut::Legend);
            result.push(Node::Row(row));
        }
        LiveRows::RecentFiles => {
            for (path, label) in &s.recent {
                let mut row = Row::new(label, Action::Recent(path.clone()));
                row.hover = format!("Replaces the cockpit on screen\n{}", path.display());
                result.push(Node::Row(row));
            }
        }
        LiveRows::WorkspaceList => {
            for (name, beside) in &s.workspace.choices {
                let mut row =
                    Row::new(name, Action::Workspace(WorkspaceCommand::Set(name.clone())));
                row.selected = name == &s.workspace.name;
                row.beside = beside.clone();
                result.push(Node::Row(row));
            }
        }
        _ => {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;
    fn opened() -> State {
        let mut state = State::default();
        Model::update(
            &mut state,
            Intent::Refresh(Box::new(Snapshot {
                open: true,
                chart_shown: true,
                workspace: Workspace {
                    name: quantick_stores::workspaces::DEFAULT_WORKSPACE.into(),
                    ..Default::default()
                },
                ..Default::default()
            })),
        );
        state
    }
    #[test]
    fn unavailable_rows_cannot_dispatch_even_with_a_stale_intent() {
        let mut state = opened();
        for action in [
            Action::Row(RowId::CommandPalette),
            Action::Row(RowId::CloseReplay),
            Action::Workspace(WorkspaceCommand::BeginRename(
                quantick_stores::workspaces::DEFAULT_WORKSPACE.into(),
            )),
        ] {
            assert_eq!(
                Model::update(&mut state, Intent::Events(vec![Event::Activate(action)])),
                Effects::default()
            );
        }
        assert!(state.snapshot.as_ref().unwrap().open);
    }
    #[test]
    fn switches_stay_open_and_commands_close_the_menu() {
        let mut state = opened();
        let effect = Model::update(
            &mut state,
            Intent::Events(vec![Event::Activate(Action::SaveOnExit(true))]),
        );
        assert_eq!(effect.actions, vec![Action::SaveOnExit(true)]);
        assert!(!effect.closed);
        let effect = Model::update(
            &mut state,
            Intent::Events(vec![Event::Activate(Action::Row(RowId::Settings))]),
        );
        assert_eq!(effect.actions, vec![Action::Row(RowId::Settings)]);
        assert!(effect.closed);
        assert_eq!(
            Model::update(
                &mut state,
                Intent::Events(vec![Event::Activate(Action::Row(RowId::Settings))])
            ),
            Effects::default()
        );
    }
    #[test]
    fn keyboard_navigation_resets_when_the_menu_changes_door() {
        let mut state = opened();
        Model::update(
            &mut state,
            Intent::Events(vec![Event::OpenSubmenu(0, SubmenuId::Panels, true)]),
        );
        assert_eq!(state.path[0], Some(SubmenuId::Panels));
        assert_eq!(state.focus_pane, Some(1));
        let mut next = state.snapshot.clone().unwrap();
        next.door = Door::Toolbar;
        next.keyboard = true;
        Model::update(&mut state, Intent::Refresh(Box::new(next)));
        assert_eq!(state.path, [None; 2]);
        assert_eq!(state.focus_pane, Some(0));
        Model::update(&mut state, Intent::Events(vec![Event::Dismiss]));
        assert_eq!(state.path, [None; 2]);
        assert_eq!(state.focus_pane, None);
    }
}
