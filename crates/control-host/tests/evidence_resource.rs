//! A non-desktop producer and reader use the public retained-resource port.
//!
//! The producer supplies a fixed canonical JSON string, not application state.
//! Expected hashes were computed independently with .NET SHA256, never with
//! the implementation under test. This is a fixture, not a production host.

use std::collections::BTreeSet;

use quantick_control::{
    cursor::PageCursor,
    error::codes,
    id::{EvidenceId, InstanceId, PermissionId, ResourceId},
    wire::WireU64,
};
use quantick_control_host::evidence::{
    BUNDLE_MEDIA_TYPE, EVIDENCE_RESOURCE_SCOPE_ID, EvidenceChunkPage, EvidenceStore, RetainedBundle,
};
use serde_json::json;

const CONTENT_DIGEST: &str =
    "sha256:3d3f0ebfcc75561800db08add23b5b5da4a7747d2e8831d7f43790eab372290c";
const FIRST_DIGEST: &str =
    "sha256:a5776a8975bb699ab00657ae1b7058057cfdff210d7b2b28b77355bb61569140";
const MIDDLE_DIGEST: &str =
    "sha256:971c42be2544f01e31b726475ce57a67ec6b9a869a1aa084f8d4519819af0a84";
const LAST_DIGEST: &str = "sha256:90cd1bea3d038db47aa0324b8797f54ba71e36d77393734ac789c9c1578595e4";

fn canonical_bytes() -> Vec<u8> {
    let mut bytes = vec![b'a'; 1_000_000];
    bytes[0] = b'"';
    bytes[999_999] = b'"';
    bytes
}

fn granted() -> BTreeSet<PermissionId> {
    ["observe", "observe.evidence", "observe.paper"]
        .map(|id| PermissionId::new(id).unwrap())
        .into_iter()
        .collect()
}

fn instance() -> InstanceId {
    InstanceId::from_bytes([11; 16])
}

fn evidence_id(seed: u8) -> EvidenceId {
    EvidenceId::from_bytes([seed; 16])
}

fn produced_bundle(seed: u8, bytes: &[u8]) -> RetainedBundle {
    RetainedBundle::new(
        evidence_id(seed),
        ResourceId::from_bytes([seed; 16]),
        WireU64::new(17),
        60_000,
        granted(),
        bytes,
    )
}

fn retained_store() -> EvidenceStore {
    let store = EvidenceStore::default();
    store
        .insert(produced_bundle(1, &canonical_bytes()), store.epoch(), 0)
        .unwrap();
    store
}

fn first_cursor(store: &EvidenceStore) -> PageCursor {
    store
        .read(&evidence_id(1), None, &instance(), &granted(), 0)
        .unwrap()
        .page
        .next_cursor
        .expect("the fixed producer emits more than one page")
}

#[test]
fn independent_bytes_have_fixed_hashes_and_reassemble_through_the_public_port() {
    let bytes = canonical_bytes();
    let bundle = produced_bundle(1, &bytes);
    assert_eq!(bundle.encoded_bytes(), 1_000_000);
    assert_eq!(bundle.content_digest().as_str(), CONTENT_DIGEST);
    let expected = [
        FIRST_DIGEST,
        MIDDLE_DIGEST,
        MIDDLE_DIGEST,
        MIDDLE_DIGEST,
        MIDDLE_DIGEST,
        LAST_DIGEST,
    ];
    assert_eq!(
        bundle
            .chunk_digests()
            .iter()
            .map(|d| d.as_str())
            .collect::<Vec<_>>(),
        expected
    );

    let store = EvidenceStore::new();
    store.insert(bundle, store.epoch(), 0).unwrap();
    // A response worker sees the exact same retained resource through a clone.
    let reader = store.clone();
    let first = reader
        .read(&evidence_id(1), None, &instance(), &granted(), 1)
        .unwrap();
    assert_eq!(first.evidence_id, evidence_id(1));
    assert_eq!(first.resource_id, ResourceId::from_bytes([1; 16]));
    assert_eq!(first.content_digest.as_str(), CONTENT_DIGEST);
    assert_eq!(first.media_type, BUNDLE_MEDIA_TYPE);
    assert_eq!(first.encoded_bytes.get(), 1_000_000);
    assert_eq!(first.chunk_count, 6);
    assert_eq!(first.expires_at_unix_ms, 60_000);
    assert_eq!(first.page.item_count, 4);
    assert!(first.page.has_more);
    let cursor = first.page.next_cursor.as_ref().unwrap();
    assert_eq!(cursor.scope_id.as_str(), EVIDENCE_RESOURCE_SCOPE_ID);
    assert_eq!(cursor.next_position.get(), 4);
    let second = reader
        .read(&evidence_id(1), Some(cursor), &instance(), &granted(), 2)
        .unwrap();
    assert_eq!(second.page.item_count, 2);
    assert!(!second.page.has_more);
    assert!(second.page.next_cursor.is_none());
    assert_eq!(second.content_digest, first.content_digest);

    let mut reassembled = Vec::new();
    for (index, chunk) in first
        .page
        .items
        .iter()
        .chain(&second.page.items)
        .enumerate()
    {
        assert_eq!(chunk.index, index);
        assert_eq!(chunk.byte_offset.get(), (index * 196_608) as u64);
        let decoded = chunk.data.decode().unwrap();
        assert_eq!(chunk.byte_length.get(), decoded.len() as u64);
        assert_eq!(chunk.digest.as_str(), expected[index]);
        reassembled.extend(decoded);
    }
    assert_eq!(reassembled, bytes);
    let wire = serde_json::to_value(&second).unwrap();
    assert_eq!(wire["encoded_bytes"], "1000000");
    assert_eq!(wire["page"]["items"][1]["byte_length"], "16960");
    assert_eq!(
        serde_json::from_value::<EvidenceChunkPage>(wire).unwrap(),
        second
    );
}

#[test]
fn a_public_reader_cannot_rebind_a_cursor_or_read_past_the_resource() {
    let store = retained_store();
    let cursor = first_cursor(&store);
    store
        .insert(produced_bundle(2, b"{}"), store.epoch(), 0)
        .unwrap();
    let foreign_query = store
        .read(&evidence_id(2), Some(&cursor), &instance(), &granted(), 0)
        .unwrap_err();
    assert_eq!(foreign_query.code.as_str(), codes::CURSOR_INVALID);
    let foreign_instance = store
        .read(
            &evidence_id(1),
            Some(&cursor),
            &InstanceId::from_bytes([12; 16]),
            &granted(),
            0,
        )
        .unwrap_err();
    assert_eq!(foreign_instance.code.as_str(), codes::CURSOR_INVALID);

    let mut foreign_resource = cursor.clone();
    foreign_resource.resource_id = Some(ResourceId::from_bytes([2; 16]));
    let mut past_end = cursor.clone();
    past_end.next_position = WireU64::new(u64::MAX);
    let mut stale = cursor;
    stale.consistency_revision = WireU64::new(18);
    for (cursor, code, retryable) in [
        (foreign_resource, codes::CURSOR_INVALID, false),
        (past_end, codes::CURSOR_INVALID, false),
        (stale, codes::PAGE_STALE, true),
    ] {
        let error = store
            .read(&evidence_id(1), Some(&cursor), &instance(), &granted(), 0)
            .unwrap_err();
        assert_eq!(error.code.as_str(), code);
        assert_eq!(error.retryable, retryable);
    }
}

#[test]
fn a_public_page_reader_must_still_hold_the_aggregated_grant() {
    let store = retained_store();
    let cursor = first_cursor(&store);
    let mut reduced = granted();
    reduced.remove(&PermissionId::new("observe.paper").unwrap());
    let error = store
        .read(&evidence_id(1), Some(&cursor), &instance(), &reduced, 1)
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({
            "code": "control.scope_denied",
            "message": "this connection no longer holds every scope the bundle aggregated",
            "retryable": false,
            "details": { "missing_permissions": ["observe.paper"] },
            "next_steps": ["Enable the required read scopes in Quantick, then reconnect."]
        })
    );
    assert!(
        store
            .read(&evidence_id(1), Some(&cursor), &instance(), &granted(), 1)
            .is_ok()
    );
}

#[test]
fn public_expiry_and_withdrawal_refuse_old_pages_and_in_flight_captures() {
    let store = retained_store();
    assert_eq!(store.retained(), 1);
    let cursor = first_cursor(&store);
    let expired = store
        .read(
            &evidence_id(1),
            Some(&cursor),
            &instance(),
            &granted(),
            60_000,
        )
        .unwrap_err();
    assert_eq!(expired.code.as_str(), codes::RESOURCE_GONE);
    assert!(!expired.retryable);
    assert_eq!(store.retained(), 0);

    let store = retained_store();
    let cursor = first_cursor(&store);
    let epoch = store.epoch();
    let in_flight = produced_bundle(2, b"{}");
    let worker = store.clone();
    store.clear();
    assert_eq!(worker.epoch(), epoch + 1);
    assert_eq!(worker.retained(), 0);
    let gone = worker
        .read(&evidence_id(1), Some(&cursor), &instance(), &granted(), 1)
        .unwrap_err();
    assert_eq!(gone.code.as_str(), codes::RESOURCE_GONE);
    assert!(!gone.retryable);
    let late = worker.insert(in_flight, epoch, 1).unwrap_err();
    assert_eq!(late.code.as_str(), codes::RESOURCE_GONE);
    assert!(!late.retryable);
    assert_eq!(worker.retained(), 0);
    assert_eq!(
        worker
            .read(&evidence_id(2), None, &instance(), &granted(), 1)
            .unwrap_err()
            .code
            .as_str(),
        codes::RESOURCE_GONE
    );
    worker
        .insert(produced_bundle(3, b"{}"), worker.epoch(), 1)
        .unwrap();
    assert_eq!(store.retained(), 1);
    assert!(
        store
            .read(&evidence_id(3), None, &instance(), &granted(), 1)
            .is_ok()
    );
}

#[test]
fn retained_count_reports_storage_until_a_caller_requests_an_expiry_sweep() {
    let store = EvidenceStore::new();
    // Insert sweeps existing entries before admitting this already-expired
    // one. The count describes storage, not a live-resource guarantee.
    store
        .insert(produced_bundle(1, b"{}"), store.epoch(), 60_000)
        .unwrap();
    assert_eq!(store.retained(), 1);
    let expired = store
        .read(&evidence_id(1), None, &instance(), &granted(), 60_000)
        .unwrap_err();
    assert_eq!(expired.code.as_str(), codes::RESOURCE_GONE);
    assert_eq!(store.retained(), 0);
}
