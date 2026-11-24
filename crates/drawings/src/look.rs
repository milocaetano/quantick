//! Where a mark's look comes from, and keeping the marks with the factory
//! look in their pane's ground's ink.
//!
//! A mark opened with the factory look — nothing the trader saved reached it
//! ([`Drawing::look`] `Theme`) — wears its tool's factory ink for the ground
//! of the pane it sits on: when the theme or the canvas the pane paints
//! moves, and when it arrives from elsewhere (a load, a paste, another
//! chart's write). That ink is a pure function of the tool and the pane's
//! palette, never the saved defaults, so no save, reload or other pane can
//! repaint it otherwise. An edit of its look makes it the trader's
//! (`Explicit`), and nothing repaints it again.

use quantick_appearance::{Inheritance, PaneCanvas, ResolvedTheme};

use crate::defaults::{factory_opening, palette_on};
use crate::{Drawing, DrawingStyle, Drawings, PresetHost, ToolCatalog};

/// The palette a pane's factory-look marks were last dressed from, and the
/// theme revision it was read at; see [`Drawings::follow_ground`].
#[derive(Debug)]
pub(crate) struct GroundInk {
    theme: u64,
    palette: Option<ResolvedTheme>,
}

impl<T> Drawing<T> {
    /// Whether it keeps the factory look, worn in its pane's ground's ink,
    /// rather than a saved or edited one: what the file and the control
    /// plane record.
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
    /// [`Self::follow_ground`] reads the right ground. A pane seeds its
    /// surface only; the first frame's `follow_ground` corrects an explicit
    /// background.
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

    /// Dress the factory-look marks in the factory ink of `canvas` — the
    /// pane's painted canvas — when it or `host`'s theme moved since the
    /// last call. A saved default is no move: it never reaches them. Called
    /// once per frame; does the work only on a move. Answers whether it did.
    pub fn follow_ground(&mut self, host: &dyn PresetHost, canvas: PaneCanvas) -> bool {
        let theme = host.theme_revision();
        let same = self
            .ground_ink
            .as_ref()
            .is_some_and(|ink| ink.theme == theme);
        if same && self.canvas == canvas {
            return false;
        }
        let palette = palette_on(host.theme(), canvas).0.cloned();
        self.canvas = canvas;
        self.ground_ink = Some(GroundInk { theme, palette });
        self.wear_ground();
        true
    }

    /// Put the factory ink of the last [`Self::follow_ground`] on every mark
    /// that keeps the factory look; what marks arriving from elsewhere get at
    /// once, so none paints a frame in another pane's ink. Nothing before the
    /// first call: that call dresses them all.
    ///
    /// Never an edit: the undo history and the gesture baseline move with the
    /// live marks (an undo must not bring back the other ground's ink), the
    /// revision stays, and the marks keep the factory look. A note keeps its
    /// words and a profile its fold.
    pub(crate) fn wear_ground(&mut self) {
        let Some(ink) = self.ground_ink.take() else {
            return;
        };
        // One opening per tool, however many copies the history holds.
        let mut looks: Vec<(T, DrawingStyle, Option<toml::Value>)> = Vec::new();
        self.each_copy(|items| {
            for drawing in items.iter_mut() {
                if drawing.look != Inheritance::Theme {
                    continue;
                }
                let at = looks
                    .iter()
                    .position(|(tool, ..)| *tool == drawing.tool)
                    .unwrap_or_else(|| {
                        let fresh = factory_opening(ink.palette.as_ref(), drawing.tool);
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
        self.ground_ink = Some(ink);
    }
}

#[cfg(test)]
#[path = "look_tests.rs"]
mod tests;
