use std::collections::VecDeque;

use super::*;
use crate::{
    fake::{FakeLink, RecordedCall},
    link::Instances,
    server::McpServer,
    tools,
};
use quantick_control::{canonical::canonical_json, id::RequestId};

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aZ1kAAAAASUVORK5CYII=";

struct Fixture {
    document: Value,
}

impl Fixture {
    fn new(padding: usize) -> Self {
        let png = Base64Bytes::new(PNG).unwrap().decode().unwrap();
        Self {
            document: json!({
                "evidence_id": EvidenceId::from_bytes([3; 16]), "bundle_version": 1,
                "instance_id": instance(), "session_id": ProcessNonce::from_bytes([4; 16]),
                "capture_revision": "7", "captured_at_unix_ms": 100,
                "coverage": {"omitted_scopes": ["session.paper"], "unavailable_fields": [],
                    "not_captured": [{"subject": "screenshot.state_skew", "reason": "one_frame"}], "complete": false},
                "screenshot": {"descriptor": {
                    "capture_revision": "7", "width_px": 1, "height_px": 1,
                    "pixels_per_point": "1", "format": "png",
                    "image_digest": raw_digest(&png), "image_bytes": png.len().to_string(),
                    "control_regions": [], "controls_without_region": []
                }, "image_base64": PNG},
                "padding": "x".repeat(padding)
            }),
        }
    }

    fn link(&self) -> ScriptedLink {
        let bytes = canonical_json(&self.document).unwrap().into_bytes();
        let chunks: Vec<_> = bytes.chunks(CONTROL_EVIDENCE_CHUNK_BYTES).collect();
        let evidence_id = self.document["evidence_id"].clone();
        let resource_id = ResourceId::from_bytes([5; 16]);
        let resource = json!({
            "evidence_id": evidence_id, "resource_id": resource_id,
            "content_digest": raw_digest(&bytes), "media_type": "application/json; charset=utf-8",
            "encoded_bytes": bytes.len().to_string(), "chunk_count": chunks.len(),
            "expires_at_unix_ms": 900100
        });
        let mut manifest = resource.clone();
        for key in [
            "bundle_version",
            "instance_id",
            "session_id",
            "capture_revision",
            "captured_at_unix_ms",
            "coverage",
        ] {
            manifest[key] = self.document[key].clone();
        }
        manifest["manifest_version"] = json!(1);
        manifest["chunk_bytes"] = json!(CONTROL_EVIDENCE_CHUNK_BYTES.to_string());
        manifest["chunk_digests"] = json!(
            chunks
                .iter()
                .map(|chunk| raw_digest(chunk))
                .collect::<Vec<_>>()
        );
        manifest["screenshot"] = self.document["screenshot"]["descriptor"].clone();
        let mut replies = VecDeque::from([Ok(envelope(manifest))]);
        let scope = SnapshotScopeId::new("evidence.bundle").unwrap();
        let id = instance();
        let query = json!({"evidence_id": evidence_id});
        let context = PageContext {
            instance_id: &id,
            scope_id: &scope,
            query: &query,
            consistency_mode: PaginationConsistency::RetainedResource,
            consistency_revision: WireU64::new(7),
            high_water_position: None,
            resource_id: Some(&resource_id),
            resource_available: true,
        };
        for start in (0..chunks.len()).step_by(CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE) {
            let end = (start + CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE).min(chunks.len());
            let items: Vec<Value> = (start..end).map(|index| json!({
                "index": index, "byte_offset": (index * CONTROL_EVIDENCE_CHUNK_BYTES).to_string(),
                "byte_length": chunks[index].len().to_string(), "digest": raw_digest(chunks[index]),
                "data": Base64Bytes::from_bytes(chunks[index])
            })).collect();
            let cursor = (end < chunks.len()).then(|| {
                quantick_control::cursor::PageCursor::first(&context, WireU64::new(end as u64))
                    .unwrap()
            });
            let mut page = resource.clone();
            page["page"] = serde_json::to_value(Page::new(items, cursor).unwrap()).unwrap();
            replies.push_back(Ok(envelope(page)));
        }
        ScriptedLink {
            replies,
            calls: vec![],
        }
    }
}

struct ScriptedLink {
    replies: VecDeque<Result<ResponseEnvelope, ControlError>>,
    calls: Vec<RecordedCall>,
}

impl ControlLink for ScriptedLink {
    fn instances(&mut self) -> Result<Instances, ControlError> {
        panic!("capture must use normal invocation routing")
    }
    fn invoke(
        &mut self,
        instance: Option<&InstanceId>,
        capability_id: &str,
        capability_version: u32,
        payload: Value,
    ) -> Result<ResponseEnvelope, ControlError> {
        assert!(matches!(
            capability_id,
            EVIDENCE_CAPTURE_CAPABILITY | EVIDENCE_READ_CAPABILITY
        ));
        self.calls.push(RecordedCall {
            instance: instance.cloned(),
            capability_id: capability_id.to_owned(),
            capability_version,
            payload,
        });
        self.replies
            .pop_front()
            .expect("capture must not retry or over-read")
    }
}

fn instance() -> InstanceId {
    InstanceId::from_bytes([2; 16])
}

fn envelope(result: Value) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: 1,
        request_id: RequestId::new("fixture").unwrap(),
        instance_id: instance(),
        capture_revision: Some(WireU64::new(7)),
        module_revisions: vec![],
        outcome: ResponseOutcome::Success { result },
        warnings: vec![],
    }
}

fn body(link: &mut ScriptedLink, index: usize) -> &mut Value {
    let ResponseOutcome::Success { result } = &mut link.replies[index].as_mut().unwrap().outcome
    else {
        panic!("success fixture")
    };
    result
}

fn invoke(link: &mut dyn ControlLink) -> ToolResult {
    tools::call(link, CAPTURE_CHART, json!({})).unwrap()
}

fn assert_refused(result: ToolResult) {
    assert!(result.is_error, "expected refusal: {result:?}");
    assert!(
        result
            .content
            .iter()
            .all(|item| matches!(item, Content::Text { .. }))
    );
    assert!(!serde_json::to_string(&result).unwrap().contains(PNG));
}

#[test]
fn capture_returns_one_native_image_and_metadata_without_duplicate_pixels() {
    let mut link = Fixture::new(0).link();
    let result = invoke(&mut link);
    assert!(!result.is_error, "{result:?}");
    let encoded = serde_json::to_value(&result).unwrap();
    assert_eq!(
        encoded["content"][1],
        json!({"type": "image", "data": PNG, "mimeType": "image/png"})
    );
    let metadata = result.structured_content.unwrap();
    assert_eq!(metadata["capture_revision"], "7");
    assert_eq!(
        metadata["coverage"]["not_captured"][0]["subject"],
        "screenshot.state_skew"
    );
    assert_eq!(metadata["image_available"], true);
    assert!(!metadata.to_string().contains(PNG));
    assert!(!encoded["content"][0].to_string().contains(PNG));
    assert_eq!(
        link.calls[0].payload,
        json!({"scopes": ["scene.controls"], "event_limit": 1, "screenshot": true})
    );
    assert_eq!(link.calls[1].instance, Some(instance()));
    assert!(link.replies.is_empty());
}

#[test]
fn capture_reads_multiple_pages_and_pins_every_page_to_the_capturing_instance() {
    let mut link = Fixture::new(CONTROL_EVIDENCE_CHUNK_BYTES * 5).link();
    assert!(!invoke(&mut link).is_error);
    assert_eq!(link.calls.len(), 3);
    assert_eq!(link.calls[2].payload["cursor"]["next_position"], "4");
    assert!(
        link.calls[1..]
            .iter()
            .all(|call| call.instance == Some(instance()))
    );
    assert!(link.replies.is_empty());
}

#[test]
fn absent_image_reports_gateway_gaps_as_an_error_without_inventing_pixels() {
    let mut fixture = Fixture::new(0);
    fixture
        .document
        .as_object_mut()
        .unwrap()
        .remove("screenshot");
    fixture.document["coverage"]["not_captured"] =
        json!([{"subject": "screenshot", "reason": "frame_not_delivered"}]);
    let result = invoke(&mut fixture.link());
    assert_eq!(
        result.structured_content.as_ref().unwrap()["image_available"],
        false
    );
    assert_eq!(
        result.structured_content.as_ref().unwrap()["coverage"]["not_captured"][0]["reason"],
        "frame_not_delivered"
    );
    assert_refused(result);
}

#[test]
fn corrupt_chunk_layout_base64_hash_and_page_metadata_are_rejected() {
    for (path, value) in [
        ("/page/items/0/data", json!("!!")),
        ("/page/items/0/digest", json!(raw_digest(b"wrong"))),
        ("/page/items/0/byte_offset", json!("1")),
        ("/page/items/0/byte_length", json!("1")),
        ("/page/items/0/index", json!(1)),
        ("/page/item_count", json!(0)),
        ("/page/has_more", json!(true)),
        ("/resource_id", json!(ResourceId::from_bytes([8; 16]))),
        ("/expires_at_unix_ms", json!(0)),
    ] {
        let mut link = Fixture::new(0).link();
        *body(&mut link, 1).pointer_mut(path).unwrap() = value;
        assert_refused(invoke(&mut link));
    }
    let mut link = Fixture::new(0).link();
    body(&mut link, 1)["page"]["items"] = json!([]);
    body(&mut link, 1)["page"]["item_count"] = json!(0);
    assert_refused(invoke(&mut link));
}

#[test]
fn corrupt_image_base64_digest_size_revision_and_geometry_are_rejected() {
    for (path, value) in [
        ("/screenshot/image_base64", json!("!!")),
        (
            "/screenshot/descriptor/image_digest",
            json!(raw_digest(b"wrong")),
        ),
        ("/screenshot/descriptor/image_bytes", json!("1")),
        ("/screenshot/descriptor/capture_revision", json!("8")),
        ("/screenshot/descriptor/width_px", json!(2)),
        ("/screenshot/descriptor/height_px", json!(0)),
        ("/screenshot/descriptor/format", json!("jpeg")),
        ("/screenshot/descriptor/pixels_per_point", json!("0")),
    ] {
        let mut fixture = Fixture::new(0);
        *fixture.document.pointer_mut(path).unwrap() = value;
        assert_refused(invoke(&mut fixture.link()));
    }
}

#[test]
fn bundle_and_image_budgets_fail_before_unbounded_work() {
    let mut link = Fixture::new(0).link();
    body(&mut link, 0)["encoded_bytes"] =
        json!((CONTROL_EVIDENCE_MAX_BUNDLE_BYTES + 1).to_string());
    assert_refused(invoke(&mut link));
    assert_eq!(link.calls.len(), 1);
    assert!(decode("AAAA", 1).is_err());
    let mut fixture = Fixture::new(0);
    fixture.document["screenshot"]["descriptor"]["image_bytes"] =
        json!((CONTROL_EVIDENCE_MAX_BUNDLE_BYTES + 1).to_string());
    assert_refused(invoke(&mut fixture.link()));
}

#[test]
fn manifest_document_hash_identity_and_cursor_cannot_be_substituted() {
    let mut link = Fixture::new(0).link();
    let wrong = json!(raw_digest(b"wrong document"));
    body(&mut link, 0)["content_digest"] = wrong.clone();
    body(&mut link, 1)["content_digest"] = wrong;
    assert_refused(invoke(&mut link));
    for index in [0, 1] {
        let mut link = Fixture::new(0).link();
        link.replies[index].as_mut().unwrap().instance_id = InstanceId::from_bytes([9; 16]);
        assert_refused(invoke(&mut link));
    }
    let mut link = Fixture::new(0).link();
    body(&mut link, 0)["session_id"] = json!(ProcessNonce::from_bytes([9; 16]));
    assert_refused(invoke(&mut link));
    for (field, value) in [
        ("next_position", json!("0")),
        ("instance_id", json!(InstanceId::from_bytes([9; 16]))),
    ] {
        let mut link = Fixture::new(CONTROL_EVIDENCE_CHUNK_BYTES * 5).link();
        body(&mut link, 1)["page"]["next_cursor"][field] = value;
        assert_refused(invoke(&mut link));
        assert_eq!(link.calls.len(), 2);
    }
}

#[test]
fn grant_revocation_expiration_and_transport_failure_abort_without_partial_image_or_retry() {
    for code in [
        codes::PERMISSION_DENIED,
        codes::SCOPE_DENIED,
        codes::RESOURCE_GONE,
        codes::TIMEOUT,
    ] {
        for transport in [false, true] {
            let mut link = Fixture::new(CONTROL_EVIDENCE_CHUNK_BYTES * 5).link();
            let error = ControlError::new(ErrorCode::new(code).unwrap(), "fixture refusal", false);
            link.replies[2] = if transport {
                Err(error)
            } else {
                let mut response = envelope(Value::Null);
                response.outcome = ResponseOutcome::Failure { error };
                Ok(response)
            };
            let result = invoke(&mut link);
            assert_eq!(
                result.structured_content.as_ref().unwrap()["error"]["code"],
                code
            );
            assert_refused(result);
            assert_eq!(link.calls.len(), 3);
        }
    }
}

#[test]
fn capture_preserves_ambiguous_instance_routing_and_rejects_extra_arguments() {
    let mut link = FakeLink::default();
    link.add_instance(instance());
    link.add_instance(InstanceId::from_bytes([9; 16]));
    let result = invoke(&mut link);
    assert_eq!(
        result.structured_content.as_ref().unwrap()["error"]["code"],
        codes::INSTANCE_AMBIGUOUS
    );
    assert_refused(result);
    assert!(tools::call(&mut link, CAPTURE_CHART, json!({"screenshot": false})).is_err());
    assert!(link.calls.is_empty());
}

#[test]
fn observer_stdio_returns_image_content_and_exposes_no_write_tools() {
    let mut server = McpServer::new(Box::new(Fixture::new(0).link()), "observer");
    let input = format!(
        "{}\n{}\n{}\n",
        json!({"jsonrpc":"2.0", "id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":CAPTURE_CHART,"arguments":{}}})
    );
    let mut output = vec![];
    server.serve(input.as_bytes(), &mut output).unwrap();
    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines[1]["result"]["content"][1]["type"], "image");
    assert_eq!(lines[1]["result"]["content"][1]["mimeType"], "image/png");
    assert_eq!(lines[1]["result"]["isError"], false);
    for tool in tools::tools("observer") {
        assert!(tool.annotations.read_only_hint);
        assert!(
            !tool.name.contains("trade")
                && !tool.name.contains("order")
                && !tool.name.contains("annotate")
        );
    }
}
