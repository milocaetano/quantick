use super::*;

const EVIDENCE_PERMISSION_ID: &str = "observe.evidence";

fn permission(id: &str) -> PermissionId {
    PermissionId::new(id).expect("static permission ID is valid")
}

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
fn bundle(seed: u8, bytes: usize, captured_at_unix_ms: i64, retention_ms: i64) -> RetainedBundle {
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

    // At 5 s the front is still alive, but the second bundle has expired.
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
