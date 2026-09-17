//! Rare, bounded observer operation over the existing evidence capabilities.
//! No window access, image encoding, file writes, retries, or authority here.
//! The gateway checks grants and resource lifetime on every page read.

use quantick_control::{
    canonical::{Sha256Digest, raw_digest},
    cursor::{Page, PageContext, PaginationConsistency},
    error::{ControlError, codes},
    id::{ErrorCode, EvidenceId, InstanceId, ProcessNonce, ResourceId, SnapshotScopeId},
    limits::{
        CONTROL_EVIDENCE_CHUNK_BYTES, CONTROL_EVIDENCE_MAX_BUNDLE_BYTES,
        CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE,
    },
    wire::{Base64Bytes, ResponseEnvelope, ResponseOutcome, WireU64},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    link::ControlLink,
    protocol::{Content, Tool, ToolAnnotations, ToolResult},
};

pub const CAPTURE_CHART: &str = "quantick_capture_chart";
pub const EVIDENCE_CAPTURE_CAPABILITY: &str = "evidence.capture";
pub const EVIDENCE_READ_CAPABILITY: &str = "evidence.read";
const MAX_CHUNKS: usize = CONTROL_EVIDENCE_MAX_BUNDLE_BYTES.div_ceil(CONTROL_EVIDENCE_CHUNK_BYTES);
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub(crate) fn tool(input_schema: Value) -> Tool {
    let mut annotations = ToolAnnotations::observer_read("Capture chart image");
    // Each call captures a new frame and displays the gateway's capture notice.
    annotations.idempotent_hint = false;
    Tool {
        name: CAPTURE_CHART.to_owned(),
        title: "Capture the Quantick window as an image".to_owned(),
        description: "Return a native PNG image of the entire Quantick window, including potentially sensitive text and paper-account information visible on screen. Requires the user's observe.evidence and observe.screenshot grants plus the scene's read scopes. Reuses evidence.capture/read and its visible capture notice. Metadata preserves capture revision, control regions and coverage gaps, including screenshot.state_skew: numerical state may be one feed drain newer than the pixels. Does not change charts or place orders. No disk output. If no frame is available, returns an error with coverage and no image.".to_owned(),
        input_schema,
        output_schema: None,
        annotations,
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct Resource {
    evidence_id: EvidenceId,
    resource_id: ResourceId,
    content_digest: Sha256Digest,
    media_type: String,
    encoded_bytes: WireU64,
    chunk_count: usize,
    expires_at_unix_ms: i64,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(flatten)]
    resource: Resource,
    manifest_version: u32,
    bundle_version: u32,
    instance_id: InstanceId,
    session_id: ProcessNonce,
    capture_revision: WireU64,
    captured_at_unix_ms: i64,
    chunk_bytes: WireU64,
    chunk_digests: Vec<Sha256Digest>,
    coverage: Value,
    screenshot: Option<ImageDescriptor>,
}

#[derive(Deserialize)]
struct ChunkPage {
    #[serde(flatten)]
    resource: Resource,
    page: Page<Chunk>,
}

#[derive(Deserialize)]
struct Chunk {
    index: usize,
    byte_offset: WireU64,
    byte_length: WireU64,
    digest: Sha256Digest,
    // Decode only after checking the encoded length against the byte budget.
    data: String,
}

#[derive(Deserialize)]
struct Document {
    evidence_id: EvidenceId,
    bundle_version: u32,
    instance_id: InstanceId,
    session_id: ProcessNonce,
    capture_revision: WireU64,
    captured_at_unix_ms: i64,
    coverage: Value,
    screenshot: Option<Image>,
}

#[derive(Deserialize)]
struct Image {
    descriptor: ImageDescriptor,
    image_base64: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct ImageDescriptor {
    capture_revision: WireU64,
    width_px: u32,
    height_px: u32,
    pixels_per_point: quantick_control::wire::CanonicalDecimal,
    format: String,
    image_digest: Sha256Digest,
    image_bytes: WireU64,
    control_regions: Vec<Value>,
    controls_without_region: Vec<Value>,
}

pub(crate) fn call(link: &mut dyn ControlLink, instance: Option<&InstanceId>) -> ToolResult {
    match capture(link, instance) {
        Ok(result) => result,
        Err(error) => ToolResult::control_error(&error),
    }
}

fn capture(
    link: &mut dyn ControlLink,
    instance: Option<&InstanceId>,
) -> Result<ToolResult, ControlError> {
    let response = link.invoke(
        instance,
        EVIDENCE_CAPTURE_CAPABILITY,
        1,
        json!({
            "scopes": ["scene.controls"], "event_limit": 1, "screenshot": true
        }),
    )?;
    let captured_by = response.instance_id.clone();
    let revision = response.capture_revision;
    let warnings = response.warnings.clone();
    let manifest: Manifest = parse(success(response, instance)?)?;
    manifest.validate(&captured_by, revision)?;
    // Pin subsequent calls to the exact instance that captured the frame.
    let bytes = read_bundle(link, &manifest)?;
    let document: Document = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("evidence document is not valid JSON"))?;
    validate_document(&document, &manifest)?;
    let mut result = ToolResult::structured(json!({
        "instance_id": manifest.instance_id,
        "evidence_id": manifest.resource.evidence_id,
        "resource_id": manifest.resource.resource_id,
        "session_id": manifest.session_id,
        "capture_revision": manifest.capture_revision,
        "captured_at_unix_ms": manifest.captured_at_unix_ms,
        "expires_at_unix_ms": manifest.resource.expires_at_unix_ms,
        "content_digest": manifest.resource.content_digest,
        "image_available": document.screenshot.is_some(),
        "capture_area": "whole_window",
        "coverage": document.coverage,
        "screenshot": manifest.screenshot,
        "warnings": warnings,
    }));
    match document.screenshot {
        Some(image) => {
            validate_image(&image, manifest.capture_revision)?;
            result.content.push(Content::Image {
                data: image.image_base64,
                mime_type: "image/png".to_owned(),
            });
        }
        None => {
            result.is_error = true;
            result.content.insert(0, Content::Text {
                text: "No screenshot was delivered. Inspect coverage.not_captured for the gateway's reason.".to_owned(),
            });
        }
    }
    Ok(result)
}

impl Manifest {
    fn validate(
        &self,
        instance: &InstanceId,
        revision: Option<WireU64>,
    ) -> Result<(), ControlError> {
        if &self.instance_id != instance
            || revision != Some(self.capture_revision)
            || self.manifest_version != 1
            || self.bundle_version != 1
            || self.resource.media_type != "application/json; charset=utf-8"
            || self.resource.expires_at_unix_ms <= self.captured_at_unix_ms
        {
            return Err(invalid(
                "evidence manifest identity, version or lifetime is inconsistent",
            ));
        }
        let size = self.resource.encoded_bytes.get();
        if size == 0 || size > CONTROL_EVIDENCE_MAX_BUNDLE_BYTES as u64 {
            return Err(too_large());
        }
        if self.chunk_bytes.get() != CONTROL_EVIDENCE_CHUNK_BYTES as u64
            || self.resource.chunk_count != (size as usize).div_ceil(CONTROL_EVIDENCE_CHUNK_BYTES)
            || self.resource.chunk_count > MAX_CHUNKS
            || self.chunk_digests.len() != self.resource.chunk_count
        {
            return Err(invalid("evidence chunk layout is inconsistent"));
        }
        Ok(())
    }
}

fn read_bundle(link: &mut dyn ControlLink, manifest: &Manifest) -> Result<Vec<u8>, ControlError> {
    let mut bytes = Vec::with_capacity(manifest.resource.encoded_bytes.get() as usize);
    let mut index = 0;
    let mut cursor = None;
    let query = json!({"evidence_id": manifest.resource.evidence_id});
    let scope = SnapshotScopeId::new("evidence.bundle").expect("static scope is valid");
    let context = PageContext {
        instance_id: &manifest.instance_id,
        scope_id: &scope,
        query: &query,
        consistency_mode: PaginationConsistency::RetainedResource,
        consistency_revision: manifest.capture_revision,
        high_water_position: None,
        resource_id: Some(&manifest.resource.resource_id),
        resource_available: true,
    };
    // Every successful iteration consumes at least one chunk; no polling or retries.
    while index < manifest.resource.chunk_count {
        let mut input = query.clone();
        if let Some(next) = cursor.take() {
            input["cursor"] = next;
        }
        let response = link.invoke(
            Some(&manifest.instance_id),
            EVIDENCE_READ_CAPABILITY,
            1,
            input,
        )?;
        let page: ChunkPage = parse(success(response, Some(&manifest.instance_id))?)?;
        page.page.validate()?;
        if page.resource != manifest.resource
            || page.page.items.is_empty()
            || page.page.items.len() > CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE
            || page.page.items.len() > manifest.resource.chunk_count - index
        {
            return Err(invalid(
                "evidence page identity or chunk count is inconsistent",
            ));
        }
        for chunk in page.page.items {
            append_chunk(&mut bytes, index, chunk, manifest)?;
            index += 1;
        }
        let more = index < manifest.resource.chunk_count;
        if more != page.page.has_more {
            return Err(invalid("evidence page ended at the wrong chunk"));
        }
        if let Some(next) = page.page.next_cursor {
            next.validate_next(&context)?;
            if next.next_position.get() != index as u64 {
                return Err(invalid("evidence cursor did not advance to the next chunk"));
            }
            cursor =
                Some(serde_json::to_value(next).map_err(|_| invalid("invalid evidence cursor"))?);
        }
    }
    if bytes.len() as u64 != manifest.resource.encoded_bytes.get()
        || raw_digest(&bytes) != manifest.resource.content_digest.as_str()
    {
        return Err(invalid("evidence document size or digest mismatch"));
    }
    Ok(bytes)
}

fn append_chunk(
    bytes: &mut Vec<u8>,
    index: usize,
    chunk: Chunk,
    manifest: &Manifest,
) -> Result<(), ControlError> {
    let remaining = manifest.resource.encoded_bytes.get() as usize - bytes.len();
    let expected = remaining.min(CONTROL_EVIDENCE_CHUNK_BYTES);
    if chunk.index != index
        || chunk.byte_offset.get() != bytes.len() as u64
        || chunk.byte_length.get() != expected as u64
        || chunk.digest != manifest.chunk_digests[index]
    {
        return Err(invalid(
            "evidence chunk index, offset, length or digest is inconsistent",
        ));
    }
    let decoded = decode(&chunk.data, expected)?;
    if decoded.len() != expected || raw_digest(&decoded) != chunk.digest.as_str() {
        return Err(invalid("evidence chunk size or digest mismatch"));
    }
    bytes.extend_from_slice(&decoded);
    Ok(())
}

fn validate_document(document: &Document, manifest: &Manifest) -> Result<(), ControlError> {
    if document.evidence_id != manifest.resource.evidence_id
        || document.bundle_version != manifest.bundle_version
        || document.instance_id != manifest.instance_id
        || document.session_id != manifest.session_id
        || document.capture_revision != manifest.capture_revision
        || document.captured_at_unix_ms != manifest.captured_at_unix_ms
        || document.coverage != manifest.coverage
        || document.screenshot.as_ref().map(|image| &image.descriptor)
            != manifest.screenshot.as_ref()
    {
        return Err(invalid("evidence document does not match its manifest"));
    }
    Ok(())
}

fn validate_image(image: &Image, revision: WireU64) -> Result<(), ControlError> {
    let descriptor = &image.descriptor;
    let size = descriptor.image_bytes.get();
    if size == 0 || size > CONTROL_EVIDENCE_MAX_BUNDLE_BYTES as u64 {
        return Err(too_large());
    }
    let png = decode(&image.image_base64, size as usize)?;
    // Forward already-encoded pixels, never decompress them in the adapter.
    // Check the PNG signature/IHDR and the dimensions stated beside the image.
    if descriptor.format != "png"
        || descriptor.capture_revision != revision
        || descriptor.width_px == 0
        || descriptor.height_px == 0
        || descriptor
            .pixels_per_point
            .as_str()
            .parse::<f64>()
            .ok()
            .is_none_or(|scale| !scale.is_finite() || scale <= 0.0)
        || png.len() as u64 != size
        || raw_digest(&png) != descriptor.image_digest.as_str()
        || png.len() < 33
        || !png.starts_with(PNG_SIGNATURE)
        || png[8..16] != [0, 0, 0, 13, b'I', b'H', b'D', b'R']
        || png[16..20] != descriptor.width_px.to_be_bytes()
        || png[20..24] != descriptor.height_px.to_be_bytes()
    {
        return Err(invalid(
            "screenshot identity, size, digest or PNG header mismatch",
        ));
    }
    Ok(())
}

fn decode(encoded: &str, max_bytes: usize) -> Result<Vec<u8>, ControlError> {
    if encoded.len() > max_bytes.div_ceil(3) * 4 {
        return Err(too_large());
    }
    let bytes = Base64Bytes::new(encoded)
        .and_then(|data| data.decode())
        .map_err(|_| invalid("evidence contains invalid base64"))?;
    if bytes.len() > max_bytes {
        return Err(too_large());
    }
    Ok(bytes)
}

fn success(
    response: ResponseEnvelope,
    expected: Option<&InstanceId>,
) -> Result<Value, ControlError> {
    if expected.is_some_and(|id| id != &response.instance_id) {
        return Err(invalid("evidence response came from a different instance"));
    }
    match response.outcome {
        ResponseOutcome::Success { result } => Ok(result),
        ResponseOutcome::Failure { error } => Err(error),
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ControlError> {
    serde_json::from_value(value).map_err(|_| invalid("evidence response has invalid fields"))
}

fn invalid(message: &str) -> ControlError {
    ControlError::new(
        ErrorCode::new(codes::INVALID_REQUEST).expect("static code is valid"),
        message,
        false,
    )
}

fn too_large() -> ControlError {
    ControlError::new(
        ErrorCode::new(codes::PAYLOAD_TOO_LARGE).expect("static code is valid"),
        "evidence exceeds the reviewed capture byte budget",
        false,
    )
}

#[cfg(test)]
mod tests;
