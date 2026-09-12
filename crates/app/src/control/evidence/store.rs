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

pub(super) struct StoreState {
    bundles: VecDeque<RetainedBundle>,
    pub(super) total_bytes: usize,
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

pub(super) struct RetainedBundle {
    pub(super) evidence_id: EvidenceId,
    pub(super) resource_id: ResourceId,
    pub(super) capture_revision: WireU64,
    pub(super) expires_at_unix_ms: i64,
    pub(super) content_digest: Sha256Digest,
    pub(super) encoded_bytes: usize,
    pub(super) source_scopes: BTreeSet<PermissionId>,
    pub(super) chunks: Vec<Vec<u8>>,
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

    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, StoreState> {
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
