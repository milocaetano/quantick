//! The retained bundles, and the read that verifies every page of one.
//!
//! Capture is the parent's business; what happens to a bundle once it exists
//! is this file's. It bounds retention by count, bytes and age, refuses a
//! capture that finished after access was withdrawn, and on every chunk read
//! rechecks the grant the bundle aggregated, the cursor and the bundle's own
//! retention -- a resource identifier is an address, never an authorization.
//! Nothing here reaches application state: both the insert and the read run
//! on the response workers, which is why the store is shared rather than owned.

use std::{
    collections::{BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

use quantick_control::{
    canonical::{Sha256Digest, raw_digest},
    cursor::{Page, PageContext, PageCursor, PaginationConsistency},
    error::{ControlError, codes},
    id::{EvidenceId, InstanceId, PermissionId, ResourceId, SnapshotScopeId},
    limits::{
        CONTROL_EVIDENCE_CHUNK_BYTES, CONTROL_EVIDENCE_MAX_BUNDLE_BYTES,
        CONTROL_EVIDENCE_MAX_BUNDLES, CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE,
        CONTROL_EVIDENCE_MAX_TOTAL_BYTES, CONTROL_EVIDENCE_RETENTION_MS,
    },
    wire::{Base64Bytes, WireU64},
};
use serde_json::json;

use super::super::types::{known_error, wire_usize};
use super::{BUNDLE_MEDIA_TYPE, EVIDENCE_RESOURCE_SCOPE_ID, EvidenceChunk, EvidenceChunkPage};

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// The retained bundles of one running instance.
///
/// Shared rather than owned by the application thread: nothing in here needs
/// application state, so both the write (a capture being encoded) and the read
/// (a client paging one) happen on the response workers that already do the
/// serializing. The application thread's only business with the store is
/// emptying it when access is withdrawn.
#[derive(Clone)]
pub(crate) struct EvidenceStore {
    state: Arc<Mutex<StoreState>>,
}

struct StoreState {
    bundles: VecDeque<RetainedBundle>,
    total_bytes: usize,
    max_bundles: usize,
    max_total_bytes: usize,
    max_bundle_bytes: usize,
    retention_ms: u64,
    /// Bumped every time the store is emptied.
    ///
    /// A capture is collected on the application thread and finishes encoding
    /// on a response worker some milliseconds later, so the trader can withdraw
    /// access in between — and an insert that landed after the clear would put
    /// a bundle into a store that was just emptied *because* the grant behind
    /// it was withdrawn, where it would sit for its full retention. Each
    /// capture carries the epoch it was collected under and is dropped if the
    /// store has moved on.
    epoch: u64,
}

/// One capture's bytes, as the store retains and pages them.
///
/// The fields are private and [`RetainedBundle::new`] is the only way in,
/// because three of them are facts *about* the bytes rather than inputs: the
/// digest, the byte count and the chunking. `insert`, `expire`,
/// `evict_to_bounds` and `read` all rely on `encoded_bytes` being the sum of
/// the chunk lengths and on `content_digest` being the hash of their
/// concatenation. When a producer built the struct literal itself, those
/// invariants were established outside the file that depends on them, and the
/// next producer — a second capture kind, an import — would have had to repeat
/// the chunking and the digest by hand, where a stale byte count silently
/// corrupts the store's accounting.
pub(super) struct RetainedBundle {
    evidence_id: EvidenceId,
    resource_id: ResourceId,
    capture_revision: WireU64,
    expires_at_unix_ms: i64,
    content_digest: Sha256Digest,
    encoded_bytes: usize,
    source_scopes: BTreeSet<PermissionId>,
    chunks: Vec<Vec<u8>>,
}

impl RetainedBundle {
    /// A bundle of `bytes`, digested, counted and chunked at
    /// [`CONTROL_EVIDENCE_CHUNK_BYTES`] here rather than by the caller.
    pub(super) fn new(
        evidence_id: EvidenceId,
        resource_id: ResourceId,
        capture_revision: WireU64,
        expires_at_unix_ms: i64,
        source_scopes: BTreeSet<PermissionId>,
        bytes: &[u8],
    ) -> Self {
        Self {
            evidence_id,
            resource_id,
            capture_revision,
            expires_at_unix_ms,
            content_digest: raw_sha256(bytes),
            encoded_bytes: bytes.len(),
            source_scopes,
            chunks: bytes
                .chunks(CONTROL_EVIDENCE_CHUNK_BYTES)
                .map(<[u8]>::to_vec)
                .collect(),
        }
    }

    /// The digest of the whole bundle, as its manifest publishes it.
    pub(super) fn content_digest(&self) -> &Sha256Digest {
        &self.content_digest
    }

    /// The bundle's length in bytes, as its manifest publishes it.
    pub(super) fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }

    /// The digest of each chunk in order, as its manifest publishes them.
    pub(super) fn chunk_digests(&self) -> Vec<Sha256Digest> {
        self.chunks.iter().map(|chunk| raw_sha256(chunk)).collect()
    }
}

impl Default for EvidenceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EvidenceStore {
    pub fn new() -> Self {
        Self::with_bounds(
            CONTROL_EVIDENCE_MAX_BUNDLES,
            CONTROL_EVIDENCE_MAX_TOTAL_BYTES,
            CONTROL_EVIDENCE_MAX_BUNDLE_BYTES,
            CONTROL_EVIDENCE_RETENTION_MS,
        )
    }

    pub fn with_bounds(
        max_bundles: usize,
        max_total_bytes: usize,
        max_bundle_bytes: usize,
        retention_ms: u64,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(StoreState {
                bundles: VecDeque::new(),
                total_bytes: 0,
                max_bundles: max_bundles.max(1),
                max_total_bytes,
                max_bundle_bytes,
                retention_ms,
                epoch: 0,
            })),
        }
    }

    /// Forget every retained bundle. Called when local access is withdrawn and
    /// when the window closes: evidence outliving the door it came through
    /// would be exactly the accumulation the retention bounds exist to stop.
    ///
    /// Bumps the epoch, which is what closes the door on captures still being
    /// encoded elsewhere as well as on the ones already retained.
    pub fn clear(&self) {
        let mut state = self.lock();
        state.bundles.clear();
        state.total_bytes = 0;
        state.epoch = state.epoch.saturating_add(1);
    }

    /// The epoch a capture collected now belongs to.
    pub fn epoch(&self) -> u64 {
        self.lock().epoch
    }

    pub fn retention_ms(&self) -> u64 {
        self.lock().retention_ms
    }

    #[cfg(test)]
    pub fn retained(&self) -> usize {
        self.lock().bundles.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoreState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn insert(
        &self,
        bundle: RetainedBundle,
        collected_under_epoch: u64,
        now_unix_ms: i64,
    ) -> Result<(), ControlError> {
        let mut state = self.lock();
        if state.epoch != collected_under_epoch {
            // Access was withdrawn while this capture was being encoded. The
            // grant it was collected under is gone, so the bundle is not
            // retained and the client is told the resource is not there —
            // which is exactly what it will find if it asks.
            return Err(known_error(
                codes::RESOURCE_GONE,
                "local access was withdrawn while this capture was being encoded",
                false,
            ));
        }
        if bundle.encoded_bytes > state.max_bundle_bytes {
            return Err(known_error(
                codes::BACKPRESSURE,
                "the capture is larger than one retained evidence bundle may be",
                false,
            ));
        }
        state.expire(now_unix_ms);
        state.total_bytes = state.total_bytes.saturating_add(bundle.encoded_bytes);
        state.bundles.push_back(bundle);
        state.evict_to_bounds();
        Ok(())
    }

    /// One page of one retained bundle.
    ///
    /// The grant is rechecked here and not only at dispatch: a resource
    /// identifier is an address, and the scopes a bundle aggregated may have
    /// been taken away since it was made.
    pub fn read(
        &self,
        evidence_id: &EvidenceId,
        cursor: Option<&PageCursor>,
        instance_id: &InstanceId,
        granted_scopes: &BTreeSet<PermissionId>,
        now_unix_ms: i64,
    ) -> Result<EvidenceChunkPage, ControlError> {
        let mut state = self.lock();
        state.expire(now_unix_ms);
        let bundle = state
            .bundles
            .iter()
            // The sweep above walks from the front and stops at the first
            // bundle still alive, which is every bundle only while the clock
            // runs forward. A wall clock can step backwards, and then a
            // retention that has run out sits behind one that has not. So the
            // bundle actually asked for is checked on its own terms too:
            // retention is a promise about this bundle, not about the queue.
            .find(|bundle| {
                &bundle.evidence_id == evidence_id && bundle.expires_at_unix_ms > now_unix_ms
            })
            .ok_or_else(|| {
                known_error(
                    codes::RESOURCE_GONE,
                    "no retained evidence bundle has that identifier",
                    false,
                )
            })?;
        if !bundle.source_scopes.is_subset(granted_scopes) {
            let missing = bundle
                .source_scopes
                .difference(granted_scopes)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let mut error = known_error(
                codes::SCOPE_DENIED,
                "this connection no longer holds every scope the bundle aggregated",
                false,
            );
            error.context.details = Some(json!({ "missing_permissions": missing }));
            error.context.next_steps =
                vec!["Enable the required read scopes in Quantick, then reconnect.".to_owned()];
            return Err(error);
        }

        let scope_id = SnapshotScopeId::new(EVIDENCE_RESOURCE_SCOPE_ID)
            .expect("static resource scope ID is valid");
        let query = json!({ "evidence_id": evidence_id.as_str() });
        let context = PageContext {
            instance_id,
            scope_id: &scope_id,
            query: &query,
            consistency_mode: PaginationConsistency::RetainedResource,
            consistency_revision: bundle.capture_revision,
            high_water_position: None,
            resource_id: Some(&bundle.resource_id),
            resource_available: true,
        };
        let start = match cursor {
            Some(cursor) => {
                cursor.validate_next(&context)?;
                usize::try_from(cursor.next_position.get()).unwrap_or(usize::MAX)
            }
            None => 0,
        };
        if start > bundle.chunks.len() {
            return Err(known_error(
                codes::CURSOR_INVALID,
                "the cursor names a chunk past the end of the bundle",
                false,
            ));
        }
        let end = start
            .saturating_add(CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE)
            .min(bundle.chunks.len());
        let mut byte_offset = start.saturating_mul(CONTROL_EVIDENCE_CHUNK_BYTES);
        let mut items = Vec::with_capacity(end.saturating_sub(start));
        for (index, chunk) in bundle.chunks.iter().enumerate().take(end).skip(start) {
            items.push(EvidenceChunk {
                index,
                byte_offset: wire_usize(byte_offset),
                byte_length: wire_usize(chunk.len()),
                digest: raw_sha256(chunk),
                data: Base64Bytes::from_bytes(chunk),
            });
            byte_offset = byte_offset.saturating_add(chunk.len());
        }
        let next_cursor = (end < bundle.chunks.len())
            .then(|| PageCursor::first(&context, wire_usize(end)))
            .transpose()?;
        Ok(EvidenceChunkPage {
            evidence_id: bundle.evidence_id.clone(),
            resource_id: bundle.resource_id.clone(),
            content_digest: bundle.content_digest.clone(),
            media_type: BUNDLE_MEDIA_TYPE.to_owned(),
            encoded_bytes: wire_usize(bundle.encoded_bytes),
            chunk_count: bundle.chunks.len(),
            expires_at_unix_ms: bundle.expires_at_unix_ms,
            page: Page::new(items, next_cursor)?,
        })
    }
}

impl StoreState {
    /// Drop every bundle whose retention has run out — all of them, not the
    /// run at the front.
    ///
    /// The deque is not ordered by deadline and cannot be: response workers
    /// insert concurrently, so two captures can land out of the order they
    /// were collected in, and a wall clock can step backwards besides. A sweep
    /// that stopped at the first live bundle would leave an expired one behind
    /// it holding a slot and its bytes, and the next count or byte eviction
    /// would then drop a *live* bundle to make room for a dead one.
    fn expire(&mut self, now_unix_ms: i64) {
        let reclaimed: usize = self
            .bundles
            .iter()
            .filter(|bundle| bundle.expires_at_unix_ms <= now_unix_ms)
            .map(|bundle| bundle.encoded_bytes)
            .sum();
        self.total_bytes = self.total_bytes.saturating_sub(reclaimed);
        self.bundles
            .retain(|bundle| bundle.expires_at_unix_ms > now_unix_ms);
    }

    fn evict_to_bounds(&mut self) {
        while self.bundles.len() > self.max_bundles
            || (self.total_bytes > self.max_total_bytes && self.bundles.len() > 1)
        {
            self.drop_front();
        }
    }

    fn drop_front(&mut self) {
        if let Some(dropped) = self.bundles.pop_front() {
            self.total_bytes = self.total_bytes.saturating_sub(dropped.encoded_bytes);
        }
    }
}

pub(super) fn raw_sha256(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::new(raw_digest(bytes)).expect("a raw digest is always well formed")
}

#[cfg(test)]
mod tests {
    use super::super::{EVIDENCE_PERMISSION_ID, permission};
    use super::*;

    fn instance() -> InstanceId {
        InstanceId::from_bytes([7; 16])
    }

    fn evidence_id(seed: u8) -> EvidenceId {
        EvidenceId::from_bytes([seed; 16])
    }

    fn scopes(ids: &[&str]) -> BTreeSet<PermissionId> {
        ids.iter().map(|id| permission(id)).collect()
    }

    /// One retained bundle of `bytes` bytes, expiring `retention_ms` after
    /// `captured_at_unix_ms`, chunked exactly as a real capture would be.
    fn bundle(
        seed: u8,
        bytes: usize,
        captured_at_unix_ms: i64,
        retention_ms: i64,
    ) -> RetainedBundle {
        RetainedBundle::new(
            evidence_id(seed),
            ResourceId::from_bytes([seed; 16]),
            WireU64::new(u64::from(seed)),
            captured_at_unix_ms + retention_ms,
            scopes(&["observe", EVIDENCE_PERMISSION_ID]),
            &vec![b'q'; bytes],
        )
    }

    #[test]
    fn retention_evicts_by_the_earlier_of_count_bytes_and_age() {
        let store = EvidenceStore::with_bounds(2, 8_192, 8_192, 1_000);
        store
            .insert(bundle(1, 16, 0, 1_000), store.epoch(), 0)
            .unwrap();
        store
            .insert(bundle(2, 16, 0, 1_000), store.epoch(), 0)
            .unwrap();
        store
            .insert(bundle(3, 16, 0, 1_000), store.epoch(), 0)
            .unwrap();
        assert_eq!(store.retained(), 2, "the count bound evicted the oldest");
        assert_eq!(
            store
                .read(
                    &evidence_id(1),
                    None,
                    &instance(),
                    &scopes(&["observe", EVIDENCE_PERMISSION_ID]),
                    0
                )
                .unwrap_err()
                .code
                .as_str(),
            codes::RESOURCE_GONE
        );

        let store = EvidenceStore::with_bounds(8, 64, 8_192, 1_000);
        store
            .insert(bundle(1, 48, 0, 1_000), store.epoch(), 0)
            .unwrap();
        store
            .insert(bundle(2, 48, 0, 1_000), store.epoch(), 0)
            .unwrap();
        assert_eq!(store.retained(), 1, "the byte bound evicted the oldest");

        let store = EvidenceStore::with_bounds(8, 8_192, 8_192, 1_000);
        store
            .insert(bundle(1, 16, 0, 1_000), store.epoch(), 0)
            .unwrap();
        assert_eq!(
            store
                .read(
                    &evidence_id(1),
                    None,
                    &instance(),
                    &scopes(&["observe", EVIDENCE_PERMISSION_ID]),
                    5_000
                )
                .unwrap_err()
                .code
                .as_str(),
            codes::RESOURCE_GONE,
            "and age evicts one the other two bounds would have kept"
        );
        assert_eq!(store.retained(), 0);
    }

    /// Retention is a promise about one bundle, not about the queue it sits
    /// in. A wall clock that steps backwards can leave an expired bundle
    /// behind a live one, where a front-to-back sweep never reaches it; the
    /// read refuses it anyway.
    #[test]
    fn a_bundle_past_its_retention_is_gone_even_when_it_is_not_at_the_front() {
        let granted = scopes(&["observe", EVIDENCE_PERMISSION_ID]);
        let store = EvidenceStore::with_bounds(8, 1 << 20, 1 << 20, 1_000);
        // Captured in an order the clock disagrees with: the first bundle
        // outlives the second.
        store
            .insert(bundle(1, 16, 0, 10_000), store.epoch(), 0)
            .unwrap();
        store
            .insert(bundle(2, 16, 0, 1_000), store.epoch(), 0)
            .unwrap();

        // At 5 s the front is still alive, so the sweep stops there.
        assert!(
            store
                .read(&evidence_id(1), None, &instance(), &granted, 5_000)
                .is_ok()
        );
        assert_eq!(
            store
                .read(&evidence_id(2), None, &instance(), &granted, 5_000)
                .unwrap_err()
                .code
                .as_str(),
            codes::RESOURCE_GONE,
            "the one behind it has run out of retention and is refused"
        );
    }

    /// A capture too large for one bundle's share of the store is refused, not
    /// admitted at the cost of every bundle already there.
    #[test]
    fn a_bundle_larger_than_its_own_share_is_refused_instead_of_emptying_the_store() {
        let store = EvidenceStore::with_bounds(4, 8_192, 64, 1_000);
        store
            .insert(bundle(1, 32, 0, 1_000), store.epoch(), 0)
            .unwrap();
        let error = store
            .insert(bundle(2, 4_096, 0, 1_000), store.epoch(), 0)
            .unwrap_err();
        assert_eq!(error.code.as_str(), codes::BACKPRESSURE);
        assert_eq!(store.retained(), 1, "the bundle already retained is intact");
    }

    #[test]
    fn a_bundle_is_paged_by_its_cursor_and_a_foreign_cursor_is_refused() {
        let granted = scopes(&["observe", EVIDENCE_PERMISSION_ID]);
        let store = EvidenceStore::with_bounds(4, 1 << 30, 1 << 30, 60_000);
        // Six chunks: two pages of four and two.
        let total = CONTROL_EVIDENCE_CHUNK_BYTES * 5 + 11;
        store
            .insert(bundle(1, total, 0, 60_000), store.epoch(), 0)
            .unwrap();
        store
            .insert(bundle(2, total, 0, 60_000), store.epoch(), 0)
            .unwrap();

        let first = store
            .read(&evidence_id(1), None, &instance(), &granted, 0)
            .unwrap();
        assert_eq!(first.chunk_count, 6);
        assert_eq!(first.page.item_count, CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE);
        assert!(first.page.has_more);
        let cursor = first.page.next_cursor.clone().expect("more chunks remain");

        let second = store
            .read(&evidence_id(1), Some(&cursor), &instance(), &granted, 0)
            .unwrap();
        assert_eq!(second.page.item_count, 2);
        assert!(!second.page.has_more);

        let reassembled = first
            .page
            .items
            .iter()
            .chain(second.page.items.iter())
            .flat_map(|chunk| chunk.data.decode().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(reassembled.len(), total);
        assert_eq!(raw_sha256(&reassembled), first.content_digest);

        // A cursor is bound to the resource it was issued for.
        assert_eq!(
            store
                .read(&evidence_id(2), Some(&cursor), &instance(), &granted, 0)
                .unwrap_err()
                .code
                .as_str(),
            codes::CURSOR_INVALID
        );
    }

    /// The identifier is an address, not an authorization: a grant that lost
    /// a scope the bundle aggregated closes the resource mid-read.
    #[test]
    fn a_chunk_read_rechecks_the_grant_the_bundle_aggregated() {
        let store = EvidenceStore::with_bounds(4, 1 << 20, 1 << 20, 60_000);
        let mut retained = bundle(1, 32, 0, 60_000);
        retained.source_scopes = scopes(&["observe", EVIDENCE_PERMISSION_ID, "observe.paper"]);
        store.insert(retained, store.epoch(), 0).unwrap();

        let error = store
            .read(
                &evidence_id(1),
                None,
                &instance(),
                &scopes(&["observe", EVIDENCE_PERMISSION_ID]),
                0,
            )
            .unwrap_err();
        assert_eq!(error.code.as_str(), codes::SCOPE_DENIED);
        assert!(
            error.context.details.unwrap()["missing_permissions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|permission| permission == "observe.paper")
        );
    }

    #[test]
    fn withdrawing_access_forgets_every_retained_bundle() {
        let store = EvidenceStore::with_bounds(4, 1 << 20, 1 << 20, 60_000);
        let epoch = store.epoch();
        store.insert(bundle(1, 32, 0, 60_000), epoch, 0).unwrap();
        store.clear();
        assert_eq!(store.retained(), 0);
    }

    /// A capture still encoding when access is withdrawn is not retained.
    ///
    /// The ingredients are collected on the application thread and the bundle
    /// is built on a response worker some milliseconds later, so the trader
    /// can disable local access in between. An insert that landed after the
    /// clear would put a bundle into a store emptied *because* the grant
    /// behind it was withdrawn, where it would sit for its whole retention.
    #[test]
    fn a_capture_that_finishes_after_access_is_withdrawn_is_not_retained() {
        let store = EvidenceStore::with_bounds(4, 1 << 20, 1 << 20, 60_000);
        // Collected under the epoch of the moment it was asked for.
        let collected_under = store.epoch();
        // The trader disables access while it encodes.
        store.clear();

        let error = store
            .insert(bundle(1, 32, 0, 60_000), collected_under, 0)
            .unwrap_err();
        assert_eq!(error.code.as_str(), codes::RESOURCE_GONE);
        assert_eq!(store.retained(), 0, "and nothing was retained");

        // A capture collected after the withdrawal is retained normally.
        let epoch = store.epoch();
        store.insert(bundle(2, 32, 0, 60_000), epoch, 0).unwrap();
        assert_eq!(store.retained(), 1);
    }

    /// Retention is per bundle, and the sweep reaches all of them.
    ///
    /// The deque is not ordered by deadline — response workers insert
    /// concurrently and a wall clock can step backwards — so a sweep that
    /// stopped at the first live bundle would leave a dead one behind it
    /// holding a slot, and the next eviction would drop a *live* bundle to
    /// make room for it.
    #[test]
    fn an_expired_bundle_behind_a_live_one_is_swept_and_gives_its_bytes_back() {
        let store = EvidenceStore::with_bounds(8, 1 << 20, 1 << 20, 1_000);
        let epoch = store.epoch();
        // Inserted in an order the clock disagrees with.
        store.insert(bundle(1, 64, 0, 10_000), epoch, 0).unwrap();
        store.insert(bundle(2, 64, 0, 1_000), epoch, 0).unwrap();
        store.insert(bundle(3, 64, 0, 10_000), epoch, 0).unwrap();
        assert_eq!(store.retained(), 3);

        // At five seconds only the middle one has run out of retention.
        store
            .insert(bundle(4, 64, 0, 10_000), epoch, 5_000)
            .unwrap();
        assert_eq!(
            store.retained(),
            3,
            "the expired bundle went; the live ones on either side stayed"
        );
        let granted = scopes(&["observe", EVIDENCE_PERMISSION_ID]);
        for id in [1, 3, 4] {
            assert!(
                store
                    .read(&evidence_id(id), None, &instance(), &granted, 5_000)
                    .is_ok(),
                "bundle {id} should still be readable"
            );
        }
        assert_eq!(
            store.lock().total_bytes,
            3 * 64,
            "and the swept bundle gave its bytes back"
        );
    }
}
