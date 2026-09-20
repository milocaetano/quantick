//! The action registry port: named, attributed calls that change session
//! state, and the first one — a human mark.
//!
//! Plan §2.2 and `arch-review`'s second operator: a capability a trader does
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
