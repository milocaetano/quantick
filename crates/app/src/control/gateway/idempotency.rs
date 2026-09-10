//! Deduplication for the keys the capability descriptors already promise.
//!
//! Eleven `layout.*` capabilities, `feed.reconnect`, `feed.reload` and the
//! `trade.*` shaping family declare [`IdempotencyPolicy::Optional`], and say
//! why in prose: "Applying the same arrangement twice leaves the same
//! arrangement, so a client may retry a dropped call without wondering what
//! the first one did." Until this module existed the gateway refused every
//! one of those keys before dispatch, so a client that read the descriptor
//! and did the correct thing got `control.invalid_request` for its trouble.
//!
//! The shape is the reference host's — [`quantick_control::fake`] has carried
//! it since the contract was written — with the differences a real gateway
//! forces:
//!
//! - **A retryable failure is never recorded.** The reference host has no
//!   backpressure or timeout paths; this one does. Recording a `BACKPRESSURE`
//!   outcome under a key would pin the client to that failure for the whole
//!   retention window, which inverts the guarantee the key exists for. Only a
//!   success or a non-retryable failure is terminal enough to replay.
//! - **An oversized result is not retained, and the call still answers.** The
//!   reference host rolls its effect back and refuses; a gateway cannot
//!   un-move a pane. The response goes out as it is and nothing is stored, so
//!   a retry re-executes — exactly what a call with no key does today, which
//!   is why this costs the client nothing it had.
//!
//! Three limits are worth stating plainly rather than leaving to be
//! discovered.
//!
//! **The guarantee is per connection, not per client.** `principal_id` is
//! minted from `random_bytes` when a connection completes its handshake, so a
//! client that reconnects arrives as a different principal and its keys
//! re-execute. That is the narrower half of what the descriptor prose
//! promises, and it is deliberate: the only identifiers that do survive a
//! reconnect are the bearer token, which every client of one grant shares,
//! and the handshake's `client_name`, which the client supplies and nothing
//! authenticates. Scoping by either would let one client replay another's
//! recorded result, so the guarantee stops where authenticated identity
//! stops. Widening it needs a durable client identity the handshake proves,
//! which is a contract change and not this module's to make.
//!
//! **Retention is bounded.** The guarantee covers the most recent
//! [`CONTROL_IDEMPOTENCY_MAX_ENTRIES`] keys within
//! [`CONTROL_IDEMPOTENCY_RETENTION_MS`].
//!
//! **A replayed answer is not marked as one on the wire.** The `warnings`
//! field that would carry such a mark has no producer anywhere in the tree,
//! and inventing its first code here would add to the published contract
//! rather than honour it.
//!
//! The store is owned by the connection authority, so it lives exactly as
//! long as one enabling of the gateway. Disabling and re-enabling access
//! mints a new token and a new descriptor; records from the old grant have no
//! business surviving into the new one.
//!
//! [`IdempotencyPolicy::Optional`]: quantick_control::registry::IdempotencyPolicy::Optional

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, PoisonError},
};

use quantick_control::{
    canonical::{Sha256Digest, canonical_sha256},
    error::{ControlError, codes},
    id::{CapabilityId, ErrorCode, IdempotencyKey, InstanceId, PrincipalId},
    limits::{
        CONTROL_IDEMPOTENCY_MAX_ENTRIES, CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES,
        CONTROL_IDEMPOTENCY_RETENTION_MS,
    },
    wire::{ModuleRevision, RequestEnvelope, ResponseEnvelope, ResponseOutcome},
};
use serde_json::json;

/// What makes two calls the same call.
///
/// `principal_id` is in the key and not merely alongside it: a record belongs
/// to the client that made it, so a second connection presenting the same key
/// gets its own execution rather than a stranger's recorded result. That is
/// the reference host's scope, kept here because the consequence of widening
/// it is a client reading another client's answer.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IdempotencyScope {
    instance_id: InstanceId,
    principal_id: PrincipalId,
    capability_id: CapabilityId,
    capability_version: u32,
    key: IdempotencyKey,
}

#[derive(Clone, Debug)]
struct IdempotencyRecord {
    input_digest: Sha256Digest,
    outcome: ResponseOutcome,
    module_revisions: Vec<ModuleRevision>,
    /// When the record was written, on the caller's clock. Retention is
    /// measured against this rather than read from a clock inside the store,
    /// so the store itself stays testable without one.
    stored_at_unix_ms: i64,
}

/// One request's place in the store, computed before dispatch and spent
/// after it.
#[derive(Clone, Debug)]
pub(super) struct IdempotencyTicket {
    scope: IdempotencyScope,
    input_digest: Sha256Digest,
    /// Held from the moment [`IdempotencyStore::admit`] lets this call
    /// through until the last clone of the ticket is dropped. The response
    /// thread's clone outlives the connection loop's, so the release happens
    /// after the record is written and never before it.
    reservation: Option<Arc<ScopeReservation>>,
}

/// One scope, held for as long as its dispatch is in flight.
///
/// Without this the guarantee has a hole exactly where it is needed most. A
/// record is written when the response is produced, and an action's response
/// is produced on a worker thread after the connection loop has gone back to
/// reading. A client that pipelines its retry — sends the second call
/// *because* the first has not answered, which is the case the descriptors
/// name — would find no record yet and act twice.
///
/// Releasing on drop is what makes every exit correct without every exit
/// having to know: the two paths that record, and the refusals that
/// dispatched nothing and must leave the key free for the retry they invite.
#[derive(Debug)]
struct ScopeReservation {
    scope: IdempotencyScope,
    store: Arc<IdempotencyStore>,
}

impl Drop for ScopeReservation {
    fn drop(&mut self) {
        self.store.release(&self.scope);
    }
}

/// Records and reservations under one lock, because "has this been answered"
/// and "is this being answered" are one question and must not be asked in two
/// steps a competing call can slip between.
#[derive(Debug, Default)]
struct Ledger {
    records: BTreeMap<IdempotencyScope, IdempotencyRecord>,
    in_flight: BTreeSet<IdempotencyScope>,
}

/// The records one enabling of the gateway retains.
#[derive(Debug, Default)]
pub(super) struct IdempotencyStore {
    ledger: Mutex<Ledger>,
}

impl IdempotencyStore {
    /// The ticket for `envelope`, or `None` when the request carries no key.
    ///
    /// Called only after [`ObserverContract::prepare`] has accepted the
    /// request, which is what makes a present key proof that the descriptor
    /// allows one: `prepare` refuses a key on a `Forbidden` capability before
    /// this is ever reached.
    ///
    /// [`ObserverContract::prepare`]: crate::control::contract::ObserverContract::prepare
    pub(super) fn ticket(
        instance_id: &InstanceId,
        principal_id: &PrincipalId,
        envelope: &RequestEnvelope,
    ) -> Result<Option<IdempotencyTicket>, ControlError> {
        let Some(key) = envelope.idempotency_key.clone() else {
            return Ok(None);
        };
        // `expected_revisions` cannot be non-empty on this host — `prepare`
        // refuses an envelope carrying any — but it is digested anyway, so a
        // later host that starts accepting them cannot forget that two calls
        // differing only there are different calls.
        let mut expected_revisions = envelope.expected_revisions.clone();
        expected_revisions.sort_by(|left, right| left.module_id.cmp(&right.module_id));
        let digest_input = json!({
            "expected_revisions": expected_revisions,
            "payload": envelope.payload,
        });
        let input_digest = canonical_sha256(&digest_input)
            .map_err(|error| ControlError::invalid_request(error.to_string()))?;
        Ok(Some(IdempotencyTicket {
            reservation: None,
            scope: IdempotencyScope {
                instance_id: instance_id.clone(),
                principal_id: principal_id.clone(),
                capability_id: envelope.capability_id.clone(),
                capability_version: envelope.capability_version,
                key,
            },
            input_digest,
        }))
    }

    /// Answer this request from the store, refuse it, or reserve its key and
    /// let it through.
    ///
    /// `Some(response)` is the whole answer and the call never reaches the
    /// application: a replay of the recorded outcome, a conflict, or the
    /// refusal a caller gets for racing its own retry. `None` means the call
    /// proceeds, and `ticket` now holds the reservation that keeps a second
    /// one out until this has finished.
    ///
    /// Expiry runs first, so a key that aged out re-executes instead of
    /// replaying a stale outcome: a retry that arrives a day late is not the
    /// retry the guarantee exists for.
    pub(super) fn admit(
        store: &Arc<Self>,
        ticket: &mut IdempotencyTicket,
        envelope: &RequestEnvelope,
        now_unix_ms: i64,
    ) -> Option<ResponseEnvelope> {
        let mut ledger = store.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        expire(&mut ledger.records, now_unix_ms);
        if let Some(record) = ledger.records.get(&ticket.scope) {
            if record.input_digest != ticket.input_digest {
                return Some(rebuild(
                    envelope,
                    Vec::new(),
                    ResponseOutcome::Failure {
                        error: ControlError::idempotency_conflict(),
                    },
                ));
            }
            return Some(rebuild(
                envelope,
                record.module_revisions.clone(),
                record.outcome.clone(),
            ));
        }
        if !ledger.in_flight.insert(ticket.scope.clone()) {
            // Retryable on purpose: the caller is told to ask again, and the
            // answer waiting for it then is the first call's own.
            return Some(rebuild(
                envelope,
                Vec::new(),
                ResponseOutcome::Failure {
                    error: ControlError::new(
                        ErrorCode::new(codes::REQUEST_IN_PROGRESS)
                            .expect("static error code is valid"),
                        "a call under this idempotency key is still in flight",
                        true,
                    ),
                },
            ));
        }
        drop(ledger);
        ticket.reservation = Some(Arc::new(ScopeReservation {
            scope: ticket.scope.clone(),
            store: Arc::clone(store),
        }));
        None
    }

    fn release(&self, scope: &IdempotencyScope) {
        self.ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .in_flight
            .remove(scope);
    }

    /// Retain `response` as this ticket's answer, when it is one worth
    /// replaying.
    ///
    /// A retryable failure is deliberately dropped rather than stored; see
    /// the module documentation for why keeping it would be the opposite of
    /// the guarantee.
    pub(super) fn record(
        &self,
        ticket: &IdempotencyTicket,
        response: &ResponseEnvelope,
        now_unix_ms: i64,
    ) {
        if !is_terminal(&response.outcome) {
            return;
        }
        let retained = (&response.outcome, &response.module_revisions);
        let retained_bytes = serde_json::to_vec(&retained)
            .map(|bytes| bytes.len())
            .unwrap_or(usize::MAX);
        if retained_bytes > CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES {
            return;
        }
        let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        make_room(&mut ledger.records);
        ledger.records.insert(
            ticket.scope.clone(),
            IdempotencyRecord {
                input_digest: ticket.input_digest.clone(),
                outcome: response.outcome.clone(),
                module_revisions: response.module_revisions.clone(),
                stored_at_unix_ms: now_unix_ms,
            },
        );
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .records
            .len()
    }

    #[cfg(test)]
    fn in_flight(&self) -> usize {
        self.ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .in_flight
            .len()
    }
}

/// A success or a refusal the caller cannot get past by trying again.
///
/// A retryable error is the whole reason a client sent a key: replaying it
/// would turn one full queue into a full queue for the rest of the window.
fn is_terminal(outcome: &ResponseOutcome) -> bool {
    match outcome {
        ResponseOutcome::Success { .. } => true,
        ResponseOutcome::Failure { error } => !error.retryable,
    }
}

/// The recorded outcome, stamped for the request asking for it now.
///
/// The request ID, protocol version and instance come from the live envelope
/// rather than the record: a replay answers *this* request, and a client that
/// matched responses by the recorded ID would never see its reply.
/// `capture_revision` is `None` because no capture happened this time.
fn rebuild(
    envelope: &RequestEnvelope,
    module_revisions: Vec<ModuleRevision>,
    outcome: ResponseOutcome,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: envelope.protocol_version,
        request_id: envelope.request_id.clone(),
        instance_id: envelope.instance_id.clone(),
        capture_revision: None,
        module_revisions,
        outcome,
        warnings: Vec::new(),
    }
}

fn expire(records: &mut BTreeMap<IdempotencyScope, IdempotencyRecord>, now_unix_ms: i64) {
    records.retain(|_, record| {
        // A record stamped in the future reads as age zero rather than as
        // expired, so a clock that steps backwards cannot flush the store.
        let age_ms = now_unix_ms.saturating_sub(record.stored_at_unix_ms).max(0);
        u64::try_from(age_ms).unwrap_or(u64::MAX) < CONTROL_IDEMPOTENCY_RETENTION_MS
    });
}

/// Evict oldest-first until one more record fits.
///
/// A busy session reaches the entry cap long before anything ages out, and
/// refusing every new key once full would never recover.
fn make_room(records: &mut BTreeMap<IdempotencyScope, IdempotencyRecord>) {
    while records.len() >= CONTROL_IDEMPOTENCY_MAX_ENTRIES {
        let Some(oldest) = records
            .iter()
            .min_by_key(|(scope, record)| (record.stored_at_unix_ms, *scope))
            .map(|(scope, _)| scope.clone())
        else {
            break;
        };
        records.remove(&oldest);
    }
}

#[cfg(test)]
mod tests {
    use quantick_control::{
        error::codes, handshake::CURRENT_PROTOCOL_VERSION, id::RequestId, wire::RequestEnvelope,
    };
    use serde_json::{Value, json};

    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn store() -> Arc<IdempotencyStore> {
        Arc::new(IdempotencyStore::default())
    }

    fn principal(byte: u8) -> PrincipalId {
        PrincipalId::from_bytes([byte; 16])
    }

    fn instance() -> InstanceId {
        InstanceId::from_bytes([7; 16])
    }

    fn envelope(request_id: &str, key: Option<&str>, payload: Value) -> RequestEnvelope {
        RequestEnvelope {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            request_id: RequestId::new(request_id).expect("test request ID is valid"),
            instance_id: instance(),
            capability_id: CapabilityId::new("layout.pane.move").expect("static ID is valid"),
            capability_version: 1,
            expected_revisions: Vec::new(),
            idempotency_key: key.map(|key| {
                IdempotencyKey::new(key.to_owned()).expect("test idempotency key is valid")
            }),
            dry_run: false,
            reason: None,
            payload,
        }
    }

    fn success(envelope: &RequestEnvelope, result: Value) -> ResponseEnvelope {
        rebuild(envelope, Vec::new(), ResponseOutcome::Success { result })
    }

    fn ticket_for(envelope: &RequestEnvelope, principal_byte: u8) -> IdempotencyTicket {
        IdempotencyStore::ticket(&instance(), &principal(principal_byte), envelope)
            .expect("a well-formed envelope has a ticket")
            .expect("the envelope carries a key")
    }

    /// One complete call, in the order the gateway makes it: admit, and if the
    /// call is let through, record its answer and release the key by dropping
    /// the ticket. `Some` is the answer the store gave without the application
    /// ever seeing the request.
    fn serve(
        store: &Arc<IdempotencyStore>,
        envelope: &RequestEnvelope,
        principal_byte: u8,
        response: &ResponseEnvelope,
        now_unix_ms: i64,
    ) -> Option<ResponseEnvelope> {
        let mut ticket = ticket_for(envelope, principal_byte);
        if let Some(answered) = IdempotencyStore::admit(store, &mut ticket, envelope, now_unix_ms) {
            return Some(answered);
        }
        store.record(&ticket, response, now_unix_ms);
        None
    }

    #[test]
    fn an_envelope_without_a_key_has_no_ticket() {
        let request = envelope("no-key", None, json!({ "tab": 1 }));
        assert!(
            IdempotencyStore::ticket(&instance(), &principal(1), &request)
                .expect("a well-formed envelope has a ticket")
                .is_none()
        );
    }

    #[test]
    fn the_same_key_and_input_replays_the_recorded_result() {
        let store = store();
        let first = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        assert!(
            serve(
                &store,
                &first,
                1,
                &success(&first, json!({ "moved": true })),
                NOW
            )
            .is_none(),
            "the first call reaches the application"
        );

        let second = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        let replayed = serve(
            &store,
            &second,
            1,
            &success(&second, json!({ "moved": "again" })),
            NOW,
        )
        .expect("the second call is answered from the store");
        assert_eq!(
            replayed.outcome,
            ResponseOutcome::Success {
                result: json!({ "moved": true })
            },
            "the replay is the first call's answer, not a second execution"
        );
    }

    #[test]
    fn a_replay_answers_the_request_that_asked_and_not_the_one_recorded() {
        let store = store();
        let first = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        serve(
            &store,
            &first,
            1,
            &success(&first, json!({ "moved": true })),
            NOW,
        );

        let second = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        let replayed = serve(&store, &second, 1, &success(&second, json!({})), NOW)
            .expect("the second call replays");
        assert_eq!(replayed.request_id.as_str(), "second");
        assert!(
            replayed.capture_revision.is_none(),
            "a replay captured nothing this time"
        );
    }

    #[test]
    fn the_same_key_with_different_input_is_a_conflict() {
        let store = store();
        let first = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        serve(
            &store,
            &first,
            1,
            &success(&first, json!({ "moved": true })),
            NOW,
        );

        let second = envelope("second", Some("key-1"), json!({ "tab": 2 }));
        let answered = serve(&store, &second, 1, &success(&second, json!({})), NOW)
            .expect("a conflicting key still answers");
        let ResponseOutcome::Failure { error } = answered.outcome else {
            panic!("a conflicting key fails");
        };
        assert_eq!(error.code.as_str(), codes::IDEMPOTENCY_CONFLICT);
        assert!(!error.retryable, "retrying the same conflict cannot help");
    }

    #[test]
    fn one_principal_cannot_replay_anothers_record() {
        let store = store();
        let first = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        serve(
            &store,
            &first,
            1,
            &success(&first, json!({ "moved": true })),
            NOW,
        );

        let stranger = envelope("stranger", Some("key-1"), json!({ "tab": 1 }));
        assert!(
            serve(&store, &stranger, 2, &success(&stranger, json!({})), NOW).is_none(),
            "a second principal presenting the same key executes its own call"
        );
    }

    #[test]
    fn a_second_call_under_a_key_still_in_flight_is_told_to_try_again() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let mut in_flight = ticket_for(&request, 1);
        assert!(
            IdempotencyStore::admit(&store, &mut in_flight, &request, NOW).is_none(),
            "the first call is let through"
        );

        // The retry a client sends *because* the first has not answered yet.
        let retry = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        let mut racing = ticket_for(&retry, 1);
        let answered = IdempotencyStore::admit(&store, &mut racing, &retry, NOW)
            .expect("the racing retry is answered rather than dispatched");
        let ResponseOutcome::Failure { error } = answered.outcome else {
            panic!("a racing retry is refused");
        };
        assert_eq!(error.code.as_str(), codes::REQUEST_IN_PROGRESS);
        assert!(
            error.retryable,
            "the caller is told to ask again, and the first answer will be waiting"
        );
    }

    #[test]
    fn dropping_the_ticket_frees_the_key_for_the_retry_it_invites() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let mut refused = ticket_for(&request, 1);
        assert!(IdempotencyStore::admit(&store, &mut refused, &request, NOW).is_none());
        assert_eq!(store.in_flight(), 1);
        // A dispatch that refused before acting — a full queue, say — records
        // nothing and drops its ticket.
        drop(refused);
        assert_eq!(store.in_flight(), 0, "the key is free again");

        let mut retry = ticket_for(&request, 1);
        assert!(
            IdempotencyStore::admit(&store, &mut retry, &request, NOW).is_none(),
            "the retry reaches the application, because nothing was recorded"
        );
    }

    #[test]
    fn a_retryable_failure_is_never_recorded() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let backpressure = rebuild(
            &request,
            Vec::new(),
            ResponseOutcome::Failure {
                error: ControlError::new(
                    ErrorCode::new(codes::BACKPRESSURE).expect("static error code is valid"),
                    "application request queue is full",
                    true,
                ),
            },
        );
        assert!(serve(&store, &request, 1, &backpressure, NOW).is_none());
        assert_eq!(store.len(), 0);

        let retry = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        assert!(
            serve(&store, &retry, 1, &success(&retry, json!({})), NOW).is_none(),
            "the retry the key was sent for still reaches the application"
        );
    }

    #[test]
    fn a_non_retryable_failure_is_recorded_so_the_refusal_is_stable() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let refusal = rebuild(
            &request,
            Vec::new(),
            ResponseOutcome::Failure {
                error: ControlError::invalid_request("no such tab"),
            },
        );
        assert!(serve(&store, &request, 1, &refusal, NOW).is_none());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn a_record_past_its_retention_window_re_executes() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        serve(
            &store,
            &request,
            1,
            &success(&request, json!({ "moved": true })),
            NOW,
        );

        let expired_at = NOW
            .checked_add(
                i64::try_from(CONTROL_IDEMPOTENCY_RETENTION_MS).expect("the window fits an i64"),
            )
            .expect("the test clock does not overflow");
        let retry = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        assert!(
            serve(&store, &retry, 1, &success(&retry, json!({})), expired_at).is_none(),
            "a key that aged out re-executes rather than replaying a stale answer"
        );
    }

    #[test]
    fn a_clock_that_steps_backwards_does_not_flush_the_store() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        serve(
            &store,
            &request,
            1,
            &success(&request, json!({ "moved": true })),
            NOW,
        );

        let retry = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        let replayed = serve(&store, &retry, 1, &success(&retry, json!({})), NOW - 60_000)
            .expect("the record survives a clock that went backwards");
        assert!(matches!(replayed.outcome, ResponseOutcome::Success { .. }));
    }

    #[test]
    fn the_store_evicts_the_oldest_rather_than_growing_past_its_cap() {
        let store = store();
        let oldest = envelope("oldest", Some("key-oldest"), json!({ "tab": 0 }));
        serve(
            &store,
            &oldest,
            1,
            &success(&oldest, json!({ "moved": true })),
            NOW,
        );
        // One more key than the cap holds: the first
        // `CONTROL_IDEMPOTENCY_MAX_ENTRIES` inserts fill it exactly and evict
        // nothing, so the eviction under test is the one the last insert forces.
        for index in 1..=CONTROL_IDEMPOTENCY_MAX_ENTRIES {
            let request = envelope(
                "filler",
                Some(&format!("key-{index}")),
                json!({ "tab": index }),
            );
            let now = NOW + index as i64;
            serve(&store, &request, 1, &success(&request, json!({})), now);
        }
        assert_eq!(store.len(), CONTROL_IDEMPOTENCY_MAX_ENTRIES);

        let retry = envelope("retry", Some("key-oldest"), json!({ "tab": 0 }));
        assert!(
            serve(&store, &retry, 1, &success(&retry, json!({})), NOW).is_none(),
            "the oldest key lost its guarantee to make room, as documented"
        );
    }

    #[test]
    fn a_result_too_large_to_retain_is_answered_and_not_stored() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let oversized = success(
            &request,
            json!({ "blob": "x".repeat(CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES + 1) }),
        );
        assert!(serve(&store, &request, 1, &oversized, NOW).is_none());
        assert_eq!(store.len(), 0);

        let retry = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        assert!(
            serve(&store, &retry, 1, &success(&retry, json!({})), NOW).is_none(),
            "a retry re-executes, exactly as it does with no key at all"
        );
    }
}
