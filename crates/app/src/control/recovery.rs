//! The cockpit tier's recovery capabilities: getting a stalled feed back.
//!
//! Both call the same `Tab` method the button in the chart's offline corner
//! calls. That
//! is the point rather than a tidiness preference — a capability with its own
//! copy of "start the feed over" would drift from the one a click takes, and
//! the drift would be an assistant and a trader disagreeing about what just
//! happened to the timeline they are both looking at.
//!
//! The pair is deliberately two capabilities rather than one with a flag,
//! because they differ in what a client has to read before calling: `reload`
//! is irreversible, carries the `timeline_rebuilt` risk flag, and needs a
//! permission of its own that is off until the trader ticks it. A single call
//! whose cost depended on its input could not answer that question in its
//! descriptor, and the answer is the whole reason the trader is offered a
//! choice at all.

pub(crate) use quantick_control_schema::recovery::*;

use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};

use serde_json::Value;

use crate::app::QuantickApp;

use super::{actions::ActionRegistry, gateway::ControlAccess};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        descriptor(
            RECONNECT_CAPABILITY_ID,
            "Reconnect a stalled feed",
            "Respawns the transport and keeps everything the chart has built: bars, drawings, indicators, armed strategies and any open paper position. The window the new session replays is dropped rather than counted twice, and a silence long enough to leave a hole in the tape is marked on the chart. The same call the Reconnect button in the chart's offline corner makes.",
            false,
            generated_schema::<RecoveryInput>(),
        ),
        reconnect,
    )?;
    registry.register(
        descriptor(
            RELOAD_CAPABILITY_ID,
            "Reload a chart from a new feed session",
            "Throws the timeline away and rebuilds it: refetches history, closes any open paper position (journaled, with its reason) and disarms every strategy. For a terminal that froze while its socket stayed open, where reconnecting fixes nothing. The same call the Reload button in the chart's offline corner makes.",
            true,
            generated_schema::<RecoveryInput>(),
        ),
        reload,
    )?;
    Ok(())
}

fn reconnect(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    recover(app, input, true)
}

fn reload(
    app: &mut QuantickApp,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    recover(app, input, false)
}

/// The shared body. `keep_timeline` picks which of the tab's two methods runs;
/// nothing else differs, so the two capabilities can never drift apart in
/// anything but the act they name.
fn recover(
    app: &mut QuantickApp,
    input: &Value,
    keep_timeline: bool,
) -> Result<Value, ControlError> {
    let input: RecoveryInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = tab_index(app, input.tab_id)?;
    let tab_id = app.control_tabs().id_at(index);
    let (tab, config) = app
        .control_tab_with_config(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    // Asked of the tab rather than inferred from one of its fields: a
    // recorded session owns the chart while it plays, and a tab whose feed id
    // has left the feed table has nothing to spawn either. Reported rather
    // than refused — "there was nothing to recover" is a true and useful
    // answer, and an error would read as "the call is broken" — but reported
    // from what actually happened, never from what was asked for.
    let respawned = if keep_timeline {
        tab.reconnect_feed(config)
    } else {
        tab.reload_feed(config)
    };
    let result = RecoveryResult {
        tab_id: WireU64::new(tab_id),
        symbol: tab.symbol.clone(),
        respawned,
        timeline_kept: keep_timeline || !respawned,
    };
    serde_json::to_value(result).map_err(|error| {
        ControlError::invalid_request(format!("the recovery result could not be encoded: {error}"))
    })
}

/// Which tab a call named, or the one the trader is looking at.
pub(crate) fn tab_index(app: &QuantickApp, tab_id: Option<WireU64>) -> Result<usize, ControlError> {
    let Some(id) = tab_id else {
        return Ok(app.control_active_tab_index());
    };
    app.control_tabs()
        .position(id.get())
        .ok_or_else(|| ControlError::invalid_request(format!("no open tab has id {}", id.get())))
}
