//! The REC control's capability: start or stop recording a tab's deal
//! counter, from a script exactly as from the button.
//!
//! The reads live in [`super::feed`]'s `feed.status` scope, whose
//! `deal_recording` field is the same [`RecordingView`] every chrome surface
//! draws. This module is the act. It sits in the `feed` module beside
//! `feed.reconnect`, under the same cockpit permission: recording writes a
//! file the trader asked for and nothing the chart holds is touched, so it
//! is neither destructive nor risky, and it can be undone by the same call.

use std::collections::BTreeSet;

use quantick_control::{
    id::{CapabilityId, CostClassId, ModuleId, PermissionId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RevisionPolicy,
    },
    schema::generated_schema,
};
use quantick_control_host::authority::{
    CAPABILITY_VERSION, COCKPIT_EFFECT_ID, COCKPIT_PERMISSION_ID, NO_CONFIRMATION_ID,
    UI_BOUNDED_COST_ID,
};

use crate::recovery::RECOVERY_MODULE_ID;
use quantick_control::wire::WireU64;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const SET_CAPABILITY_ID: &str = "feed.deal_recording.set";

/// Which tab, and whether to record. Omitted tab means the one the trader is
/// looking at — the same default every other cockpit call takes.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DealRecordingInput {
    /// The tab's id, as `observe.feed.status` reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<WireU64>,
    /// `true` starts recording (resuming today's file if there is one),
    /// `false` stops it and keeps what was written. Omitted leaves the
    /// recorder as it is — for a call that only loads a day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// A recorded day to load into the tab's panes, `YYYY-MM-DD` as
    /// `feed.status` lists it under `recorded_days` — the popover's own
    /// click, as a call. A day that is not recorded is refused by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_day: Option<String>,
    /// The standing choice — record by default on every tab whose feed
    /// carries a counter — the Tools menu's checkbox, as a call. Saved with
    /// the workspace. Omitted leaves it as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_by_default: Option<bool>,
}

/// The recorder as the chrome reads it, on the wire.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DealRecordingSnapshot {
    /// `off`, `recording`, `stale` (the tape flows, the counter does not),
    /// `recorded` (the readings on screen came from a file) or `unsupported`
    /// (the feed declares no counter; a day recorded earlier may still be
    /// listed and loaded).
    pub state: String,
    /// Where the open file starts, on the tape's clock — the recording's own
    /// "since", resumed or written this run. What the REC button shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_unix_ms: Option<i64>,
    /// When the first reading of this run arrived, written to a file or not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_reading_unix_ms: Option<i64>,
    /// The standing choice: record by default wherever a feed carries a
    /// counter — what the Tools menu's checkbox reads, saved with the
    /// workspace.
    pub record_by_default: bool,
    /// The counter has not moved for the stale span while prints kept
    /// coming — REC on (state `stale`) or off, where the chart's chip says
    /// `counter stale` or `counter stuck at 0` (`session_deals` 0).
    pub counter_stale: bool,
    /// The newest reading of the venue's session deal counter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_deals: Option<WireU64>,
    /// The file being written, while recording.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Lines written to that file this run.
    pub samples_written: WireU64,
    /// Whether a `trades` pane has a count to cut on right now.
    pub deal_count_available: bool,
    /// Days recorded under this symbol, oldest first.
    pub recorded_days: Vec<RecordedDaySnapshot>,
    /// Days whose readings were loaded into the panes this session.
    pub loaded_days: Vec<String>,
    /// The last write or read error, if the recorder hit one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecordedDaySnapshot {
    /// `YYYY-MM-DD` in the display timezone.
    pub day: String,
    pub first_unix_ms: i64,
    pub last_unix_ms: i64,
    /// The counter's last reading that day.
    pub session_deals: WireU64,
    pub samples: WireU64,
    /// Whether the recording started with the counter barely begun.
    pub from_open: bool,
}

/// What the call did: the tab, and the recorder as it stands afterwards.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct DealRecordingResult {
    pub tab_id: WireU64,
    pub symbol: String,
    /// `None` when the tab's feed has no deal counter — the call changed
    /// nothing, and says so rather than failing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<DealRecordingSnapshot>,
}

/// The capability as the registry lists it.
#[must_use]
pub fn descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
            id: CapabilityId::new(SET_CAPABILITY_ID).expect("static capability ID is valid"),
            version: CAPABILITY_VERSION,
            title: "Record the venue's deal counter".to_owned(),
            description: "Starts or stops writing the session deal counter a MetaTrader B3 bridge stamps on its live ticks, so the tab's trades bars cover the day and a recorded day reopens as the same chart, and loads a recorded day's readings into the tab's panes. Starting resumes today's file when there is one; stopping keeps what was written. The same calls the REC control beside the symbol makes. A tab whose feed has no deal counter answers with no recording and changes nothing, unless a day recorded earlier is on disk: then it lists it (state `unsupported`) and `load_day` opens it. `record_by_default` sets the standing choice the Tools menu's checkbox sets, saved with the workspace, and every answer reports it.".to_owned(),
            module: ModuleId::new(RECOVERY_MODULE_ID).expect("static module ID is valid"),
            input_schema: generated_schema::<DealRecordingInput>(),
            output_schema: generated_schema::<DealRecordingResult>(),
            examples: Vec::new(),
            effect: quantick_control::id::EffectId::new(COCKPIT_EFFECT_ID)
                .expect("static effect ID is valid"),
            risk_flags: BTreeSet::<RiskFlagId>::new(),
            read_only: false,
            // Setting the same state twice leaves the recorder where it was;
            // a client may retry a dropped call.
            idempotency: IdempotencyPolicy::Optional,
            revision_policy: RevisionPolicy::OptionalForAdditive,
            stale_input_safety: Some(
                "A stale caller can only set a state the recorder is already in, or start a recording a hand just stopped — which the next call undoes. The result names the tab and the recorder's state afterwards, so a caller that guessed wrong can see it did.".to_owned(),
            ),
            dry_run_supported: false,
            persistence: EffectPersistence::Durable,
            reversible: true,
            destructive: false,
            risk_reducing: false,
            required_permissions: BTreeSet::from([
                PermissionId::new(COCKPIT_PERMISSION_ID).expect("static permission ID is valid"),
            ]),
            preconditions: Vec::new(),
            confirmation_class: quantick_control::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
                .expect("static confirmation class is valid"),
            availability: Availability::available(),
            expected_cost: ExpectedCost {
                class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
                max_items: None,
                max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
            },
            pagination: None,
        }
}
