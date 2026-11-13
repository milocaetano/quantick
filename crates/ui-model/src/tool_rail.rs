//! Catalog-owned drawing rail decisions; drawing policy stays in Drawings.
pub use quantick_chart_interaction::tool_rail::ToolboxDock;
use quantick_drawings::{DrawingTool, ToolFamily};
use quantick_stores::ui_state::SavedRailDock;
use std::collections::BTreeMap;
/// A chart-acting tool. Only one is armed at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Pointer,
    Crosshair,
    Drawing(DrawingTool),
}

impl Tool {
    /// The stable wire name, for the places that name a tool rather than draw
    /// it. The drawing tools lend their own registered identifier, so a tool
    /// is called the same thing in the rail, in a saved workspace and in the
    /// semantic scene.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Crosshair => "crosshair",
            Self::Drawing(tool) => tool.id(),
        }
    }

    #[must_use]
    pub fn drawing_tool(self) -> Option<DrawingTool> {
        match self {
            Self::Drawing(tool) => Some(tool),
            Self::Pointer | Self::Crosshair => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer",
            Self::Crosshair => "Crosshair",
            Self::Drawing(tool) => tool.name(),
        }
    }

    #[must_use]
    pub fn hover_text(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer - pan, zoom, select and move (1, Esc)",
            Self::Crosshair => "Crosshair (2)",
            Self::Drawing(tool) => tool.hover_text(),
        }
    }

    /// The shortcut label shown in menus, `None` when the tool has no key.
    #[must_use]
    pub fn shortcut_label(self) -> Option<String> {
        match self {
            Self::Pointer => Some("1".to_owned()),
            Self::Crosshair => Some("2".to_owned()),
            Self::Drawing(tool) => tool.shortcut().map(|shortcut| {
                let key = shortcut.key.to_string();
                if shortcut.shift {
                    format!("Shift+{key}")
                } else {
                    key.to_owned()
                }
            }),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub count: usize,
    pub draft: bool,
    pub draft_len: usize,
    pub all_hidden: bool,
    pub all_locked: bool,
    pub manager_open: bool,
}
#[derive(Debug)]
pub struct State {
    pub tool: Tool,
    pub visible: bool,
    pub dock: ToolboxDock,
    pub repeat: bool,
    pub magnet: bool,
    pub magnet_request: Option<bool>,
    pub last_family_member: BTreeMap<&'static str, DrawingTool>,
    pub favorites: Vec<DrawingTool>,
    pub favorites_changed: bool,
    pub reveal_armed: bool,
    pub drag_preview: Option<ToolboxDock>,
    pub dragging: bool,
    pub drag_cancelled: bool,
    pub flyout: Option<&'static str>,
    pub requested_flyout: Option<String>,
    pub requested_scroll: Option<f32>,
    pub facts: Facts,
}
impl Default for State {
    fn default() -> Self {
        Self {
            tool: Tool::Pointer,
            visible: true,
            dock: ToolboxDock::Left,
            repeat: false,
            magnet: false,
            magnet_request: None,
            last_family_member: BTreeMap::new(),
            favorites: Vec::new(),
            favorites_changed: false,
            reveal_armed: false,
            drag_preview: None,
            dragging: false,
            drag_cancelled: false,
            flyout: None,
            requested_flyout: None,
            requested_scroll: None,
            facts: Facts::default(),
        }
    }
}
impl State {
    pub fn family_member(
        &self,
        family: ToolFamily,
        members: &[DrawingTool],
    ) -> Option<DrawingTool> {
        self.last_family_member
            .get(family.id)
            .copied()
            .filter(|member| members.contains(member))
    }
}
#[derive(Debug)]
pub enum Intent {
    Observe(Facts),
    Arm(Tool),
    SetVisible(bool),
    SetDock(ToolboxDock),
    SetRepeat(bool),
    ToggleFavorite(DrawingTool),
    RestoreFavorites(Vec<String>),
    AcknowledgeFavorites,
    RequestMagnet,
    ConfirmMagnet(bool),
    AcknowledgeMagnet,
    AcknowledgeReveal,
    BeginDrag,
    CancelDrag,
    PreviewDrag(ToolboxDock),
    EndDrag(Option<ToolboxDock>),
    OpenFlyout(&'static str),
    CloseFlyout,
    RequestFlyout(String),
    RequestScroll(f32),
    AcknowledgeScroll,
    SetAllHidden(bool),
    SetAllLocked(bool),
    SetManagerOpen(bool),
}
#[derive(Debug, PartialEq)]
pub enum Effect {
    SetAllHidden(bool),
    SetAllLocked(bool),
    SetManagerOpen(bool),
    UnknownFavorites(Vec<String>),
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Vec<Effect>;
    fn update(state: &mut State, intent: Intent) -> Vec<Effect> {
        match intent {
            Intent::Observe(facts) => state.facts = facts,
            Intent::Arm(tool) => {
                if let Tool::Drawing(drawing) = tool
                    && let Some(family) = drawing.family()
                {
                    state.last_family_member.insert(family.id, drawing);
                }
                state.reveal_armed |= state.tool != tool;
                state.tool = tool;
            }
            Intent::SetVisible(visible) => {
                state.visible = visible;
                if !visible {
                    state.tool = Tool::Pointer;
                }
            }
            Intent::SetDock(dock) => state.dock = dock,
            Intent::SetRepeat(repeat) => state.repeat = repeat,
            Intent::ToggleFavorite(tool) => {
                if let Some(index) = state.favorites.iter().position(|entry| *entry == tool) {
                    state.favorites.remove(index);
                } else {
                    state.favorites.push(tool);
                }
                state.favorites_changed = true;
            }
            Intent::RestoreFavorites(ids) => {
                state.favorites.clear();
                let mut unknown = Vec::new();
                for id in ids {
                    match DrawingTool::by_id(&id) {
                        Some(tool) if !state.favorites.contains(&tool) => {
                            state.favorites.push(tool)
                        }
                        Some(_) => {}
                        None => unknown.push(id),
                    }
                }
                return vec![Effect::UnknownFavorites(unknown)];
            }
            Intent::AcknowledgeFavorites => state.favorites_changed = false,
            Intent::RequestMagnet => state.magnet_request = Some(!state.magnet),
            Intent::ConfirmMagnet(on) => state.magnet = on,
            Intent::AcknowledgeMagnet => state.magnet_request = None,
            Intent::AcknowledgeReveal => state.reveal_armed = false,
            Intent::BeginDrag => {
                state.dragging = true;
                state.drag_cancelled = false;
            }
            Intent::CancelDrag => {
                state.dragging = false;
                state.drag_cancelled = true;
                state.drag_preview = None;
            }
            Intent::PreviewDrag(dock) => {
                if state.dragging {
                    state.drag_preview = Some(dock);
                }
            }
            Intent::EndDrag(dock) => {
                if state.dragging
                    && !state.drag_cancelled
                    && let Some(dock) = dock
                {
                    state.dock = dock;
                }
                state.dragging = false;
                state.drag_cancelled = false;
                state.drag_preview = None;
            }
            Intent::OpenFlyout(family) => {
                state.flyout = Some(family);
                state.requested_flyout = None;
            }
            Intent::CloseFlyout => state.flyout = None,
            Intent::RequestFlyout(family) => state.requested_flyout = Some(family),
            Intent::RequestScroll(offset) => state.requested_scroll = Some(offset.max(0.0)),
            Intent::AcknowledgeScroll => state.requested_scroll = None,
            Intent::SetAllHidden(hidden) => {
                return vec![Effect::SetAllHidden(hidden)];
            }
            Intent::SetAllLocked(locked) => {
                return vec![Effect::SetAllLocked(locked)];
            }
            Intent::SetManagerOpen(open) => {
                return vec![Effect::SetManagerOpen(open)];
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
#[path = "tool_rail/tests.rs"]
mod tests;

/// The saved word for the rail's dock.
pub const fn saved_rail_dock(dock: ToolboxDock) -> SavedRailDock {
    match dock {
        ToolboxDock::Left => SavedRailDock::Left,
        ToolboxDock::Top => SavedRailDock::Top,
        ToolboxDock::Bottom => SavedRailDock::Bottom,
    }
}

/// The rail's dock for a saved word; the retired right dock opens left.
pub const fn rail_dock(saved: SavedRailDock) -> ToolboxDock {
    match saved {
        SavedRailDock::Left | SavedRailDock::Right => ToolboxDock::Left,
        SavedRailDock::Top => ToolboxDock::Top,
        SavedRailDock::Bottom => ToolboxDock::Bottom,
    }
}
