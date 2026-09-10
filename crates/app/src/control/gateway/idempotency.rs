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
//! **Retention is bounded, and so is the wait behind it.** The guarantee
//! covers the most recent [`CONTROL_IDEMPOTENCY_MAX_ENTRIES`] keys within
//! [`CONTROL_IDEMPOTENCY_RETENTION_MS`].
//!
//! The wait is the subtler half. A `control.timeout` says the response thread
//! stopped waiting, not that the action did not happen: `execute_on_ui`
//! refuses only a request whose deadline had already passed when it was
//! dequeued, so one dequeued just before it runs in full. A timeout is
//! retryable and therefore never recorded, which would leave a keyed retry
//! free to act a second time. So the thread keeps the reservation and follows
//! the call for one more request window, records what actually happened
//! without sending it, and the retry replays that.
//!
//! That follow-on wait is bounded by the request's own life rather than by a
//! clock, and the difference matters. The sender lives inside the queued
//! `UiRequest`, so the wait ends when the application answers or when the
//! queue is dropped — never later. A wall-clock window was tried instead and
//! was worse: an action slower than the window (a `feed.reload` rebuilding a
//! chart) would free its key with nothing recorded, and the keyed retry would
//! run it a second time, which is the one outcome this module exists to
//! prevent. The only state that parks a thread indefinitely is an application
//! that never finishes a request it accepted, and a frame loop wedged that
//! badly has already cost the trader more than a thread.
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
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
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
    /// The digest travels with the reservation so a key reused over different
    /// input is a conflict while the first call is still running, exactly as
    /// it is once the first call has finished. Telling such a caller to retry
    /// would be sending it back for an answer it can never get.
    in_flight: BTreeMap<IdempotencyScope, Sha256Digest>,
}

/// The records one enabling of the gateway retains.
#[derive(Debug, Default)]
pub(super) struct IdempotencyStore {
    ledger: Mutex<Ledger>,
}

impl IdempotencyStore {
    /// The ledger, or nothing when a panic left it unreadable.
    ///
    /// A poisoned lock means the store's own history is unknown, and this
    /// repository's rule for that is the safe side rather than the convenient
    /// one — `ConnectionSlots::is_in_flight` reads a poisoned lock as "in
    /// flight" for exactly this reason. Here the safe side is to admit
    /// nothing: a keyed call whose history cannot be read might already have
    /// run, and letting it through is the double effect the key exists to
    /// prevent. Calls carrying no key never reach this lock and are untouched.
    fn ledger(&self) -> Option<MutexGuard<'_, Ledger>> {
        self.ledger.lock().ok()
    }

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
        let Some(mut ledger) = store.ledger() else {
            return Some(rebuild(
                envelope,
                Vec::new(),
                ResponseOutcome::Failure {
                    error: ControlError::new(
                        ErrorCode::new(codes::CAPABILITY_UNAVAILABLE)
                            .expect("static error code is valid"),
                        "this instance cannot account for idempotency keys",
                        false,
                    ),
                },
            ));
        };
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
        if let Some(running) = ledger.in_flight.get(&ticket.scope) {
            let error = if running == &ticket.input_digest {
                // Retryable on purpose: the caller is told to ask again, and
                // the answer waiting for it then is the first call's own.
                ControlError::new(
                    ErrorCode::new(codes::REQUEST_IN_PROGRESS).expect("static error code is valid"),
                    "a call under this idempotency key is still in flight",
                    true,
                )
            } else {
                ControlError::idempotency_conflict()
            };
            return Some(rebuild(
                envelope,
                Vec::new(),
                ResponseOutcome::Failure { error },
            ));
        }
        ledger
            .in_flight
            .insert(ticket.scope.clone(), ticket.input_digest.clone());
        drop(ledger);
        ticket.reservation = Some(Arc::new(ScopeReservation {
            scope: ticket.scope.clone(),
            store: Arc::clone(store),
        }));
        None
    }

    /// Drop everything a principal owns, because the connection it was
    /// minted for has gone.
    ///
    /// `principal_id` lives for one handshake, so a closed connection's
    /// records can never be replayed by anyone — but they would still hold
    /// entries against the cap for the whole retention window, and a client
    /// that reconnects a few times would evict the records of the connections
    /// still running. Releasing them at the door keeps the cap meaning what
    /// it says.
    pub(super) fn forget_principal(&self, principal_id: &PrincipalId) {
        let Some(mut ledger) = self.ledger() else {
            return;
        };
        ledger
            .records
            .retain(|scope, _| &scope.principal_id != principal_id);
        ledger
            .in_flight
            .retain(|scope, _| &scope.principal_id != principal_id);
    }

    fn release(&self, scope: &IdempotencyScope) {
        if let Some(mut ledger) = self.ledger() {
            ledger.in_flight.remove(scope);
        }
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
        let Some(mut ledger) = self.ledger() else {
            return;
        };
        // The reservation is this record's licence to exist. `record` runs
        // while the ticket is alive, so in every ordinary path the scope is
        // still reserved — except one: a response worker is detached and
        // nobody joins it, so it can finish after its connection closed and
        // `forget_principal` swept. Writing then would leave a record no
        // client can ever reach, holding a slot against the cap that the
        // sweep exists to free.
        if !ledger.in_flight.contains_key(&ticket.scope) {
            return;
        }
        make_room(&mut ledger.records, &ticket.scope.principal_id);
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
        self.ledger()
            .expect("the test store is readable")
            .records
            .len()
    }

    #[cfg(test)]
    fn in_flight(&self) -> usize {
        self.ledger()
            .expect("the test store is readable")
            .in_flight
            .len()
    }
}

/// The ticket this request proceeds under, or the answer it gets instead.
///
/// `Err` is a complete response and the call never reaches the application: a
/// replay of the recorded outcome, a conflict, or a refusal. `Ok(Some)`
/// proceeds holding its key until the ticket drops; `Ok(None)` is a request
/// that carried no key and is subject to none of this.
///
/// This lives here rather than in the connection loop so the gateway's trunk
/// carries a call to it and not a body — the rule the size ratchet enforces,
/// which caught this very change growing `server.rs` past its threshold.
///
/// The answer is boxed because a `ResponseEnvelope` is far larger than a
/// ticket, and the common return is the ticket; the repository boxes the big
/// side of a lopsided type elsewhere for the same reason
/// (`ControlError::context`, `UiRequest::actor`).
pub(super) fn admitted(
    store: &Arc<IdempotencyStore>,
    instance_id: &InstanceId,
    principal_id: &PrincipalId,
    envelope: &RequestEnvelope,
    now_unix_ms: i64,
) -> Result<Option<IdempotencyTicket>, Box<ResponseEnvelope>> {
    let mut ticket = match IdempotencyStore::ticket(instance_id, principal_id, envelope) {
        Ok(ticket) => ticket,
        Err(error) => {
            return Err(Box::new(rebuild(
                envelope,
                Vec::new(),
                ResponseOutcome::Failure { error },
            )));
        }
    };
    if let Some(held) = ticket.as_mut()
        && let Some(answered) = IdempotencyStore::admit(store, held, envelope, now_unix_ms)
    {
        return Err(Box::new(answered));
    }
    Ok(ticket)
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

/// Evict until one more record fits, taking from whichever principal holds
/// the most.
///
/// A busy session reaches the entry cap long before anything ages out, and
/// refusing every new key once full would never recover. Which record goes is
/// the part worth choosing: oldest-first across the whole store lets one
/// chatty connection spend the cap and silently withdraw the guarantee every
/// other connection was published. Taking from the largest holder — the
/// incoming principal itself when it is the largest, which is the common case
/// — keeps one client's traffic from costing another its retries. Ties go to
/// the oldest record of that principal, so a holder still loses its stalest
/// key rather than an arbitrary one.
fn make_room(records: &mut BTreeMap<IdempotencyScope, IdempotencyRecord>, incoming: &PrincipalId) {
    while records.len() >= CONTROL_IDEMPOTENCY_MAX_ENTRIES {
        let mut held = BTreeMap::<&PrincipalId, usize>::new();
        for scope in records.keys() {
            *held.entry(&scope.principal_id).or_default() += 1;
        }
        // `max_by_key` keeps the last maximum, and the map is ordered, so the
        // tie-break is the highest principal id rather than an arbitrary one;
        // the incoming principal wins its own tie so a client cannot grow past
        // its share by racing another of the same size.
        let Some(fullest) = held
            .into_iter()
            .max_by_key(|(principal, count)| (*count, *principal == incoming, *principal))
            .map(|(principal, _)| principal.clone())
        else {
            break;
        };
        let Some(oldest) = records
            .iter()
            .filter(|(scope, _)| scope.principal_id == fullest)
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
    fn a_key_reused_over_different_input_while_in_flight_is_a_conflict() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let mut in_flight = ticket_for(&request, 1);
        assert!(IdempotencyStore::admit(&store, &mut in_flight, &request, NOW).is_none());

        let different = envelope("second", Some("key-1"), json!({ "tab": 2 }));
        let mut racing = ticket_for(&different, 1);
        let answered = IdempotencyStore::admit(&store, &mut racing, &different, NOW)
            .expect("a conflicting key is answered rather than dispatched");
        let ResponseOutcome::Failure { error } = answered.outcome else {
            panic!("a conflicting key fails");
        };
        assert_eq!(error.code.as_str(), codes::IDEMPOTENCY_CONFLICT);
        assert!(
            !error.retryable,
            "sending this caller back for an answer it can never get is worse than refusing it"
        );
    }

    /// The client was told `TIMEOUT` and the application ran the action
    /// anyway. The outcome is recorded under the key it was sent with, so the
    /// retry replays it instead of doing the same thing a second time.
    #[test]
    fn an_answer_recorded_after_a_timeout_is_what_the_retry_replays() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let mut ticket = ticket_for(&request, 1);
        assert!(IdempotencyStore::admit(&store, &mut ticket, &request, NOW).is_none());

        let timeout = rebuild(
            &request,
            Vec::new(),
            ResponseOutcome::Failure {
                error: ControlError::new(
                    ErrorCode::new(codes::TIMEOUT).expect("static error code is valid"),
                    "request did not complete before its deadline",
                    true,
                ),
            },
        );
        store.record(&ticket, &timeout, NOW);
        assert_eq!(store.len(), 0, "a retryable answer is not the one to keep");

        store.record(&ticket, &success(&request, json!({ "moved": true })), NOW);
        drop(ticket);

        let retry = envelope("second", Some("key-1"), json!({ "tab": 1 }));
        let replayed = serve(&store, &retry, 1, &success(&retry, json!({})), NOW)
            .expect("the retry is answered from the store");
        assert_eq!(
            replayed.outcome,
            ResponseOutcome::Success {
                result: json!({ "moved": true })
            }
        );
    }

    #[test]
    fn a_poisoned_store_admits_nothing_rather_than_risking_a_double_effect() {
        let store = store();
        let poisoner = Arc::clone(&store);
        let _ = std::thread::spawn(move || {
            let _held = poisoner.ledger.lock().expect("the store starts readable");
            panic!("poison the ledger");
        })
        .join();

        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let mut ticket = ticket_for(&request, 1);
        let answered = IdempotencyStore::admit(&store, &mut ticket, &request, NOW)
            .expect("a store that cannot read its own history admits nothing");
        let ResponseOutcome::Failure { error } = answered.outcome else {
            panic!("a keyed call is refused rather than let through");
        };
        assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
        assert!(
            !error.retryable,
            "a poisoned lock does not un-poison, so asking again cannot help"
        );
    }

    /// A response worker is detached and nobody joins it, so it can finish
    /// after its connection closed and the sweep already ran. The record it
    /// would write is one no client can ever reach.
    #[test]
    fn a_record_from_a_worker_that_outlived_its_connection_is_not_written() {
        let store = store();
        let request = envelope("first", Some("key-1"), json!({ "tab": 1 }));
        let mut ticket = ticket_for(&request, 1);
        assert!(IdempotencyStore::admit(&store, &mut ticket, &request, NOW).is_none());

        store.forget_principal(&principal(1));
        store.record(&ticket, &success(&request, json!({ "moved": true })), NOW);

        assert_eq!(
            store.len(),
            0,
            "a record for a connection that is gone holds no slot against the cap"
        );
    }

    /// The cap is shared, so who loses a record when it is reached decides
    /// whether one client can withdraw the guarantee another was published.
    #[test]
    fn a_chatty_connection_cannot_spend_another_connections_retries() {
        let store = store();
        let quiet = envelope("quiet", Some("key-quiet"), json!({ "tab": 0 }));
        serve(
            &store,
            &quiet,
            2,
            &success(&quiet, json!({ "moved": true })),
            NOW,
        );

        // One connection spends the whole cap, and its keys are all newer than
        // the quiet connection's single record — so oldest-first eviction
        // across the store would take the quiet one first.
        for index in 0..=CONTROL_IDEMPOTENCY_MAX_ENTRIES {
            let request = envelope(
                "chatty",
                Some(&format!("key-{index}")),
                json!({ "tab": index }),
            );
            let now = NOW + 1 + index as i64;
            serve(&store, &request, 1, &success(&request, json!({})), now);
        }

        let retry = envelope("retry", Some("key-quiet"), json!({ "tab": 0 }));
        assert!(
            serve(&store, &retry, 2, &success(&retry, json!({})), NOW).is_some(),
            "the quiet connection still has the guarantee it was published"
        );
    }

    #[test]
    fn a_closed_connection_takes_its_records_with_it() {
        let store = store();
        let mine = envelope("mine", Some("key-1"), json!({ "tab": 1 }));
        serve(
            &store,
            &mine,
            1,
            &success(&mine, json!({ "moved": true })),
            NOW,
        );
        let theirs = envelope("theirs", Some("key-1"), json!({ "tab": 1 }));
        serve(
            &store,
            &theirs,
            2,
            &success(&theirs, json!({ "moved": true })),
            NOW,
        );
        assert_eq!(store.len(), 2);

        store.forget_principal(&principal(1));

        assert_eq!(
            store.len(),
            1,
            "only the gone connection's record is dropped"
        );
        let retry = envelope("retry", Some("key-1"), json!({ "tab": 1 }));
        assert!(
            serve(&store, &retry, 2, &success(&retry, json!({})), NOW).is_some(),
            "the connection that is still running keeps its guarantee"
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
