//! Thin host reads and intents over the catalog-owned tool rail.
use super::{
    Intent, RailControl, RailControlKind, RailSlot, RailStage, Tool, ToolRail, ToolboxDock,
    tool_slots,
};
use crate::drawings::{DRAWING_TOOLS, DrawingTool};
use eframe::egui;
pub(crate) trait ToolRailExt {
    #[cfg(test)]
    fn new_rail() -> Self;
    fn tool(&self) -> Tool;
    fn visible(&self) -> bool;
    fn dock(&self) -> ToolboxDock;
    fn set_dock(&mut self, dock: ToolboxDock);
    fn set_visible(&mut self, visible: bool);
    fn painted_controls(&self) -> Vec<RailControl>;
    fn favorite_control(&self, tool: DrawingTool) -> RailControl;
    fn drag_active(&self) -> bool;
    fn toggle_visible(&mut self);
    fn arm(&mut self, tool: Tool);
    fn favorites(&self) -> &[DrawingTool];
    #[cfg(test)]
    fn is_favorite(&self, tool: DrawingTool) -> bool;
    #[cfg(test)]
    fn toggle_favorite(&mut self, tool: DrawingTool);
    fn take_favorites_change(&mut self) -> bool;
    fn set_favorites(&mut self, ids: &[String]) -> Vec<String>;
    #[cfg(any(feature = "drawing-harness", test))]
    fn set_band_offset(&mut self, offset: f32);
    fn repeat(&self) -> bool;
    fn magnet(&self) -> bool;
    fn set_magnet(&mut self, magnet: bool) -> bool;
    fn take_magnet_request(&mut self) -> Option<bool>;
    fn magnet_painted(&self) -> bool;
    #[cfg(any(feature = "drawing-harness", test))]
    fn request_flyout(&mut self, family_id: String);
    fn handle_keys(&mut self, ctx: &egui::Context);
    #[cfg(test)]
    fn set_repeat(&mut self, repeat: bool);
    #[cfg(test)]
    fn magnet_button_rect(&self) -> Option<egui::Rect>;
    #[cfg(test)]
    fn button_rect(&self, tool: Tool) -> Option<egui::Rect>;
    #[cfg(test)]
    fn objects_button_rect(&self) -> Option<egui::Rect>;
    #[cfg(test)]
    fn flyout_row_rect(&self, tool: DrawingTool) -> Option<egui::Rect>;
    #[cfg(any(feature = "drawing-harness", test))]
    fn apply_launch(&mut self, launch: ToolRailLaunch) -> ToolRailLaunchOutcome;
    #[cfg(test)]
    fn family_member(
        &self,
        family: crate::drawings::ToolFamily,
        members: &[DrawingTool],
    ) -> Option<DrawingTool>;
}
impl ToolRailExt for ToolRail {
    #[cfg(test)]
    fn new_rail() -> Self {
        Self::new(super::View::default(), super::effects)
            .in_slots(&[quantick_ui_model::Slot::EdgePanels])
    }
    fn tool(&self) -> Tool {
        self.model_state.tool
    }
    fn visible(&self) -> bool {
        self.model_state.visible
    }
    fn dock(&self) -> ToolboxDock {
        self.model_state.dock
    }
    fn set_dock(&mut self, dock: ToolboxDock) {
        self.update(Intent::SetDock(dock));
    }
    fn set_visible(&mut self, visible: bool) {
        self.update(Intent::SetVisible(visible));
        if !visible {
            self.view.last_stage = None;
            self.view.last_band = None;
        }
    }
    fn painted_controls(&self) -> Vec<RailControl> {
        if !self.model_state.visible {
            return Vec::new();
        }
        let Some(stage) = self.view.last_stage else {
            return Vec::new();
        };
        let armed_drawing = self.model_state.tool.drawing_tool();
        let mut controls = vec![RailControl {
            kind: RailControlKind::Tool,
            id: Tool::Pointer.id(),
            label: Tool::Pointer.name(),
            armed: self.model_state.tool == Tool::Pointer,
        }];
        // Minimal drops the crosshair; every wider stage keeps it.
        if stage != RailStage::Minimal {
            controls.push(RailControl {
                kind: RailControlKind::Tool,
                id: Tool::Crosshair.id(),
                label: Tool::Crosshair.name(),
                armed: self.model_state.tool == Tool::Crosshair,
            });
        }
        match stage {
            // The whole run has a button, and so does every star: nothing is
            // clipped and nothing is folded away at this width.
            RailStage::Full => {
                for tool in &self.model_state.favorites {
                    controls.push(self.favorite_control(*tool));
                }
                for slot in tool_slots() {
                    controls.push(slot_control(slot, armed_drawing));
                }
            }
            // The band clips. Only the anchored stars stay outside it; the
            // rest of the stars and the whole tool run scroll behind two
            // chevrons, and only the window the draw recorded is on screen.
            RailStage::Scroll => {
                let Some(band) = self.view.last_band else {
                    return controls;
                };
                let anchored = band.anchored.min(self.model_state.favorites.len());
                for tool in &self.model_state.favorites[..anchored] {
                    controls.push(self.favorite_control(*tool));
                }
                let slots = tool_slots();
                let spilled = self.model_state.favorites.len() - anchored;
                let content = spilled + slots.len();
                let first = band.first.min(content);
                let last = first.saturating_add(band.visible).min(content);
                for index in first..last {
                    controls.push(match index.checked_sub(spilled) {
                        Some(slot) => slot_control(&slots[slot], armed_drawing),
                        None => self.favorite_control(self.model_state.favorites[anchored + index]),
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
            armed: self.model_state.tool == Tool::Drawing(tool),
        }
    }
    fn drag_active(&self) -> bool {
        self.model_state.dragging
    }
    fn toggle_visible(&mut self) {
        self.set_visible(!self.visible());
    }
    fn arm(&mut self, tool: Tool) {
        self.update(Intent::Arm(tool));
    }
    fn favorites(&self) -> &[DrawingTool] {
        &self.model_state.favorites
    }
    #[cfg(test)]
    fn is_favorite(&self, tool: DrawingTool) -> bool {
        self.model_state.favorites.contains(&tool)
    }
    #[cfg(test)]
    fn toggle_favorite(&mut self, tool: DrawingTool) {
        self.update(Intent::ToggleFavorite(tool));
    }
    fn take_favorites_change(&mut self) -> bool {
        let changed = self.model_state.favorites_changed;
        self.update(Intent::AcknowledgeFavorites);
        changed
    }
    fn set_favorites(&mut self, ids: &[String]) -> Vec<String> {
        let response = self.update(Intent::RestoreFavorites(ids.to_vec()));
        response
            .model_effects
            .into_iter()
            .find_map(|effect| {
                if let quantick_ui_model::HostEffect::ToolRail(effects) = effect {
                    effects.into_iter().find_map(|effect| match effect {
                        quantick_ui_model::tool_rail::Effect::UnknownFavorites(ids) => Some(ids),
                        _ => None,
                    })
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn set_band_offset(&mut self, offset: f32) {
        self.update(Intent::RequestScroll(offset));
    }
    fn repeat(&self) -> bool {
        self.model_state.repeat
    }
    fn magnet(&self) -> bool {
        self.model_state.magnet
    }
    fn set_magnet(&mut self, magnet: bool) -> bool {
        let changed = self.model_state.magnet != magnet;
        self.update(Intent::ConfirmMagnet(magnet));
        changed
    }
    fn take_magnet_request(&mut self) -> Option<bool> {
        let request = self.model_state.magnet_request;
        self.update(Intent::AcknowledgeMagnet);
        request
    }
    fn magnet_painted(&self) -> bool {
        self.model_state.visible
            && self
                .view
                .last_stage
                .is_some_and(|stage| stage != RailStage::Minimal)
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn request_flyout(&mut self, family_id: String) {
        self.update(Intent::RequestFlyout(family_id));
    }
    fn handle_keys(&mut self, ctx: &egui::Context) {
        // A hidden rail arms nothing (audit M9): with no rail on screen an
        // armed tool has no indication anywhere, and the next chart click
        // would draw instead of pan — the keyboard twin of the invariant
        // `hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed`.
        if !self.model_state.visible {
            return;
        }
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let armed = ctx.input(|input| {
            if input.modifiers.command || input.modifiers.alt {
                return None;
            }
            // Bare digits belong to the quick bar switch, as in ProfitChart;
            // Escape still returns to the pointer.
            DRAWING_TOOLS.into_iter().find_map(|tool| {
                tool.shortcut()
                    .filter(|shortcut| {
                        // A tool's shortcut is a *bare* key (with Shift where
                        // the tool asks for it). Ctrl+M, Cmd+M and Alt+M
                        // belong to whoever claims them — the mark hotkey does
                        // — and must not also arm a tool out from under the
                        // trader's hand.
                        crate::drawings::shortcut_key(*shortcut)
                            .is_some_and(|key| input.key_pressed(key))
                            && input.modifiers.shift == shortcut.shift
                            && !input.modifiers.ctrl
                            && !input.modifiers.command
                            && !input.modifiers.alt
                    })
                    .map(|_| Tool::Drawing(tool))
            })
        });
        if let Some(tool) = armed {
            self.arm(tool);
        }
    }
    #[cfg(test)]
    fn set_repeat(&mut self, repeat: bool) {
        self.update(Intent::SetRepeat(repeat));
    }
    #[cfg(test)]
    fn magnet_button_rect(&self) -> Option<egui::Rect> {
        self.view.magnet_rect
    }
    #[cfg(test)]
    fn button_rect(&self, tool: Tool) -> Option<egui::Rect> {
        self.view
            .button_rects
            .iter()
            .flatten()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }
    #[cfg(test)]
    fn objects_button_rect(&self) -> Option<egui::Rect> {
        self.view.objects_rect
    }
    #[cfg(test)]
    fn flyout_row_rect(&self, tool: DrawingTool) -> Option<egui::Rect> {
        self.view
            .flyout_rects
            .iter()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }
    #[cfg(any(feature = "drawing-harness", test))]
    fn apply_launch(&mut self, launch: ToolRailLaunch) -> ToolRailLaunchOutcome {
        if let Some(tool) = launch.tool {
            self.arm(Tool::Drawing(tool));
        }
        if launch.magnet {
            self.set_magnet(true);
        }
        let favorites_staged = launch.favorites.is_some();
        if let Some(ids) = launch.favorites {
            self.set_favorites(&ids);
        }
        if let Some(dock) = launch.dock {
            self.set_dock(dock);
        }
        if let Some(offset) = launch.scroll {
            self.set_band_offset(offset);
        }
        if let Some(family) = launch.flyout {
            self.request_flyout(family);
        }
        ToolRailLaunchOutcome { favorites_staged }
    }
    #[cfg(test)]
    fn family_member(
        &self,
        family: crate::drawings::ToolFamily,
        members: &[DrawingTool],
    ) -> Option<DrawingTool> {
        self.model_state.family_member(family, members)
    }
}
/// Inputs captured once for the drawing rail's existing launch phase.
#[cfg(any(feature = "drawing-harness", test))]
#[derive(Default)]
pub(crate) struct ToolRailLaunch {
    tool: Option<DrawingTool>,
    magnet: bool,
    favorites: Option<Vec<String>>,
    dock: Option<ToolboxDock>,
    scroll: Option<f32>,
    flyout: Option<String>,
}

#[cfg(any(feature = "drawing-harness", test))]
impl ToolRailLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<std::ffi::OsString>) -> Self {
        let mut read = |name| lookup(name).and_then(|value| value.into_string().ok());
        let tool = read("QUANTICK_DRAWING_TOOL").and_then(|id| {
            DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == id.trim())
        });
        let magnet = read("QUANTICK_DRAWING_MAGNET").is_some_and(|value| value == "1");
        let favorites = read("QUANTICK_TOOL_FAVORITES").map(|ids| {
            ids.split(',')
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .collect()
        });
        let dock = read("QUANTICK_TOOLBOX_DOCK").and_then(|edge| match edge.trim() {
            "left" => Some(ToolboxDock::Left),
            "top" => Some(ToolboxDock::Top),
            "bottom" => Some(ToolboxDock::Bottom),
            _ => None,
        });
        let scroll = read("QUANTICK_TOOLBAR_SCROLL").and_then(|offset| match offset.trim() {
            "end" => Some(f32::INFINITY),
            other => other.parse::<f32>().ok().filter(|at| at.is_finite()),
        });
        let flyout = read("QUANTICK_TOOLBOX_FLYOUT").map(|id| id.trim().to_owned());
        Self {
            tool,
            magnet,
            favorites,
            dock,
            scroll,
            flyout,
        }
    }
}

/// The host keeps persistence policy; the rail reports explicit staging.
#[cfg(any(feature = "drawing-harness", test))]
pub(crate) struct ToolRailLaunchOutcome {
    pub favorites_staged: bool,
}

fn slot_control(slot: &RailSlot, armed_drawing: Option<DrawingTool>) -> RailControl {
    match slot {
        RailSlot::Single(tool) => RailControl {
            kind: RailControlKind::Tool,
            id: tool.id(),
            label: tool.name(),
            armed: armed_drawing == Some(*tool),
        },
        RailSlot::Family { family, members } => RailControl {
            kind: RailControlKind::Family,
            id: family.id,
            label: family.title,
            armed: armed_drawing.is_some_and(|tool| members.contains(&tool)),
        },
    }
}
