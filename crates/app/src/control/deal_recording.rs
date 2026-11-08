//! The REC control's capability: start or stop recording a tab's deal
//! counter, from a script exactly as from the button.
//!
//! The reads live in [`super::feed`]'s `feed.status` scope, whose
//! `deal_recording` field is the same recorder view every chrome surface
//! draws. This module is the act. It sits in the `feed` module beside
//! `feed.reconnect`, under the same cockpit permission: recording writes a
//! file the trader asked for and nothing the chart holds is touched, so it
//! is neither destructive nor risky, and it can be undone by the same call.
//! What the act says back, the view on the wire and every refusal, is
//! [`quantick_control_handlers::deal_recording`]'s.

pub(crate) use quantick_control_schema::deal_recording::*;

use crate::app::{RecordingPort, TabsMutPort, TabsPort};
use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};

use serde_json::Value;

use crate::deal_recording::DealRecordingAction;

use super::{actions::ActionRegistry, gateway::ControlAccess};
pub(crate) use quantick_control_handlers::deal_recording::snapshot;
use quantick_control_handlers::deal_recording::{recorded_day, recording_error, replaying};
use quantick_control_handlers::{TabDirectory, tab_index};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: TabDirectory + TabsPort + TabsMutPort + RecordingPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: DealRecordingInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.tab_id)?;
    let tab_id = app.tab_reads().tabs().id_at(index);
    let (tab, _config) = app
        .tabs_mut()
        .tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    if tab.replay.is_some() {
        return Err(replaying(tab_id));
    }
    // Resolve every data-dependent refusal before changing the durable
    // default. A call is atomic with respect to that setting: bad day, replay,
    // or unsupported start leaves the workspace untouched.
    let day_index = match &input.load_day {
        Some(day) => Some(recorded_day(
            tab.deal_recording_view().as_ref(),
            day,
            &tab.symbol,
        )?),
        None => None,
    };
    let _ = tab;
    let (tab, _config) = app
        .tabs_mut()
        .tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    if let Some(index) = day_index {
        tab.load_recorded_day_checked(index)
            .map_err(recording_error)?;
    }
    if let Some(enabled) = input.enabled {
        if enabled {
            tab.start_deal_recording_checked(crate::metrics::wall_clock_ms())
                .map_err(recording_error)?;
        } else {
            tab.apply_deal_recording(DealRecordingAction::Stop);
        }
    }
    let _ = tab;
    if let Some(on) = input.record_by_default {
        app.set_deal_recording_default(on);
    }
    let (tab, _config) = app
        .tabs_mut()
        .tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let result = DealRecordingResult {
        tab_id: WireU64::new(tab_id),
        symbol: tab.symbol.clone(),
        recording: tab.deal_recording_view().as_ref().map(snapshot),
    };
    serde_json::to_value(result).map_err(|error| {
        ControlError::invalid_request(format!(
            "the deal recording result could not be encoded: {error}"
        ))
    })
}
