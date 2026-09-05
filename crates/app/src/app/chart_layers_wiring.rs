//! Keeping the chart layers and what the panes do with them in step.
//!
//! The layer actions a pane's menu could not apply itself, the footprint
//! change, the mask the panes read, and the restore that puts the saved layer
//! states back. Named `chart_layers_wiring` rather than `chart_layers`
//! because [`crate::chart_layers`] is the layer model itself: this is the
//! window's side of the wire, and one name for both would make the import
//! at the top of `app.rs` ambiguous to a reader.

use std::time::Instant;

use crate::chart_layers::{self, ChartLayer};

use super::QuantickApp;

impl QuantickApp {
    /// What a pane's layer menu could not switch itself.
    ///
    /// Drained right after the canvas, so the frame that clicked the entry is
    /// the frame that applies it. Both wishes reach the real owner — the shared
    /// style, the indicator state file — instead of a second copy on the pane.
    pub(super) fn apply_layer_actions(&mut self) {
        let actions = std::mem::take(&mut self.layer_actions);
        if let Some(visible) = actions.grid {
            self.style.canvas.grid_enabled = visible;
            // The appearance panel's own edits bump this; the renderer and the
            // style log both read it to know something moved.
            self.style_revision = self.style_revision.saturating_add(1);
        }
        if actions.indicators_changed {
            self.mark_indicator_state_dirty();
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
    /// (see [`Self::maintain_indicator_state`]). A tab's second pane opens
    /// matching it and is in-session from there.
    pub(super) fn layer_mask(&self) -> u32 {
        self.active_tab().flow_pane.layer_mask(&self.style)
    }

    /// Save the layer visibility when it differs from what is on disk.
    ///
    /// Called once per frame instead of from each switch: the layers are owned
    /// by four different pieces of chrome, and a save hook on each is four
    /// chances to forget one.
    pub(super) fn maintain_chart_layers(&mut self) {
        let mask = self.layer_mask();
        // Layer state is per-pane, and this reads the *active* tab's flow pane
        // — so activating a tab whose chart is set up differently changes the
        // mask with nobody having touched a switch. Left alone, Ctrl+Tab
        // records the other tab's opinion as the trader's choice, thrashes the
        // file between two of them, and fills the log below with switches no
        // hand moved. A different chart answering is a re-baseline, not an
        // edit; the file keeps whatever the last real switch put there.
        let tab = self.active_tab().id;
        if tab != self.workspace.layers().tab() {
            self.workspace.layers_mut().rebaseline(tab, mask);
            return;
        }
        if mask == self.workspace.layers().mask() {
            return;
        }
        // Name every switch that moved, before writing it down.
        //
        // This file is the trader's own answer, and it outranks the shipped
        // default from the next launch on — so a layer that goes off without a
        // click is not a display glitch, it is a choice attributed to someone
        // who never made it, and it lasts. The bug that motivated this line was
        // exactly that shape and cost a day to chase, because the only record
        // was the file itself: a trio of `false`s with no timestamp, no
        // sequence and nothing to say whether a hand had been near them.
        //
        // Off the frame path in every sense that matters: the mask compare
        // above already gates it, so this runs on the frames a switch actually
        // moves — a handful in a session — and not one of the other 60 a
        // second.
        let flipped = mask ^ self.workspace.layers().mask();
        for (bit, layer) in ChartLayer::ALL.into_iter().enumerate() {
            if flipped & (1 << bit) == 0 {
                continue;
            }
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYER_SWITCHED",
                layer = layer.id(),
                on = mask & (1 << bit) != 0,
                action = "persist_switch",
                "a chart layer switch moved; recording it as the trader's choice"
            );
        }
        chart_layers::save(
            self.workspace.chart_layers_path(),
            &self.active_tab().flow_pane.layer_states(&self.style),
        );
        self.workspace.layers_mut().record(mask);
    }

    /// Apply the saved layer visibility to the tab the app opened with.
    ///
    /// Runs before the autostart env vars so an explicit `QUANTICK_*_AUTOSTART`
    /// still wins for the run it was set on: a validation session asks for the
    /// heatmap on the command line and gets it, whatever the file remembers.
    pub(super) fn restore_chart_layers(&mut self) {
        let defaults = chart_layers::load(self.workspace.chart_layers_path());
        // Whatever the file said (including nothing at all) is now on screen;
        // only a change from here is worth another write.
        if defaults.is_empty() {
            // Only reachable when the *shipped* config failed to parse, since
            // every other path in `load` falls back to it — a build-time
            // mistake, and the one launch where saying nothing would be worst:
            // the chart opens on whatever the code decided and no one is told
            // the product's own answer never arrived.
            tracing::error!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYERS_UNAVAILABLE",
                path = %self.workspace.chart_layers_path().display(),
                action = "keep_code_defaults",
                "no layer visibility to apply; the shipped config did not parse"
            );
            let (tab, mask) = (self.active_tab().id, self.layer_mask());
            self.workspace.layers_mut().rebaseline(tab, mask);
            return;
        }
        if let Some(grid) = defaults.get(&ChartLayer::Grid) {
            self.style.canvas.grid_enabled = *grid;
        }
        self.apply_layer_defaults(&defaults);
        let (tab, mask) = (self.active_tab().id, self.layer_mask());
        self.workspace.layers_mut().rebaseline(tab, mask);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CHART_LAYERS_RESTORED",
            path = %self.workspace.chart_layers_path().display(),
            // `off`, not `hidden`: the map now always speaks for every layer,
            // the shipped-off `backfill_divider` included, so a count over it
            // is no longer "how many the trader switched off". Renamed rather
            // than quietly redefined — a field that keeps its name and changes
            // its meaning misleads every dashboard already reading it.
            off = defaults.values().filter(|visible| !**visible).count(),
            layers = defaults.len(),
            "chart layer visibility restored"
        );
    }

    /// Put every open pane on the saved visibility, once, at startup.
    ///
    /// A tab opened later does *not* come through here: it inherits the layers
    /// the active tab is showing at that moment (`inherited_layers` in
    /// [`Self::adopt_tab`]), which is the live state rather than the map read
    /// off disk during boot. The two policies differ on purpose — see the note
    /// there — so a reader arriving here for new-tab behaviour is in the wrong
    /// function.
    fn apply_layer_defaults(&mut self, states: &std::collections::BTreeMap<ChartLayer, bool>) {
        for tab in &mut self.tabs {
            // Every pane, by address: a default applied to "the flow pane and
            // the time pane" left the second stacked chart on whatever the
            // previous defaults were, so one canvas drew the same layer two
            // ways.
            for pane in tab.panes_mut() {
                pane.apply_layer_states(states);
            }
        }
    }
}
