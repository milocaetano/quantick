//! The tab's canvas layout and the pane settings it persists: switching
//! layout, the layout a restored pane opens with, and the bar spec changes
//! a pane applies.

use super::{CanvasLayout, LegendFold, Tab};
use crate::canvas_layout::{MAX_CONTEXT_PANES, PaneIdAllocator, PaneKind};
use crate::chart_layers::ChartLayer;
use crate::config::AppConfig;
use crate::loading::LoadingTask;
use crate::pane::{ChartPane, PaneIndex, PaneSide, clamp_pane_fraction};
use crate::style::ChartStyle;

impl Tab {
    /// Switch which panes the canvas shows (§11).
    ///
    /// The first layout that needs the time pane builds it and seeds it from
    /// the trades the flow pane already holds, so it opens showing the same
    /// market rather than an empty chart waiting for the next print. Leaving
    /// a layout only stops drawing its pane: indicators, drawings and bars
    /// survive, and the pane keeps being fed, so re-showing it never has to
    /// catch up.
    ///
    /// Focus follows what the switch reveals: the pane that just appeared is
    /// the one the user asked to work on, so the chrome speaks for it without
    /// a second click. A switch that reveals nothing new (Time + Flow from
    /// Time + Flow) keeps the focus where it was.
    pub fn set_layout(&mut self, layout: CanvasLayout) {
        let previous = self.layout;
        // A layout chosen from the picker shows the panes its thumbnail drew.
        // Leaving the column collapsed meant the cell lit, two panes were
        // promised, and an 8 px rail arrived instead. Before the early return,
        // because picking the arrangement that is *already* selected is
        // exactly how a trader asks for the charts they can see promised in a
        // lit cell — and a return above this line answered that with the rail.
        self.context_collapsed = false;
        if layout == previous {
            return;
        }
        self.layout = layout;
        let wanted = layout
            .kinds()
            .iter()
            .filter(|kind| matches!(kind, PaneKind::Time))
            .count();
        // Set, never raised: switching to a three-pane layout and back before
        // the next frame used to leave the count where the wider layout put
        // it, and the tab then built a pane no layout had asked for — seeded
        // from the whole retained tape and fed every trade thereafter.
        self.pending_context_panes = wanted.saturating_sub(self.time_panes.len());
        if wanted > self.time_panes.len() {
            // Seeding replays every retained trade, which on a deep history
            // holds the render thread long enough to notice. Armed here and
            // done on the next frame, exactly as a bar-spec change is: the
            // frame carrying the menu click paints the loading overlay first,
            // so the wait reads as the chart working rather than the app
            // hanging.
            self.loading.begin(LoadingTask::BarRebuild);
        }
        self.focus = match layout {
            CanvasLayout::Single => PaneSide::Flow,
            CanvasLayout::Time => PaneSide::Time(0),
            // The split reveals whichever pane the previous layout was not
            // showing: the time pane coming from Single, the flow pane coming
            // from Time.
            CanvasLayout::TimeAndFlow | CanvasLayout::TimeTimeAndFlow => match previous {
                CanvasLayout::Single => PaneSide::Time(0),
                CanvasLayout::Time => PaneSide::Flow,
                CanvasLayout::TimeAndFlow | CanvasLayout::TimeTimeAndFlow => self.focus,
            },
        };
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANVAS_LAYOUT",
            layout = ?layout,
            time_pane_bars = self.time_pane().map(|pane| pane.state.bars().len()),
            action = if self.pending_context_panes > 0 {
                "build_time_pane_next_frame"
            } else {
                "relayout_canvas"
            },
            "canvas layout changed"
        );
    }

    /// Build the context panes the last layout change asked for, if any are
    /// due.
    ///
    /// Runs at the top of the frame after the click, so the overlay armed by
    /// [`Self::set_layout`] has already been painted once. `ids` is the
    /// window's allocator rather than the tab's: pane ids namespace egui
    /// interaction state across the whole window, so a tab may not mint its
    /// own.
    pub fn apply_pending_layout(
        &mut self,
        config: &AppConfig,
        style: &ChartStyle,
        ids: &mut PaneIdAllocator,
    ) {
        if self.pending_context_panes == 0 {
            return;
        }
        self.pending_context_panes -= 1;
        // The slot this pane will take is the next free one in the stack.
        let interval_ms = self
            .context_opening_intervals_ms
            .get(self.time_panes.len())
            .copied()
            .unwrap_or(self.time_pane_opening_interval_ms);
        let mut pane = ChartPane::time(ids.alloc(), interval_ms);
        pane.legend_collapsed = self.time_pane_opening_legend_collapsed;
        pane.layout = self
            .context_opening_layouts
            .get(self.time_panes.len())
            .copied()
            .flatten()
            .map(crate::layouts::LayoutId);
        pane.seed_from(
            self.flow_pane.state.trades(),
            self.flow_pane.state.backfill_trade_count(),
        );
        // The pane opens looking like the one it splits away from: a user who
        // switched the crosshair off is not asking for it back by opening a
        // second view of the same market. Orientation is part of that look —
        // an upside-down market does not turn back over by being given a
        // second view, and the boot's QUANTICK_INVERTED hook fires before
        // this pane exists at all.
        // Copying the *switches* rather than a list of field names: an earlier
        // version cloned `hidden_layers` alone, which left the footprint
        // behind — a per-pane field of its own — so the split opened with the
        // ladder on in the flow pane and off in the time pane, contradicting
        // the paragraph above and darkening the toolbar's footprint lamp the
        // moment the trader clicked into the left chart. `apply_layer_states`
        // drops whatever this pane does not draw, which is the whole of what
        // §11 asks for, and it covers `hidden_layers` in passing.
        //
        // Every layer but one. `Drawings` resolves to `DrawingStore`, whose
        // setter records an undo entry — right for a click, wrong for a pane
        // being born: seeded through it, a time pane holding zero objects
        // opens with a non-empty history, and the trader's first Ctrl+Z there
        // un-hides drawings rather than doing nothing. It is seeded through
        // the store's own opening setter instead.
        let mut states = self.flow_pane.layer_states(style);
        states.remove(&ChartLayer::Drawings);
        pane.apply_layer_states(&states);
        pane.drawings
            .open_all_hidden(self.flow_pane.drawings.all_hidden());
        pane.price_view
            .set_inverted(self.flow_pane.price_view.is_inverted());
        self.time_panes.push(pane);
        // One pane per frame, for the reason the first one waits a frame at
        // all: seeding replays every retained trade, and building three at
        // once would hold the render thread for three times as long. The
        // overlay stays up until the last one lands.
        if self.pending_context_panes == 0 {
            self.loading.end(LoadingTask::BarRebuild);
        }
        // The pane exists now, so there is something for a prefix to go in
        // front of. A base already held (a layout toggled off and on) is
        // folded rather than re-fetched; otherwise this is the first moment
        // asking for one means anything.
        if self.ohlcv_base.is_some() {
            self.refold_history_prefix();
        } else {
            self.request_ohlcv_history(config);
        }
    }

    /// The layout each pane opens on, from a saved workspace: the flow pane's
    /// now, each context pane's when it is built.
    ///
    /// `context` is the wire form — [`crate::ui_state::LAYOUT_UNRECORDED`]
    /// for a slot the file did not state — and it is read back into an
    /// `Option` here, at the one place the file's shape meets the pane's.
    ///
    /// **Ordering.** This names what a pane *will* show; it does not move a
    /// pane that is already showing something. A caller that reaches a tab
    /// whose stack is standing must follow with
    /// `QuantickApp::reload_layouts`, which clears every pane's set and seeds
    /// it again from the layout named here — otherwise a seeded pane keeps
    /// the old layout's indicators, drawings and header label under the new
    /// layout's id, and the next edit is written into the wrong entries.
    /// Both callers do (`restore_workspace` and the bundle import, each
    /// through `reload_cockpit_stores`).
    pub fn set_opening_layouts(&mut self, flow: Option<u64>, context: &[u64]) {
        self.flow_pane.layout = flow.map(crate::layouts::LayoutId);
        self.context_opening_layouts = context
            .iter()
            .map(|id| (*id != crate::ui_state::LAYOUT_UNRECORDED).then_some(*id))
            .take(MAX_CONTEXT_PANES)
            .collect();
        // A context pane already built takes its name now: an import lands on
        // a tab whose stack is standing, and the stash alone would only reach
        // the panes still to come.
        for (slot, pane) in self.time_panes.iter_mut().enumerate() {
            if let Some(id) = context
                .get(slot)
                .copied()
                .filter(|id| *id != crate::ui_state::LAYOUT_UNRECORDED)
            {
                pane.layout = Some(crate::layouts::LayoutId(id));
            }
        }
    }

    /// Name the layout a context pane will open on when the stack builds it.
    ///
    /// Only for a pane that is not standing yet — a canvas change lands its
    /// panes a frame after the layout that asked for them. A pane already on
    /// the canvas is moved by `QuantickApp::switch_pane_layout`, which
    /// carries its indicators and its drawings across too; writing
    /// `pane.layout` under a standing pane would leave the field disagreeing
    /// with what that chart is showing.
    pub fn set_opening_layout(&mut self, side: PaneSide, id: crate::layouts::LayoutId) {
        // The flow pane is built with the tab and is never pending, so the
        // only address that can be waiting is a context slot.
        let PaneSide::Time(slot) = side else {
            debug_assert!(false, "the flow pane is never waiting to be built");
            return;
        };
        if slot >= MAX_CONTEXT_PANES {
            return;
        }
        if self.context_opening_layouts.len() <= slot {
            self.context_opening_layouts.resize(slot + 1, None);
        }
        self.context_opening_layouts[slot] = Some(id.0);
    }

    /// Put this tab's canvas back the way a saved workspace recorded it: the
    /// layout, the divider, the focused pane, and the interval the time pane
    /// opens on.
    ///
    /// One method rather than four public fields, because the order matters
    /// and only the tab knows it. The opening interval has to be set *before*
    /// [`Self::set_layout`] arms the time pane, or the pane is built on the
    /// header default and the saved interval lands one frame too late. The
    /// focus is applied *after*, because `set_layout` moves it to whatever the
    /// switch reveals — right for a menu click, wrong for a restore, where the
    /// saved focus is the answer.
    ///
    /// Startup-scoped, like [`Self::apply_feed_declared_layout`]: the only
    /// caller is the app restoring a workspace into a tab it has just opened.
    pub fn restore_canvas(
        &mut self,
        layout: CanvasLayout,
        split_fraction: Option<f32>,
        context_collapsed: bool,
        focus: Option<PaneSide>,
        context_intervals_ms: &[i64],
        legends: LegendFold,
    ) {
        // The top chart's interval is also the one every slot past the list
        // opens on, which is what a one-chart file has always meant.
        if let Some(ms) = context_intervals_ms.first() {
            self.time_pane_opening_interval_ms = *ms;
        }
        self.context_opening_intervals_ms = context_intervals_ms
            .iter()
            .copied()
            .take(MAX_CONTEXT_PANES)
            .collect();
        self.set_layout(layout);
        // *After* `set_layout`, for the reason the focus below is: a switch
        // opens the column it just revealed, which is right for a menu click
        // and wrong for a restore, where the saved state is the answer.
        // Assigned before, a workspace saved with its charts put away reopened
        // with them out — and the next `capture_arrangement` wrote that over
        // the trader's choice.
        self.context_collapsed = context_collapsed;
        if let Some(fraction) = split_fraction {
            self.split_fraction = clamp_pane_fraction(fraction);
        }
        if let Some(side) = focus {
            self.focus = side;
        }
        self.flow_pane.legend_collapsed = legends.flow;
        // The time pane may not exist yet. `set_layout` only *arms* it
        // (`pending_time_pane`); `apply_pending_layout` builds it a frame
        // later, which is the very reason the opening interval above is
        // stashed rather than assigned. Writing the fold here alone would
        // write it to `None` and the pane would open expanded — and the next
        // `capture_arrangement` would then persist that `false` over the
        // trader's choice.
        self.time_pane_opening_legend_collapsed = legends.time;
        if let Some(time) = self.time_pane_mut() {
            time.legend_collapsed = legends.time;
        }
    }

    /// Let every pane's selectors settle, then mirror the result onto the
    /// rebuild indicator: it is up while *any* pane has a rebuild pending.
    pub fn apply_spec_changes(&mut self) {
        // Every pane, by address. Settling "the flow pane and the time pane"
        // left the second stacked chart's selector armed for ever: its header
        // chip lit, its interval changed, and its bars never rebuilt.
        for pane in 0..self.pane_count() {
            self.apply_spec_change_at(pane);
        }
        let rebuilding = self
            .panes()
            .any(|(pane, _side)| pane.spec.pending.is_some());
        self.loading.set_active(LoadingTask::BarRebuild, rebuilding);
    }

    /// Apply one pane's bar-type/parameter change, a frame after its selectors
    /// settle.
    ///
    /// Switching the spec replays every retained trade synchronously, which
    /// can hold this thread long enough to notice on a deep history. Deferring
    /// the rebuild by one frame lets the frame that carries the change paint
    /// the loading overlay first, so the wait reads as the chart working
    /// rather than the app hanging. A selector still moving (a dragged
    /// parameter) keeps pushing the pending spec forward, which also debounces
    /// the rebuild to one per gesture.
    ///
    /// The two panes run this independently: the toolbar's BARS group governs
    /// the focused pane and the time pane's own header governs the time pane
    /// (§11), so a change to one pane must not rebuild the chart beside it.
    fn apply_spec_change_at(&mut self, index: PaneIndex) {
        let Some(desired) = self.pane_at(index).map(ChartPane::current_spec) else {
            return;
        };
        let Some(pane) = self.pane_at_mut(index) else {
            return;
        };
        if desired == *pane.state.spec() {
            // Selection and chart agree — nothing is pending any more (a feed
            // switch or reset may have rebuilt the state under a pending spec).
            pane.spec.pending = None;
            return;
        }
        match pane.spec.pending.take() {
            // The frame that changed the selector: arm the indicator, paint.
            None => pane.spec.pending = Some(desired),
            // Still moving: wait for the selector to settle for a frame.
            Some(pending) if pending != desired => pane.spec.pending = Some(desired),
            // Settled since last frame: do the rebuild.
            Some(_) => {
                // Where the user is looking, in market time — the one thing a
                // rebuild preserves. The new series cuts the same trades into
                // a different number of bars, so the old right-edge *index*
                // may not exist in it at all: keeping it would leave the
                // window past the end of the data, drawing nothing.
                let anchor = pane.right_edge_time();
                // The series the drawings are still anchored to, captured
                // before it is replaced: their bar indices are meaningless in
                // the new cut and have to be re-derived from the market time
                // each anchor carries.
                let old_slots = pane.slots();
                pane.set_spec(desired);
                // The venue prefix folds to the new interval before the view
                // is reanchored: the market time the user was looking at has
                // to resolve against the series they will be looking at.
                // Either pane: `bars → time` on the flow pane is exactly the
                // spec change this refold exists for (audit S1).
                //
                // Installing a prefix rebuilds the indicators over the whole
                // composed series, so the plain rebuild is only sent when the
                // refold did not — two rebuilds of ~130k bars per settled drag
                // frame, with no coalescing in the worker, is the cost of
                // sending both.
                let refolded = self.refold_history_prefix();
                if !refolded && let Some(pane) = self.pane_at_mut(index) {
                    pane.send_indicator_rebuild();
                }
                let Some(pane) = self.pane_at_mut(index) else {
                    return;
                };
                let slot = anchor.and_then(|ms| pane.slot_at_time(ms));
                let slots = pane.slots();
                pane.viewport.reanchor(slot, slots);
                // The marks follow the view: same market time, this pane's
                // new bar space. Nothing is lost, so there is nothing to
                // announce.
                pane.reanchor_drawings(old_slots);
                // The strategies do not follow: the body average that
                // defines a force bar means something else under another
                // bar spec, so the instances disarm and say why. The tape
                // itself continues, so any pending bot entry is swept here
                // and now — through the same funnel manual orders use.
                let cleanup = pane
                    .strategies
                    .anchors
                    .disarm_all(quantick_strategy::DisarmReason::BarSpecChanged);
                let _ = pane.take_strategy_bars();
                for command in cleanup {
                    let _ = self.paper.account_mut().apply_strategy_command(command);
                }
                self.drop_overlay_gestures();
            }
        }
    }
}
