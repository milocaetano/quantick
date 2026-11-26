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

use crate::defaults::palette_on;
use crate::{Drawing, Drawings, PresetHost, ToolCatalog};

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
        self.style == other.style && self.payload.same_look(other.payload.as_ref())
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

    /// This pane's saved defaults, read and written through `host`: the one
    /// value every path that opens, saves or resets a default for this pane
    /// holds.
    #[must_use]
    pub fn preset_context<H>(&self, host: H) -> crate::PresetContext<H> {
        crate::PresetContext::new(host, self.canvas)
    }

    /// The palette of this pane's ground, from the last
    /// [`Self::follow_ground`]: what its marks paint their theme tokens
    /// with (a profile's casing and POC), so a time chart made dark paints
    /// them as the dark ground does. `None` before the first call.
    #[must_use]
    pub fn palette(&self) -> Option<&ResolvedTheme> {
        self.ground_ink.as_ref()?.palette.as_ref()
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
        let palette = ink.palette.as_ref();
        self.each_copy(|items| {
            for drawing in items.iter_mut() {
                if drawing.look != Inheritance::Theme {
                    continue;
                }
                // Only what the theme decides: the tool's ink and the
                // payload's theme defaults. A band's extension, a Fib's
                // levels and a note's words are config, never re-imported.
                drawing.style = drawing.tool.themed_style(palette);
                if let Some(palette) = palette {
                    drawing.payload.apply_theme_defaults(palette);
                }
            }
        });
        self.ground_ink = Some(ink);
    }
}

#[cfg(test)]
#[path = "look_tests.rs"]
mod tests;
