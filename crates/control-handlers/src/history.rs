//! `feed.history.load` and `feed.history.cancel`: the History button's press
//! and its cancel, as the tab session's own acts. The handler picks the
//! [`HistoryAct`] and judges its outcome; the window routes the act to the
//! tab through the toolbar's own press and cancel ([`HistoryPort`]).

use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};
use quantick_control_schema::history::{
    HistoryCancelInput, HistoryCancelResult, HistoryLoadInput, HistoryLoadResult,
    HistoryLoadSnapshot, cancel_descriptor, load_descriptor,
};
use quantick_session::{HistoryAct, HistoryOutcome, SessionSnapshot};
use quantick_sources::history_reach::{HistoryReach, MAX_REACH_HOURS, MAX_REACH_SESSIONS};
use quantick_sources::history_run::{Cancelled, Press, RunStatus};
use serde_json::{Value, json};

use crate::dock::ActionDock;
use crate::tabs::{TabDirectory, tab_closed, tab_index};

/// The window's routing of the History button's acts to one tab.
pub trait HistoryPort: TabDirectory {
    /// Whether the feed of the tab at `index` can page older trades; `None`
    /// when that tab is gone.
    fn history_paging(&self, index: usize) -> Option<bool>;
    /// Make the toolbar's own press or cancel on the tab at `index` — a
    /// taken press moves the window's main-click default
    /// ([`quantick_session::sets_window_default`]) — and answer with what
    /// this act did and the session as it then stands; `None` when the tab
    /// is gone.
    fn dispatch_history(
        &mut self,
        index: usize,
        act: HistoryAct,
    ) -> Option<(HistoryOutcome, SessionSnapshot)>;
}

/// Dock the press and its cancel.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: HistoryPort + ?Sized,
{
    registry.register(load_descriptor(), load)?;
    registry.register(cancel_descriptor(), cancel)
}

/// A refusal that names the field, what was sent and what is accepted.
fn refused(message: String, sent: &str, next_step: &str) -> ControlError {
    let mut error = ControlError::invalid_request(message);
    error.context.details = Some(json!({
        "field": "reach",
        "sent": sent,
        "accepted": [format!("hours:1..={MAX_REACH_HOURS}"), format!("sessions:1..={MAX_REACH_SESSIONS}")],
    }));
    error.context.next_steps = vec![next_step.to_owned()];
    error
}

fn load<H: HistoryPort + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: HistoryLoadInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let reach = HistoryReach::parse(&input.reach).map_err(|error| {
        refused(
            error.to_string(),
            &input.reach,
            "send reach as hours:N or sessions:N",
        )
    })?;
    let index = tab_index(app, input.tab_id)?;
    let tab_id = app.tab_id_at(index);
    if !app.history_paging(index).ok_or_else(tab_closed)? {
        return Err(refused(
            "this tab's feed cannot page older trades".to_owned(),
            &input.reach,
            "read feed.status capabilities.history_paging; venue candles load with the History menu's + older candles",
        ));
    }
    let (outcome, session) = app
        .dispatch_history(index, HistoryAct::Press(reach))
        .ok_or_else(tab_closed)?;
    let press = match outcome {
        HistoryOutcome::Pressed(Press::Start) if matches!(session.run, RunStatus::Idle) => {
            "finished"
        }
        HistoryOutcome::Pressed(Press::Start) => "started",
        HistoryOutcome::Pressed(Press::Queued) => "queued",
        // A press is answered with a press; any other answer took nothing.
        HistoryOutcome::Pressed(Press::AlreadyRunning) | HistoryOutcome::Cancelled(_) => {
            "already_running"
        }
    };
    let result = HistoryLoadResult {
        tab_id: WireU64::new(tab_id),
        reach: reach.token(),
        press: press.to_owned(),
        status: snapshot(session.run, session.main_reach),
    };
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn cancel<H: HistoryPort + ?Sized, A>(
    app: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: HistoryCancelInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.tab_id)?;
    let tab_id = app.tab_id_at(index);
    let (outcome, session) = app
        .dispatch_history(index, HistoryAct::Cancel)
        .ok_or_else(tab_closed)?;
    let cancelled = match outcome {
        HistoryOutcome::Cancelled(Cancelled::Run(_)) => "run",
        HistoryOutcome::Cancelled(Cancelled::Queued(_)) => "queued",
        // A cancel is answered with a cancel; any other answer stopped nothing.
        HistoryOutcome::Cancelled(Cancelled::Nothing) | HistoryOutcome::Pressed(_) => "nothing",
    };
    let result = HistoryCancelResult {
        tab_id: WireU64::new(tab_id),
        cancelled: cancelled.to_owned(),
        status: snapshot(session.run, session.main_reach),
    };
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// One tab's run as the control plane reads it, in `feed.status` too.
#[must_use]
pub fn snapshot(status: RunStatus, main: HistoryReach) -> HistoryLoadSnapshot {
    let (reach, progress) = match status {
        RunStatus::Idle => (None, None),
        RunStatus::Queued(reach) => (Some(reach), None),
        RunStatus::Loading(progress) | RunStatus::Paused(progress) => {
            (Some(progress.reach), Some(progress))
        }
    };
    HistoryLoadSnapshot {
        state: status.token().to_owned(),
        reach: reach.map(HistoryReach::token),
        main_reach: main.token(),
        back_to_unix_ms: progress.map(|progress| progress.oldest_ms),
        sessions_reached: progress
            .filter(|progress| matches!(progress.reach, HistoryReach::Sessions(_)))
            .map(|progress| progress.sessions_reached),
        traded_ms: progress.map(|progress| progress.traded_ms),
        prints_pulled: progress.map(|progress| {
            WireU64::new(u64::try_from(progress.prints_pulled).unwrap_or(u64::MAX))
        }),
        pages: progress.map(|progress| progress.pages),
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod history_tests;

#[cfg(test)]
#[path = "history_session_tests.rs"]
mod history_session_tests;
