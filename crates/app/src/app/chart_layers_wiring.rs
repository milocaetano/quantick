//! Keeping the chart layers and what the panes do with them in step.
//!
//! The layer actions a pane's menu could not apply itself, the footprint
//! change, the mask the panes read, and the restore that puts the saved layer
//! states back. Named `chart_layers_wiring` rather than `chart_layers`
//! because [`crate::chart_layers`] is the layer model itself: this is the
//! window's side of the wire, and one name for both would make the import
//! at the top of `app.rs` ambiguous to a reader.

use std::time::Instant;

use crate::chart_layers;

use super::QuantickApp;

impl QuantickApp {
    /// What a pane's layer menu could not switch itself.
    ///
    /// Drained right after the canvas, so the frame that clicked the entry is
    /// the frame that applies it. Both wishes reach the real owner — the shared
    /// style, the indicator state file — instead of a second copy on the pane.
    pub(super) fn apply_layer_actions(&mut self) {
        let actions = std::mem::take(&mut self.workspace.layers_mut().actions);
        if let Some(visible) = actions.grid {
            self.style.canvas.grid_enabled = visible;
            // The appearance panel's own edits bump this; the renderer and the
            // style log both read it to know something moved.
            self.style_revision = self.style_revision.saturating_add(1);
        }
        if actions.indicators_changed {
            self.note_indicator_edit_at(self.active_tab().id, self.active_tab().focused_side());
        }
        if actions.footprint_changed {
            crate::footprint_config::save(
                self.workspace.footprint_settings_path(),
                &self.footprint_config,
            );
        }
        if actions.open_footprint_settings {
            self.surfaces.footprint_settings.open();
        }
    }

    /// Settle every tab's paper panel and hand its acknowledgement to the
    /// window's one toast.
    ///
    /// # One lane, and what that cost
    ///
    /// The panel used to draw its own toast: the same `CENTER_BOTTOM` anchor
    /// as `ToastSurface`, 96px up instead of 44, on a 4-second clock instead
    /// of 8. Two acknowledgements could therefore sit in one lane, at two
    /// heights, disagreeing about how long an acknowledgement lasts. There is
    /// one now, and this is where the panel's messages join it.
    ///
    /// # Every tab, not just the one on screen
    ///
    /// `settle` runs for all of them because the jobs it finishes — an
    /// export, an import — belong to the tab that started them, and a trader
    /// who starts an export and then looks at another chart should not have
    /// to come back for it to land. The acknowledgements follow: a stop
    /// filling on a chart the trader is not looking at is precisely the news
    /// they most need, and dropping it silently is what the old per-tab toast
    /// did.
    ///
    /// A message from a background tab is **named**, because an unlabelled
    /// "SIM: dropped at the fill" would read as being about the chart on
    /// screen.
    ///
    /// # Which message wins a slot that holds one
    ///
    /// The watched tab's, always: it carries no prefix and is posted last, so
    /// it takes the slot from any background message raised on the same
    /// frame. Among background tabs the **first** in tab order wins and the
    /// rest of that frame are dropped — the same first-wins rule
    /// `SurfaceResponse::merge` uses for a request that carries a value, so
    /// the window has one tie-break rule rather than two. Posting each of
    /// them in turn would look like it showed them all and would in fact
    /// show whichever `tabs.iter()` reached last, which is tab order deciding
    /// in silence.
    pub(super) fn settle_paper_panels(&mut self, now: Instant) {
        let Self {
            tabs,
            active_tab,
            surfaces,
            ..
        } = self;
        let mut watched = None;
        let mut background = None;
        for (index, tab) in tabs.iter_mut().enumerate() {
            tab.paper.settle();
            let Some(message) = tab.paper.take_toast() else {
                continue;
            };
            if index == *active_tab {
                watched = Some(message);
            } else if background.is_none() {
                // The interpunct is the window's own separator — the status
                // bar, the tape's axis caption and the layout strip all use
                // it, and the messages themselves already carry a colon
                // (`SIM: …`). A second one would read as two labels.
                background = Some(format!("{} · {message}", tab.symbol));
            }
        }
        if let Some(message) = background {
            surfaces.toast.note(message, now);
        }
        if let Some(message) = watched {
            surfaces.toast.note(message, now);
        }
    }

    /// Apply what the footprint settings window settled on.
    ///
    /// Whatever is edited also becomes the window default, which is what a
    /// trader configuring their first chart means; a second chart diverges
    /// only when they configure it too.
    pub(super) fn apply_footprint_change(&mut self, change: crate::surfaces::FootprintChange) {
        let side = self.active_tab().focused_side();
        match change {
            crate::surfaces::FootprintChange::Applied(edited) => {
                self.active_tab_mut().pane_mut(side).footprint.config = Some((*edited).clone());
                self.footprint_config = *edited;
                crate::footprint_config::save(
                    self.workspace.footprint_settings_path(),
                    &self.footprint_config,
                );
            }
            crate::surfaces::FootprintChange::ResetToDefault => {
                self.active_tab_mut().pane_mut(side).footprint.config = None;
            }
        }
    }

    /// The visibility this app persists, as one bit per layer.
    ///
    /// Read off the active tab's flow pane: the file records the canvas
    /// quantick is built around, the same scope the indicator state file has
    /// (see [`Self::apply_pending_indicator_state`]). A tab's second pane opens
    /// matching it and is in-session from there.
    pub(super) fn layer_mask(&self) -> u32 {
        self.active_tab().flow_pane.layer_mask(&self.style)
    }

    pub(super) fn maintain_chart_layers(&mut self) {
        let tab = &self.tabs[self.active_tab];
        chart_layers::maintain(&mut self.workspace, tab.id, &tab.flow_pane, &self.style);
    }

    pub(super) fn restore_chart_layers(&mut self) {
        chart_layers::restore(
            &mut self.workspace,
            &mut self.tabs,
            self.active_tab,
            &mut self.style,
        );
    }
}
