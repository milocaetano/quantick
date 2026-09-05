//! Running the armed strategy instances against the bars that just closed.
//!
//! The bridge between a print landing and a paper account hearing about it:
//! evaluate whatever the panes have armed, route the simulator's answers back
//! to the instance that asked, and clean up after a drawing menu disarms one.
//! Deliberately cheap when nothing is armed — the common case, and the one a
//! per-trade path must not start charging for.

use super::*;

impl Tab {
    /// Evaluate the armed instances against the bars that just closed and
    /// route the simulator's answers back to them.
    ///
    /// Runs inside the same sweep that ingested the trades, so the slots
    /// queued with each bar and the drawings' anchors are read against one
    /// cut of the series. Order per pane: the prints' events first (they
    /// resolve earlier operations), then closed bars oldest-first, each
    /// instance's commands applied immediately so its own acknowledgement
    /// reaches it before anything else does. Cost is zero on a chart with
    /// no instances — the pane queues no bars and the paper host buffers no
    /// events.
    pub(super) fn run_strategies(&mut self) {
        // The clock is read only when something is listening for it. This
        // runs once per print and `SystemTime::now` is a syscall: a chart
        // with no alarm armed — which is every chart that existed before
        // this feature — must not start paying for one per trade.
        let now_ms = if self.any_alarm_armed() {
            metrics::wall_clock_ms()
        } else {
            0
        };
        self.run_strategies_at(now_ms);
    }

    /// Whether any instance on this tab carries an alarm. Usually a walk
    /// over an empty `Vec`.
    fn any_alarm_armed(&self) -> bool {
        self.panes().any(|(pane, _side)| {
            pane.strategies
                .instances
                .iter()
                .any(|instance| instance.alarm.is_some())
        })
    }

    /// The sweep with its clock handed in.
    ///
    /// Only the alarm's repeat rule reads it — a cooldown is the one thing
    /// here measured in seconds a human waits rather than in prints — and
    /// the kernel never reads a clock of its own, so the reading enters
    /// through this one door and the tests drive it from a fixture.
    fn run_strategies_at(&mut self, now_ms: i64) {
        let now_ms = u64::try_from(now_ms).unwrap_or(0);
        let print_events = self.paper.account_mut().drain_bot_events();
        let Self {
            paper,
            flow_pane,
            time_panes,
            ..
        } = self;
        let mut watching = 0;
        let mut sounds: Vec<crate::audio::Cue> = Vec::new();
        // The alarm's sounds come from `main`; walking every pane in the
        // context stack rather than a single time pane comes from this branch.
        // Both are wanted: a strategy armed on the second stacked chart has to
        // ring like one armed on the first.
        for pane in std::iter::once(flow_pane).chain(time_panes.iter_mut()) {
            if pane.strategies.is_empty() {
                continue;
            }
            // The batch to each instance, applying what it answers with —
            // the self-protection close after a dropped bracket. Applying
            // a returned command emits no events of its own (kernel
            // contract), so one echo settles it.
            if !print_events.is_empty() {
                for index in 0..pane.strategies.instances.len() {
                    let responses = pane.strategies.instances[index]
                        .armed
                        .on_sim_events(&print_events);
                    for command in responses {
                        let events = paper.account_mut().apply_strategy_command(command);
                        let _ = pane.strategies.instances[index]
                            .armed
                            .on_sim_events(&events);
                    }
                }
            }
            let bars = pane.take_strategy_bars();
            if !bars.is_empty() {
                // An instance whose drawing was deleted from a surface that
                // could not remove it directly dies here, in the sweep.
                let alive: Vec<crate::drawings::DrawingId> = pane
                    .strategies
                    .instances
                    .iter()
                    .map(|instance| instance.drawing)
                    .filter(|id| pane.drawings.index_of(*id).is_some())
                    .collect();
                let orphan_cleanup = pane.strategies.drop_orphans(|id| alive.contains(&id));
                for command in orphan_cleanup {
                    // A dead drawing's bot must not leave its entry resting
                    // with no badge over it; swept through the same funnel.
                    let _ = paper.account_mut().apply_strategy_command(command);
                }
            }
            for (bar, slot) in &bars {
                for index in 0..pane.strategies.instances.len() {
                    let drawing = pane.strategies.instances[index].drawing;
                    // A region that cannot honestly be tested (hidden, off
                    // its series, another market) holds fire but never
                    // starves the ruler: the trigger's contract is every
                    // closed bar, so the gates shut instead of the feed.
                    let (region, active) = pane.strategy_region(drawing, *slot).unwrap_or((
                        quantick_strategy::Region::new(
                            rust_decimal::Decimal::ZERO,
                            rust_decimal::Decimal::ZERO,
                        ),
                        false,
                    ));
                    let flat = paper.is_flat();
                    let commands = pane.strategies.instances[index]
                        .armed
                        .on_closed_bar(bar, &region, active, flat);
                    // The alarm judges the same bar, from the kernel's own
                    // reading of it rather than from whether an order went
                    // out — a busy account and a spent one shot silence the
                    // order, never the signal. Every closed bar is offered,
                    // qualifying or not: a preview that failed to hold is
                    // reported here, and this bar's repeat budget resets.
                    sounds.extend(pane.strategies.instances[index].alarm_on_closed_bar(now_ms));
                    for command in commands {
                        let events = paper.account_mut().apply_strategy_command(command);
                        let _ = pane.strategies.instances[index]
                            .armed
                            .on_sim_events(&events);
                    }
                }
            }
            // The bar still forming, judged for the alarm only. Nothing
            // here can place an order or move a state machine: the kernel's
            // preview path is `&self` all the way down, and this is the one
            // caller of it.
            //
            // This whole block is per *print*, so it opens with the question
            // that costs least: does any instance on this pane even carry an
            // alarm? A pane full of ordinary strategies answers no and pays
            // nothing further — not the partial bar's copy, not the
            // progress read.
            let any_alarm = pane
                .strategies
                .instances
                .iter()
                .any(|instance| instance.alarm.is_some());
            if let Some(partial) = any_alarm.then(|| pane.state.partial().cloned()).flatten() {
                let progress = pane.state.progress().map(|(progress, _unit)| progress);
                let slot = pane.closed_slots();
                for index in 0..pane.strategies.instances.len() {
                    // Cheapest gate next, before the region is resolved: on
                    // all but a handful of prints the alarm answers "not
                    // yet" and this costs one comparison.
                    let wants = pane.strategies.instances[index]
                        .alarm
                        .as_ref()
                        .is_some_and(|alarm| alarm.wants_forming_check(progress, now_ms));
                    if !wants {
                        continue;
                    }
                    let drawing = pane.strategies.instances[index].drawing;
                    let (region, active) = pane.strategy_region(drawing, slot).unwrap_or((
                        quantick_strategy::Region::new(
                            rust_decimal::Decimal::ZERO,
                            rust_decimal::Decimal::ZERO,
                        ),
                        false,
                    ));
                    sounds.extend(
                        pane.strategies.instances[index]
                            .alarm_on_forming_bar(&partial, &region, active, progress, now_ms),
                    );
                }
            }
            watching += pane.strategies.watching();
        }
        paper.account_mut().set_bot_listening(watching > 0);
        self.pending_alarm_sounds.extend(sounds);
    }

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
            for command in pane.take_strategy_cleanup() {
                let _ = paper.account_mut().apply_strategy_command(command);
            }
        }
    }

    /// Throw away everything loaded and wait for the source to refill it.
    ///
    /// Sent by a source that rewound — seeking a replay, for instance. The
    /// chart is rebuilt from the history that follows rather than patched,
    /// because bars that already closed cannot be reopened.
    pub fn reset_market_state(&mut self) {
        for pane in self.panes_mut() {
            pane.reset_series();
            // Indicators follow the chart into the empty state; the refill's
            // Backfilled event replays them (replay seek funnels through here,
            // so seeking inherits correct indicator behavior for free).
            pane.send_indicator_rebuild();
            pane.last_lane_divider_x = None;
            // Judgements armed on the old timeline do not carry into the
            // rebuilt one — the same honesty rule the simulator's flatten
            // follows, with the reason on the badge. No cleanup commands
            // come back under this reason: `paper.on_timeline_reset` below
            // sweeps every order with the honest `reset` label.
            let _ = pane
                .strategies
                .disarm_all(quantick_strategy::DisarmReason::TimelineReset);
        }
        self.drop_overlay_gestures();
        // The timeline these describe is the one being rebuilt. A gap
        // re-anchors itself by market time, so one left behind would paint
        // itself onto the refilled series as though a reconnect had happened.
        self.feed_gaps.clear();
        self.resume_floor_ms = None;
        self.history_trades = 0;
        // A run anchored to a tape that no longer exists cannot continue, and
        // `restart` below drops the waits its outstanding request would have
        // resolved. Its verdict goes with it.
        self.abandon_history_run();
        self.latest_trade_latency_ms = None;
        self.latest_trade_ms = None;
        // The refill arrives as one backfill batch; keep the loading indicator
        // up until it lands. Requests sent to the source before the reset will
        // never be answered, so the count restarts rather than accumulates.
        self.opening_slices_remaining = None;
        self.loading.restart(LoadingTask::History);
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);
        // The simulator flattens at its last mark and says so — a position
        // cannot honestly survive into a rebuilt timeline.
        self.paper.on_timeline_reset();
    }

    /// Everything this tab must settle before it is dropped.
    ///
    /// Feeds and workers need nothing: their loops end when the channels go
    /// with the tab. The simulator does — an open position is state the user
    /// created, and the honesty contract says it ends in an explicit, labeled,
    /// journaled flatten, never by silently vanishing with its window.
    pub fn close(&mut self) {
        self.paper.on_timeline_reset();
    }
}
