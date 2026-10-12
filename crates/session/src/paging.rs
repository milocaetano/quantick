//! Trade-history paging: the run a *load older* press starts, and the note
//! it leaves behind.
//!
//! The run itself — its target, a press queued behind the opening fill, the
//! campaign paging toward the target — is [`HistoryRun`]; this is the part
//! that carries out what it says against a tab: when a press may begin, what
//! one page asks for, when the panes hold their rebuild, and the one sentence
//! the trader is told when it is over.

use std::time::Instant;

use quantick_civil::fmt_weekday_minute;
use quantick_engine::Trade;
use quantick_sources::history_reach::{HistoryReach, ReachOutcome, TapeFacts};
use quantick_sources::history_run::{Cancelled, HistoryRun, Poll, Press, RunAction, RunStatus};

use crate::{
    Delivery, Effect, FeedRequest, HISTORY_NOTE_LINGER, HistoryFrame, Refusal, Resume,
    SessionEvent, Step, Wait, WaitChange,
};

/// What the trader is told when a rebuild of older pages fails.
pub const REBUILD_FAILED_NOTE: &str = "History could not be built. Press History to retry.";

/// What the last *load older* press had to say, and when it said it.
///
/// Raised only when the press left the chart where it was, or stopped short of
/// the reach it promised. A press that landed what it promised raises nothing:
/// the bars are the acknowledgement.
#[derive(Debug, Clone)]
struct Note {
    /// Written by [`ReachOutcome::sentence`], so the outcome and the run that
    /// produced it cannot drift into two accounts of the same press.
    text: String,
    raised_at: Instant,
}

/// One tab's trade-history paging.
#[derive(Debug, Default)]
pub(crate) struct TradePaging {
    /// The run: last target, a queued press, the campaign.
    run: HistoryRun,
    note: Option<Note>,
    /// What the window told this tab about its History button, last frame.
    pub(crate) frame: HistoryFrame,
    /// What the last press did, for a caller that has to say so.
    pub(crate) last_press: Option<Press>,
    /// What the last cancel stopped.
    pub(crate) last_cancel: Option<Cancelled>,
}

impl TradePaging {
    pub(crate) fn main_reach(&self) -> HistoryReach {
        self.run.main_reach(self.frame.default_reach)
    }

    pub(crate) fn status(&self) -> RunStatus {
        self.run.status()
    }

    pub(crate) const fn holds_pages(&self) -> bool {
        self.run.holds_pages()
    }

    pub(crate) const fn awaiting_reply(&self) -> bool {
        self.run.awaiting_reply()
    }

    pub(crate) fn tape_copies(&self) -> Option<usize> {
        self.run.tape_copies()
    }

    pub(crate) fn note(&self) -> Option<&str> {
        self.note.as_ref().map(|note| note.text.as_str())
    }

    /// A pane's copy of the tape counts against a running campaign's memory
    /// ceiling.
    pub(crate) fn count_copies(&mut self, copies: usize) {
        self.run.set_tape_copies(copies);
    }

    /// The first half of a press: retry every failed rebuild, keeping every
    /// page it held, then judge the press against the tab that leaves.
    pub(crate) fn prepare_press(&mut self, step: &mut Step<'_>, reach: HistoryReach) {
        step.effects.push(Effect::RetryRebuilds);
        // A new press owns its own outcome.
        self.clear_note(step);
        step.effects.push(Effect::Resume(Resume::Press(reach)));
    }

    /// Load back to `reach`: begin now, or queue behind the opening fill.
    pub(crate) fn press(
        &mut self,
        step: &mut Step<'_>,
        reach: HistoryReach,
    ) -> Result<(), Refusal> {
        let press = self.run.press(reach, step.tab.history_idle());
        tracing::info!(target: "quantick::app", event_code = "HISTORY_REACH_PRESSED", symbol = %step.tab.symbol,
            reach = %reach.token(), press = ?press, "a history target was pressed");
        self.last_press = Some(press);
        step.event(SessionEvent::Pressed { reach, press });
        match press {
            Press::Start => self.begin(step, reach),
            Press::Queued => {}
            Press::AlreadyRunning => return Err(Refusal::AlreadyRunning),
        }
        Ok(())
    }

    /// Stop the run and keep what arrived; the note says where it stopped.
    pub(crate) fn cancel(&mut self, step: &mut Step<'_>) {
        let cancelled = self.run.cancel();
        if let Cancelled::Run(outcome) = cancelled {
            self.finish(step, outcome);
        }
        step.hold(self.run.holds_pages());
        self.last_cancel = Some(cancelled);
        step.event(SessionEvent::Cancelled(cancelled));
    }

    fn begin(&mut self, step: &mut Step<'_>, reach: HistoryReach) {
        // Every pane holds its own copy of the tape, and a silence the feed
        // marked as lost is not a session close.
        let facts = TapeFacts {
            copies: step.tab.panes.len(),
            outages: step.tab.outages.clone(),
        };
        let action = self.run.begin(
            reach,
            step.tab.tape,
            &facts,
            step.tab.reach_bounds,
            step.tab.page_size(),
        );
        self.carry_out(step, action);
    }

    /// Do what the run said. A page request is only out once the feed took
    /// it, so the panes' hold follows its delivery ([`Self::acknowledge`]).
    fn carry_out(&mut self, step: &mut Step<'_>, action: RunAction) {
        match action {
            RunAction::Send(count) => {
                step.feed(FeedRequest::OlderTrades { count });
                return;
            }
            RunAction::Wait => {}
            RunAction::Finished(outcome) => self.finish(step, outcome),
        }
        step.hold(self.run.holds_pages());
    }

    /// What became of one `load_older`: its wait goes up, or the run learns
    /// the page never left.
    pub(crate) fn acknowledge(&mut self, step: &mut Step<'_>, delivery: Delivery) {
        if delivery == Delivery::Sent {
            step.wait(Wait::OlderTrades, WaitChange::Begin);
        } else {
            tracing::warn!(target: "quantick::app", ?delivery, "older-history request not queued");
            if let Some(outcome) = self.run.send_failed() {
                self.finish(step, outcome);
            }
        }
        step.hold(self.run.holds_pages());
    }

    /// Once a frame: begin a queued press when the chart is idle, resume a
    /// run paused while its tab was hidden.
    pub(crate) fn poll(&mut self, step: &mut Step<'_>) {
        match self.run.poll(step.tab.history_idle(), self.frame.visible) {
            Poll::Begin(reach) => self.begin(step, reach),
            Poll::Send(count) => self.carry_out(step, RunAction::Send(count)),
            Poll::Nothing => {}
        }
    }

    /// A reply landed: judge its raw page before any rebuild, so the next
    /// request goes out while the chart is still being rebuilt.
    pub(crate) fn judge_page(&mut self, step: &mut Step<'_>, page: &[Trade]) {
        let action = self.run.on_reply(
            page,
            step.tab.capabilities.history_paging,
            self.frame.visible,
        );
        self.carry_out(step, action);
    }

    fn finish(&mut self, step: &mut Step<'_>, outcome: ReachOutcome) {
        tracing::info!(target: "quantick::app", schema_version = 1_u8, event_code = "HISTORY_REACH_SETTLED",
            symbol = %step.tab.symbol, reach = %outcome.reach.token(),
            reached_ms = outcome.oldest_ms.unwrap_or(0), sessions = outcome.sessions_reached,
            complete = outcome.complete(), action = outcome.end.action(), "a history run finished");
        step.event(SessionEvent::RunSettled(outcome));
        let tz = self.frame.tz;
        self.raise_note(step, outcome.sentence(|ms| fmt_weekday_minute(ms, tz)));
    }

    /// A rebuild failed: end the run where it stood. Unlike a change of
    /// market, the request already out is still owed a reply, so it stays
    /// counted until that reply lands; a press queued behind the rebuild stays
    /// queued, and the press that retries the rebuild runs it.
    pub(crate) fn rebuild_failed(&mut self, step: &mut Step<'_>) {
        if self.note() == Some(REBUILD_FAILED_NOTE) {
            return;
        }
        self.run.stop_keeping_queued();
        step.hold(self.run.holds_pages());
        self.raise_note(step, REBUILD_FAILED_NOTE.to_owned());
    }

    /// Drop the run and the verdict it produced: both belong to a tape this
    /// tab no longer shows.
    pub(crate) fn abandon(&mut self, step: &mut Step<'_>) {
        self.run.reset();
        self.clear_note(step);
        step.hold(self.run.holds_pages());
    }

    /// Put one sentence about the last press where the trader is looking.
    pub(crate) fn raise_note(&mut self, step: &mut Step<'_>, text: String) {
        step.event(SessionEvent::NoteRaised(text.clone()));
        self.note = Some(Note {
            text,
            raised_at: step.now,
        });
    }

    /// Drop the note once it has had its [`HISTORY_NOTE_LINGER`]; a failed
    /// rebuild's note stays until the press that retries it.
    ///
    /// Rate: **per-frame**, and trivial while there is no note.
    pub(crate) fn expire_note(&mut self, step: &mut Step<'_>) {
        if step.tab.panes.iter().any(|pane| pane.rebuild_failed) {
            return;
        }
        if self.note.as_ref().is_some_and(|note| {
            step.now.saturating_duration_since(note.raised_at) >= HISTORY_NOTE_LINGER
        }) {
            self.clear_note(step);
        }
    }

    fn clear_note(&mut self, step: &mut Step<'_>) {
        if self.note.take().is_some() {
            step.event(SessionEvent::NoteCleared);
        }
    }
}
