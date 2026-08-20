use quantick_control::{
    cursor::{
        EventCursor, EventJournalBounds, EventStart, Page, PageContext, PageCursor,
        PaginationConsistency, resolve_event_read,
    },
    error::codes,
    id::{InstanceId, ResourceId, SnapshotScopeId},
    wire::WireU64,
};
use serde_json::json;

#[test]
fn later_page_cannot_change_its_query() {
    let instance = InstanceId::from_bytes([1; 16]);
    let scope = SnapshotScopeId::new("chart.history").unwrap();
    let first_query = json!({"symbol": "TEST", "side": "both"});
    let first = PageContext {
        instance_id: &instance,
        scope_id: &scope,
        query: &first_query,
        consistency_mode: PaginationConsistency::RevisionLocked,
        consistency_revision: WireU64::new(4),
        high_water_position: None,
        resource_id: None,
        resource_available: true,
    };
    let cursor = PageCursor::first(&first, WireU64::new(20)).unwrap();
    let changed_query = json!({"symbol": "OTHER", "side": "both"});
    let changed = PageContext {
        query: &changed_query,
        ..first
    };
    assert_eq!(
        cursor.validate_next(&changed).unwrap_err().code.as_str(),
        codes::CURSOR_INVALID
    );
}

#[test]
fn append_only_accepts_later_appends_but_rejects_prior_corrections() {
    let instance = InstanceId::from_bytes([1; 16]);
    let scope = SnapshotScopeId::new("chart.history").unwrap();
    let query = json!({"symbol": "TEST"});
    let first = PageContext {
        instance_id: &instance,
        scope_id: &scope,
        query: &query,
        consistency_mode: PaginationConsistency::AppendOnly,
        consistency_revision: WireU64::new(7),
        high_water_position: Some(WireU64::new(100)),
        resource_id: None,
        resource_available: true,
    };
    let cursor = PageCursor::first(&first, WireU64::new(20)).unwrap();

    // A later append does not change the content-generation revision or the
    // cursor's fixed high-water boundary.
    cursor.validate_next(&first).unwrap();

    let corrected_before_high_water = PageContext {
        consistency_revision: WireU64::new(8),
        ..first
    };
    assert_eq!(
        cursor
            .validate_next(&corrected_before_high_water)
            .unwrap_err()
            .code
            .as_str(),
        codes::PAGE_STALE
    );
}

#[test]
fn event_cursor_requires_an_explicit_start_and_reports_retention_loss() {
    let instance = InstanceId::from_bytes([1; 16]);
    let bounds = EventJournalBounds {
        oldest_sequence: WireU64::new(10),
        next_sequence: WireU64::new(20),
    };
    assert_eq!(
        resolve_event_read(&instance, None, None, bounds)
            .unwrap_err()
            .code
            .as_str(),
        codes::CURSOR_INVALID
    );
    assert_eq!(
        resolve_event_read(&instance, None, Some(EventStart::Latest), bounds)
            .unwrap()
            .next_sequence,
        WireU64::new(20)
    );

    let stale = EventCursor {
        instance_id: instance.clone(),
        next_sequence: WireU64::new(4),
    };
    let resumed = resolve_event_read(&instance, Some(&stale), None, bounds).unwrap();
    assert_eq!(resumed.next_sequence, WireU64::new(10));
    assert_eq!(
        resumed.dropped_before.unwrap().next_sequence,
        WireU64::new(10)
    );
}

#[test]
fn retained_resource_must_exist_before_a_cursor_is_issued() {
    let instance = InstanceId::from_bytes([1; 16]);
    let scope = SnapshotScopeId::new("evidence.resource").unwrap();
    let resource = ResourceId::from_bytes([2; 16]);
    let query = json!({});
    let expired = PageContext {
        instance_id: &instance,
        scope_id: &scope,
        query: &query,
        consistency_mode: PaginationConsistency::RetainedResource,
        consistency_revision: WireU64::new(1),
        high_water_position: None,
        resource_id: Some(&resource),
        resource_available: false,
    };
    assert_eq!(
        PageCursor::first(&expired, WireU64::new(0))
            .unwrap_err()
            .code
            .as_str(),
        codes::RESOURCE_GONE
    );
}

#[test]
fn page_metadata_and_item_count_are_bounded_and_self_consistent() {
    let page = Page::<u8>::new(vec![1, 2], None).unwrap();
    page.validate().unwrap();

    let inconsistent = Page {
        items: vec![1],
        item_count: 2,
        has_more: false,
        next_cursor: None,
    };
    assert!(inconsistent.validate().is_err());
    assert!(
        Page::<u8>::new(
            vec![0; quantick_control::limits::CONTROL_MAX_PAGE_ITEMS + 1],
            None
        )
        .is_err()
    );
}
