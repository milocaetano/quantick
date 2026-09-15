//! The retained bundles, and the read that verifies every page of one.
//!
//! Capture is the caller's business; what happens to a bundle once it exists
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
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::admission::known_error;

/// The name the page cursor carries for the resource it walks.
///
/// Shaped like a snapshot scope because that is the field the contract's
/// cursor declares, but it names a *retained resource*, not a projection: no
/// module registers it and no capture builds it.
pub const EVIDENCE_RESOURCE_SCOPE_ID: &str = "evidence.bundle";

/// The encoding a reassembled bundle is in.
pub const BUNDLE_MEDIA_TYPE: &str = "application/json; charset=utf-8";

/// Chunks the largest permitted bundle takes.
///
/// Derived from the two limits that decide it rather than written down beside
/// them, so the manifest's declared bound cannot drift from the chunking that
/// produces it.
pub const MAX_CHUNKS_PER_BUNDLE: usize =
    CONTROL_EVIDENCE_MAX_BUNDLE_BYTES.div_ceil(CONTROL_EVIDENCE_CHUNK_BYTES);

/// One page of a retained bundle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceChunkPage {
    pub evidence_id: EvidenceId,
    pub resource_id: ResourceId,
    pub content_digest: Sha256Digest,
    pub media_type: String,
    #[schemars(extend("x-unit" = "bytes"))]
    pub encoded_bytes: WireU64,
    #[schemars(range(max = MAX_CHUNKS_PER_BUNDLE))]
    pub chunk_count: usize,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub expires_at_unix_ms: i64,
    pub page: Page<EvidenceChunk>,
}

/// One byte run of the canonical document.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceChunk {
    pub index: usize,
    #[schemars(extend("x-unit" = "bytes"))]
    pub byte_offset: WireU64,
    #[schemars(extend("x-unit" = "bytes"))]
    pub byte_length: WireU64,
    pub digest: Sha256Digest,
    pub data: Base64Bytes,
}

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
pub struct EvidenceStore {
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
pub struct RetainedBundle {
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
    ///
    /// The producer supplies canonical JSON and every permission it aggregated;
    /// this byte store neither constructs captures nor admits capabilities.
    pub fn new(
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
    pub fn content_digest(&self) -> &Sha256Digest {
        &self.content_digest
    }

    /// The bundle's length in bytes, as its manifest publishes it.
    pub fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }

    /// The digest of each chunk in order, as its manifest publishes them.
    pub fn chunk_digests(&self) -> Vec<Sha256Digest> {
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

    /// Configure retention with the existing host bounds. A count below one is
    /// clamped to one; byte eviction always keeps the last admitted bundle.
    /// Producers set each bundle's deadline using the reported retention time.
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

    /// The physical number of bundles currently retained in the store.
    ///
    /// This does not advance caller-supplied time or expire entries. Only an
    /// insert or read sweeps expiry; a clear forgets every retained entry.
    pub fn retained(&self) -> usize {
        self.lock().bundles.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoreState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Retain one completed capture only if access has not been withdrawn since
    /// its ingredients were collected. The caller supplies time and the epoch
    /// obtained at collection; this store never reads either from the host.
    pub fn insert(
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
            // The full sweep above reclaims every expired bundle. The lookup
            // also states the requested bundle's retention condition explicitly:
            // retention is a promise about this bundle, not about queue order.
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

/// Hash raw bytes using the same canonical digest representation as bundle pages.
///
/// Capture adapters use this for related binary artifacts, such as screenshots.
pub fn raw_sha256(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::new(raw_digest(bytes)).expect("a raw digest is always well formed")
}

fn wire_usize(value: usize) -> WireU64 {
    WireU64::new(u64::try_from(value).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests;
