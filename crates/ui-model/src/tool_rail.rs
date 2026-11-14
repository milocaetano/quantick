//! Catalog-owned drawing rail decisions; drawing policy stays in Drawings.
pub use quantick_chart_interaction::tool_rail::ToolboxDock;
use quantick_chart_interaction::tool_rail::{BandWindow, RailControl, RailControlKind, RailStage};
use quantick_drawings::{DRAWING_TOOLS, DrawingTool, ToolFamily, ToolShortcut};
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

    /// The tool a key press arms, `None` when it arms nothing.
    ///
    /// A hidden rail arms nothing (audit M9): with no rail on screen an armed
    /// tool has no indication anywhere, and the next chart click would draw
    /// instead of pan. A tool's shortcut is a *bare* key, with Shift where the
    /// tool asks for it: Ctrl, Cmd and Alt chords belong to whoever claims
    /// them (the mark hotkey does), and bare digits to the quick bar switch.
    pub fn keyed_tool(&self, chord: Chord, pressed: impl Fn(ToolShortcut) -> bool) -> Option<Tool> {
        if !self.visible || chord.ctrl || chord.command || chord.alt {
            return None;
        }
        DRAWING_TOOLS.into_iter().find_map(|tool| {
            tool.shortcut()
                .filter(|shortcut| shortcut.shift == chord.shift && pressed(*shortcut))
                .map(|_| Tool::Drawing(tool))
        })
    }

    /// The tools a stage swallowed into the More flyout: exactly the ones
    /// without a rail slot, in registry order.
    #[must_use]
    pub fn swallowed_tools(&self, stage: RailStage) -> Vec<Tool> {
        let armed = self.tool.drawing_tool();
        let mut swallowed = Vec::new();
        if stage == RailStage::Minimal {
            swallowed.push(Tool::Crosshair);
        }
        swallowed.extend(
            DRAWING_TOOLS
                .into_iter()
                .filter(|tool| Some(*tool) != armed)
                .map(Tool::Drawing),
        );
        swallowed
    }

    /// Whether the magnet is on screen: the trailing cluster of every drawn
    /// stage but Minimal, where it folds into More, and never on a hidden rail.
    #[must_use]
    pub fn magnet_painted(&self, stage: Option<RailStage>) -> bool {
        self.visible && stage.is_some_and(|stage| stage != RailStage::Minimal)
    }

    /// The controls the last draw put on screen, given the stage it settled on
    /// and the band window it showed; nothing before the first draw or while
    /// the rail is hidden.
    #[must_use]
    pub fn painted_controls(
        &self,
        stage: Option<RailStage>,
        band: Option<BandWindow>,
    ) -> Vec<RailControl> {
        let Some(stage) = stage.filter(|_| self.visible) else {
            return Vec::new();
        };
        let armed_drawing = self.tool.drawing_tool();
        let mut controls = vec![RailControl {
            kind: RailControlKind::Tool,
            id: Tool::Pointer.id(),
            label: Tool::Pointer.name(),
            armed: self.tool == Tool::Pointer,
        }];
        // Minimal drops the crosshair; every wider stage keeps it.
        if stage != RailStage::Minimal {
            controls.push(RailControl {
                kind: RailControlKind::Tool,
                id: Tool::Crosshair.id(),
                label: Tool::Crosshair.name(),
                armed: self.tool == Tool::Crosshair,
            });
        }
        match stage {
            // The whole run has a button, and so does every star: nothing is
            // clipped and nothing is folded away at this width.
            RailStage::Full => {
                for tool in &self.favorites {
                    controls.push(self.favorite_control(*tool));
                }
                for slot in tool_slots() {
                    controls.push(slot.control(armed_drawing));
                }
            }
            // The band clips. Only the anchored stars stay outside it; the
            // rest of the stars and the whole tool run scroll behind two
            // chevrons, and only the window the draw recorded is on screen.
            RailStage::Scroll => {
                let Some(band) = band else {
                    return controls;
                };
                let anchored = band.anchored.min(self.favorites.len());
                for tool in &self.favorites[..anchored] {
                    controls.push(self.favorite_control(*tool));
                }
                let slots = tool_slots();
                let spilled = self.favorites.len() - anchored;
                let content = spilled + slots.len();
                let first = band.first.min(content);
                let last = first.saturating_add(band.visible).min(content);
                for index in first..last {
                    controls.push(match index.checked_sub(spilled) {
                        Some(slot) => slots[slot].control(armed_drawing),
                        None => self.favorite_control(self.favorites[anchored + index]),
                    });
                }
            }
            // Only the armed tool keeps a button of its own; the rest are
            // behind More, which is not a control the scene can name yet.
            // The stars go with them — these stages draw no pinned section.
            RailStage::Compact | RailStage::Minimal => {
                if let Some(tool) = armed_drawing {
                    controls.push(RailControl {
                        kind: RailControlKind::Tool,
                        id: tool.id(),
                        label: tool.name(),
                        armed: true,
                    });
                }
            }
        }
        controls
    }

    fn favorite_control(&self, tool: DrawingTool) -> RailControl {
        RailControl {
            kind: RailControlKind::Favorite,
            id: tool.id(),
            label: tool.name(),
            armed: self.tool == Tool::Drawing(tool),
        }
    }
}

/// The modifiers held with a key press, as the host read them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub shift: bool,
    pub ctrl: bool,
    pub command: bool,
    pub alt: bool,
}

/// One leading-cluster slot after family folding: a lone tool, or a family
/// of consecutive registry entries sharing the slot.
#[derive(Debug)]
pub enum RailSlot {
    Single(DrawingTool),
    Family {
        family: ToolFamily,
        members: Vec<DrawingTool>,
    },
}

impl RailSlot {
    fn control(&self, armed_drawing: Option<DrawingTool>) -> RailControl {
        match self {
            Self::Single(tool) => RailControl {
                kind: RailControlKind::Tool,
                id: tool.id(),
                label: tool.name(),
                armed: armed_drawing == Some(*tool),
            },
            Self::Family { family, members } => RailControl {
                kind: RailControlKind::Family,
                id: family.id,
                label: family.title,
                armed: armed_drawing.is_some_and(|tool| members.contains(&tool)),
            },
        }
    }
}

/// Fold the registry into rail slots. Consecutive entries with the same
/// family id share one slot — consecutive, not sorted, so rail order stays
/// registry order and adding a tool cannot silently reorder the rail.
/// Folded once per process: the registry is `const`, so rebuilding this
/// every frame would be per-frame allocation of stable data.
pub fn tool_slots() -> &'static [RailSlot] {
    static SLOTS: std::sync::OnceLock<Vec<RailSlot>> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| {
        let mut slots: Vec<RailSlot> = Vec::new();
        for tool in DRAWING_TOOLS {
            match tool.family() {
                Some(family) => {
                    if let Some(RailSlot::Family {
                        family: previous,
                        members,
                    }) = slots.last_mut()
                        && previous.id == family.id
                    {
                        members.push(tool);
                    } else {
                        slots.push(RailSlot::Family {
                            family,
                            members: vec![tool],
                        });
                    }
                }
                None => slots.push(RailSlot::Single(tool)),
            }
        }
        slots
    })
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
