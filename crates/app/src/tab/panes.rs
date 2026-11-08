//! Addressing the tab's panes: the context column's collapse and order,
//! the index and side accessors, focus, and the tape the flow pane shows —
//! and the panes as the session reads them and installs a prefix into.

use smallvec::SmallVec;

use quantick_engine::DealSample;

use super::Tab;
use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::canvas_layout::PaneKind;
use crate::deal_recording::{DealRecordingAction, RecordingView};
use crate::deal_recording_ui::{self, DealChip};
use crate::loading::LoadingTask;
use crate::metrics;
use crate::orderflow_view::OrderflowView;
use crate::pane::{ChartPane, PaneIndex, PaneSide};
use crate::state::BarKind;
use quantick_chart_interaction::pane_focus::{DrawnPanes, resolve_focus};
use quantick_feed::{FeedConnectionState, FeedNotice};
use quantick_session::{PaneView, SeamTrace};

impl Tab {
    /// Put the context column away, or bring it back.
    ///
    /// **The one collapse path.** The divider drag, the rail, the View menu,
    /// `Ctrl+0` and `layout.pane.collapse` all arrive here. Before this
    /// existed the only way a *trader* could collapse the column was a mouse
    /// drag, while an assistant had a named call for it — the second operator
    /// holding a capability the first could not reach from the keyboard,
    /// which is the rule inverted.
    ///
    /// `split_fraction` is deliberately untouched: it is the width the column
    /// springs back to, and spending it here would hand back a different chart
    /// from the one that was put away.
    ///
    /// Returns whether anything changed.
    pub fn set_context_collapsed(&mut self, collapsed: bool) -> bool {
        if self.context_collapsed == collapsed && (!collapsed || !self.flow_collapsed) {
            return false;
        }
        self.context_collapsed = collapsed;
        if collapsed {
            self.flow_collapsed = false;
        }
        true
    }

    /// Fold or restore the flow pane while retaining the split's open width.
    pub fn set_flow_collapsed(&mut self, collapsed: bool) -> bool {
        if self.flow_collapsed == collapsed && (!collapsed || !self.context_collapsed) {
            return false;
        }
        self.flow_collapsed = collapsed;
        if collapsed {
            self.context_collapsed = false;
        }
        true
    }

    /// Move the context pane at `from` to `to`, keeping the rest in order.
    ///
    /// **The one reposition path.** The View menu, the keyboard and the
    /// control plane all arrive here, so none of them can grow its own idea of
    /// what moving a pane does; a drag gesture, when it lands, is sugar over
    /// this call rather than a second implementation of it.
    ///
    /// Addresses are [`PaneIndex`]es, so `0` names the flow pane. The flow
    /// pane does not move: it is the protagonist and its column is the one
    /// thing every preset agrees on. Refused rather than clamped — a caller
    /// that asked to move the heatmap meant something this cannot do, and
    /// quietly moving a different pane would be worse than saying no.
    ///
    /// Returns whether anything moved.
    pub fn move_context_pane(&mut self, tab_id: u64, from: PaneIndex, to: PaneIndex) -> bool {
        let (Some(from_slot), Some(to_slot)) = (from.checked_sub(1), to.checked_sub(1)) else {
            return false;
        };
        if from_slot >= self.time_panes.len() || to_slot >= self.time_panes.len() {
            return false;
        }
        if from_slot == to_slot {
            return false;
        }
        let pane = self.time_panes.remove(from_slot);
        self.time_panes.insert(to_slot, pane);
        if from_slot < self.context_stack.heights.len()
            && to_slot < self.context_stack.heights.len()
        {
            let height = self.context_stack.heights.remove(from_slot);
            self.context_stack.heights.insert(to_slot, height);
        }
        // Focus follows the pane the trader just moved, so the next command
        // lands on the chart they were working with rather than on whichever
        // one slid into its place.
        self.focus = PaneSide::Time(to_slot);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUT_PANE_MOVED",
            tab_id = tab_id,
            from = from,
            to = to,
            "a context pane was moved within the stack"
        );
        true
    }

    /// How many panes this tab holds, drawn or not.
    #[must_use]
    pub fn pane_count(&self) -> PaneIndex {
        1 + self.time_panes.len()
    }

    /// The pane at `index`: `0` is the flow pane, `1..` the context stack.
    #[must_use]
    pub fn pane_at(&self, index: PaneIndex) -> Option<&ChartPane> {
        match index {
            0 => Some(&self.flow_pane),
            other => self.time_panes.get(other - 1),
        }
    }

    /// The pane at `index`, mutably.
    pub fn pane_at_mut(&mut self, index: PaneIndex) -> Option<&mut ChartPane> {
        match index {
            0 => Some(&mut self.flow_pane),
            other => self.time_panes.get_mut(other - 1),
        }
    }

    /// The first context pane, if this tab has built one.
    ///
    /// Most of the chrome speaks about *the* context chart because most
    /// layouts show one. The ones that show more reach for `time_panes`
    /// directly; this is the convenience, not the truth.
    #[must_use]
    pub fn time_pane(&self) -> Option<&ChartPane> {
        self.time_panes.first()
    }

    /// The first context pane, mutably.
    pub fn time_pane_mut(&mut self) -> Option<&mut ChartPane> {
        self.time_panes.first_mut()
    }

    /// Whether this tab has built any context pane at all.
    #[must_use]
    pub fn has_time_pane(&self) -> bool {
        !self.time_panes.is_empty()
    }

    /// Whether this tab has trouble worth showing on its chip while it sits in
    /// the background (§11: honesty at a glance).
    ///
    /// A recording never reports transport trouble — it has no transport — and
    /// "still connecting" is not trouble either. This is a feed that had a
    /// connection and lost it, or one asking the user to fix something.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.replay.is_none()
            && (self.feed_connection == FeedConnectionState::Reconnecting
                || matches!(self.notice, FeedNotice::Attention { .. }))
    }

    /// The canvas width the last drawn frame used, for callers that have to
    /// reason in pixels without a frame in hand — the control plane's resize,
    /// which must honour the same floor a drag does.
    ///
    /// Zero before the first frame, which callers read as "no opinion" rather
    /// than as a canvas of no width.
    #[must_use]
    pub fn last_canvas_width(&self) -> f32 {
        self.last_canvas.width()
    }

    /// Put the tab back in the state it opens in: never drawn, so no canvas
    /// width is known — how a background tab looks after a workspace restore.
    #[cfg(test)]
    pub fn forget_canvas_width_for_test(&mut self) {
        self.last_canvas.max = self.last_canvas.min;
    }

    /// Whether any context chart is actually on screen.
    ///
    /// The layout has to *hold* one, the tab has to have *built* one, and the
    /// column must not be collapsed. Three conditions that chrome kept
    /// re-deriving one variant at a time — and got wrong twice: the legend
    /// gate matched `TimeAndFlow` alone, so the stacked charts drew no legend
    /// and a collapsed column drew one over the flow chart against a stale
    /// rect.
    #[must_use]
    pub fn shows_context_charts(&self) -> bool {
        self.layout.shows_time() && self.has_time_pane() && !self.context_collapsed
    }

    /// The pane the chrome speaks for. Only a split canvas has a choice to
    /// make: a single-pane layout *is* its one visible pane, whatever the
    /// last split left `focus` set to. Time falls back to the flow pane for
    /// the frame between asking for the layout and the pane being built.
    pub fn focused_side(&self) -> PaneSide {
        // Read from what is *drawn*, never from which variant or collapse flag
        // it is. Deciding by flag is how a click on the second context chart
        // did nothing once the flow pane was put away: focus fell to the top
        // chart and the click was thrown away. A pane nobody can see never
        // holds focus either, or order entry, the ladder and the trade HUD go
        // dead on the one pane that is drawn.
        // Nothing drawn yet is the frame between asking for a layout and its
        // pane being built; a layout with no flow pane names its context chart.
        let fallback = if self.has_time_pane() && !self.layout.shows_flow() {
            PaneSide::Time(0)
        } else {
            PaneSide::Flow
        };
        resolve_focus(self.focus, self.drawn_panes()).unwrap_or(fallback)
    }

    /// The panes on screen. A layout with no flow pane has no context column
    /// to put away, so its collapse flag names nothing there.
    pub fn drawn_panes(&self) -> DrawnPanes {
        let column = !(self.layout.shows_flow() && self.context_collapsed);
        let heights = &self.context_stack.heights;
        let slot_drawn =
            |slot: usize| column && !heights.get(slot).is_some_and(|h| h.is_collapsed());
        DrawnPanes::new(
            self.layout.shows_flow() && !self.flow_collapsed,
            (0..self.context_panes_shown()).map(slot_drawn),
        )
    }

    /// How many context panes the layout draws — bounded by how many exist,
    /// for the frame between asking for a layout and its panes being built.
    pub fn context_panes_shown(&self) -> usize {
        self.layout
            .kinds()
            .iter()
            .filter(|kind| **kind == PaneKind::Time)
            .count()
            .min(self.time_panes.len())
    }

    /// The pane on `side`, falling back to the flow pane when the time pane
    /// has never been opened.
    pub fn pane(&self, side: PaneSide) -> &ChartPane {
        match side {
            PaneSide::Time(slot) => self.time_panes.get(slot).unwrap_or(&self.flow_pane),
            PaneSide::Flow => &self.flow_pane,
        }
    }

    /// See [`Self::pane`].
    /// Point exactly one of this tab's panes at the object whose content is
    /// being typed off-canvas, and clear the other.
    ///
    /// Both are written every time on purpose: the flag suppresses an
    /// object's own painting, so a pane left holding a stale index keeps a
    /// note invisible for the rest of the session. The tab is what knows
    /// whether a time pane exists at all, which is why the loop lives here
    /// rather than in the host.
    pub fn set_content_editing(&mut self, target: Option<(PaneSide, usize)>) {
        self.flow_pane.gestures.content_editing = target
            .filter(|(side, _)| *side == PaneSide::Flow)
            .map(|(_, index)| index);
        for (slot, time) in self.time_panes.iter_mut().enumerate() {
            time.gestures.content_editing = target
                .filter(|(side, _)| *side == PaneSide::Time(slot))
                .map(|(_, index)| index);
        }
    }

    pub fn pane_mut(&mut self, side: PaneSide) -> &mut ChartPane {
        match side {
            PaneSide::Time(slot) => self.time_panes.get_mut(slot).unwrap_or(&mut self.flow_pane),
            PaneSide::Flow => &mut self.flow_pane,
        }
    }

    /// Every side this tab can address, flow first — the order
    /// [`Self::panes`] walks. The one place "which panes exist" is spelled
    /// out for the chrome, so a surface that asks each pane a question walks
    /// the stack rather than the two sides the split used to have.
    pub fn sides(&self) -> impl Iterator<Item = PaneSide> + '_ {
        (0..self.pane_count()).map(PaneSide::from_index)
    }

    /// The pane every chrome surface reads from — see [`Self::focused_side`].
    pub fn focused_pane(&self) -> &ChartPane {
        self.pane(self.focused_side())
    }

    /// See [`Self::focused_pane`].
    pub fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.pane_mut(self.focused_side())
    }

    /// Make the pane with id `pane` the one the drawing chrome speaks for:
    /// focus it and drop every other pane's selection, so
    /// [`Self::drawing_side`] answers it. The chart menu's object entries
    /// act through the drawing chrome on the chart they were opened on, and
    /// the Ctrl+Z after them must land there too. `false` if no pane has it.
    pub fn aim_drawing_at(&mut self, pane: u64) -> bool {
        let Some(side) = self.sides().find(|side| self.pane(*side).id == pane) else {
            return false;
        };
        self.focus = side;
        let others: Vec<_> = self.sides().filter(|other| *other != side).collect();
        for other in others {
            self.pane_mut(other).drawings.select(None);
        }
        true
    }

    /// The pane holding the drawing selection — which is not always the
    /// focused one.
    ///
    /// A shared mark can be taken from either chart it appears on, and it
    /// stays in the store of the pane it was drawn on. So the inspector, the
    /// keyboard and the object manager follow the *object*, not the pane the
    /// pointer happens to be over: selecting a level on the time pane and
    /// pressing Delete has to delete that level, wherever it lives.
    ///
    /// Exactly one pane holds a selection at a time
    /// ([`Self::apply_shared_interactions`] drops the other's), so this asks
    /// the focused pane first and takes the answer it finds.
    pub fn drawing_side(&self) -> PaneSide {
        let focused = self.focused_side();
        if self.pane(focused).drawings.selected().is_some() || self.time_panes.is_empty() {
            return focused;
        }
        self.sides()
            .find(|side| *side != focused && self.pane(*side).drawings.selected().is_some())
            .unwrap_or(focused)
    }

    /// The pane every drawing surface reads from — see [`Self::drawing_side`].
    pub fn drawing_pane(&self) -> &ChartPane {
        self.pane(self.drawing_side())
    }

    /// See [`Self::drawing_pane`].
    pub fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.pane_mut(self.drawing_side())
    }

    /// Every pane holding this market's bars, on screen or not, each beside
    /// the side it answers to — flow first, so a walk of one tab is stable and
    /// a capture stays diffable against the one before it.
    ///
    /// An iterator and not a `Vec`: this is walked per frame by the control
    /// plane's journal comparison, which must not touch the allocator on a
    /// quiet frame.
    pub fn panes(&self) -> impl Iterator<Item = (&ChartPane, PaneSide)> {
        std::iter::once((&self.flow_pane, PaneSide::Flow)).chain(
            self.time_panes
                .iter()
                .enumerate()
                .map(|(slot, time)| (time, PaneSide::Time(slot))),
        )
    }

    /// Every pane holding this market's bars, on screen or not. One tape, and
    /// however many charts the layout has ever shown read off it.
    pub fn panes_with_sides_mut(&mut self) -> impl Iterator<Item = (&mut ChartPane, PaneSide)> {
        let Self {
            flow_pane,
            time_panes,
            ..
        } = self;
        std::iter::once((flow_pane, PaneSide::Flow)).chain(
            time_panes
                .iter_mut()
                .enumerate()
                .map(|(slot, pane)| (pane, PaneSide::Time(slot))),
        )
    }

    /// See [`Self::panes_with_sides_mut`], without the addresses.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut ChartPane> {
        // Destructured rather than borrowed field by field: the flow pane and
        // the context stack are two disjoint parts of `self`, and the compiler
        // only knows that when it is told in one pattern.
        let Self {
            flow_pane,
            time_panes,
            ..
        } = self;
        std::iter::once(flow_pane).chain(time_panes.iter_mut())
    }

    /// The flow pane's tape.
    ///
    /// The flow pane is built with one ([`ChartPane::flow`]) and never gives it
    /// up; the `Option` on the pane exists so a *time* pane can go without a
    /// book worker, not because this one can be missing.
    pub fn tape(&self) -> &OrderflowView {
        self.flow_pane
            .orderflow
            .as_ref()
            .expect("the flow pane is built with a tape and never drops it")
    }

    /// See [`Self::tape`].
    pub fn tape_mut(&mut self) -> &mut OrderflowView {
        self.flow_pane
            .orderflow
            .as_mut()
            .expect("the flow pane is built with a tape and never drops it")
    }

    /// Route the source clock through the applied chart tape before painting:
    /// the replay's playhead ([`quantick_feed::replay::ReplayLink::clock_after`]),
    /// or the host's monotonic instant on a live feed.
    pub(crate) fn update_tape_clock_at(&mut self, monotonic_ms: u64) {
        let applied = self.flow_pane.state.trades();
        let applied_ms = applied.last().map(|trade| trade.timestamp_ms);
        match self
            .replay
            .as_ref()
            .map(|replay| replay.clock_after(applied.len()))
        {
            Some((position_ms, next_ms)) => {
                self.tape_mut()
                    .set_replay_clock_at(position_ms, applied_ms, next_ms);
            }
            None => self.tape_mut().set_live_clock_at(applied_ms, monotonic_ms),
        }
    }

    /// Put a prefix in front of one pane; a seam bar that changed, or says
    /// why it holds no lead, is logged beside the pane.
    pub(super) fn install_prefix(
        &mut self,
        index: usize,
        bars: Vec<quantick_engine::Bar>,
        lead: Option<quantick_engine::Bar>,
        covers_seam: bool,
        seam: Option<SeamTrace>,
    ) -> bool {
        let Some(pane) = self.pane_at_mut(index) else {
            return false;
        };
        let had = seam.and_then(|_| pane.state.venue_lead().cloned());
        let changed = pane.install_venue_history(bars, lead, covers_seam);
        let seated = pane.state.venue_lead();
        if let Some(seam) = seam
            && (had.as_ref() != seated
                || seated.is_none() && seam.token != "nothing_before_first_trade")
        {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SEAM_LEAD",
                pane = pane.id,
                interval_ms = seam.interval_ms,
                first_print_ms = seam.first_print_ms,
                lead_open_ms = seated.map_or(0, |bar| bar.open_time),
                lead_trades = seated.map_or(0, |bar| bar.trade_count),
                action = if seated.is_some() { "merged_into_seam_bar" } else { seam.token },
                "the venue's history of the seam bucket before the first trade"
            );
        }
        changed
    }
}

/// The deal readings every pane retains: what the feed counts, what the REC
/// control asks for, and the views the chrome reads. What the recorder
/// decides alone is [`DealRecorder`](crate::deal_recording::DealRecorder)'s.
impl Tab {
    /// One reading from the feed: written down if recording, retained by
    /// every pane either way, so a later switch to `trades` cuts from it.
    pub fn observe_deal_counter(&mut self, sample: DealSample) {
        let resumed = self.deal_recorder.observe(sample, metrics::wall_clock_ms());
        for pane in self.panes_mut() {
            pane.state.observe_deals(sample);
        }
        // A rotation into a day whose file exists resumes it, as a start
        // does: what the file holds reaches the panes.
        self.retain_deal_samples(&resumed);
    }

    /// Per drain: the recorder's schedule. A start it makes resumes today's
    /// file, so the day's earlier prints cut as trades bars again.
    pub fn tick_deal_recording(&mut self) {
        let available = self.feed_capabilities.borrow().deal_counter;
        let resumed = self.deal_recorder.tick(available, metrics::wall_clock_ms());
        self.retain_deal_samples(&resumed);
    }

    pub(crate) fn retain_deal_samples(&mut self, samples: &[DealSample]) {
        if samples.is_empty() {
            return;
        }
        for pane in self.panes_mut() {
            pane.state.observe_deals_batch(samples);
        }
        // The retained series changed under the bars. Only a pane cutting
        // by deals reads it now; the others replay it on their next switch,
        // and a rebuild over a day of prints is not free. A rewrite of the
        // series is a rewrite: the view, the marks, the indicators and the
        // strategies follow it the way they follow a spec switch.
        for index in 0..self.pane_count() {
            // The rule the builder cuts under, not the selector: a combo
            // mid-flick has moved the selector a frame ahead of the bars.
            let cutting_deals = self
                .pane_at(index)
                .is_some_and(|pane| pane.state.spec().kind().needs_deal_counter());
            if cutting_deals {
                // The series was rewritten under the bars, not the rule
                // that cuts them: what a replay seek says when it disarms.
                self.recut_pane_with(
                    index,
                    quantick_strategy::DisarmReason::TimelineReset,
                    |pane| pane.rebuild_bars(),
                );
            }
        }
    }

    /// What the REC control asked for.
    pub fn apply_deal_recording(&mut self, action: DealRecordingAction) {
        match action {
            // Starting resumes today's file: the readings it held reach
            // every pane, so the day's earlier prints cut as trades bars
            // again after a restart.
            DealRecordingAction::Start => {
                let resumed = self.deal_recorder.start(metrics::wall_clock_ms());
                self.retain_deal_samples(&resumed);
            }
            DealRecordingAction::Stop => self.deal_recorder.stop(),
            // The selector's own path, on the pane the popover described —
            // the focused one, like every bar control; the change is picked
            // up a frame later by the same spec sync a toolbar click goes
            // through.
            DealRecordingAction::ShowAsTrades => {
                let index = self.focused_side().index();
                let spec = self.pane_at(index).map_or_else(
                    || {
                        quantick_engine::bar_registry::BUILTIN_BARS
                            .find("trades")
                            .expect("registered trades")
                            .default_config()
                    },
                    |pane| *pane.spec.retained(BarKind::Trades),
                );
                let _ = self.set_pane_bar_spec(index, spec);
            }
            DealRecordingAction::OpenFolder => {
                crate::paper_trading::reveal_folder(&self.deal_recorder.view(None).dir);
            }
            DealRecordingAction::LoadDay(index) => {
                let loaded = self.deal_recorder.load_day(index);
                self.retain_deal_samples(&loaded);
            }
        }
    }

    /// The recorder as the chrome sees it now ([`DealRecorder::live_view`]),
    /// judged on the tape's clock: the newest print the tab holds.
    ///
    /// [`DealRecorder::live_view`]: crate::deal_recording::DealRecorder::live_view
    #[must_use]
    pub fn deal_recording_view(&self) -> Option<RecordingView> {
        self.deal_recorder
            .live_view(self.replay.is_some(), self.latest_trade_ms)
    }

    /// The chart-corner chip for the flow pane, when it has something to say.
    #[must_use]
    pub fn deal_chip(&self) -> Option<DealChip> {
        let view = self
            .deal_recorder
            .chip_view(self.replay.is_some(), self.latest_trade_ms);
        let pane = &self.flow_pane;
        deal_recording_ui::chip_for(
            &view,
            pane.spec.spec().definition(),
            pane.state.deal_samples().last().is_some(),
            pane.state.uncounted_trades(),
        )
    }
}

/// The strategies armed on the panes, as the tab settles them against its
/// paper account, and the market state a rewound source throws away.
impl Tab {
    /// Apply the cleanup commands the panes' drawing menus queued this
    /// frame — a disarm or removal over a resting retest limit cancels the
    /// order. Runs on the UI frame that clicked, not on the next print: a
    /// cancel that waits for the market to move may lose the race to it.
    pub fn apply_strategy_cleanup(&mut self) {
        let Self {
            paper,
            flow_pane,
            time_panes,
            ..
        } = self;
        for pane in std::iter::once(flow_pane).chain(time_panes.iter_mut()) {
            for command in pane.strategies.take_cleanup() {
                let _ = paper.account_mut().apply_strategy_command(command);
            }
        }
    }

    /// Arm `form` on the drawing through its pane's strategies
    /// ([`crate::pane::strategies::PaneStrategies::arm`]), then settle what
    /// the arm owes the paper account and the alert sink.
    pub(crate) fn arm_strategy_instance(
        &mut self,
        alerts: &mut dyn crate::audio::AlertSink,
        side: PaneSide,
        drawing: crate::drawings::DrawingId,
        form: &crate::strategy_presets::StoredPreset,
        preset_label: String,
    ) -> Result<(), String> {
        let pane = self.pane_mut(side);
        let series = quantick_drawings::gesture::PaneSeriesRead {
            history_prefix: &pane.history_prefix,
            state: &pane.state,
            spec: &pane.spec,
        };
        let (replaced_cleanup, cue) =
            pane.strategies
                .arm(&pane.drawings, series, drawing, form, preset_label)?;
        for command in replaced_cleanup {
            // Arming over an instance with a pending entry sweeps that
            // entry — a resting order must never outlive its bot.
            let _ = self.paper.account_mut().apply_strategy_command(command);
        }
        self.paper.account_mut().set_bot_listening(true);
        // Only now, past every gate: the sink opens its device at arm time
        // so the first signal does not pay for it on the tape's path, but a
        // *refused* arm must open nothing. Ctrl+D over a band the copy
        // cannot be armed on discards the `Err` by design — the absent
        // badge is the message — and warming above the gates turned that
        // silence into an audio stack enumerated once per keypress, against
        // the sink's own promise that a chart which never arms an alarm
        // never touches a device.
        if let Some(cue) = cue {
            alerts.warm_up(cue);
        }
        Ok(())
    }

    /// Throw away everything loaded and wait for the source to refill it.
    ///
    /// Sent by a source that rewound — seeking a replay, for instance. The
    /// chart is rebuilt from the history that follows rather than patched,
    /// because bars that already closed cannot be reopened.
    pub fn reset_market_state(&mut self, keep_readings: bool) {
        self.session.timeline_reset();
        for pane in self.panes_mut() {
            pane.reset_series_with(keep_readings);
            // Indicators follow the chart into the empty state; the refill's
            // Backfilled event replays them (replay seek funnels through here,
            // so seeking inherits correct indicator behavior for free).
            pane.send_indicator_rebuild();
            pane.frame.lane_divider_x = None;
            // Judgements armed on the old timeline do not carry into the
            // rebuilt one — the same honesty rule the simulator's flatten
            // follows, with the reason on the badge. No cleanup commands
            // come back under this reason: `paper.on_timeline_reset` below
            // sweeps every order with the honest `reset` label.
            let _ = pane
                .strategies
                .anchors
                .disarm_all(quantick_strategy::DisarmReason::TimelineReset);
        }
        self.drop_overlay_gestures();
        // The timeline these describe is the one being rebuilt. A gap
        // re-anchors itself by market time, so one left behind would paint
        // itself onto the refilled series as though a reconnect had happened.
        self.feed_gaps.clear();
        self.feed_integrity = quantick_feed::FeedIntegrity::default();
        self.resume_floor_ms = None;
        self.history_trades = 0;
        // A run anchored to a tape that no longer exists cannot continue, and
        // `restart` below drops the waits its outstanding request would have
        // resolved. Its verdict goes with it.
        self.run_session(None, quantick_session::Act::AbandonRun);
        self.latest_trade_latency_ms = None;
        self.latest_trade_ms = None;
        // The refill arrives as one backfill batch; keep the loading indicator
        // up until it lands. Requests sent to the source before the reset will
        // never be answered, so the count restarts rather than accumulates.
        self.run_session(None, quantick_session::Act::OpeningFill(None));
        self.loading.restart(LoadingTask::History);
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);
        // The simulator flattens at its last mark and says so — a position
        // cannot honestly survive into a rebuilt timeline.
        self.paper.on_timeline_reset();
    }
}

/// Every pane holding this market's bars, as the session reads them: flow
/// first, then the context stack, the order an `InstallPrefix` addresses.
pub(super) fn pane_views<'a>(
    flow: &'a ChartPane,
    context: &'a [ChartPane],
) -> SmallVec<[PaneView<'a>; MAX_CANVAS_PANES]> {
    std::iter::once(flow)
        .chain(context)
        .map(|pane| PaneView {
            interval_ms: pane.state.spec().time_interval_ms(),
            first_print_open_ms: pane.state.first_print_open_ms(),
            first_bar: pane.state.bars().first(),
            partial: pane.state.partial(),
            rebuild_pending: pane.history_pending(),
            rebuild_failed: pane.history_failed(),
        })
        .collect()
}
