//! Loading a chart's history back to a target, and stopping it.
//!
//! The same two acts the toolbar's History button performs: a press of a
//! target (`hours:N` of traded time before the oldest print, or `sessions:N`
//! back to the Nth previous session's open) and the cancel the loading button
//! becomes. Progress and the honest end are read in `feed.status`, under
//! `tabs[].history_load`, from the same run the button draws.

use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence},
    schema::generated_schema,
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};
use quantick_control::registry::IdempotencyPolicy::Optional;

pub const LOAD_CAPABILITY_ID: &str = "feed.history.load";

pub const CANCEL_CAPABILITY_ID: &str = "feed.history.cancel";

/// Which tab, and how far back.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryLoadInput {
    /// The tab's id, as `feed.status` reports it. Omitted means the tab the
    /// trader is looking at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    /// `hours:N` (1..=48) of traded time before the oldest loaded print, or
    /// `sessions:N` (1..=10) back to the Nth previous session's open, counted
    /// from the live edge. `sessions:1` is yesterday.
    pub reach: String,
}

/// What a load call did.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryLoadResult {
    pub tab_id: WireU64,
    /// The target, normalised to its token.
    pub reach: String,
    /// `started`, `queued` (the session is still loading; it starts after),
    /// `already_running` (a run is paging; nothing new started), or
    /// `finished` (nothing to fetch: the note says why).
    pub press: String,
    /// The run as `feed.status` reads it right after the call.
    pub status: HistoryLoadSnapshot,
}

/// Which tab's run to stop.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryCancelInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
}

/// What a cancel call stopped.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryCancelResult {
    pub tab_id: WireU64,
    /// `run` (stopped where it stood, what arrived kept), `queued` (a press
    /// that had not begun) or `nothing`.
    pub cancelled: String,
    pub status: HistoryLoadSnapshot,
}

/// One tab's history run, as the History button draws it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryLoadSnapshot {
    /// `idle`, `queued`, `loading`, or `paused` (the tab is off screen).
    pub state: String,
    /// The target being loaded or queued; absent while idle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<String>,
    /// What the tab's main click loads.
    pub main_reach: String,
    /// The oldest print held or fetched so far by the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub back_to_unix_ms: Option<i64>,
    /// Previous sessions whose open the run has proven on the chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions_reached: Option<u32>,
    /// Traded time the run has counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub traded_ms: Option<i64>,
    /// Prints the run has pulled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prints_pulled: Option<WireU64>,
    /// Requests the run has made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
}

fn descriptor<Input: JsonSchema, Output: JsonSchema>(
    id: &str,
    title: &str,
    description: &str,
    stale_input_safety: &str,
) -> CapabilityDescriptor {
    let mut descriptor =
        crate::recovery::descriptor(id, title, description, false, generated_schema::<Input>());
    descriptor.output_schema = generated_schema::<Output>();
    descriptor.persistence = EffectPersistence::Transient;
    descriptor.stale_input_safety = Some(stale_input_safety.to_owned());
    descriptor
}

pub fn load_descriptor() -> CapabilityDescriptor {
    descriptor::<HistoryLoadInput, HistoryLoadResult>(
        LOAD_CAPABILITY_ID,
        "Load a chart's history back to a target",
        "The History button's press: `hours:N` loads N more hours of traded time before the oldest print (nights and weekends crossed, not counted); `sessions:N` loads back to the open of the Nth previous session, found from the tape's overnight gaps and counted from the live edge (24 h of tape per day on a market that never closes). Pages are fetched one at a time and the chart is rebuilt once when the run ends, so the visible bars hold still while it loads. A press while the session is still loading is queued; a press while a run pages starts nothing new. Budgets scale with the target and the run ends partial, with the reason, rather than claiming a target it did not reach. Progress and the outcome read in feed.status as tabs[].history_load and tabs[].history_reach_note.",
        "A stale caller can only load more history than it meant, which costs memory and requests within the run's budgets and removes nothing; the result names the tab and target it acted on.",
    )
}

pub fn cancel_descriptor() -> CapabilityDescriptor {
    descriptor::<HistoryCancelInput, HistoryCancelResult>(
        CANCEL_CAPABILITY_ID,
        "Stop loading a chart's history",
        "The loading History button's click (or Esc): stops the run where it stands and keeps every page that arrived; the chart is rebuilt once with them and the note says where it stopped and how far it got. A queued press is dropped. Cancelling when nothing runs changes nothing.",
        "Stopping twice stops once; a stale caller can only end a run early, keeping what arrived.",
    )
}

/// The run's state moves when either call acts.
pub const HISTORY_TEST: &str =
    "a_history_load_runs_reads_back_and_cancels_through_the_control_plane";

pub const READBACKS: &[Readback] = &[
    snapshot(
        LOAD_CAPABILITY_ID,
        Optional,
        crate::feed::SCOPE_ID,
        "tabs[].history_load.state",
        "the tab's run reads `loading` or `queued` for the target asked for (a target already on the chart answers `finished` and the state stays `idle`, with the note saying so)",
        &[EVERY_OPTIONAL_TEST, HISTORY_TEST],
    ),
    snapshot(
        CANCEL_CAPABILITY_ID,
        Optional,
        crate::feed::SCOPE_ID,
        "tabs[].history_load.state",
        "the tab's run reads `idle`: nothing pages and nothing is queued",
        &[EVERY_OPTIONAL_TEST, HISTORY_TEST],
    ),
];
