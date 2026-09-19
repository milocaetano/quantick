//! Replay transport, history back-fill, alarms, and the harness hooks that
//! script all three.
//!
//! Two owners share the file because they share a frame phase, not a
//! subject. [`AlertState`] is the alarm sink and the one rule about it: a
//! sound that did not reach the trader is reported, never assumed. The
//! [`Harness`] methods are the per-frame *appliers* the frame runs once
//! each, reading at most one hook and either doing its one thing against the
//! active tab or returning. Neither sees the window: the frame hands them the
//! tabs and the config and posts whatever they hand back.
#[cfg(any(feature = "scenario-harness", test))]
use eframe::egui;

use crate::config::AppConfig;
#[cfg(any(feature = "scenario-harness", test))]
use crate::harness::{ContextMenuPane, Harness};
#[cfg(any(feature = "scenario-harness", test))]
use crate::loading::LoadingTask;
#[cfg(any(feature = "scenario-harness", test))]
use crate::pane::ChartPane;
use crate::replay_view::ReplayAction;
use crate::tab::Tab;

use quantick_feed::FeedCommand;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_feed::ReplayControl;

use super::arrangement_host::ArrangementHost;

/// Where a signal alarm is played, and why the last one was not.
///
/// The two travel together because a sink that refused is only ever reported
/// through the failure beside it: an alarm the trader never heard is never
/// assumed heard.
pub(super) struct AlertState {
    /// Where signal alarms are played. The shipped sink is the platform's
    /// own sounds; a test swaps in a recorder, which is how "the alarm
    /// sounded, once, and it was the sound the preset named" is asserted
    /// without a build machine making noise.
    pub(super) alerts: Box<dyn crate::audio::AlertSink>,

    /// The last reason a sound could not be played, shown once in the
    /// dialog. A build with no audio backend, or a platform that refused,
    /// is reported: an alarm the trader never heard is never assumed heard.
    pub(super) alert_failure: Option<String>,
}

impl AlertState {
    /// Play `cues` and record whether they reached the trader. The text to
    /// post on the window's acknowledgement lane, on the first refusal of a
    /// run only.
    pub(crate) fn play(&mut self, cues: &[crate::audio::Cue]) -> Option<String> {
        let outcome = self.alerts.play(cues);
        self.report(outcome)
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
    pub(crate) fn play_pending(&mut self, tabs: &mut ArrangementHost) -> Option<String> {
        // Order of first request, duplicates dropped — `dedup` alone would
        // only collapse neighbours.
        let mut distinct: Vec<crate::audio::Cue> = Vec::new();
        for tab in tabs.iter_mut() {
            for cue in tab.pending_alarm_sounds.drain(..) {
                if !distinct.contains(&cue) {
                    distinct.push(cue);
                }
            }
        }
        if distinct.is_empty() {
            return None;
        }
        self.play(&distinct)
    }

    /// Record whether a sound actually reached the trader.
    ///
    /// A notification that never arrived is reported, never assumed — so a
    /// first failure raises a toast rather than waiting for the trader to
    /// reopen the arming dialog, which they may never do. Only the *first*
    /// of a run: a build with no audio backend fails on every alarm, and a
    /// toast per bar would be its own noise. A success clears the reason, so
    /// one transient refusal does not leave a permanent red line behind it.
    fn report(&mut self, outcome: Result<(), &'static str>) -> Option<String> {
        match outcome {
            Ok(()) => {
                self.alert_failure = None;
                None
            }
            Err(reason) => {
                let first = self.alert_failure.as_deref() != Some(reason);
                self.alert_failure = Some(reason.to_owned());
                first.then(|| format!("no alarm sound was played: {reason}"))
            }
        }
    }
}

/// Carry out what the replay interface asked for, on the tab it addresses.
/// Whether the action reached its destination. Only a transport control
/// can fail to — see the drop below — and the one caller that gets a
/// single shot at it (the scripted seek) reads this before spending it.
pub(super) fn apply_replay_action(tab: &mut Tab, config: &AppConfig, action: ReplayAction) -> bool {
    match action {
        ReplayAction::Open(request) => {
            tab.open_replay(config, *request);
            true
        }
        ReplayAction::Close => {
            tab.close_replay(config);
            true
        }
        ReplayAction::Control(control) => {
            // A dropped transport click is not worth a retry queue: the
            // worker drains commands every 8 ms, so a full channel means
            // the click was already superseded.
            if let Err(e) = tab.commands.try_send(FeedCommand::Replay(control)) {
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

#[cfg(any(feature = "scenario-harness", test))]
impl ContextMenuPane {
    /// Where a scripted right-click should land to reach this pane's menu on
    /// `flow`, the active tab's flow pane.
    ///
    /// Uses geometry published by the draw; `None` until that geometry exists.
    pub(crate) fn scripted_position(self, flow: &ChartPane) -> Option<egui::Pos2> {
        if self == ContextMenuPane::Axis {
            return Some(flow.frame.price_gutter?.center());
        }
        if self == ContextMenuPane::Time {
            return Some(flow.frame.time_strip?.center());
        }
        if self == ContextMenuPane::Indicator {
            return flow.first_indicator_pane_center();
        }
        crate::harness::context_menu_canvas_position(
            self,
            flow.frame.chart_rect?,
            flow.frame.lane_divider_x,
        )
    }
}

#[cfg(any(feature = "scenario-harness", test))]
fn active_with_id(tabs: &mut ArrangementHost) -> (u64, &mut Tab) {
    let index = tabs.active_index();
    (tabs.id_at(index), tabs.runtime_mut(index))
}

#[cfg(any(feature = "scenario-harness", test))]
impl Harness {
    /// Where `QUANTICK_POINTER` puts the mouse this frame, in window points.
    ///
    /// Resolved against the *drawing* area rather than the whole chart, so a
    /// fraction means a place among the candles whatever share of the canvas
    /// the live lane has taken, and against `flow` — the active tab's flow
    /// pane — for the same reason [`ContextMenuPane::scripted_position`]
    /// does: one canvas per capture. `None` until the pane has drawn once:
    /// there is no candle area to be a fraction of before then, and guessing
    /// one would park the pointer somewhere the author did not ask for.
    pub(crate) fn scripted_pointer_pos(&self, flow: &ChartPane) -> Option<egui::Pos2> {
        let fraction = self.pointer()?;
        let candles = crate::bands::drawing_area(flow.frame.chart_rect?, flow.frame.lane_divider_x);
        Some(egui::pos2(
            candles.left() + fraction.x * candles.width(),
            candles.top() + fraction.y * candles.height(),
        ))
    }

    /// Deliver the parked pointer, every frame it is parked.
    pub(crate) fn push_scripted_pointer(&self, flow: &ChartPane, raw_input: &mut egui::RawInput) {
        if let Some(position) = self.scripted_pointer_pos(flow) {
            raw_input.events.push(egui::Event::PointerMoved(position));
        }
    }

    /// The scripted view hooks (`QUANTICK_CANDLE_WIDTH`, `QUANTICK_PAN_PX`),
    /// re-applied every frame to the active tab's flow pane.
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
    pub(crate) fn apply_scripted_view(&self, tabs: &mut ArrangementHost) {
        let (width, pan) = self.scripted_view();
        if width.is_none() && pan.is_none() {
            return;
        }
        let pane = &mut active_with_id(tabs).1.flow_pane;
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
    pub(crate) fn apply_load_older(&mut self, tabs: &mut ArrangementHost, config: &AppConfig) {
        let Some(pages) = self.load_older_pages() else {
            return;
        };
        let (tab_id, tab) = active_with_id(tabs);
        if tab.flow_pane.slots() == 0 {
            // Nothing charted yet. Wait, but not forever.
            if self.spend_load_older_frame().gave_up {
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
        if tab.loading.is_active(LoadingTask::History) {
            // The previous page is still coming. Asking now would be refused
            // and answered empty, which is not what the hook is for.
            return;
        }
        tab.request_older_history(tab_id, config);
        self.load_older_page_sent();
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
    pub(crate) fn apply_history_note_hook(&mut self, tabs: &mut ArrangementHost) {
        let Some(end) = self.history_note_ending() else {
            return;
        };
        if self.spend_history_note_frame().gave_up {
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
        let (_, tab) = active_with_id(tabs);
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
        tab.raise_history_note(notice);
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
    pub(crate) fn apply_load_older_candles(
        &mut self,
        tabs: &mut ArrangementHost,
        config: &AppConfig,
    ) {
        let Some(spans) = self.load_older_candle_spans() else {
            return;
        };
        let (tab_id, tab) = active_with_id(tabs);
        let capabilities = tab.capabilities(config);
        // Waiting costs budget, but a *slower* budget. A span really being
        // fetched is the feature working, and charging it at the same rate as
        // an empty chart would give up around the fourth of the documented
        // thirteen spans. Charging it nothing, though, is how a venue that
        // simply never answers hangs a capture run for the life of the
        // process — which is the exact failure this counter exists to bound,
        // and what the doc above promises it does. So a fetching frame spends
        // one tick of a budget scaled to how long fetching legitimately takes.
        if tab.loading.is_active(LoadingTask::VenueHistory) {
            if self.spend_load_older_candles_frame().gave_up {
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
        if !tab.can_load_older_candles(capabilities) {
            // Nothing to reach back *from* yet, or the venue's record starts
            // here. Both are worth waiting a bounded while for, and both end
            // the same way; the log names what the tab held so an operator can
            // tell them apart.
            if self.spend_load_older_candles_frame().gave_up {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = crate::harness::LOAD_OLDER_CANDLES_HOOK_FRAMES,
                    candles_held = tab.venue_candles_held(),
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
        if tab.request_older_ohlcv_history(tab_id, capabilities) {
            self.load_older_candles_span_sent();
        } else if self.spend_load_older_candles_frame().gave_up {
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
    pub(crate) fn apply_replay_restart(&mut self, tabs: &mut ArrangementHost, config: &AppConfig) {
        let Some(after) = self.replay_restart_after() else {
            return;
        };
        let (_, tab) = active_with_id(tabs);
        if tab.replay.is_none() || tab.paper.session_trades().len() < after {
            return;
        }
        // Spent only once the transport took it. A hook that cleared itself
        // on a dropped command would leave the capture photographing an
        // un-seeked timeline while the harness believed otherwise; the next
        // frame simply tries again.
        if apply_replay_action(tab, config, ReplayAction::Control(ReplayControl::Restart)) {
            self.replay_restart_taken();
        }
    }
}
