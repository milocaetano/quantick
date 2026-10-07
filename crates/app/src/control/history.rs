//! `feed.history.load` and `feed.history.cancel`: the History button's press
//! and its cancel, through the same calls a click makes
//! ([`crate::app::control_host::TabsMut::press_history`] for the press).

use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};
use quantick_control_schema::history::{
    HistoryCancelInput, HistoryCancelResult, HistoryLoadInput, HistoryLoadResult,
    HistoryLoadSnapshot, cancel_descriptor, load_descriptor,
};
use quantick_feed::history_reach::{HistoryReach, MAX_REACH_HOURS, MAX_REACH_SESSIONS};
use quantick_feed::history_run::{Cancelled, Press, RunStatus};
use serde_json::{Value, json};

use super::recovery::tab_index;
use super::{actions::ActionRegistry, gateway::ControlAccess};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
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

fn load<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
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
    let tab_id = app.tab_reads().tabs().id_at(index);
    let closed = || ControlError::invalid_request("the tab closed while the call ran");
    let (tab, config) = app.tabs_mut().tab_with_config(index).ok_or_else(closed)?;
    if !tab.capabilities(config).history_paging {
        return Err(refused(
            "this tab's feed cannot page older trades".to_owned(),
            &input.reach,
            "read feed.status capabilities.history_paging; venue candles load with the History menu's + older candles",
        ));
    }
    // The toolbar's own press, so the main-click default moves with it.
    let (tab, press) = app
        .tabs_mut()
        .press_history(index, reach)
        .ok_or_else(closed)?;
    let press = match press {
        Press::Start if matches!(tab.history_status(), RunStatus::Idle) => "finished",
        Press::Start => "started",
        Press::Queued => "queued",
        Press::AlreadyRunning => "already_running",
    };
    let result = HistoryLoadResult {
        tab_id: WireU64::new(tab_id),
        reach: reach.token(),
        press: press.to_owned(),
        status: snapshot(tab.history_status(), tab.main_history_reach()),
    };
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn cancel<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: HistoryCancelInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.tab_id)?;
    let tab_id = app.tab_reads().tabs().id_at(index);
    let (tab, _) = app
        .tabs_mut()
        .tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let cancelled = match tab.cancel_history() {
        Cancelled::Run(_) => "run",
        Cancelled::Queued(_) => "queued",
        Cancelled::Nothing => "nothing",
    };
    let result = HistoryCancelResult {
        tab_id: WireU64::new(tab_id),
        cancelled: cancelled.to_owned(),
        status: snapshot(tab.history_status(), tab.main_history_reach()),
    };
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// One tab's run as the control plane reads it, in `feed.status` too.
pub(crate) fn snapshot(status: RunStatus, main: HistoryReach) -> HistoryLoadSnapshot {
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
