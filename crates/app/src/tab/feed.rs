//! The feed's life: switching it, draining it, and losing and regaining it.
//!
//! A tab's connection to a market from the moment it is chosen to the moment
//! it is replaced. Choosing and re-choosing a source (with the depth capture
//! and the presets that follow a switch), the per-frame drains that move
//! trades and depth events into the panes, and the recovery paths — replay
//! opened and closed, a reconnect, a reload, a gap marked where market time
//! has no print. Written against the `FeedEvent` channel alone, so a replay
//! and a live venue take the same path through it.

use super::*;
use quantick_feed as feed;

impl Tab {
    /// Allocate a capture generation well above all reconnect generations from
    /// the previous UI capture epoch.
    pub fn next_book_generation(&mut self) -> u64 {
        self.book_capture_epoch = self.book_capture_epoch.saturating_add(1);
        self.book_capture_epoch
            .saturating_mul(BOOK_GENERATION_STRIDE)
    }

    /// Keep the recorder running for any feed that can stream depth.
    ///
    /// Capture is a data concern: it starts with the feed and stops only when
    /// the market itself changes (feed/symbol switch, or a replay taking the
    /// chart over). Showing and hiding the map never reaches this far, which is
    /// what lets a hidden heatmap come back with its history intact.
    ///
    /// Recording with nobody watching stays inside the retention budget the
    /// heatmap already had — `retention_ms` (30 min by default) bounded by
    /// `max_history_runs` / `max_history_bytes` — so the ceiling is the same
    /// one an open map pays for, not a new one.
    ///
    /// Idempotent and cheap, so the frame loop can call it as a heartbeat on
    /// top of the lifecycle calls: already recording costs one bool read, and
    /// a replay costs one more `Option` check.
    pub fn ensure_book_capture(&mut self, config: &AppConfig) {
        if self.tape().enabled() || !self.capabilities(config).book_capture {
            return;
        }
        self.request_book_capture(config, true);
    }

    /// Start or stop the independent depth pipeline without touching aggTrades
    /// or candle construction. UI state changes only if the command is queued.
    pub fn request_book_capture(&mut self, config: &AppConfig, enabled: bool) {
        if !self.capabilities(config).book_capture {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROVIDER_UNSUPPORTED",
                feed = self.feed_id.as_str(),
                symbol = self.symbol.as_str(),
                enabled,
                action = "leave_capture_disabled",
                "selected provider has no order-book pipeline"
            );
            return;
        }

        let generation = self.next_book_generation();
        let command = FeedCommand::SetBookCapture {
            enabled,
            initial_generation: generation,
        };
        match self.commands.try_send(command) {
            Ok(()) => self.tape_mut().set_enabled(enabled, generation),
            Err(mpsc::error::TrySendError::Full(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_COMMAND_BACKPRESSURE",
                symbol = self.symbol.as_str(),
                enabled,
                generation,
                action = "retry_on_next_frame",
                "book capture command channel is full"
            ),
            Err(mpsc::error::TrySendError::Closed(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_COMMAND_CHANNEL_CLOSED",
                symbol = self.symbol.as_str(),
                enabled,
                generation,
                action = "keep_current_capture_state",
                "book capture command channel is closed"
            ),
        }
    }

    /// Restart capture after a semantic configuration change such as base
    /// price grouping. The view commits its staged reset only after this
    /// command is accepted, preserving current history on backpressure.
    pub fn restart_book_capture(&mut self) {
        if !self.tape().enabled() {
            return;
        }
        let generation = self.next_book_generation();
        match self.commands.try_send(FeedCommand::RestartBookCapture {
            initial_generation: generation,
        }) {
            Ok(()) => self.tape_mut().accept_capture_grouping_restart(generation),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.tape_mut()
                    .reject_capture_grouping_restart("command_channel_full");
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_RESTART_BACKPRESSURE",
                    symbol = self.symbol.as_str(),
                    generation,
                    action = "keep_existing_capture",
                    "book restart command channel is full"
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.tape_mut()
                    .reject_capture_grouping_restart("command_channel_closed");
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_RESTART_CHANNEL_CLOSED",
                    symbol = self.symbol.as_str(),
                    generation,
                    action = "keep_existing_capture",
                    "book restart command channel is closed"
                );
            }
        }
    }

    /// Respawn the feed and reset the chart when the selected feed or symbol
    /// differs from what is currently streaming. A no-op otherwise.
    pub fn maybe_switch_feed(&mut self, config: &AppConfig) {
        // A replay owns the chart until it is closed. The selectors are not
        // drawn while it plays, so nothing can diverge here — but a stale
        // selection must not respawn a live feed underneath the recording.
        if self.replay.is_some() {
            return;
        }
        if self.active == (self.feed_id.clone(), self.symbol.clone()) {
            return;
        }
        let (previous_feed, previous_symbol) = self.active.clone();
        let Some(provider) = config.provider_of(&self.feed_id) else {
            tracing::warn!(
                target: "quantick::app",
                feed = %self.feed_id,
                "selected feed is not in the config; ignoring switch"
            );
            // Snap the selection back to what is actually running.
            (self.feed_id, self.symbol) = self.active.clone();
            return;
        };
        // The recorder follows the feed, not the toggle: a market that can
        // stream depth is recorded from the moment it starts streaming. Kept
        // for the log line below; the start itself goes through
        // [`Self::ensure_book_capture`], the one place that decides it.
        let resume_book_capture = provider.capabilities().book_capture;

        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_SWITCH",
            feed = %self.feed_id,
            symbol = %self.symbol,
            provider = ?provider,
            resume_book_capture,
            action = "reset_market_state",
            "switching feed/symbol; resetting chart"
        );

        // Dropping the old handle stops the old feed thread. The new feed starts
        // with a fresh backfill in flight.
        let handle = feed::spawn_live(provider, &self.symbol, &config.metatrader, shelf_dir());
        self.attach(handle);

        // Rebuild every pane from scratch for the new stream, each keeping its
        // own bar spec. Retained trades from the old symbol must not leak in.
        for pane in self.panes_mut() {
            pane.reset_series();
        }
        self.drop_overlay_gestures();
        // The marks stay — only the trader deletes a drawing — but they were
        // drawn on the instrument that just left. Time survives a symbol
        // switch and price does not, so without this a BTC level would paint
        // at full strength over an index chart, at a number that means
        // nothing there (§D7b).
        for pane in self.panes_mut() {
            pane.drawings.mark_market_changed();
            // The regions those instances watched belong to the market that
            // just left; a bot must never fire on a level from another
            // instrument. Disarmed by name, never silently dropped. Their
            // pending entries need no sweep of ours: the paper reset a few
            // lines down cancels every order with the honest `reset` label.
            let _ = pane
                .strategies
                .disarm_all(quantick_strategy::DisarmReason::MarketChanged);
            let _ = pane.take_strategy_bars();
        }
        self.history_trades = 0;
        // The old feed's unanswered loads died with its channel; the new feed
        // opens with exactly one backfill in flight.
        self.opening_slices_remaining = None;
        self.loading.restart(LoadingTask::History);
        self.latest_trade_latency_ms = None;
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);
        // The simulated position cannot follow this tab onto another market:
        // flatten at the old symbol's last mark, labeled and journaled (the
        // journal still targets the old symbol — it only follows `self.symbol`
        // at the top of the next drain).
        self.paper.on_timeline_reset();

        self.active = (self.feed_id.clone(), self.symbol.clone());
        self.refresh_chip_label(config);
        self.ensure_book_capture(config);
        self.apply_feed_bubble_preset_after_switch(config, &previous_feed, &previous_symbol);
    }

    /// Apply the arrived-at declared preset — when the switch crossed feeds,
    /// or when it crossed symbols whose declared looks differ. A symbol hop
    /// between two symbols that declare nothing of their own keeps the user's
    /// panel tweaks, exactly as before per-symbol declarations existed: the
    /// declared look belongs to the feed, and to the symbols that state one.
    ///
    /// One asymmetry is deliberate: hopping *off* a declared symbol onto one
    /// that declares nothing (on a feed that also declares nothing) keeps the
    /// look just left on screen — nothing remembers what the panel wore
    /// before the declaration applied, and inventing a "previous look" store
    /// for that one hop would be a second owner for the panel's state.
    pub fn apply_feed_bubble_preset_after_switch(
        &mut self,
        config: &AppConfig,
        previous_feed: &str,
        previous_symbol: &str,
    ) {
        if previous_feed == self.feed_id {
            let feed = config.feed(&self.feed_id);
            let arrived = feed.and_then(|feed| feed.bubble_preset_for(&self.symbol));
            let left = feed.and_then(|feed| feed.bubble_preset_for(previous_symbol));
            if arrived.is_none() || arrived == left {
                return;
            }
        }
        self.apply_feed_bubble_preset(config);
    }

    /// Apply the bubble preset declared for the current feed and symbol, if
    /// one is declared ([`FeedConfig::bubble_preset_for`]'s ladder: the
    /// symbol's own entry first, the feed-wide declaration behind it).
    ///
    /// A feed declaring nothing changes nothing: the panel keeps the look the
    /// user last chose. An unknown name is reported and ignored — the presets
    /// file is user-edited, and a typo there must not silently restyle the
    /// chart.
    pub fn apply_feed_bubble_preset(&mut self, config: &AppConfig) {
        let Some(name) = config
            .feed(&self.feed_id)
            .and_then(|feed| feed.bubble_preset_for(&self.symbol))
            .map(str::to_owned)
        else {
            return;
        };
        let applied = self.tape_mut().apply_preset(&name);
        if applied {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FEED_BUBBLE_PRESET",
                feed = %self.feed_id,
                symbol = %self.symbol,
                preset = name.as_str(),
                action = "apply_preset",
                "feed declares a bubble preset; applied"
            );
        } else {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FEED_BUBBLE_PRESET_UNKNOWN",
                feed = %self.feed_id,
                symbol = %self.symbol,
                preset = name.as_str(),
                action = "keep_current_look",
                "feed declares a bubble preset that is not in the presets file; ignoring"
            );
        }
    }

    /// Open wearing the layout the feed declares, if it declares one.
    ///
    /// Startup-scoped, like `default_feed`: it decides what a tab on this
    /// feed *opens* showing, and never touches a layout the user has since
    /// chosen — the callers are tab creation, nothing else. A feed with no
    /// `default_layout` changes nothing, so the factory default stays the
    /// flow pane (the decision on record in the UX audit §3).
    pub fn apply_feed_declared_layout(&mut self, config: &AppConfig) {
        let Some(declared) = config
            .feed(&self.feed_id)
            .and_then(|feed| feed.default_layout)
        else {
            return;
        };
        let layout = CanvasLayout::from(declared);
        // A declared time-bar spec names the interval the declared layout's
        // time pane opens on; the pane's own header takes over from there.
        if layout.shows_time()
            && let Some(BarSpec::Time(ms)) = config.startup_spec_for(&self.feed_id)
        {
            self.time_pane_opening_interval_ms = ms;
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_DECLARED_LAYOUT",
            feed = %self.feed_id,
            layout = ?layout,
            action = "open_declared_layout",
            "feed declares an opening layout; applied"
        );
        self.set_layout(layout);
        // Opening is not switching. [`Self::set_layout`] focuses whatever the
        // switch revealed, which is right for a menu click — the trader asked
        // for that pane. On a tab that has never been on screen there was no
        // gesture and nothing was revealed, and leaving the focus there put a
        // fresh window's BARS group and status line on the *context* chart:
        // the first thing a trader touched changed the timeframe pane instead
        // of the flow chart quantick exists to show. So the flow pane takes
        // the focus whenever this layout draws it.
        self.focus = if layout.shows_flow() {
            PaneSide::Flow
        } else {
            PaneSide::Time(0)
        };
    }

    /// Drain every feed event available this frame into the engine, tracking the
    /// observed arrival latency and live-trade counts for the metrics.
    pub fn drain_feed(&mut self) {
        self.drain_feed_with_clock(metrics::wall_clock_ms);
    }

    /// Clock-injected drain used to prove that one UI cycle is one observation.
    pub fn drain_feed_with_clock(&mut self, mut wall_clock_ms: impl FnMut() -> i64) {
        // The journal follows this tab's symbol; synced before the drain so a
        // new feed's first trades are never attributed to the old symbol.
        // Every tab drains every frame, so every journal tracks its own market
        // whether or not that tab is the one on screen.
        let Self { paper, symbol, .. } = self;
        paper.set_symbol(symbol);
        let mut live = false;
        let mut received_at_ms = None;
        loop {
            match self.events.try_recv() {
                Ok(FeedEvent::Backfilled(trades)) => {
                    self.loading.end(LoadingTask::History);
                    // A session that resumed onto a kept timeline opens by
                    // replaying its recent window, which the chart already
                    // holds. Whatever is genuinely newer is forwarded as live,
                    // in order, exactly as the feed does inside one session.
                    if self.resume_floor_ms.is_some() {
                        live |= self.ingest_resumed(&trades);
                        continue;
                    }
                    self.history_trades += trades.len();
                    // History only seeds the simulator's mark — filling
                    // against the past would be look-ahead.
                    if let Some(last) = trades.last() {
                        self.paper.seed(last);
                    }
                    // One tape, every pane: the split multiplies views of the
                    // market, never the stream behind them.
                    for pane in self.panes_mut() {
                        pane.ingest_backfill(&trades);
                    }
                }
                Ok(FeedEvent::HistoryPrepended(trades)) => {
                    // The reply — even an empty one — answers exactly one
                    // pending load; the indicator survives until the last one.
                    self.loading.end(LoadingTask::History);
                    // The MetaTrader bridge delivers its recovery window on
                    // this event rather than as backfill, so a resumed session
                    // is filtered here too. Prepending it would put a block the
                    // chart already holds in front of the bars it duplicates.
                    if self.resume_floor_ms.is_some() {
                        live |= self.ingest_resumed(&trades);
                        continue;
                    }
                    self.history_trades += trades.len();
                    // Each pane cuts the older trades into its own bars, so
                    // each shifts its own anchors by its own count.
                    for pane in self.panes_mut() {
                        pane.prepend_history(&trades);
                    }
                    // The first engine bar just moved backwards in time, and
                    // the prefix was trimmed against where it used to be. Any
                    // venue candle now covering a re-cut minute has to go.
                    self.refold_history_prefix();
                    // And the reach that asked for this page decides whether
                    // to ask for another, and what to tell the trader if it
                    // will not. After the prepend, so it judges the tape the
                    // trader can actually see.
                    self.settle_history_page(trades.len());
                }
                Ok(FeedEvent::OpeningPrepended { trades, remaining }) => {
                    // What is left of the fill, so the chart and an operator
                    // reading the control plane can both say how much of the
                    // session is still arriving instead of watching a number
                    // rise with no denominator. Cleared at zero: the field
                    // means "a fill is running", and a stale count would keep
                    // saying so after the last slice landed.
                    self.opening_slices_remaining = match remaining {
                        Some(0) | None => None,
                        some => some,
                    };
                    // The rest of the opening session, drawn but not counted.
                    // Everything the reply path does *except* the two things
                    // that belong to a request: the loading indicator a press
                    // raised stays up, and the campaign that press started is
                    // not handed a page it did not fetch.
                    if self.resume_floor_ms.is_some() {
                        live |= self.ingest_resumed(&trades);
                        continue;
                    }
                    self.history_trades += trades.len();
                    for pane in self.panes_mut() {
                        pane.prepend_history(&trades);
                    }
                    self.refold_history_prefix();
                }
                Ok(FeedEvent::Live(trade)) => {
                    if self.resume_floor_ms.is_some() {
                        live |= self.ingest_resumed(std::slice::from_ref(&trade));
                        continue;
                    }
                    let received_at_ms = *received_at_ms.get_or_insert_with(&mut wall_clock_ms);
                    self.ingest_live_trade_at(&trade, received_at_ms);
                    live = true;
                }
                Ok(FeedEvent::LiveBatch(trades)) => {
                    if !trades.is_empty() {
                        if self.resume_floor_ms.is_some() {
                            live |= self.ingest_resumed(&trades);
                            continue;
                        }
                        let received_at_ms = *received_at_ms.get_or_insert_with(&mut wall_clock_ms);
                        for trade in &trades {
                            self.ingest_live_trade_at(trade, received_at_ms);
                        }
                        live = true;
                    }
                }
                Ok(FeedEvent::Reset) => self.reset_market_state(),
                Ok(FeedEvent::OhlcvHistory {
                    interval_ms,
                    bars,
                    slice,
                }) => {
                    self.take_ohlcv_history(interval_ms, bars, slice);
                }
                Err(_) => break,
            }
        }
        // One forming-bar update per pane for the whole drain, however many
        // prints arrived: only its latest value is ever read.
        if live {
            for pane in self.panes_mut() {
                pane.publish_partial();
            }
        }
        // After the bars exist, so the hooked gap has something to sit
        // between. Costs one `Option` test per drain when the hook is unset,
        // which is every run but a capture.
        self.land_demo_gap();
        // A reset left the marks waiting for bars to anchor to, and this is
        // the drain that may have just delivered them. One flag test per pane
        // when nothing is owed.
        for pane in self.panes_mut() {
            pane.settle_pending_reanchor();
        }
    }

    /// Take the newest feed notice, if the feed sent any this frame.
    ///
    /// Level-triggered rather than queued: only the latest state matters, and
    /// a burst of bridge output must not queue up cards to show one by one.
    /// A closed channel (a feed with nothing to report) simply yields nothing.
    pub fn drain_notices(&mut self) {
        self.drain_notices_at(metrics::wall_clock_ms());
    }

    /// Clock-injected half, so a test can drive a notice across its stall
    /// budget without waiting for one.
    pub fn drain_notices_at(&mut self, now_ms: i64) {
        while let Ok(notice) = self.notices.try_recv() {
            let next = match notice {
                FeedNotice::Connected => {
                    self.set_connection(FeedConnectionState::Connected, now_ms);
                    FeedNotice::Clear
                }
                FeedNotice::Reconnecting { .. } => {
                    self.set_connection(FeedConnectionState::Reconnecting, now_ms);
                    notice
                }
                FeedNotice::Working { .. } | FeedNotice::Attention { .. } => notice,
                FeedNotice::Clear => FeedNotice::Clear,
            };
            // Stamped only on a *change*. A supervisor repeating the same line
            // every few seconds would otherwise keep resetting the very budget
            // it is supposed to run out of, and the chart would sit on
            // "connecting" for as long as the retry loop lived — which is the
            // failure this clock exists to end.
            if next != self.notice {
                self.notice = next;
                self.notice_since_ms = now_ms;
            }
        }
    }

    /// Where a feed's trouble is actually visible: the painted pane with the
    /// least on it, and how many bar slots that pane holds.
    ///
    /// One tab is one feed, so its notice is tab-scoped information — but its
    /// *consequence* never is. A MetaTrader tab whose time panes are full of
    /// the terminal's candle history and whose flow pane has not seen one tick
    /// is starved in exactly one place, and the card explaining it was being
    /// centred across all three. The trader read a sentence attached to
    /// nothing, floating over two charts with nothing wrong with them.
    ///
    /// Fewest slots wins; the larger pane breaks a tie, so an all-empty canvas
    /// puts the card on the biggest chart — which is where it landed before
    /// this existed, and why a single-pane tab looks unchanged.
    #[must_use]
    pub fn starved_pane(&self) -> Option<(egui::Rect, usize)> {
        self.panes()
            .filter_map(|(pane, _)| pane.last_area.map(|area| (area, pane.slots())))
            .min_by(|(left_area, left_slots), (right_area, right_slots)| {
                left_slots
                    .cmp(right_slots)
                    .then_with(|| right_area.area().total_cmp(&left_area.area()))
            })
    }

    /// Move the transport to `state`, stamping when it got there.
    ///
    /// Only a real transition stamps: a provider that reports the same state
    /// twice has not changed anything, and treating it as a change would reset
    /// the budget measured from it.
    fn set_connection(&mut self, state: FeedConnectionState, now_ms: i64) {
        if self.feed_connection != state {
            self.feed_connection = state;
            self.connection_since_ms = now_ms;
        }
    }

    /// This tab's own judgement about a feed that has stopped delivering, or
    /// `None` while it is merely slow.
    ///
    /// Deterministic half: the caller supplies wall clock, exactly as
    /// [`Self::tape_age_at`] does. The decision itself lives in
    /// [`quantick_feed::stall`] and touches no clock at all.
    #[must_use]
    pub fn stall_at(&self, config: &AppConfig, now_ms: i64) -> Option<Stall> {
        let provider = config.provider_of(&self.feed_id)?;
        if let Some(forced) = self.forced_stall {
            return Some(forced.stall(provider, self.feed_display_name(config)));
        }
        stall::assess(
            &StallInput {
                notice: &self.notice,
                connection_since_ms: self.connection_since_ms,
                connection: self.feed_connection,
                tape_age_ms: self.tape_age_at(now_ms),
                attached_ms: self.feed_attached_ms,
                provider,
                provider_name: self.feed_display_name(config),
                replaying: self.replay.is_some(),
            },
            now_ms,
        )
    }

    /// Deterministic half of live ingestion: `received_at_ms` is the UI's epoch
    /// observation time, supplied explicitly so tests never wait on a clock.
    ///
    /// The transport observation is the window's; what the trade does to the
    /// bars, the tape and the indicators is the pane's.
    pub fn ingest_live_trade_at(&mut self, trade: &quantick_engine::Trade, received_at_ms: i64) {
        self.latest_trade_latency_ms =
            metrics::feed_lag_ms(received_at_ms, Some(trade.timestamp_ms));
        self.latest_trade_ms = Some(trade.timestamp_ms);
        self.live_trades += 1;
        // The simulator taps the same per-trade point the bar engine does, so
        // paper trading works identically on a live feed and a replay — and on
        // a tab the user is not looking at, whose position keeps marking
        // against its own tape.
        self.paper.on_trade(trade);
        for pane in self.panes_mut() {
            pane.ingest_live_trade(trade);
        }
        self.run_strategies();
    }

    /// Drain a bounded number of synchronized depth events. The separate
    /// channel and budget ensure heatmap work cannot block candle ingestion.
    pub fn drain_book_feed(&mut self) {
        self.drain_book_feed_with_clock(metrics::wall_clock_ms);
    }

    /// Clock-injected depth drain; a burst handled by one UI frame has one
    /// observation time, matching the trade-side metric and avoiding O(n)
    /// system-clock reads.
    pub fn drain_book_feed_with_clock(&mut self, mut wall_clock_ms: impl FnMut() -> i64) {
        let mut received_at_ms = None;
        for _ in 0..BOOK_DRAIN_BUDGET {
            match self.book_events.try_recv() {
                Ok(event) => {
                    let received_at_ms = *received_at_ms.get_or_insert_with(&mut wall_clock_ms);
                    self.tape_mut().handle_depth_event_at(event, received_at_ms);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if self.tape().enabled() && !self.book_channel_closed_reported {
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "HEATMAP_EVENT_CHANNEL_CLOSED",
                            symbol = self.symbol.as_str(),
                            action = "retain_last_book_and_wait_for_feed_switch",
                            "depth event channel closed"
                        );
                        self.book_channel_closed_reported = true;
                    }
                    break;
                }
            }
        }
    }

    /// Delay observed when the newest live trade reached the UI.
    ///
    /// `None` while a session is replaying: those prints are as old as the day
    /// they were recorded, so their original arrival latency is unavailable.
    ///
    /// This figure freezes between prints — it is an observation, not a
    /// measurement of now. [`Self::tape_age_ms`] is the one that ages.
    pub fn trade_arrival_ms(&self) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        // Never the forced split's figure. This is a *measurement* — it reaches
        // the log as APP_HIGH_TRADE_LAG, the control feed scope and the health
        // view, none of which carry a marker saying a capture run invented it,
        // and inferred data that is not labelled as such is the one thing this
        // repo does not ship. The hook drives the readout through the latency
        // port instead, where every consumer already knows the figure is the
        // feed's own and not the chart's.
        self.latest_trade_latency_ms
    }

    /// How old the newest event on the tape is, right now.
    ///
    /// Deterministic half: the caller supplies wall clock. Takes the newer of
    /// the trade stream and the book, so a symbol with depth but a thin tape
    /// is not called stale while its book is live. `None` while replaying, and
    /// before anything has arrived — nothing to be stale about yet.
    pub fn tape_age_at(&self, now_ms: i64) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        let newest = match (self.latest_trade_ms, self.tape().last_event_ms()) {
            (Some(trade), Some(book)) => Some(trade.max(book)),
            (trade, book) => trade.or(book),
        }?;
        Some(now_ms.saturating_sub(newest).max(0))
    }

    /// Data-honesty label for how each print is known, or `None` when the venue
    /// reports true trades and true sides (§8 — the status bar's middle
    /// section). The label shares its row with the machinery readouts, so it
    /// stays short and the full story lives in the hover.
    ///
    /// A venue that prints nothing at all takes precedence over how sides were
    /// decided: on a quote-driven feed *every* print is derived, and saying so
    /// is the more important disclosure. Without it a chart of one-unit prints
    /// reads as a market where every trade happened to be the same size.
    pub fn side_note(&self, config: &AppConfig) -> Option<(String, Option<String>)> {
        if let Some(link) = &self.replay {
            Some((
                match link.session.header.side_source.as_deref() {
                    Some(source) => format!("side: {source}"),
                    None => "side: not recorded".to_owned(),
                },
                None,
            ))
        } else if config.provider_of(&self.active.0).is_some()
            && !self.capabilities(config).traded_volume
        {
            Some((
                "prints: quote-derived".to_owned(),
                Some(
                    "this venue quotes prices but prints no trades: every candle is built \
                     from one synthetic print per tick, at the mid of bid and ask, carrying \
                     one unit — never a traded size"
                        .to_owned(),
                ),
            ))
        } else {
            // The running feed, not the still-uncommitted selection.
            config
                .side_note(&self.active.0)
                .map(|note| (note.to_owned(), None))
        }
    }

    /// Make a recorded session the chart's source, replacing whatever feed is
    /// running. The live selection is untouched, so closing the replay comes
    /// back to exactly the feed and symbol that were streaming before.
    pub fn open_replay(&mut self, config: &AppConfig, request: quantick_feed::ReplayRequest) {
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_OPENED",
            session = %request.session.label(),
            file = %request.session.path.display(),
            trades = request.session.trades.len(),
            speed = request.options.speed,
            action = "replace_feed_source",
            "opening a recorded session"
        );

        // A position on the tape belongs to the session that is ending:
        // flatten it while the journal still carries that session's
        // source. attach() flips the source to the new feed's, and a
        // flatten after it would file the old session's trade under the
        // new session's name — a live trade laundered into the practice
        // record. (reset_market_state below flattens again: a no-op.)
        self.paper.on_timeline_reset();
        let source = feed::FeedSource::Replay(Box::new(request));
        let handle = feed::spawn(source, &config.metatrader, shelf_dir());
        self.attach(handle);

        if let Some(link) = &self.replay {
            self.symbol = link.symbol().to_string();
        }
        self.refresh_chip_label(config);
        // Depth is not in a recording; the toggle is disabled by capability,
        // and the view must not keep drawing a book from the live feed.
        let generation = self.next_book_generation();
        self.tape_mut().set_enabled(false, generation);
        self.reset_market_state();
    }

    /// Leave replay and put the live feed back.
    pub fn close_replay(&mut self, config: &AppConfig) {
        if self.replay.take().is_none() {
            return;
        }
        let (feed_id, symbol) = self.active.clone();
        self.feed_id = feed_id;
        self.symbol = symbol;
        self.refresh_chip_label(config);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_CLOSED",
            feed = %self.feed_id,
            symbol = %self.symbol,
            action = "respawn_live_feed",
            "leaving market replay"
        );

        let Some(provider) = config.provider_of(&self.feed_id) else {
            // The configuration changed under us; there is nothing to go back
            // to, so the chart stays as it is rather than dying.
            self.reset_market_state();
            return;
        };
        // Same ordering rule as open_replay: the flatten of a replay
        // position must journal under the replay source, before attach()
        // flips the journal back to live — or a practice trade counts in
        // the real track record.
        self.paper.on_timeline_reset();
        let handle = feed::spawn_live(provider, &self.symbol, &config.metatrader, shelf_dir());
        self.attach(handle);
        self.reset_market_state();
    }

    /// Respawn the transport and keep everything the chart has built:
    /// bars, drawings, indicators, armed strategies, the open paper position.
    ///
    /// The cheap half of recovery, and the one a trader reaches for when the
    /// feed hiccuped: nothing on screen moves. Before it existed the only
    /// button the feed's notice offered said "Try again" and quietly ran
    /// [`Self::reload_feed`] — which flattens the position and disarms every
    /// strategy. A control that costs a trader their position without saying so
    /// is not a recovery control.
    ///
    /// The new session replays its own recent window, so the market time the
    /// chart had already reached becomes a floor
    /// ([`quantick_feed::past_resume_floor`]): everything at or before it is
    /// overlap and is dropped, and the first print past it decides whether the
    /// silence was long enough to be a marked gap. A replay owns the chart
    /// while it plays and has no transport to recover.
    /// Returns whether the feed was really respawned. `false` means there was
    /// nothing to respawn — a recorded session owns the chart, or the tab's
    /// feed id is no longer in the feed table — and a caller told `true` when
    /// nothing happened is exactly the inferred-versus-observed lie the
    /// honesty rule forbids.
    pub fn reconnect_feed(&mut self, config: &AppConfig) -> bool {
        if self.replay.is_some() {
            return false;
        }
        let Some(provider) = config.provider_of(&self.feed_id) else {
            return false;
        };
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_RECONNECTED_BY_USER",
            feed = %self.feed_id,
            symbol = %self.symbol,
            action = "respawn_feed_keeping_timeline",
            resume_floor_ms = self.latest_trade_ms,
            "reconnecting the feed and keeping the timeline"
        );
        // Taken before the handle is swapped: it is a fact about the chart, not
        // about the session, and it has to survive the attach.
        self.resume_floor_ms = self.latest_trade_ms;
        let handle = feed::spawn_live(provider, &self.symbol, &config.metatrader, shelf_dir());
        self.attach_resuming(handle);
        // The book from the dropped socket is gone with it. Nothing is reset
        // here on purpose: the new session opens with a complete snapshot, and
        // a snapshot is exactly what replaces a book wholesale.
        self.ensure_book_capture(config);
        true
    }

    /// Throw the timeline away and rebuild it from zero.
    ///
    /// The expensive half, for the case the trader described: MetaTrader itself
    /// froze, so the bars on screen are not merely behind, they are wrong.
    /// Reconnecting a socket that is already open fixes nothing there; what
    /// they did by hand was close quantick and open it again, and this is that
    /// act without the restart.
    ///
    /// It costs what a restart costs, which is why the card says so before it
    /// is pressed: [`Self::reset_market_state`] disarms every strategy and
    /// [`PaperTrading::on_timeline_reset`] closes and journals an open
    /// position. A position cannot honestly survive into a rebuilt timeline.
    /// Returns whether the chart was really rebuilt; see
    /// [`Self::reconnect_feed`] for why the answer is reported rather than
    /// assumed.
    pub fn reload_feed(&mut self, config: &AppConfig) -> bool {
        if self.replay.is_some() {
            return false;
        }
        let Some(provider) = config.provider_of(&self.feed_id) else {
            return false;
        };
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_RELOADED_BY_USER",
            feed = %self.feed_id,
            symbol = %self.symbol,
            action = "respawn_feed",
            "rebuilding the chart from a new feed session"
        );
        // A rebuild has no timeline to resume onto, so no floor and no seam.
        self.resume_floor_ms = None;
        let handle = feed::spawn_live(provider, &self.symbol, &config.metatrader, shelf_dir());
        self.attach(handle);
        self.reset_market_state();
        // The live market is back and it can stream depth again; start
        // recording immediately rather than waiting for the map to be opened.
        self.ensure_book_capture(config);
        true
    }

    /// Take in a batch from a session that resumed onto a kept timeline,
    /// keeping only what the chart has not already seen.
    ///
    /// The prints that survive reach the bars, the tape and the simulator's
    /// *mark*, and nothing else — no fill, no strategy, no arrival-latency
    /// reading. They are history: the market made them minutes ago while
    /// nobody was listening. Run through the live path they would fill a
    /// resting limit at a price the trader could never have been filled at,
    /// fire a strategy on a bar that is already over, and report the length of
    /// the outage as this feed's delay. `Backfilled` states the same rule in
    /// its own words — history only seeds the mark — and this is the other
    /// place history arrives.
    ///
    /// The floor lives for exactly one event, whatever that event contained.
    /// A session replays its window once, at the start; leaving the floor up
    /// past that swallowed the next *load older* answer whole (every older
    /// print is below the floor), and on a venue that replays nothing it
    /// waited for the next natural print and called the quiet in between a
    /// gap.
    ///
    /// Returns whether anything was actually ingested, so the caller's live
    /// flag means the same thing on this path as on the ordinary one.
    fn ingest_resumed(&mut self, trades: &[quantick_engine::Trade]) -> bool {
        let Some(floor) = self.resume_floor_ms.take() else {
            return false;
        };
        let fresh = past_resume_floor(trades, floor);
        let overlap = trades.len() - fresh.len();
        if overlap > 0 {
            // Logged rather than silent, like the same rule inside
            // `quantick-feed-mt5`: a print that was dropped is a print the
            // chart chose not to count, and that choice is reviewable.
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FEED_RESUME_OVERLAP_DROPPED",
                feed = %self.feed_id,
                symbol = %self.symbol,
                dropped = overlap,
                floor_ms = floor,
                "dropped the window a resumed session replayed"
            );
        }
        let Some(first) = fresh.first() else {
            return false;
        };
        self.record_gap(floor, first.timestamp_ms);
        self.history_trades += fresh.len();
        self.latest_trade_ms = Some(fresh[fresh.len() - 1].timestamp_ms);
        for trade in fresh {
            for pane in self.panes_mut() {
                pane.ingest_live_trade(trade);
            }
        }
        // The mark, and only the mark — the same seeding `Backfilled` does.
        self.paper.seed(&fresh[fresh.len() - 1]);
        true
    }

    /// Land the gap `QUANTICK_FEED_GAP` asked for, once the chart has bars for
    /// it to sit between.
    ///
    /// Placed at the open of the bar halfway through the series, so the seam is
    /// on screen at the zoom a capture opens on rather than off the left edge.
    /// It goes through [`Self::record_gap`], so a hooked run and a real
    /// reconnect draw the same mark from the same list.
    fn land_demo_gap(&mut self) {
        let Some(silence_ms) = self.pending_demo_gap_ms else {
            return;
        };
        let bars = self.flow_pane.state.bars();
        // Two bars at least: a seam is a boundary, and a boundary needs a bar
        // on each side of it.
        if bars.len() < 2 {
            return;
        }
        let to_ms = bars[bars.len() / 2].open_time;
        self.pending_demo_gap_ms = None;
        self.record_gap(to_ms - silence_ms, to_ms);
    }

    /// Record the silence a reconnect left, when it is long enough to be worth
    /// marking.
    ///
    /// Short silences are the recovery path working and marking them would be
    /// noise that teaches the trader to stop reading marks; see
    /// [`MIN_MARKED_GAP_MS`].
    fn record_gap(&mut self, from_ms: i64, to_ms: i64) {
        let gap = FeedGap { from_ms, to_ms };
        if gap.duration_ms() < MIN_MARKED_GAP_MS {
            return;
        }
        // Bounded: the newest gaps are the ones on screen, so the oldest is
        // what falls off.
        if self.feed_gaps.len() >= MAX_REMEMBERED_GAPS {
            self.feed_gaps.remove(0);
        }
        self.feed_gaps.push(gap);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_GAP_MARKED",
            feed = %self.feed_id,
            symbol = %self.symbol,
            from_ms = gap.from_ms,
            to_ms = gap.to_ms,
            duration_ms = gap.duration_ms(),
            "marked a stretch of market time no print covers"
        );
    }
}
