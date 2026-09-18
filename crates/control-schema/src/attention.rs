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

use quantick_control_host::authority::CAPABILITY_VERSION;

/// The first registered action: a human (or, later, an agent) points at what
/// is under the pointer and says "this".
pub const MARK_CAPABILITY_ID: &str = "attention.mark.create";

pub const MARK_EVENT_KIND: &str = "attention.mark.created";

/// The annotate tier's identifiers this action docks into (contract §7).

/// The version of `attention.mark.create` the hotkey and the hook invoke.
pub const MARK_CAPABILITY_VERSION: u32 = CAPABILITY_VERSION;
