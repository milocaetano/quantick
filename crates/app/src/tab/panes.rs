//! Addressing the tab's panes: the context column's order, the index and
//! side accessors, focus, and the tape the flow pane shows — the panes a
//! layout switch builds and a bar spec change re-cuts — and the panes as the
//! session reads them and installs a prefix into.

use smallvec::SmallVec;

use quantick_engine::DealSample;

use super::{CanvasCollapseRestore, CanvasLayout, LegendFold, Tab};
use crate::canvas_layout::{MAX_CANVAS_PANES, MAX_CONTEXT_PANES, PaneIdAllocator, PaneKind};
use crate::config::AppConfig;
use crate::deal_recording::{DealRecordingAction, RecordingView};
use crate::deal_recording_ui::{self, DealChip};
use crate::loading::LoadingTask;
use crate::metrics;
use crate::orderflow_view::OrderflowView;
use crate::pane::{ChartPane, PaneIndex, PaneSide, clamp_pane_fraction};
use crate::state::BarKind;
use crate::style::ChartStyle;
use quantick_chart_interaction::pane_focus::{DrawnPanes, resolve_focus};
use quantick_feed::{FeedConnectionState, FeedNotice};
use quantick_session::{PaneView, SeamTrace};
use quantick_workspace::canvas::seat_drawn;

impl Tab {
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
        if from_slot < self.canvas.context_stack.heights.len()
            && to_slot < self.canvas.context_stack.heights.len()
        {
            let height = self.canvas.context_stack.heights.remove(from_slot);
            self.canvas.context_stack.heights.insert(to_slot, height);
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
        self.link.replay.is_none()
            && (self.link.feed_connection == FeedConnectionState::Reconnecting
                || matches!(self.link.notice, FeedNotice::Attention { .. }))
    }

    /// The canvas width the last drawn frame used, for callers that have to
    /// reason in pixels without a frame in hand — the control plane's resize,
    /// which must honour the same floor a drag does.
    ///
    /// Zero before the first frame, which callers read as "no opinion" rather
    /// than as a canvas of no width.
    #[must_use]
    pub fn last_canvas_width(&self) -> f32 {
        self.canvas.last_canvas.width()
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
        self.canvas.shows_context_charts(&self.time_panes)
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
        let fallback = if self.has_time_pane() && !self.canvas.layout.shows_flow() {
            PaneSide::Time(0)
        } else {
            PaneSide::Flow
        };
        resolve_focus(self.focus, self.drawn_panes()).unwrap_or(fallback)
    }

    /// The panes on screen: the canvas model's, unless the column painter
    /// draws, whose collapse flags hold.
    pub fn drawn_panes(&self) -> DrawnPanes {
        let canvas = &self.canvas;
        let model = |seat| seat_drawn(&canvas.state, &canvas.seats, seat);
        if !canvas.column_painted() {
            let time = (0..self.time_panes.len()).map(|seat| model(Some(seat)));
            return DrawnPanes::new(model(None), time);
        }
        let open = |side| !canvas.pane_collapsed(side);
        let time = (0..self.context_panes_shown()).map(|slot| open(PaneSide::Time(slot)));
        DrawnPanes::new(canvas.layout.shows_flow() && open(PaneSide::Flow), time)
    }

    /// How many context panes the layout draws — bounded by how many exist,
    /// for the frame between asking for a layout and its panes being built.
    pub fn context_panes_shown(&self) -> usize {
        self.canvas
            .layout
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
            .link
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

#[cfg(test)]
impl Tab {
    /// Put the tab back in the state it opens in: never drawn, so no canvas
    /// width is known — how a background tab looks after a workspace restore.
    pub fn forget_canvas_width_for_test(&mut self) {
        self.canvas.last_canvas.max = self.canvas.last_canvas.min;
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
        let available = self.link.feed_capabilities.borrow().deal_counter;
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
            .live_view(self.link.replay.is_some(), self.link.latest_trade_ms)
    }

    /// The chart-corner chip for the flow pane, when it has something to say.
    #[must_use]
    pub fn deal_chip(&self) -> Option<DealChip> {
        let view = self
            .deal_recorder
            .chip_view(self.link.replay.is_some(), self.link.latest_trade_ms);
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
        self.link.forget_timeline();
        self.history_trades = 0;
        // A run anchored to a tape that no longer exists cannot continue, and
        // `restart` below drops the waits its outstanding request would have
        // resolved. Its verdict goes with it.
        self.run_session(None, quantick_session::Act::AbandonRun);
        self.link.latest_trade_latency_ms = None;
        self.link.latest_trade_ms = None;
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
        let built = self.time_panes.len();
        let Some(focus) = self.canvas.switch_layout(layout, built, self.focus) else {
            return;
        };
        if self.canvas.pending_context_panes > 0 {
            // Seeding replays every retained trade, which on a deep history
            // holds the render thread long enough to notice. Armed here and
            // done on the next frame, exactly as a bar-spec change is: the
            // frame carrying the menu click paints the loading overlay first,
            // so the wait reads as the chart working rather than the app
            // hanging.
            self.loading.begin(LoadingTask::BarRebuild);
        }
        self.focus = focus;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANVAS_LAYOUT",
            layout = ?layout,
            time_pane_bars = self.time_pane().map(|pane| pane.state.bars().len()),
            action = if self.canvas.pending_context_panes > 0 {
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
        tab_id: u64,
        config: &AppConfig,
        style: &ChartStyle,
        ids: &mut PaneIdAllocator,
    ) {
        // A chart a removal freed and a new slot sat on is built again, so it
        // never shows the removed chart's look; else the next free slot.
        let built = self.time_panes.len();
        let reseed = (self.canvas.seats.take_reseed()).filter(|seat| *seat < built);
        let slot = reseed.unwrap_or(built);
        let pane = match reseed {
            Some(seat) => Some(self.canvas.fresh_pane(seat, ids)),
            None => self.canvas.take_pending_pane(slot, ids),
        };
        let Some(mut pane) = pane else {
            return;
        };
        pane.seed_from(
            self.flow_pane.state.trades(),
            self.flow_pane.state.backfill_trade_count(),
            self.flow_pane.state.deal_samples(),
        );
        // Seeded from what the flow pane shows, so it also takes every page
        // the flow pane holds unpublished — a run's held pages, a recut in
        // progress — and publishes the same tape its siblings will.
        for page in self.flow_pane.unpublished_history() {
            pane.receive_history(page, true);
        }
        pane.hold_history(self.session.history_reach_running());
        // The pane opens looking like the one it splits away from: a user who
        // switched the crosshair off is not asking for it back by opening a
        // second view of the same market. Orientation is part of that look —
        // an upside-down market does not turn back over by being given a
        // second view, and the boot's QUANTICK_INVERTED hook fires before
        // this pane exists at all.
        // Headless policy selects inheritable switches and marks their effects
        // as opening state, so feature owners do not create user undo entries.
        pane.layer_write()
            .inherit_states(&self.flow_pane.layer_read().states(style));
        pane.price_view
            .set_inverted(self.flow_pane.price_view.is_inverted());
        match reseed {
            Some(seat) => self.time_panes[seat] = pane,
            None => self.time_panes.push(pane),
        }
        // One pane per frame, for the reason the first one waits a frame at
        // all: seeding replays every retained trade, and building three at
        // once would hold the render thread for three times as long. The
        // overlay stays up until the last one lands.
        if self.canvas.pending_context_panes == 0 {
            self.loading.end(LoadingTask::BarRebuild);
        }
        // Its copy counts against a running campaign's memory ceiling, and
        // there is something for a prefix to go in front of now.
        if reseed.is_none() {
            self.run_session(Some(config), quantick_session::Act::PaneAdded { tab_id });
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
        self.flow_pane
            .request_opening_layout(flow.map(crate::layouts::LayoutId));
        self.canvas.context_opening.layouts = context
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
                pane.request_opening_layout(Some(crate::layouts::LayoutId(id)));
            }
        }
    }

    pub(crate) fn candle_styles(
        &self,
    ) -> Vec<quantick_workspace::arrangement_document::SavedPaneStyle> {
        quantick_stores::pane_style::capture_all(
            self.panes()
                .enumerate()
                .map(|(index, (pane, _))| (index, pane.presentation.candles)),
            &self.canvas.context_opening.styles,
        )
    }

    pub(crate) fn restore_candle_styles(
        &mut self,
        saved: &[quantick_workspace::arrangement_document::SavedPaneStyle],
    ) {
        self.flow_pane.presentation.candles = quantick_stores::pane_style::restore(saved, 0);
        for (index, pane) in self.time_panes.iter_mut().enumerate() {
            pane.presentation.candles = quantick_stores::pane_style::restore(saved, index + 1);
        }
        let existing = self.time_panes.len();
        self.canvas.context_opening.set_styles(saved, existing);
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
        collapse: CanvasCollapseRestore<'_>,
        focus: Option<PaneSide>,
        context_intervals_ms: &[i64],
        legends: LegendFold,
    ) {
        self.canvas.open_context_on(context_intervals_ms);
        self.set_layout(layout);
        // *After* `set_layout`, for the reason the focus below is: a switch
        // opens the column it just revealed, which is right for a menu click
        // and wrong for a restore, where the saved state is the answer.
        // Assigned before, a workspace saved with its charts put away reopened
        // with them out — and the next `capture_arrangement` wrote that over
        // the trader's choice.
        self.canvas.context_collapsed = collapse.context;
        self.canvas.flow_collapsed = collapse.flow && !collapse.context;
        self.canvas
            .restore_context_heights(collapse.heights, collapse.collapsed_slots);
        if let Some(fraction) = split_fraction {
            self.canvas.split_fraction = clamp_pane_fraction(fraction);
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
        self.canvas.time_pane_opening_legend_collapsed = legends.time;
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
            .any(|(pane, _side)| pane.spec.pending().is_some());
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
    /// The panes run this independently: each pane's own header governs that
    /// pane (§11), so a change to one must not rebuild the chart beside it.
    fn apply_spec_change_at(&mut self, index: PaneIndex) {
        let Some(pane) = self.pane_at_mut(index) else {
            return;
        };
        if let quantick_engine::bar_selection::SelectionEffect::Rebuild(desired) =
            pane.spec.settle(*pane.state.spec())
        {
            self.recut_pane_with(
                index,
                quantick_strategy::DisarmReason::BarSpecChanged,
                |pane| pane.set_spec(desired),
            );
        }
    }

    /// Apply one complete rule through the shared selector-and-recut path.
    /// The toolbar, REC popover, and control plane all land here.
    pub(crate) fn set_pane_bar_spec(
        &mut self,
        index: usize,
        spec: impl Into<crate::state::BarConfiguration>,
    ) -> Result<bool, quantick_engine::bar_selection::SelectionError> {
        let spec = spec.into();
        let Some(pane) = self.pane_at(index) else {
            return Ok(false);
        };
        let capabilities = *self.link.feed_capabilities.borrow();
        let inputs = quantick_engine::bar_selection::BarInputAvailability {
            traded_volume: capabilities.traded_volume,
            deal_counter: capabilities.deal_counter,
            deal_count: !pane.state.deal_samples().is_empty(),
        };
        let pane = self.pane_at_mut(index).expect("pane checked above");
        let effect = pane.spec.update(
            quantick_engine::bar_selection::SelectionCommand::Replace(spec),
            inputs,
        )?;
        let spec = pane.spec.spec();
        if effect == quantick_engine::bar_selection::SelectionEffect::Unchanged
            && pane.state.spec() == &spec
        {
            return Ok(false);
        }
        self.recut_pane_with(
            index,
            quantick_strategy::DisarmReason::BarSpecChanged,
            |pane| pane.set_spec(spec),
        );
        Ok(true)
    }

    /// Re-cut one pane while preserving its view and drawing anchors and
    /// disarming strategies that belonged to the old timeline.
    pub(crate) fn recut_pane_with(
        &mut self,
        index: PaneIndex,
        reason: quantick_strategy::DisarmReason,
        recut: impl FnOnce(&mut ChartPane),
    ) {
        {
            let Some(pane) = self.pane_at_mut(index) else {
                return;
            };
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
            recut(pane);
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
            let refolded = self.refold_history_prefix().rebuilt_panes & (1 << index) != 0;
            if !refolded && let Some(pane) = self.pane_at_mut(index) {
                pane.send_indicator_rebuild();
            }
            let Some(pane) = self.pane_at_mut(index) else {
                return;
            };
            let slot = anchor.and_then(|ms| pane.slot_at_time(ms));
            let slots = pane.slots();
            pane.model.viewport.reanchor(slot, slots);
            // The marks follow the view: same market time, this pane's
            // new bar space. Nothing is lost, so there is nothing to
            // announce.
            pane.reanchor_drawings(old_slots);
            // The strategies do not follow: the body average that
            // defines a force bar means something else under another
            // bar spec, so the instances disarm and say why. The tape
            // itself continues, so any pending bot entry is swept here
            // and now — through the same funnel manual orders use.
            let cleanup = pane.strategies.anchors.disarm_all(reason);
            let _ = pane.strategies.take_bars();
            for command in cleanup {
                let _ = self.paper.account_mut().apply_strategy_command(command);
            }
            self.drop_overlay_gestures();
        }
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
            first_visible_slot: pane.visible_slots().map(|(start, _)| start),
        })
        .collect()
}
