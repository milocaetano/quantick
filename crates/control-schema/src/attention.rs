//! The action registry port: named, attributed calls that change session
//! state, and the first one — a human mark.
//!
//! Plan §2.2 and `CLAUDE.md`'s *Operable without a hand*: a capability a trader does
//! exists as a named call with an actor in its signature, not only inside a
//! click handler. The UI hotkey, the `QUANTICK_CONTROL_MARK` hook, a
//! deterministic test and — in a later pull request — an authorized agent all
//! arrive at the same handler through [`ActionRegistry`]. The gateway keeps
//! every action here unavailable to remote observer clients: an action's
//! permissions are not in the observer ceiling, so a remote invocation is
//! refused before dispatch.

use crate::readback::{EVERY_FORBIDDEN_TEST, Readback, journal};
use quantick_control::registry::IdempotencyPolicy::Forbidden;
use quantick_control_host::authority::CAPABILITY_VERSION;

/// The first registered action: a human (or, later, an agent) points at what
/// is under the pointer and says "this".
pub const MARK_CAPABILITY_ID: &str = "attention.mark.create";

pub const MARK_EVENT_KIND: &str = "attention.mark.created";

// The annotate tier's identifiers this action docks into (contract §7).

/// The version of `attention.mark.create` the hotkey and the hook invoke.
pub const MARK_CAPABILITY_VERSION: u32 = CAPABILITY_VERSION;

pub const MARK_TEST: &str = "an_interrupted_attention_mark_is_resolved_by_its_readback";
/// How a client reconciles an interrupted attention mark.
///
/// `retry_matrix` joins every family's rows into one table; a row belongs
/// here, beside the capability it reconciles.
pub const READBACKS: &[Readback] = &[journal(
    "attention.mark.create",
    Forbidden,
    MARK_EVENT_KIND,
    "payload.note",
    "an event after the pre-call cursor carries the call's own `note`; send a note unique to the call, since an unnoted mark (the trader's shortcut included) matches any other",
    &[MARK_TEST, EVERY_FORBIDDEN_TEST],
)];

use crate::interaction::CursorSnapshot;
use quantick_control::{limits::CONTROL_REASON_MAX_BYTES, wire::WireU64};
use quantick_control_host::journal::EventActor;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a mark takes: an optional note the human typed, and optionally the
/// target itself. The hotkey resolves the pointer and supplies the target, so
/// the input alone determines the mark and a control trace can replay it
/// identically; a caller that supplies none gets what is under the pointer
/// at that moment — and a replayed entry without one is refused rather than
/// resolved against the rerun's pointer, which is not the one that was
/// marked.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = CONTROL_REASON_MAX_BYTES))]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<CursorSnapshot>,
}

/// What a mark returns: where it landed in the journal and exactly what was
/// pointed at when it was taken. The journal event at `sequence` carries the
/// wall-clock time; the result does not repeat it, so the trace's result
/// digest depends on what was marked and on its place in the journal, never
/// on the clock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarkResult {
    pub sequence: WireU64,
    pub target: CursorSnapshot,
    /// Who resolved the target: `pointer` — the human's pointer, at the
    /// gesture (the hotkey and the hook pass it) or at the call (a caller
    /// that passed none); `supplied` — an agent passed a target it read from
    /// a snapshot; `replayed` — a control trace re-injected a recorded mark.
    pub target_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub actor: EventActor,
}

/// Published schemas, owned beside their wire records.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "attention-mark-input-v1.schema.json",
            quantick_control::schema::generated_schema::<crate::attention::MarkInput>(),
        ),
        (
            "attention-mark-result-v1.schema.json",
            quantick_control::schema::generated_schema::<crate::attention::MarkResult>(),
        ),
    ]
}
