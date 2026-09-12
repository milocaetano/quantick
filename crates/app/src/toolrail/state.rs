//! The rail's own state: the armed tool, the dock, the starred tools,
//! the scroll band's offset, the hit rectangles a script reads, and the
//! keyboard shortcuts that arm a tool.

use super::*;

impl ToolRail {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }

    #[must_use]
    pub fn dock(&self) -> ToolboxDock {
        self.dock
    }

    /// Dock the rail against `dock` — the menu path beside dragging.
    pub fn set_dock(&mut self, dock: ToolboxDock) {
        self.dock = dock;
    }

    /// Show or hide the rail outright, for a saved workspace restoring the
    /// state it recorded rather than toggling from whatever this launch
    /// happens to be in.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.toggle_visible();
        }
    }

    /// The controls the rail painted last frame, in rail order.
    ///
    /// Folded through the very `tool_slots()` the draw folds through, and cut
    /// by the stage and the band window the draw recorded, so this can never
    /// name a button that is behind a flyout, off the scrolled run, or inside
    /// the More menu. A rail nobody can see, and one that has not been drawn
    /// yet, both report nothing: the guarantee is made here rather than left
    /// to whichever caller remembers to ask [`Self::visible`] first.
    #[must_use]
    pub(crate) fn painted_controls(&self) -> Vec<RailControl> {
        if !self.visible {
            return Vec::new();
        }
        let Some(stage) = self.last_stage else {
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
                    controls.push(Self::slot_control(slot, armed_drawing));
                }
            }
            // The band clips. Only the anchored stars stay outside it; the
            // rest of the stars and the whole tool run scroll behind two
            // chevrons, and only the window the draw recorded is on screen.
            RailStage::Scroll => {
                let Some(band) = self.last_band else {
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
                        Some(slot) => Self::slot_control(&slots[slot], armed_drawing),
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

    /// One entry of the folded tool run, as the draw paints it.
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

    /// One pinned button. A star is a second button for a tool that also has
    /// a slot in the run, so it is named in its own namespace rather than
    /// answering to the slot's name.
    fn favorite_control(&self, tool: DrawingTool) -> RailControl {
        RailControl {
            kind: RailControlKind::Favorite,
            id: tool.id(),
            label: tool.name(),
            armed: self.tool == Tool::Drawing(tool),
        }
    }

    /// Whether a grip drag is live this frame — the app's escape stack must
    /// yield Esc to the drag while it is.
    #[must_use]
    pub fn drag_active(&self) -> bool {
        self.dragging
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.tool = Tool::Pointer;
            // Nothing is painted while the rail is away, so what the last
            // draw settled on stops describing the screen. Forgotten rather
            // than kept: control captures are served before the rail draws,
            // so a stale stage would answer the first capture after a hide
            // with the buttons of a rail nobody can see.
            self.last_stage = None;
            self.last_band = None;
        }
    }

    pub fn arm(&mut self, tool: Tool) {
        if let Tool::Drawing(drawing_tool) = tool
            && let Some(family) = drawing_tool.family()
        {
            self.last_family_member.insert(family.id, drawing_tool);
        }
        if self.tool != tool {
            self.reveal_armed = true;
        }
        self.tool = tool;
    }

    /// The starred tools, in the order they were starred.
    #[must_use]
    pub fn favorites(&self) -> &[DrawingTool] {
        &self.favorites
    }

    #[must_use]
    pub fn is_favorite(&self, tool: DrawingTool) -> bool {
        self.favorites.contains(&tool)
    }

    /// Star or unstar a tool. Starring appends — the pinned section grows at
    /// its far end, and the tools already pinned never move.
    pub fn toggle_favorite(&mut self, tool: DrawingTool) {
        if let Some(index) = self.favorites.iter().position(|entry| *entry == tool) {
            self.favorites.remove(index);
        } else {
            self.favorites.push(tool);
        }
        self.favorites_changed = true;
    }

    /// Whether the trader starred or unstarred something since this was last
    /// asked, clearing the flag.
    ///
    /// The rail curates the list; it does not know where lists are kept. The
    /// app reads this on the frame the star was clicked and writes the choice
    /// down — the same hand-off the replay browser's folder pick uses, and for
    /// the same reason: "it forgot my tools again" must not be one crash away.
    pub fn take_favorites_change(&mut self) -> bool {
        std::mem::take(&mut self.favorites_changed)
    }

    /// Restore the starred list from saved tool ids — the workspace file
    /// path. An id no registered tool carries is dropped, a duplicate keeps
    /// its first position, and saved order is kept: it is the order the
    /// trader starred in.
    ///
    /// Returns the ids it did not recognise, so the caller can say so. The
    /// prune used to be private and harmless — it only reached the disk on an
    /// explicit save. Now the trader's next star click writes this list back,
    /// which makes the loss permanent, and losing a saved id without a word is
    /// exactly the silent patching `CLAUDE.md` rules out. The sibling restore
    /// logs `UI_STATE_TAB_DROPPED` for a tab it cannot open, for the same
    /// reason.
    pub fn set_favorites(&mut self, ids: &[String]) -> Vec<String> {
        self.favorites.clear();
        let mut unknown = Vec::new();
        for id in ids {
            match DrawingTool::by_id(id) {
                Some(tool) if !self.favorites.contains(&tool) => self.favorites.push(tool),
                // A duplicate is not a loss: the first position stands.
                Some(_) => {}
                None => unknown.push(id.clone()),
            }
        }
        unknown
    }

    /// Park the scrolling tool band at `offset` px along the rail — the
    /// validation hook for a state that otherwise takes a chevron click.
    /// The band clamps it on the next frame, so `f32::INFINITY` means "the
    /// far end" and a rail that does not scroll ignores it entirely.
    pub fn set_band_offset(&mut self, offset: f32) {
        self.band_target = Some(offset.max(0.0));
    }

    /// Whether the repeat pin keeps the tool armed after an object completes.
    #[must_use]
    pub fn repeat(&self) -> bool {
        self.repeat
    }

    #[cfg(test)]
    pub(crate) fn set_repeat(&mut self, repeat: bool) {
        self.repeat = repeat;
    }

    /// Whether placed anchors snap to the bar's open / high / low / close.
    #[must_use]
    pub fn magnet(&self) -> bool {
        self.magnet
    }

    /// Arm the magnet without a click — the `QUANTICK_DRAWING_MAGNET` hook
    /// and the tests both come through here, so neither can drift from what
    /// the button does.
    pub(crate) fn set_magnet(&mut self, magnet: bool) {
        self.magnet = magnet;
    }

    /// Ask for a family flyout without a click — the
    /// `QUANTICK_TOOLBOX_FLYOUT` hook. The slot honours it on its next draw,
    /// when the anchor rect exists; an unknown family id is simply never
    /// matched and stays pending, which draws nothing.
    pub(crate) fn request_flyout(&mut self, family_id: String) {
        self.hook_flyout = Some(family_id);
    }

    #[cfg(test)]
    pub(crate) fn button_rect(&self, tool: Tool) -> Option<egui::Rect> {
        self.button_rects
            .iter()
            .flatten()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }

    #[cfg(test)]
    pub(crate) fn objects_button_rect(&self) -> Option<egui::Rect> {
        self.objects_rect
    }

    #[cfg(test)]
    pub(crate) fn flyout_row_rect(&self, tool: DrawingTool) -> Option<egui::Rect> {
        self.flyout_rects
            .iter()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }

    /// Tool-arming keys. Escape lives in the app's escape stack (rail drag →
    /// input → draft → selection → Pointer), not here. Each drawing tool
    /// declares its own shortcut through the registry.
    pub fn handle_keys(&mut self, ctx: &egui::Context) {
        // A hidden rail arms nothing (audit M9): with no rail on screen an
        // armed tool has no indication anywhere, and the next chart click
        // would draw instead of pan — the keyboard twin of the invariant
        // `hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed`.
        if !self.visible {
            return;
        }
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let armed = ctx.input(|input| {
            if input.modifiers.command || input.modifiers.alt {
                return None;
            }
            if input.key_pressed(egui::Key::Num1) {
                return Some(Tool::Pointer);
            }
            if input.key_pressed(egui::Key::Num2) {
                return Some(Tool::Crosshair);
            }
            DRAWING_TOOLS.into_iter().find_map(|tool| {
                tool.shortcut()
                    .filter(|shortcut| {
                        // A tool's shortcut is a *bare* key (with Shift where
                        // the tool asks for it). Ctrl+M, Cmd+M and Alt+M
                        // belong to whoever claims them — the mark hotkey does
                        // — and must not also arm a tool out from under the
                        // trader's hand.
                        input.key_pressed(shortcut.key)
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
}
