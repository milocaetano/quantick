//! The shape of a readback row, and the two reads a row can name.
//!
//! A family module declares its own rows beside the capabilities they
//! reconcile, and [`crate::retry_matrix`] joins them. The row type lives here
//! rather than there so a family does not import the matrix that imports it:
//! `crates/guards/src/cycle.rs` states why a module cycle is worth one small
//! module.

use quantick_control::registry::IdempotencyPolicy;
use quantick_control_host::authority::SNAPSHOT_CAPABILITY_ID;
use quantick_control_host::events::READ_CAPABILITY_ID as EVENTS_READ_CAPABILITY_ID;

/// One mutable capability, and how a client reconciles a call to it whose
/// answer it never saw.
#[derive(Clone, Copy, Debug)]
pub struct Readback {
    /// The capability this row covers.
    pub capability: &'static str,
    /// The policy the descriptor is expected to publish. Not a second source
    /// of truth — the matrix renders the descriptor's own — but the tripwire:
    /// a descriptor whose policy moves without this row being revisited is
    /// [`crate::retry_matrix::Drift::PolicyMoved`], because the readback that suited a retryable
    /// call rarely suits one that is not.
    pub policy: IdempotencyPolicy,
    /// The read capability that shows the effect.
    pub read: &'static str,
    /// The `snapshot.read` scope, when `read` is `snapshot.read`.
    pub scope: Option<&'static str>,
    /// The journal event kind, when `read` is `events.read`.
    pub event: Option<&'static str>,
    /// The field that shows the effect: a dotted path, `[]` stepping into an
    /// array. Inside the scope's value for a snapshot, inside each event of
    /// `event`'s kind for the journal.
    pub field: &'static str,
    /// What the field says when the call applied, against a reading taken
    /// before the call.
    pub applied_when: &'static str,
    /// The transport tests that exercise this row.
    pub proven_by: &'static [&'static str],
}

/// Every reachable `optional` row, one keyed call and its retry each.
pub const EVERY_OPTIONAL_TEST: &str =
    "every_reachable_optional_row_replays_a_dropped_answer_and_begins_once";

/// Every reachable `forbidden` row, one keyed call each.
pub const EVERY_FORBIDDEN_TEST: &str =
    "every_reachable_forbidden_row_refuses_a_key_before_the_application";

pub const UNKNOWN_OUTCOME_TEST: &str =
    "a_keyed_action_held_past_its_deadline_is_refused_as_unknown_and_reconciled_by_readback";

/// A row read back through one `snapshot.read` scope.
pub const fn snapshot(
    capability: &'static str,
    policy: IdempotencyPolicy,
    scope: &'static str,
    field: &'static str,
    applied_when: &'static str,
    proven_by: &'static [&'static str],
) -> Readback {
    Readback {
        capability,
        policy,
        read: SNAPSHOT_CAPABILITY_ID,
        scope: Some(scope),
        event: None,
        field,
        applied_when,
        proven_by,
    }
}

/// A row read back through the journal, from a cursor taken before the call.
pub const fn journal(
    capability: &'static str,
    policy: IdempotencyPolicy,
    event: &'static str,
    field: &'static str,
    applied_when: &'static str,
    proven_by: &'static [&'static str],
) -> Readback {
    Readback {
        capability,
        policy,
        read: EVENTS_READ_CAPABILITY_ID,
        scope: None,
        event: Some(event),
        field,
        applied_when,
        proven_by,
    }
}

/// How many rows the declared families hold between them.
pub(crate) const fn total(families: &[&[Readback]]) -> usize {
    let mut counted = 0;
    let mut family = 0;
    while family < families.len() {
        counted += families[family].len();
        family += 1;
    }
    counted
}

/// The declared families as one table, at compile time: the joined array is
/// `const` data, exactly as the single written array was.
///
/// The seed row is the first family's first, so a family declared empty and
/// listed first fails compilation rather than silently shortening the table.
pub(crate) const fn flatten<const N: usize>(families: &[&[Readback]]) -> [Readback; N] {
    assert!(
        N == total(families),
        "the joined table is as long as the declared families, and no longer"
    );
    let mut joined = [families[0][0]; N];
    let mut next = 0;
    let mut family = 0;
    while family < families.len() {
        let rows = families[family];
        let mut row = 0;
        while row < rows.len() {
            joined[next] = rows[row];
            next += 1;
            row += 1;
        }
        family += 1;
    }
    joined
}
