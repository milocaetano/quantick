//! Replay transport, history back-fill, alarms, and the harness hooks that
//! script all three.
//!
//! One file because most of them share a shape rather than a subject: an
//! *applier* run once per frame from [`super::QuantickApp::draw_frame`], which
//! reads at most one harness hook and either does its one thing or returns.
//! The scripted pointer helpers are here for the same reason — they are what
//! lets a capture drive the transport without a mouse.
//!
//! Two are not appliers: `duplicate_selected_drawing` and its
//! `carry_strategy_to_duplicate` half are reached from
//! [`super::drawing_input`] on a gesture, and sit here because
//! `DUPLICATE_OFFSET_BARS` and the alarm the copy can inherit came with this
//! group. Moving them beside the rest of the drawing chrome is a follow-up,
//! not this cut's business.

use eframe::egui;

use crate::drawings;
use crate::harness::ContextMenuPane;
use crate::loading::LoadingTask;
use crate::pane;
use crate::replay_view::ReplayAction;

use quantick_feed::{FeedCommand, ReplayControl};

use super::QuantickApp;

/// Horizontal offset of a duplicated drawing, so the copy is visibly a copy.
pub(super) const DUPLICATE_OFFSET_BARS: f32 = 2.0;

impl QuantickApp {
    /// Carry out what the replay interface asked for.
    /// Whether the action reached its destination. Only a transport control
    /// can fail to — see the drop below — and the one caller that gets a
    /// single shot at it (the scripted seek) reads this before spending it.
    pub(super) fn apply_replay_action(&mut self, action: ReplayAction) -> bool {
        match action {
            ReplayAction::Open(request) => {
                let (tab, config) = self.active_with_config();
                tab.open_replay(config, *request);
                true
            }
            ReplayAction::Close => {
                let (tab, config) = self.active_with_config();
                tab.close_replay(config);
                true
            }
            ReplayAction::Control(control) => {
                // A dropped transport click is not worth a retry queue: the
                // worker drains commands every 8 ms, so a full channel means
                // the click was already superseded.
                if let Err(e) = self
                    .active_tab()
                    .commands
                    .try_send(FeedCommand::Replay(control))
                {
                    tracing::debug!(
                        target: "quantick::app",
                        event_code = "REPLAY_COMMAND_DROPPED",
                        reason = %e,
                        "transport command not queued"
                    );
                    return false;
                }
                true
            }
        }
    }

    /// Where a scripted right-click should land to reach `pane`'s menu.
    ///
    /// Mid-height, and mid-pane horizontally, off the geometry the draw
    /// published — so the click lands on the canvas rather than on the axis,
    /// the legend or the divider handle. `None` until the pane has drawn once
    /// (no divider yet), and `None` for the tape on a canvas that has none:
    /// there is no tape menu to open where there is no tape.
    pub(super) fn scripted_context_menu_pos(&self, pane: ContextMenuPane) -> Option<egui::Pos2> {
        let flow = &self.active_tab().flow_pane;
        // The axis's menu lives on the gutter, off the canvas entirely — the
        // draw publishes that band the same way it publishes the divider.
        if pane == ContextMenuPane::Axis {
            return Some(flow.frame.price_gutter?.center());
        }
        // The time axis, likewise off the canvas — and its own published band,
        // because the segment past the lane divider is the tape's.
        if pane == ContextMenuPane::Time {
            return Some(flow.frame.time_strip?.center());
        }
        let rect = flow.frame.chart_rect?;
        let divider = flow.frame.lane_divider_x;
        let x = match (pane, divider) {
            (ContextMenuPane::Tape, Some(divider)) => (divider + rect.right()) / 2.0,
            (ContextMenuPane::Tape, None) => return None,
            // Axis and Time returned above; anything else is the candles'
            // canvas.
            (_, Some(divider)) => (rect.left() + divider) / 2.0,
            (_, None) => rect.center().x,
        };
        Some(egui::pos2(x, rect.center().y))
    }

    /// Where `QUANTICK_POINTER` puts the mouse this frame, in window points.
    ///
    /// Resolved against the *drawing* area rather than the whole chart, so a
    /// fraction means a place among the candles whatever share of the canvas
    /// the live lane has taken, and against the flow pane for the same reason
    /// [`Self::scripted_context_menu_pos`] does — one canvas per capture.
    /// `None` until the pane has drawn once: there is no candle area to be a
    /// fraction of before then, and guessing one would park the pointer
    /// somewhere the author did not ask for.
    pub(super) fn scripted_pointer_pos(&self) -> Option<egui::Pos2> {
        let fraction = self.harness.pointer()?;
        let flow = &self.active_tab().flow_pane;
        let candles = flow.drawing_area(flow.frame.chart_rect?);
        Some(egui::pos2(
            candles.left() + fraction.x * candles.width(),
            candles.top() + fraction.y * candles.height(),
        ))
    }

    /// Deliver the parked pointer, every frame it is parked.
    pub(super) fn push_scripted_pointer(&self, raw_input: &mut egui::RawInput) {
        if let Some(position) = self.scripted_pointer_pos() {
            raw_input.events.push(egui::Event::PointerMoved(position));
        }
    }

    /// The scripted view hooks (`QUANTICK_CANDLE_WIDTH`, `QUANTICK_PAN_PX`),
    /// re-applied every frame.
    ///
    /// Every frame rather than once at boot, for two reasons. A pan needs bars
    /// to move over and at boot there are none — repeating it is what makes
    /// `QUANTICK_PAN_PX=-9000` mean "as far left as it goes" whatever the
    /// zoom: each frame pushes, the per-frame clamp holds, and the view settles
    /// on the projection margin.
    ///
    /// And the view is *rebuilt* under both hooks by anything that re-cuts the
    /// series: `ChartPane::reset_series` hands back a fresh `Viewport`, which a
    /// replay autostart does before its first frame. A zoom set at boot was
    /// therefore thrown away, and every scripted capture of a recorded session
    /// photographed the default zoom rather than the one it asked for.
    ///
    /// A run with neither variable set does nothing here.
    pub(super) fn apply_scripted_view(&mut self) {
        let (width, pan) = self.harness.scripted_view();
        if width.is_none() && pan.is_none() {
            return;
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        if let Some(px) = width {
            pane.viewport.set_px_per_bar(px);
        }
        let slots = pane.slots();
        if let Some(dx) = pan
            && slots > 0
        {
            pane.viewport.pan_pixels(dx, slots);
        }
    }

    /// The `QUANTICK_LOAD_OLDER` hook: press "+ older" this many times, once
    /// the chart has something to page back from.
    ///
    /// Goes through [`crate::tab::Tab::request_older_history`] — the very function the
    /// toolbar button calls — rather than reaching for the feed command itself,
    /// so a run under this hook exercises the trader's path including its
    /// loading indicator, and cannot drift from it.
    ///
    /// One page per frame at most: the pages are answered asynchronously and
    /// the feed serves one request at a time, so firing them together would
    /// have every page after the first refused and answered empty — a capture
    /// of the drop path rather than of the feature.
    pub(super) fn apply_load_older(&mut self) {
        let Some(pages) = self.harness.load_older_pages() else {
            return;
        };
        if self.active_tab().flow_pane.slots() == 0 {
            // Nothing charted yet. Wait, but not forever.
            if self.harness.spend_load_older_frame().gave_up {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_AUTOSTART_GAVE_UP",
                    pages,
                    frames_waited = crate::harness::LOAD_OLDER_HOOK_FRAMES,
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER found no bars to page back from"
                );
            }
            return;
        }
        if self.active_tab().loading.is_active(LoadingTask::History) {
            // The previous page is still coming. Asking now would be refused
            // and answered empty, which is not what the hook is for.
            return;
        }
        let (tab, config) = self.active_with_config();
        tab.request_older_history(config);
        self.harness.load_older_page_sent();
    }

    /// The `QUANTICK_HISTORY_NOTE` hook: the sentence a settled reach leaves,
    /// held up over a chart for as long as the hook's budget lasts.
    ///
    /// Re-applied every frame rather than raised once, the way
    /// `QUANTICK_PAN_PX` re-applies its drag — and for a reason a one-shot
    /// could not survive. Switching source clears the note along with the run
    /// that raised it, exactly as it should: a new market has nothing to say
    /// about the last one's press. But a launch under `QUANTICK_REPLAY_AUTOSTART`
    /// *is* a source switch, arriving a second after the first bars, so a note
    /// raised once was swept away before any shutter could open on it.
    ///
    /// Holding it re-raises only while it is absent, so the surface itself is
    /// unchanged — the same sentence, from the same call, in the same lane.
    /// When the budget runs out the note keeps its ordinary
    /// [`crate::tab::HISTORY_NOTE_LINGER`] from the last raise and then leaves
    /// on its own, so even a hooked run photographs a note that expires.
    pub(super) fn apply_history_note_hook(&mut self) {
        let Some(end) = self.harness.history_note_ending() else {
            return;
        };
        if self.harness.spend_history_note_frame().gave_up {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_NOTE_HOOK_RELEASED",
                ending = end.action(),
                frames_held = crate::harness::HISTORY_NOTE_HOOK_FRAMES,
                action = "note_left_to_expire",
                "QUANTICK_HISTORY_NOTE let go of its sentence"
            );
            return;
        }
        let tab = self.active_tab();
        // Nothing charted yet, or the note is already up: nothing to raise.
        //
        // And never while a request is out. Paired with `QUANTICK_LOAD_OLDER`
        // — which the harness table pairs it with — the press clears the note
        // and sends a real `load_older`, and re-raising here would paint a
        // settled verdict over a request still in flight, with the spinner
        // turning above it. That is the dishonesty this branch removes, and a
        // hook has no business manufacturing it for a capture.
        if tab.flow_pane.slots() == 0
            || tab.history_note().is_some()
            || tab.loading.is_active(LoadingTask::History)
        {
            return;
        }
        // Always `Some`: the hook only ever holds an ending the env read above
        // kept, and it keeps only endings that have words.
        let Some(notice) = end.notice() else {
            return;
        };
        self.active_tab_mut().raise_history_note(notice);
    }

    /// The `QUANTICK_LOAD_OLDER_CANDLES` hook: the history menu's "+ older
    /// candles" entry, pressed without a hand, once per frame at most.
    ///
    /// Same shape and same reasons as [`Self::apply_load_older`], against a
    /// different record: it goes through `Tab::request_older_ohlcv_history`
    /// rather than the feed command, so a run under this hook exercises the
    /// trader's own path; it waits, because there is nothing to reach back
    /// *from* until the opening request has landed; and it gives up rather
    /// than hanging a capture on a venue that never answers.
    pub(super) fn apply_load_older_candles(&mut self) {
        let Some(spans) = self.harness.load_older_candle_spans() else {
            return;
        };
        let capabilities = self.active_tab().capabilities(&self.config);
        // Waiting costs budget, but a *slower* budget. A span really being
        // fetched is the feature working, and charging it at the same rate as
        // an empty chart would give up around the fourth of the documented
        // thirteen spans. Charging it nothing, though, is how a venue that
        // simply never answers hangs a capture run for the life of the
        // process — which is the exact failure this counter exists to bound,
        // and what the doc above promises it does. So a fetching frame spends
        // one tick of a budget scaled to how long fetching legitimately takes.
        if self
            .active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory)
        {
            if self.harness.spend_load_older_candles_frame().gave_up {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = crate::harness::LOAD_OLDER_CANDLES_HOOK_FRAMES,
                    reason = "venue_never_answered",
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER_CANDLES gave up waiting for a span to arrive"
                );
            }
            return;
        }
        if !self.active_tab().can_load_older_candles(capabilities) {
            // Nothing to reach back *from* yet, or the venue's record starts
            // here. Both are worth waiting a bounded while for, and both end
            // the same way; the log names what the tab held so an operator can
            // tell them apart.
            if self.harness.spend_load_older_candles_frame().gave_up {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = crate::harness::LOAD_OLDER_CANDLES_HOOK_FRAMES,
                    candles_held = self.active_tab().venue_candles_held(),
                    ohlcv_history = capabilities.ohlcv_history,
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER_CANDLES found nothing to reach back from"
                );
            }
            return;
        }
        // Only a request that actually went out costs a *span*. A full command
        // channel is a busy frame, not a span delivered, and counting it as one
        // would quietly shorten the reach the operator asked for — but it still
        // costs a frame of budget, or a permanently saturated channel leaves
        // the hook armed for the life of the process with nothing ever logged.
        if self
            .active_tab_mut()
            .request_older_ohlcv_history(capabilities)
        {
            self.harness.load_older_candles_span_sent();
        } else if self.harness.spend_load_older_candles_frame().gave_up {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                spans,
                frames_waited = crate::harness::LOAD_OLDER_CANDLES_HOOK_FRAMES,
                reason = "request_never_queued",
                action = "chart_left_as_it_is",
                "QUANTICK_LOAD_OLDER_CANDLES could not get a request out"
            );
        }
    }

    /// Play the alarm cues every tab's armed instances asked for this
    /// frame, and empty their queues.
    ///
    /// Every tab, not only the active one: a tab the trader is not looking
    /// at keeps its feed running and its instances judging, and an alarm
    /// exists precisely to be heard when the eyes are elsewhere.
    ///
    /// One cue per *distinct* cue per frame, across every tab. The kernel's
    /// repeat rule has already thinned each instance's stream to one per
    /// bar (or one per cooldown); this is the second, blunter guard, for
    /// the frame that ingested a burst of prints and closed several bars at
    /// once — four identical beeps stacked into one instant are one noise,
    /// not four alarms.
    ///
    /// Deduplicating by *cue* rather than collapsing to one is the whole
    /// point of letting a preset choose a sound: a trader who gave two
    /// regions two sounds did it to tell them apart, and swallowing the
    /// second because it shared a frame with the first would hide a signal
    /// and leave no trace that it had. The frame's cues go to the sink as
    /// one batch, which plays them in order — so the second is heard after
    /// the first rather than instead of it. The set of sounds is small and
    /// fixed, so this is bounded by the catalogue however busy the tape.
    ///
    /// Per frame, but cheap: the walk is over a handful of tabs whose
    /// queues are empty on every frame but the one a signal happened on,
    /// and the sink is only asked when something is queued.
    pub(super) fn play_pending_alarms(&mut self) {
        // Order of first request, duplicates dropped — `dedup` alone would
        // only collapse neighbours.
        let mut distinct: Vec<crate::audio::Cue> = Vec::new();
        for tab in &mut self.tabs {
            for cue in tab.pending_alarm_sounds.drain(..) {
                if !distinct.contains(&cue) {
                    distinct.push(cue);
                }
            }
        }
        if distinct.is_empty() {
            return;
        }
        let outcome = self.alerts.play(&distinct);
        self.report_alert_attempt(outcome);
    }

    /// Record whether a sound actually reached the trader.
    ///
    /// A notification that never arrived is reported, never assumed — so a
    /// first failure raises a toast rather than waiting for the trader to
    /// reopen the arming dialog, which they may never do. Only the *first*
    /// of a run: a build with no audio backend fails on every alarm, and a
    /// toast per bar would be its own noise. A success clears the reason, so
    /// one transient refusal does not leave a permanent red line behind it.
    pub(super) fn report_alert_attempt(&mut self, outcome: Result<(), &'static str>) {
        match outcome {
            Ok(()) => self.alert_failure = None,
            Err(reason) => {
                let first = self.alert_failure.as_deref() != Some(reason);
                self.alert_failure = Some(reason.to_owned());
                if first {
                    self.show_agent_toast(format!("no alarm sound was played: {reason}"));
                }
            }
        }
    }

    /// Arm one instance on a drawing: compile the form, warm the trigger on
    /// the bars already closed (gates shut, so nothing fires from history),
    /// attach it, and start the paper host listening. `Err` carries the
    /// human-readable refusal for the dialog to show.
    /// Duplicate the selected drawing with everything riding it.
    ///
    /// The one door, because a duplication is not only a copied mark: an
    /// armed strategy rides the drawing today and whatever docks next will
    /// ride it too. Two call sites — the hotkey and the context bar — each
    /// spelling out "copy, then carry" is a third one that copies and
    /// forgets, and a band that silently loses its bot is exactly the class
    /// of silence this change exists to end.
    ///
    /// Rate: rare — one keystroke or one click.
    pub(super) fn duplicate_selected_drawing(&mut self) {
        let side = self.active_tab().drawing_side();
        let Some(duplicated) = self
            .drawing_pane_mut()
            .drawings
            .duplicate_selected(DUPLICATE_OFFSET_BARS)
        else {
            return;
        };
        self.carry_strategy_to_duplicate(side, duplicated);
    }

    /// Carry an armed strategy across a duplication.
    ///
    /// A copied region is a region the trader wants watched the same way —
    /// duplicating the band and then re-typing the form is a step that only
    /// exists because the copy forgot. The copy is armed through
    /// [`Self::arm_strategy_instance`], the same door the dialog uses, from
    /// the stored form the source kept: one construction path, so a copy
    /// cannot quietly differ from what the dialog would have built.
    ///
    /// **Only a watching instance travels.** A source that is `Done`, or
    /// disarmed for any reason, was stopped — by the trader's own hand, by
    /// a rejected entry, by a spent one shot — and a copy that springs back
    /// to life places orders the trader last said no to. The copy lands
    /// offset to the *right*, the direction that makes a dead span live
    /// again, so Ctrl+D would otherwise be the one gesture that silently
    /// revives what was deliberately stopped.
    ///
    /// **State does not travel either.** The copy starts `Armed`, with a
    /// fresh ruler warmed on this pane's own bars, a fresh alarm (no
    /// inherited cooldown, no inherited preview mark) and no order id.
    /// Cloning a `Fired` instance would hang a second badge on one order.
    ///
    /// A refusal is reported rather than swallowed: `duplicate_selected`
    /// clones `hidden`, `off_series` and `foreign_market` verbatim — only
    /// `locked` is reset — and arming refuses all three. A trader who
    /// pressed hide-all and then Ctrl+D would otherwise unhide to two
    /// identical bands wearing one badge and believe both were watching.
    ///
    /// Rate: rare — one keystroke.
    fn carry_strategy_to_duplicate(
        &mut self,
        side: pane::PaneSide,
        duplicated: drawings::Duplicated,
    ) {
        use quantick_strategy::ArmedState;
        let Some((spec, label)) = self
            .active_tab()
            .pane(side)
            .strategies
            .for_drawing(duplicated.source)
            .filter(|instance| {
                matches!(
                    instance.armed.state(),
                    ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition
                )
            })
            .map(|instance| (instance.spec.clone(), instance.preset.clone()))
        else {
            return;
        };
        if let Err(reason) = self.arm_strategy_instance(side, duplicated.copy, &spec, label) {
            self.note_workspace(format!("the copy carries no strategy: {reason}"));
        }
    }

    /// The `QUANTICK_REPLAY_RESTART_AFTER` hook: press the transport's own
    /// Restart once the session has closed that many round trips.
    ///
    /// The seek is the only way to put a closed trade ahead of the tape the
    /// chart holds — the recording starts over, the round trips stay in the
    /// ledger because they happened, and their fills are now at instants no
    /// bar on screen covers. That is the state the marks used to stack on
    /// the edge bar in, and it takes a click on a transport button a
    /// scripted capture cannot make. Nothing happens without a recording
    /// playing: there is no timeline to seek on a live feed.
    ///
    /// Consumed once, whether or not the trades ever arrived — an env var
    /// is a request for this run, not a standing rule.
    pub(super) fn apply_replay_restart(&mut self) {
        let Some(after) = self.harness.replay_restart_after() else {
            return;
        };
        let tab = self.active_tab();
        if tab.replay.is_none() || tab.paper.session_trades().len() < after {
            return;
        }
        // Spent only once the transport took it. A hook that cleared itself
        // on a dropped command would leave the capture photographing an
        // un-seeked timeline while the harness believed otherwise; the next
        // frame simply tries again.
        if self.apply_replay_action(ReplayAction::Control(ReplayControl::Restart)) {
            self.harness.replay_restart_taken();
        }
    }
}
