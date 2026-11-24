//! Where a mark's look comes from, and keeping the marks that follow their
//! pane's ground in step with it.
//!
//! A mark opened from its tool's defaults follows the ground of the pane it
//! sits on ([`Drawing::look`] `Theme`): when the theme or the canvas the pane
//! paints moves, it takes what a new mark there would open with: that
//! ground's saved default, else the factory look. Saving a default moves
//! neither, so it leaves the marks on the chart as they are. An edit of its
//! look makes it the trader's (`Explicit`), and nothing repaints it again.

use quantick_appearance::{Inheritance, PaneCanvas};

use crate::{Drawing, DrawingStyle, Drawings, PresetHost, ToolCatalog, new_drawing_from_defaults};

impl<T> Drawing<T> {
    /// Whether its look follows its pane's ground rather than being the
    /// trader's own: what the file and the control plane record.
    #[must_use]
    pub fn follows_ground(&self) -> bool {
        self.look == Inheritance::Theme
    }

    /// Whether `other` looks the same: the style, and the payload's preset
    /// export, which is the look a default stores. A note's words are
    /// content, so writing them leaves the look alone.
    fn same_look(&self, other: &Self) -> bool {
        self.style == other.style
            && (self.payload.eq_dyn(other.payload.as_ref())
                || self.payload.export_preset() == other.payload.export_preset())
    }

    /// Make the look the trader's when this edit of `before` changed it.
    /// Once the trader's, an edit never hands it back to the ground.
    pub(crate) fn own_look_edit(&mut self, before: &Self) {
        if before.look == Inheritance::Explicit || !self.same_look(before) {
            self.look = Inheritance::Explicit;
        }
    }
}

impl<T> Drawings<T> {
    /// An empty store whose marks will sit on `canvas`: a pane seeds the
    /// canvas it is built with, so what it places or saves before its first
    /// [`Self::follow_ground`] reads the right ground.
    #[must_use]
    pub fn on_canvas(canvas: PaneCanvas) -> Self {
        Self {
            canvas,
            ..Self::default()
        }
    }
}

impl<T: ToolCatalog> Drawings<T> {
    /// The canvas this pane's marks were last resolved on: what every path
    /// that opens, saves or edits a default for this pane reads its ground
    /// from ([`Self::follow_ground`]).
    #[must_use]
    pub fn canvas(&self) -> PaneCanvas {
        self.canvas
    }

    /// Bring the marks that follow their ground to the look a new mark of
    /// their tool opens with on `canvas` — the pane's painted canvas — when
    /// it or `host`'s theme moved since the last call, and the marks that
    /// arrived from elsewhere since. A saved default is no move. Called once
    /// per frame; does the work only on a move or an arrival.
    ///
    /// Never an edit: the undo history and the gesture baseline move with
    /// the live marks (an undo must not bring back the other ground's ink),
    /// the revision stays, and the marks keep following. A note keeps its
    /// words and a profile its fold. Answers whether it re-resolved.
    pub fn follow_ground(&mut self, host: &dyn PresetHost, canvas: PaneCanvas) -> bool {
        let theme = host.theme_revision();
        let moved = self.canvas != canvas || self.look_revision != Some(theme);
        if !moved && self.arrived.is_empty() {
            return false;
        }
        self.canvas = canvas;
        self.look_revision = Some(theme);
        let arrived = std::mem::take(&mut self.arrived);
        // One opening per tool, however many copies the history holds.
        let mut looks: Vec<(T, DrawingStyle, Option<toml::Value>)> = Vec::new();
        self.each_copy(|items| {
            for drawing in items.iter_mut() {
                if drawing.look != Inheritance::Theme || !(moved || arrived.contains(&drawing.id)) {
                    continue;
                }
                let at = looks
                    .iter()
                    .position(|(tool, ..)| *tool == drawing.tool)
                    .unwrap_or_else(|| {
                        let fresh = new_drawing_from_defaults(host, canvas, drawing.tool);
                        looks.push((drawing.tool, fresh.style, fresh.payload.export_preset()));
                        looks.len() - 1
                    });
                let (_, style, payload) = &looks[at];
                drawing.style = *style;
                if let Some(payload) = payload {
                    drawing.payload.import_preset(payload);
                }
            }
        });
        true
    }
}

#[cfg(test)]
#[path = "look_tests.rs"]
mod tests;
