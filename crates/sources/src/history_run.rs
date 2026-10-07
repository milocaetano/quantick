//! One tab's history run: what its main click repeats, a press queued behind
//! the opening fill, the campaign paging toward a target, and how it ends.
//!
//! The tab owns one of these and asks it what to do; it never decides paging
//! on its own. Headless and told everything — whether the chart is idle,
//! whether the tab is on screen, what each reply brought — so every path a
//! trader can take through the History button is a unit test here.

use quantick_engine::Trade;
use quantick_engine::trade_tape::TradeSeq;

use crate::history_reach::{
    Campaign, CampaignEnd, CampaignStart, CampaignStep, HistoryReach, ReachBounds, ReachOutcome,
    ReachProgress,
};

/// What a press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// The chart is idle: the caller begins the run now.
    Start,
    /// The chart is still filling, or a reply is still out: the press waits
    /// and [`HistoryRun::poll`] hands it back.
    Queued,
    /// A run is already paging; nothing new starts.
    AlreadyRunning,
}

/// What the caller does after the run judged something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunAction {
    /// Send one `load_older` of this many prints.
    Send(usize),
    /// The run is over; tell the trader.
    Finished(ReachOutcome),
    /// Nothing to do now.
    Wait,
}

/// What a frame's poll asks of the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    /// A queued press may begin now.
    Begin(HistoryReach),
    /// A paused run's next request may go out now.
    Send(usize),
    /// Nothing to do.
    Nothing,
}

/// What a cancel stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancelled {
    /// A running run, ended where it stood.
    Run(ReachOutcome),
    /// A press still waiting to begin.
    Queued(HistoryReach),
    /// Nothing was running.
    Nothing,
}

/// Where the run stands, for the button and the control plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// No run, no press waiting.
    Idle,
    /// A press is waiting for the chart to finish filling.
    Queued(HistoryReach),
    /// Paging toward the target.
    Loading(ReachProgress),
    /// Paging is held while the tab is off screen.
    Paused(ReachProgress),
}

impl RunStatus {
    /// The stable word the control plane reports.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Queued(_) => "queued",
            Self::Loading(_) => "loading",
            Self::Paused(_) => "paused",
        }
    }
}

/// One tab's history run.
#[derive(Debug, Clone, Default)]
pub struct HistoryRun {
    /// What this tab's main click repeats, once it has pressed.
    last: Option<HistoryReach>,
    queued: Option<HistoryReach>,
    campaign: Option<Campaign>,
    /// A request is out, perhaps from a run since cancelled. The transports
    /// serve one at a time, so nothing new is sent until its reply lands.
    in_flight: bool,
    /// A reply landed while the tab was hidden; the next request waits.
    paused: bool,
}

impl HistoryRun {
    /// What the main click repeats: this tab's last action, else `default`.
    #[must_use]
    pub fn main_reach(&self, default: HistoryReach) -> HistoryReach {
        self.last.unwrap_or(default)
    }

    /// A press of `reach`. `idle` is whether the chart is done filling and
    /// rebuilding, so a run can judge what it actually holds.
    pub fn press(&mut self, reach: HistoryReach, idle: bool) -> Press {
        if self.campaign.is_some() {
            return Press::AlreadyRunning;
        }
        self.last = Some(reach);
        if idle && !self.in_flight {
            self.queued = None;
            Press::Start
        } else {
            self.queued = Some(reach);
            Press::Queued
        }
    }

    /// Begin a run of `reach` over the chart's tape, after [`Press::Start`]
    /// or [`Poll::Begin`].
    pub fn begin<T: TradeSeq + ?Sized>(
        &mut self,
        reach: HistoryReach,
        held: &T,
        bounds: ReachBounds,
        page_size: usize,
    ) -> RunAction {
        self.queued = None;
        self.paused = false;
        match Campaign::start(held, reach, bounds, page_size) {
            CampaignStart::Run(campaign) => self.send_next(campaign),
            CampaignStart::AlreadyMet(outcome) | CampaignStart::NothingCharted(outcome) => {
                RunAction::Finished(outcome)
            }
        }
    }

    fn send_next(&mut self, mut campaign: Campaign) -> RunAction {
        let count = campaign.next_request();
        self.campaign = Some(campaign);
        self.in_flight = true;
        RunAction::Send(count)
    }

    /// Once a frame: a queued press begins when the chart is idle, and a
    /// paused run resumes when its tab is on screen.
    pub fn poll(&mut self, idle: bool, visible: bool) -> Poll {
        if let Some(reach) = self.queued
            && idle
            && !self.in_flight
            && self.campaign.is_none()
        {
            self.queued = None;
            return Poll::Begin(reach);
        }
        if self.paused
            && visible
            && let Some(campaign) = self.campaign.take()
        {
            self.paused = false;
            if let RunAction::Send(count) = self.send_next(campaign) {
                return Poll::Send(count);
            }
        }
        Poll::Nothing
    }

    /// A reply landed: judge its raw page and say what comes next.
    pub fn on_reply(&mut self, page: &[Trade], can_page: bool, visible: bool) -> RunAction {
        self.in_flight = false;
        let Some(mut campaign) = self.campaign.take() else {
            // The answer to a cancelled request: kept on the chart, judged by
            // nobody.
            return RunAction::Wait;
        };
        match campaign.advance(page, can_page) {
            CampaignStep::Ask if visible => self.send_next(campaign),
            CampaignStep::Ask => {
                self.campaign = Some(campaign);
                self.paused = true;
                RunAction::Wait
            }
            CampaignStep::Stop(end) => RunAction::Finished(campaign.finish(end)),
        }
    }

    /// The request [`RunAction::Send`] asked for could not be queued.
    pub fn send_failed(&mut self) -> Option<ReachOutcome> {
        self.in_flight = false;
        self.paused = false;
        self.campaign
            .take()
            .map(|campaign| campaign.finish(CampaignEnd::RequestRefused))
    }

    /// Stop now and keep what arrived. The request already out still lands
    /// on the chart; nothing more is asked.
    pub fn cancel(&mut self) -> Cancelled {
        self.paused = false;
        if let Some(campaign) = self.campaign.take() {
            self.queued = None;
            return Cancelled::Run(campaign.finish(CampaignEnd::Cancelled));
        }
        self.queued
            .take()
            .map_or(Cancelled::Nothing, Cancelled::Queued)
    }

    /// The market changed: the old feed's request died with its channel and
    /// the run belongs to a tape this tab no longer shows.
    pub fn reset(&mut self) {
        self.queued = None;
        self.campaign = None;
        self.in_flight = false;
        self.paused = false;
    }

    /// Whether a run is paging. The chart holds its pages until it ends, so
    /// the visible bars do not move while it loads.
    #[must_use]
    pub const fn holds_pages(&self) -> bool {
        self.campaign.is_some()
    }

    /// Where the run stands.
    #[must_use]
    pub fn status(&self) -> RunStatus {
        match (&self.campaign, self.queued) {
            (Some(campaign), _) if self.paused => RunStatus::Paused(campaign.progress()),
            (Some(campaign), _) => RunStatus::Loading(campaign.progress()),
            (None, Some(reach)) => RunStatus::Queued(reach),
            (None, None) => RunStatus::Idle,
        }
    }
}

#[cfg(test)]
#[path = "history_run_tests.rs"]
mod tests;
