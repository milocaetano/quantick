//! Evidence bundles: one coherent, redacted, self-describing capture of a
//! running session, retained in memory and read back as a paginated resource.
//!
//! ## What it is for
//!
//! Every other read answers one question. An investigation needs the answers
//! to have been true *at the same instant*: the chart said this while the feed
//! said that while the frame cost this much. A bundle is that instant, taken
//! once, hashed, and handed back by identifier — so a defect is reproduced
//! from what was recorded rather than from what someone remembers seeing.
//!
//! ## What it carries
//!
//! The coherent snapshot of the scopes the caller named — which is where the
//! health metrics and the semantic scene live, both being ordinary registered
//! scopes — a page of the semantic event journal with the cursor that
//! continues it, the build and host it was taken on, the effective
//! configuration with its paths removed, an optional screenshot stamped with
//! the *same* capture revision as the scene, and, above all, an account of
//! what it does **not** carry.
//!
//! ## What it refuses to do
//!
//! It never writes to disk: exporting is a cockpit action and does not ship
//! under observer authority. It never launders a scope — a bundle requires
//! `observe.evidence` *plus* every scope it aggregates, and every chunk read
//! rechecks that grant against the manifest, because a resource identifier is
//! an address, never an authorization. It never records itself in the event
//! journal: the journal is for semantic transitions the trader would
//! recognise, and observer traffic evicting a human's mark would be a bug.
//!
//! ## Rate class and cost
//!
//! On-demand captures. Nothing here runs unless a client asks. The application
//! thread does only what needs application state — the projection pass, a
//! bounded journal read, a copy of the effective configuration and, when one
//! was asked for, the pixels of the frame just painted. Encoding, canonical
//! JSON, hashing, chunking and retention all happen after the capture has left
//! that thread, on the same response worker that already serializes a
//! snapshot.

use std::collections::BTreeSet;

use quantick_control::{
    canonical::{Sha256Digest, canonical_json},
    cursor::{EventCursor, Page, PageCursor},
    error::{ControlError, codes},
    id::{EvidenceId, InstanceId, PermissionId, ProcessNonce, ResourceId, SnapshotScopeId},
    limits::{
        CONTROL_DEFAULT_PAGE_ITEMS, CONTROL_EVIDENCE_CHUNK_BYTES,
        CONTROL_EVIDENCE_MAX_BUNDLE_BYTES, CONTROL_MAX_SNAPSHOT_SCOPES, CONTROL_SCENE_MAX_CONTROLS,
    },
    wire::{Base64Bytes, CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{AppConfig, DeclaredLayout, FeedConfig, Mt5SideSource, ProviderKind};

use super::{
    contract::UiReadContext,
    events::read_page,
    gateway::runtime_id_bytes,
    journal::EventPage,
    registry::{SerializedSnapshotCapture, SnapshotCapture},
    scene::{CONTROLS_SCOPE_ID as SCENE_CONTROLS_SCOPE_ID, SceneSnapshot},
    system::{SystemSnapshot, snapshot as system_snapshot},
    types::{known_error, wire_usize},
};

mod image;
mod store;

// Re-exported at the visibility they had: the gateway hands a frame over as
// `RawScreenshot`, and the contract and the gateway share the store by path.
pub(crate) use image::{RawScreenshot, ScreenshotPixels};
use image::{encode_screenshot, region_gap};
pub(crate) use store::EvidenceStore;
use store::{RetainedBundle, raw_sha256};

/// The module that owns both evidence capabilities.
pub(crate) const EVIDENCE_MODULE_ID: &str = "evidence";
/// The scope the tier is gated on. Sensitive and off by default: a bundle is
/// every granted scope at once, in one durable object.
pub(crate) const EVIDENCE_PERMISSION_ID: &str = "observe.evidence";
/// Rasterising the window is its own decision, separately granted.
pub(crate) const SCREENSHOT_PERMISSION_ID: &str = "observe.screenshot";
/// The scope one projection publishes the trader's own words under.
const USER_TEXT_PERMISSION_ID: &str = "observe.user_text";

pub(crate) const CAPTURE_CAPABILITY_ID: &str = "evidence.capture";
pub(crate) const READ_CAPABILITY_ID: &str = "evidence.read";

/// The name the page cursor carries for the resource it walks.
///
/// Shaped like a snapshot scope because that is the field the contract's
/// cursor declares, but it names a *retained resource*, not a projection: no
/// module registers it and no capture builds it.
pub(crate) const EVIDENCE_RESOURCE_SCOPE_ID: &str = "evidence.bundle";

/// The encoding a reassembled bundle is in.
const BUNDLE_MEDIA_TYPE: &str = "application/json; charset=utf-8";
/// The renderer this build links, from the `eframe` feature the application
/// manifest selects. Reported so a defect that reproduces on one backend can
/// be told apart from one that does not.
///
/// A `const`, because the feature it names is not visible to this crate as a
/// `cfg` — so `the_reported_graphics_backend_is_the_one_the_manifest_selects`
/// reads the manifest and fails if the two ever part company. A bundle that
/// named the wrong renderer would send an investigation after the wrong bug.
const GRAPHICS_BACKEND: &str = "glow";
/// The subject every screenshot gap is filed under, so a client branching on
/// "is there an image" matches one name whatever the reason is.
const SCREENSHOT_GAP_SUBJECT: &str = "screenshot";
const BUNDLE_VERSION: u32 = 1;
const MANIFEST_VERSION: u32 = 1;

/// Feed symbols one bundle copies out of the effective configuration.
///
/// A configured catalogue is a handful of instruments; the bound exists so a
/// pathological file cannot turn the configuration section into the payload. A
/// capture that meets it says so in its coverage rather than truncating in
/// silence.
const MAX_CONFIGURATION_SYMBOLS: usize = 64;

/// Unavailable fields one bundle's coverage names.
///
/// The walk that finds them runs over data the projections have already
/// bounded, so this is a backstop rather than a working limit; meeting it is
/// reported like every other gap.
const MAX_UNAVAILABLE_FIELDS: usize = 256;

/// Chunks the largest permitted bundle takes.
///
/// Derived from the two limits that decide it rather than written down beside
/// them, so the manifest's declared bound cannot drift from the chunking that
/// produces it.
const MAX_CHUNKS_PER_BUNDLE: usize =
    CONTROL_EVIDENCE_MAX_BUNDLE_BYTES.div_ceil(CONTROL_EVIDENCE_CHUNK_BYTES);

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// `evidence.capture`: which scopes to freeze, how much of the journal to
/// carry with them, and whether to rasterise the window as well.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceCaptureInput {
    /// Named explicitly, like every snapshot: a bundle may not aggregate its
    /// way into a scope the connection was not granted, and a caller that did
    /// not ask for a scope is told it was omitted rather than left to guess.
    #[schemars(length(min = 1, max = CONTROL_MAX_SNAPSHOT_SCOPES))]
    pub scopes: Vec<SnapshotScopeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = CONTROL_DEFAULT_PAGE_ITEMS))]
    pub event_limit: Option<usize>,
    /// Requires `observe.screenshot`. A capture that asks for one and cannot
    /// have it says why in its coverage; it never silently answers with text
    /// alone.
    #[serde(default)]
    pub screenshot: bool,
}

/// `evidence.read`: one bundle by identifier, and where the last page stopped.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceReadInput {
    pub evidence_id: EvidenceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<PageCursor>,
}

// ---------------------------------------------------------------------------
// Manifest: what a capture answers with, before anything is paged
// ---------------------------------------------------------------------------

/// Everything about a bundle except the bundle.
///
/// Returned by `evidence.capture` so a client can decide whether the capture
/// is worth reading at all — what was covered, what was not, how large it is
/// and how many chunks that will take — without paging a byte. The fields it
/// repeats from the document (the environment, the coverage, the screenshot's
/// geometry) are repeated deliberately: they are the ones that decide whether
/// to read further, and a reader that had to page in order to learn them would
/// pay for the whole bundle to answer a question about its size.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceManifest {
    pub evidence_id: EvidenceId,
    pub resource_id: ResourceId,
    pub manifest_version: u32,
    pub bundle_version: u32,
    pub instance_id: InstanceId,
    pub session_id: ProcessNonce,
    /// The one revision every projection in the bundle was taken at — and the
    /// revision the screenshot, if there is one, is stamped with.
    pub capture_revision: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub captured_at_unix_ms: i64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub expires_at_unix_ms: i64,
    /// Quantick Canonical JSON v1 digest of the whole document. The chunks
    /// below are byte runs of exactly that canonical text, so a client that
    /// concatenates them and hashes the result reproduces this value without
    /// re-canonicalising anything.
    pub content_digest: Sha256Digest,
    pub media_type: String,
    #[schemars(extend("x-unit" = "bytes"))]
    pub encoded_bytes: WireU64,
    #[schemars(extend("x-unit" = "bytes"))]
    pub chunk_bytes: WireU64,
    #[schemars(range(max = MAX_CHUNKS_PER_BUNDLE))]
    pub chunk_count: usize,
    /// Raw-byte digest of each chunk, in order (contract section 3).
    #[schemars(length(max = MAX_CHUNKS_PER_BUNDLE))]
    pub chunk_digests: Vec<Sha256Digest>,
    /// Every scope this bundle aggregates. Rechecked against the connection's
    /// grant on every chunk read, so a revoked scope closes an open resource.
    pub source_scopes: BTreeSet<PermissionId>,
    pub environment: EvidenceEnvironment,
    pub coverage: EvidenceCoverage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<EvidenceScreenshot>,
}

/// The build and the machine the capture was taken on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceEnvironment {
    /// The application's own identity projection, verbatim: version, commit
    /// and its provenance, target triple, build profile, protocol version.
    /// Taken from the same function `system.info` publishes, so a bundle and a
    /// snapshot can never disagree about what build produced them.
    pub system: SystemSnapshot,
    pub graphics_backend: String,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub process_started_at_unix_ms: i64,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub process_uptime_ms: i64,
}

/// The honest bound on everything else in the bundle.
///
/// Absence is reported here; qualification is reported in place. A value that
/// is inferred rather than measured carries its own provenance where it lives
/// — the aggressor side under `feed.status`, the tick-rule delta under
/// `orderflow.tape` — because a label separated from its value is a label that
/// drifts. What this section answers is the other question: what is *not*
/// here, and why.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceCoverage {
    /// Registered scopes this capture did not include, because the caller did
    /// not name them.
    pub omitted_scopes: Vec<SnapshotScopeId>,
    /// Fields the captured projections themselves reported as unavailable,
    /// with the pointer that finds each one in the document and the coded
    /// reason it gave. Derived by walking the capture, never hand-kept.
    #[schemars(length(max = MAX_UNAVAILABLE_FIELDS))]
    pub unavailable_fields: Vec<EvidenceUnavailableField>,
    /// What this bundle does not carry at all, and why.
    pub not_captured: Vec<EvidenceGap>,
    /// True only when every list above is empty. It is never true today, and
    /// says so rather than letting a reader take a bundle for the session.
    pub complete: bool,
}

/// One thing the bundle does not carry.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceGap {
    /// What is missing, as a stable name.
    pub subject: String,
    /// Why, as a stable code — never the sentence an interface shows a human.
    /// A client made to parse that sentence would break the day it is
    /// reworded, and translating it would break every such client at once.
    pub reason: String,
}

/// One field a projection reported it could not fill.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceUnavailableField {
    /// JSON Pointer (RFC 6901) into the bundle document.
    pub pointer: String,
    pub reason: String,
}

/// The geometry and integrity of a captured frame, without its pixels.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceScreenshot {
    /// The capture this image belongs to — the same revision as the scene in
    /// the same bundle. That equality is the whole point: it is what turns a
    /// rectangle of pixels into the name of a control.
    ///
    /// It ties the image to the *scene*, and to nothing stronger. The pixels
    /// are of the frame that was painted; the projections are taken at the top
    /// of the next frame, after it has drained whatever the feed delivered in
    /// between. Controls do not move in a drain, so every region below still
    /// names what is under it — but a price or a bar count elsewhere in the
    /// bundle can be one drain newer than the picture, and
    /// `coverage.not_captured` says so rather than leaving a reader to assume
    /// otherwise.
    pub capture_revision: WireU64,
    pub width_px: u32,
    pub height_px: u32,
    /// Physical pixels per logical point: the factor the scene's own
    /// point-valued bounds were multiplied by to reach the regions below.
    pub pixels_per_point: CanonicalDecimal,
    pub format: String,
    /// Digest of the raw image bytes, not of their transport encoding.
    pub image_digest: Sha256Digest,
    #[schemars(extend("x-unit" = "bytes"))]
    pub image_bytes: WireU64,
    /// Where each named control sits in this image.
    #[schemars(length(max = CONTROL_SCENE_MAX_CONTROLS))]
    pub control_regions: Vec<EvidenceControlRegion>,
    /// Controls the scene named that have no region here, each with the reason
    /// the scene itself gave for having no bounds. Listed rather than dropped:
    /// a reader must be able to tell "not on screen" from "not measured".
    pub controls_without_region: Vec<EvidenceGap>,
}

/// One control's rectangle in the captured image, in physical pixels.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceControlRegion {
    pub control_id: String,
    #[schemars(extend("x-unit" = "pixels"))]
    pub x_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "pixels"))]
    pub y_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "pixels"))]
    pub width_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "pixels"))]
    pub height_px: CanonicalDecimal,
    /// Whether the rectangle lies wholly inside the image. A control the
    /// window is clipping is exactly what a screenshot gets asked about, so it
    /// is reported with its real numbers and this flag rather than trimmed.
    pub within_image: bool,
}

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

/// The bundle itself: what the chunks reassemble into.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceDocument {
    pub evidence_id: EvidenceId,
    pub bundle_version: u32,
    pub instance_id: InstanceId,
    pub session_id: ProcessNonce,
    pub capture_revision: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub captured_at_unix_ms: i64,
    pub environment: EvidenceEnvironment,
    pub configuration: EvidenceConfiguration,
    pub coverage: EvidenceCoverage,
    /// The coherent projection pass: every scope the caller named, at one
    /// revision. Health and scene are ordinary scopes and live here.
    pub snapshot: SerializedSnapshotCapture,
    /// The semantic events around the capture, and the cursor that continues
    /// them through `events.read`.
    pub events: EventPage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<EvidenceImage>,
}

/// A captured frame and its pixels.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceImage {
    pub descriptor: EvidenceScreenshot,
    pub image_base64: Base64Bytes,
}

// ---------------------------------------------------------------------------
// Effective configuration, redacted
// ---------------------------------------------------------------------------

/// The effective configuration with everything that names the user's machine
/// removed.
///
/// A path is the one configuration value that is never about Quantick: it is
/// about whose computer this is. Ports, provider kinds and flags explain a
/// failure; a home directory only identifies a person. What is dropped is
/// listed by key in [`Self::redacted_keys`], so a reader is told a setting
/// exists and was withheld rather than left to conclude it is unset.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceConfiguration {
    pub default_feed_id: String,
    pub default_symbol: String,
    pub feeds: Vec<EvidenceFeedConfiguration>,
    pub metatrader: EvidenceMetaTraderConfiguration,
    pub paper: EvidencePaperConfiguration,
    /// Configuration keys this bundle deliberately does not carry.
    pub redacted_keys: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceFeedConfiguration {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub symbol_count: usize,
    #[schemars(length(max = MAX_CONFIGURATION_SYMBOLS))]
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_bars: Option<String>,
    pub bubble_preset_configured: bool,
    pub symbol_bubble_preset_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceMetaTraderConfiguration {
    /// The port a bridge dials, without the address it is bound to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_port: Option<u16>,
    pub listen_host_is_loopback: bool,
    pub symbol_port_count: usize,
    pub side_source: String,
    pub bridge_autostart: bool,
    /// Whether a bridge command is configured. The command itself is a program
    /// path into the user's machine and never travels.
    pub bridge_command_configured: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidencePaperConfiguration {
    /// Whether the trade journal has been pointed somewhere other than its
    /// default home. Where, is a path.
    pub trades_dir_configured: bool,
}

// ---------------------------------------------------------------------------
// The paged resource
// ---------------------------------------------------------------------------

/// One page of a retained bundle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct EvidenceChunkPage {
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
pub(crate) struct EvidenceChunk {
    pub index: usize,
    #[schemars(extend("x-unit" = "bytes"))]
    pub byte_offset: WireU64,
    #[schemars(extend("x-unit" = "bytes"))]
    pub byte_length: WireU64,
    pub digest: Sha256Digest,
    pub data: Base64Bytes,
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/// What names this process, beside the instance identifier it publishes.
#[derive(Clone, Debug)]
pub(crate) struct SessionIdentity {
    pub session_id: ProcessNonce,
    pub process_started_at_unix_ms: i64,
}

/// The ingredients of one bundle, collected on the application thread.
///
/// Nothing here is encoded, hashed or chunked yet. The value travels to the
/// response worker exactly as a [`SnapshotCapture`] does, and everything
/// expensive happens there.
pub(crate) struct EvidenceCapture {
    pub evidence_id: EvidenceId,
    pub resource_id: ResourceId,
    pub instance_id: InstanceId,
    pub session: SessionIdentity,
    pub snapshot: SnapshotCapture,
    pub events: EventPage,
    pub configuration: EvidenceConfiguration,
    pub screenshot: Option<RawScreenshot>,
    /// Gaps the application thread already knows about: a screenshot that was
    /// not asked for or never arrived, a configuration list that was cut.
    pub pending_gaps: Vec<EvidenceGap>,
    pub source_scopes: BTreeSet<PermissionId>,
    pub store: EvidenceStore,
    /// The store's epoch when this capture was collected. If the store has
    /// been emptied since — the trader withdrew access while this was
    /// encoding — the bundle is not retained.
    pub store_epoch: u64,
    pub captured_at_unix_ms: i64,
}

/// Collect one bundle's ingredients on the application thread.
///
/// The module's own assembly, called by the registered capability the way
/// `chart::chart_window_prevalidated` is: the dispatch layer decides *whether*
/// a read may run, and this decides *what* it reads, so neither has to know
/// the other's business. Everything expensive is left to
/// [`EvidenceCapture::into_manifest`], which runs off this thread.
///
/// `source_scopes` arrives already checked against the connection's grant —
/// the capability's prepare step computed it, and the dispatcher refused the
/// request if it exceeded the grant. It is carried into the bundle so every
/// later chunk read can recheck it against a grant that may since have
/// changed.
pub(crate) fn capture_prevalidated(
    context: UiReadContext<'_>,
    input: &EvidenceCaptureInput,
    source_scopes: BTreeSet<PermissionId>,
) -> Result<EvidenceCapture, ControlError> {
    let snapshot = context
        .projections
        .capture(context.app, context.instance_id, &input.scopes)?;
    let events = recent_events(context.journal, context.instance_id, input.event_limit)?;
    let (configuration, mut pending_gaps) = redact_configuration(context.app.control_config());
    let screenshot = if input.screenshot {
        let taken = context.screenshot.take();
        if taken.is_none() {
            pending_gaps.push(screenshot_gap("frame_not_delivered"));
        }
        taken
    } else {
        pending_gaps.push(screenshot_gap("not_requested"));
        None
    };
    Ok(EvidenceCapture {
        evidence_id: EvidenceId::from_bytes(runtime_id_bytes()?),
        resource_id: ResourceId::from_bytes(runtime_id_bytes()?),
        instance_id: context.instance_id.clone(),
        session: context.session.clone(),
        snapshot,
        events,
        configuration,
        screenshot,
        pending_gaps,
        source_scopes,
        store: context.evidence.clone(),
        store_epoch: context.evidence.epoch(),
        captured_at_unix_ms: crate::metrics::wall_clock_ms(),
    })
}

/// The events *around* the capture: the newest page the journal holds, and the
/// cursor that carries on from there.
///
/// The newest, not the oldest. A session that has emitted three thousand
/// events would otherwise hand back the first two hundred and fifty-six — the
/// application starting up — while the moment the bundle was taken to explain
/// sits eleven pages further on. What an investigation wants is what just
/// happened, and what a client wants next is to keep reading forward from
/// there, which is what `next_cursor` then gives it.
fn recent_events(
    journal: &super::journal::EventJournal,
    instance_id: &InstanceId,
    limit: Option<usize>,
) -> Result<EventPage, ControlError> {
    let bounds = journal.bounds();
    let limit = limit
        .unwrap_or(CONTROL_DEFAULT_PAGE_ITEMS)
        .clamp(1, CONTROL_DEFAULT_PAGE_ITEMS);
    let start = bounds
        .next_sequence
        .get()
        .saturating_sub(limit as u64)
        .max(bounds.oldest_sequence.get());
    read_page(
        journal,
        instance_id,
        Some(&EventCursor {
            instance_id: instance_id.clone(),
            next_sequence: WireU64::new(start),
        }),
        None,
        Some(limit),
        false,
    )
    .map(redact_event_prose)
}

/// Payload keys under which an operator's own words reach the journal:
///
/// - `note` — what a human types at a mark (`attention.mark.created`).
/// - `message` — what an assistant puts in a notification (`notify.raised`).
/// - `text` — the words on a label, inside an annotation payload.
/// - `label` — what an object is *called*: `Drawing::display_label` returns
///   the name the trader gave it, and the annotate events carry it as `label`.
/// - `name`, `title` — a script's own name and an indicator's, inside the
///   `indicator.script.*` payloads.
///
/// This list is the contract. A new action that records prose adds its key
/// here, because nothing else can find prose in a free-form payload — and the
/// day one is forgotten is the day a bundle carries the trader's words while
/// its coverage says it does not.
const USER_PROSE_PAYLOAD_KEYS: &[&str] = &["note", "message", "text", "label", "name", "title"];

/// Take the operator's own words out of the events a bundle carries.
///
/// The projections already strip user text before it reaches a snapshot, but a
/// bundle also embeds a page of the *journal*, and the journal records prose
/// verbatim: the note a trader typed at a mark, the message an assistant sent,
/// the words on a label. Carrying those while `coverage` claimed user text was
/// redacted would be a false statement made by the one section whose whole job
/// is to be honest about what is missing.
///
/// Redacted unconditionally, not per grant. `events.read` still serves the
/// prose to a client holding `observe.events`, unchanged; what a *bundle* must
/// not become is the aggregate durable object where every scope's text is
/// gathered in one place. The marker says redacted rather than empty, so a
/// reader can tell a withheld note from a mark that never had one.
fn redact_event_prose(mut page: EventPage) -> EventPage {
    for event in &mut page.events {
        redact_prose_value(&mut event.payload);
    }
    page
}

fn redact_prose_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if USER_PROSE_PAYLOAD_KEYS.contains(&key.as_str()) && child.is_string() {
                    *child = json!({
                        "redacted": true,
                        "reason": "user_text_is_not_carried_in_a_bundle",
                    });
                } else {
                    redact_prose_value(child);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                redact_prose_value(child);
            }
        }
        _ => {}
    }
}

impl EvidenceCapture {
    /// Encode, hash, chunk, retain, and answer with the manifest.
    ///
    /// Runs on the response worker. The order matters: the document is built
    /// first so its size is known, the screenshot is left out of it if it
    /// would take the bundle past its own ceiling, and only a bundle that fits
    /// is retained — a capture that cannot be kept is refused rather than
    /// answered with an identifier nothing can read.
    pub fn into_manifest(self) -> Result<(EvidenceManifest, WireU64), ControlError> {
        let snapshot = self.snapshot.into_serialized().map_err(|error| {
            known_error(
                codes::CAPABILITY_UNAVAILABLE,
                format!("the evidence capture could not be serialized: {error}"),
                false,
            )
        })?;
        let capture_revision = snapshot.capture_revision;

        let mut gaps = fixed_gaps();
        gaps.push(user_text_gap(&self.source_scopes));
        gaps.extend(self.pending_gaps);

        // Three outcomes, and the bundle has to be able to tell them apart:
        // the scene was not captured, the scene was captured and read, or the
        // scene is sitting right there and could not be read. Reporting the
        // third as the first would be a false statement about a scope
        // populated in the same document — the one thing the coverage section
        // exists not to do. Borrowed, not cloned: `SceneSnapshot` can be
        // deserialized straight out of the value the capture already built.
        let scene = match snapshot
            .scopes
            .get(&SnapshotScopeId::new(SCENE_CONTROLS_SCOPE_ID).expect("static scope ID is valid"))
        {
            None => {
                gaps.push(region_gap("scene_scope_not_captured"));
                None
            }
            Some(scope) => match SceneSnapshot::deserialize(&scope.value) {
                Ok(scene) => Some(scene),
                Err(_) => {
                    gaps.push(region_gap("scene_scope_not_readable"));
                    None
                }
            },
        };
        let image = match self.screenshot {
            None => None,
            Some(raw) => match encode_screenshot(raw, capture_revision, scene.as_ref()) {
                Ok(image) => {
                    // The pixels are of the frame that was painted; the
                    // projections beside them were taken at the top of the
                    // next one, after its drain. Controls do not move in a
                    // drain, so the regions hold — but the market data can
                    // be one step newer than the picture, and a reader is
                    // told rather than left to assume.
                    gaps.push(EvidenceGap {
                        subject: "screenshot.state_skew".to_owned(),
                        reason: "pixels_precede_projections_by_one_drain".to_owned(),
                    });
                    Some(image)
                }
                Err(gap) => {
                    gaps.push(gap);
                    None
                }
            },
        };

        let environment = EvidenceEnvironment {
            system: system_snapshot(),
            graphics_backend: GRAPHICS_BACKEND.to_owned(),
            process_started_at_unix_ms: self.session.process_started_at_unix_ms,
            process_uptime_ms: self
                .captured_at_unix_ms
                .saturating_sub(self.session.process_started_at_unix_ms)
                .max(0),
        };

        let mut document = EvidenceDocument {
            evidence_id: self.evidence_id.clone(),
            bundle_version: BUNDLE_VERSION,
            instance_id: self.instance_id.clone(),
            session_id: self.session.session_id.clone(),
            capture_revision,
            captured_at_unix_ms: self.captured_at_unix_ms,
            environment: environment.clone(),
            configuration: self.configuration,
            coverage: EvidenceCoverage {
                omitted_scopes: snapshot.omitted_scopes.clone(),
                unavailable_fields: Vec::new(),
                not_captured: Vec::new(),
                complete: false,
            },
            snapshot,
            events: self.events,
            screenshot: image,
        };

        // Derived, not hand-kept: the projections said what they could not
        // fill, and the walk repeats it with the pointer that finds it.
        let (unavailable, truncated) = collect_unavailable_fields(&document.snapshot);
        if truncated {
            gaps.push(EvidenceGap {
                subject: "coverage.unavailable_fields".to_owned(),
                reason: "truncated_at_named_limit".to_owned(),
            });
        }
        gaps.sort();
        gaps.dedup();
        document.coverage.unavailable_fields = unavailable;
        document.coverage.not_captured = gaps;
        document.coverage.complete = document.coverage.omitted_scopes.is_empty()
            && document.coverage.unavailable_fields.is_empty()
            && document.coverage.not_captured.is_empty();

        let mut canonical = canonical_bytes(&document)?;
        // The image is the one part of a bundle worth dropping to save the
        // rest. A capture that overflows with a picture in it is re-encoded
        // without one and says why, which is what this module and the contract
        // both promise; a capture that overflows *without* one has nothing left
        // to give up and is refused below.
        if canonical.len() > CONTROL_EVIDENCE_MAX_BUNDLE_BYTES && document.screenshot.is_some() {
            document.screenshot = None;
            let gap = screenshot_gap("exceeds_evidence_bundle_budget");
            if !document.coverage.not_captured.contains(&gap) {
                document.coverage.not_captured.push(gap);
                document.coverage.not_captured.sort();
            }
            canonical = canonical_bytes(&document)?;
        }

        let screenshot = document
            .screenshot
            .as_ref()
            .map(|image| image.descriptor.clone());
        let content_digest = raw_sha256(&canonical);
        let bytes = canonical;
        let encoded_bytes = bytes.len();
        let chunks = bytes
            .chunks(CONTROL_EVIDENCE_CHUNK_BYTES)
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>();
        let chunk_digests = chunks
            .iter()
            .map(|chunk| raw_sha256(chunk))
            .collect::<Vec<_>>();
        let chunk_count = chunks.len();

        let retention_ms = self.store.retention_ms();
        let expires_at_unix_ms = self
            .captured_at_unix_ms
            .saturating_add(i64::try_from(retention_ms).unwrap_or(i64::MAX));
        self.store.insert(
            RetainedBundle {
                evidence_id: self.evidence_id.clone(),
                resource_id: self.resource_id.clone(),
                capture_revision,
                expires_at_unix_ms,
                content_digest: content_digest.clone(),
                encoded_bytes,
                source_scopes: self.source_scopes.clone(),
                chunks,
            },
            self.store_epoch,
            self.captured_at_unix_ms,
        )?;

        Ok((
            EvidenceManifest {
                evidence_id: self.evidence_id,
                resource_id: self.resource_id,
                manifest_version: MANIFEST_VERSION,
                bundle_version: BUNDLE_VERSION,
                instance_id: self.instance_id,
                session_id: self.session.session_id,
                capture_revision,
                captured_at_unix_ms: self.captured_at_unix_ms,
                expires_at_unix_ms,
                content_digest,
                media_type: BUNDLE_MEDIA_TYPE.to_owned(),
                encoded_bytes: wire_usize(encoded_bytes),
                chunk_bytes: wire_usize(CONTROL_EVIDENCE_CHUNK_BYTES),
                chunk_count,
                chunk_digests,
                source_scopes: self.source_scopes,
                environment,
                coverage: document.coverage,
                screenshot,
            },
            capture_revision,
        ))
    }
}

/// The document as the bytes a client will reassemble: Quantick Canonical
/// JSON v1, built once.
///
/// Once matters. `canonical_sha256` *is* `canonical_json` followed by a
/// digest, so asking for both would materialise the whole text twice — up to
/// eight mebibytes of `String` built and thrown away for a hash the bytes
/// about to be chunked give for free.
fn canonical_bytes(document: &EvidenceDocument) -> Result<Vec<u8>, ControlError> {
    let value = serde_json::to_value(document).map_err(|error| {
        known_error(
            codes::CAPABILITY_UNAVAILABLE,
            format!("the evidence bundle could not be encoded: {error}"),
            false,
        )
    })?;
    canonical_json(&value)
        .map(String::into_bytes)
        .map_err(|error| {
            known_error(
                codes::CAPABILITY_UNAVAILABLE,
                format!("the evidence bundle is not canonicalizable: {error}"),
                false,
            )
        })
}

/// Why this bundle has no image, in the one vocabulary that reports it.
fn screenshot_gap(reason: &str) -> EvidenceGap {
    EvidenceGap {
        subject: SCREENSHOT_GAP_SUBJECT.to_owned(),
        reason: reason.to_owned(),
    }
}

/// What `bytes` bytes cost once base64 has had them.
///
/// The size that actually matters for anything travelling the wire or sitting
/// inside the document: four characters for every three bytes, rounded up.
const fn base64_len(bytes: usize) -> usize {
    bytes.div_ceil(3).saturating_mul(4)
}

/// What no bundle in this tier carries, whatever was asked for.
///
/// Each entry is a decision recorded where a reader will meet it, rather than
/// a silence they would have to interpret.
fn fixed_gaps() -> Vec<EvidenceGap> {
    vec![
        EvidenceGap {
            subject: "diagnostic_logs".to_owned(),
            reason: "not_captured_in_this_tier".to_owned(),
        },
        EvidenceGap {
            // The journal records prose verbatim whatever the grant, so the
            // event page is stripped by key on its way into every bundle.
            // What the *projections* do is a separate question, answered by
            // `user_text_gap` below, because one scope is allowed to publish
            // it and a blanket claim here would be false whenever it does.
            subject: "user_authored_text_in_events".to_owned(),
            reason: "redacted_by_payload_key".to_owned(),
        },
        EvidenceGap {
            subject: "configuration_paths".to_owned(),
            reason: "redacted_path_values".to_owned(),
        },
        EvidenceGap {
            subject: "disk_export".to_owned(),
            reason: "cockpit_tier_capability".to_owned(),
        },
        EvidenceGap {
            subject: "chart_bars_beyond_the_visible_window".to_owned(),
            reason: "separately_paginated_capability".to_owned(),
        },
    ]
}

/// What this bundle has to say about user-authored text in its *projections*.
///
/// Not a constant, because it is not always the same answer. Most scopes strip
/// the trader's words before evidence ever sees them — but `analysis.indicators`
/// deliberately publishes the titles of the scripts they wrote, gated on
/// `observe.user_text`, because an agent cannot address an indicator it cannot
/// name. A bundle that captured such a scope therefore *carries* prose, and
/// saying otherwise would be a false statement in the section that exists to
/// be honest about what is missing. So the answer is derived from the scopes
/// the bundle actually aggregated.
fn user_text_gap(source_scopes: &BTreeSet<PermissionId>) -> EvidenceGap {
    let carries_user_text = source_scopes
        .iter()
        .any(|permission| permission.as_str() == USER_TEXT_PERMISSION_ID);
    EvidenceGap {
        subject: "user_authored_text_in_projections".to_owned(),
        reason: if carries_user_text {
            "retained_under_observe_user_text".to_owned()
        } else {
            "redacted_by_projection_policy".to_owned()
        },
    }
}

// ---------------------------------------------------------------------------
// Coverage derivation
// ---------------------------------------------------------------------------

/// Walk the captured projections for the availability shape they all use, and
/// report each unavailable one by pointer.
///
/// One shape, one walk: `{ "available": false, "reason": "<code>" }` is the
/// mould `AvailabilitySnapshot` gives every scope, so a module that adds a new
/// unavailable field joins this report without touching this file.
///
/// It walks the scope values and nothing else — they are already `Value`, so
/// the walk costs no serialization, and an image measured in megabytes is
/// never traversed looking for a field it cannot contain.
fn collect_unavailable_fields(
    snapshot: &SerializedSnapshotCapture,
) -> (Vec<EvidenceUnavailableField>, bool) {
    // Every scope gets its own share, and a scope that spends it is cut off
    // rather than allowed to eat the report. One scope can hold hundreds of
    // these markers — the scene reports "bounds are not recorded" on nearly
    // every control it names — and a single shared budget walked in key order
    // would let that scope alone fill the list and silently drop the real
    // gaps of every scope sorting after it.
    let budget = MAX_UNAVAILABLE_FIELDS
        .checked_div(snapshot.scopes.len().max(1))
        .unwrap_or(MAX_UNAVAILABLE_FIELDS)
        .max(1);
    let mut found = Vec::new();
    let mut truncated = false;
    for (scope_id, scope) in &snapshot.scopes {
        let prefix = format!(
            "/snapshot/scopes/{}/value",
            escape_pointer(scope_id.as_str())
        );
        let mut of_this_scope = Vec::new();
        walk_unavailable(
            &prefix,
            &scope.value,
            budget,
            &mut of_this_scope,
            &mut truncated,
        );
        found.append(&mut of_this_scope);
    }
    found.sort();
    found.dedup();
    (found, truncated)
}

fn walk_unavailable(
    prefix: &str,
    root: &Value,
    budget: usize,
    found: &mut Vec<EvidenceUnavailableField>,
    truncated: &mut bool,
) {
    let mut pending = vec![(prefix.to_owned(), root)];
    while let Some((pointer, value)) = pending.pop() {
        match value {
            Value::Object(map) => {
                if map.get("available") == Some(&Value::Bool(false)) {
                    // The budget is spent at the moment a field would be
                    // *added*, not at the top of the walk. Checking early
                    // marked a scope truncated whenever any node was still
                    // queued — a scalar, an object with nothing unavailable in
                    // it — so coverage could report that something was dropped
                    // when nothing was, in the one section whose job is to be
                    // honest about what is missing.
                    if found.len() >= budget {
                        *truncated = true;
                        return;
                    }
                    found.push(EvidenceUnavailableField {
                        pointer: pointer.clone(),
                        reason: map
                            .get("reason")
                            .and_then(Value::as_str)
                            .unwrap_or("unavailable")
                            .to_owned(),
                    });
                }
                for (key, child) in map {
                    pending.push((format!("{pointer}/{}", escape_pointer(key)), child));
                }
            }
            Value::Array(values) => {
                for (index, child) in values.iter().enumerate() {
                    pending.push((format!("{pointer}/{index}"), child));
                }
            }
            _ => {}
        }
    }
}

fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

// ---------------------------------------------------------------------------
// Configuration redaction
// ---------------------------------------------------------------------------

/// The effective configuration with every path taken out of it.
pub(crate) fn redact_configuration(
    config: &AppConfig,
) -> (EvidenceConfiguration, Vec<EvidenceGap>) {
    let mut gaps = Vec::new();
    let feeds = config
        .feeds
        .iter()
        .map(|feed| redact_feed(feed, &mut gaps))
        .collect();
    let (listen_port, listen_host_is_loopback) = split_listen_addr(&config.metatrader.listen_addr);
    (
        EvidenceConfiguration {
            default_feed_id: config.default_feed.clone(),
            default_symbol: config.default_symbol.clone(),
            feeds,
            metatrader: EvidenceMetaTraderConfiguration {
                listen_port,
                listen_host_is_loopback,
                symbol_port_count: config.metatrader.ports.len(),
                side_source: side_source_id(config.metatrader.side_source).to_owned(),
                bridge_autostart: config.metatrader.bridge_autostart,
                bridge_command_configured: !config.metatrader.bridge_command.is_empty(),
            },
            paper: EvidencePaperConfiguration {
                trades_dir_configured: config.paper.trades_dir.is_some(),
            },
            redacted_keys: vec![
                "metatrader.bridge_command".to_owned(),
                "metatrader.listen_addr.host".to_owned(),
                "paper.trades_dir".to_owned(),
            ],
        },
        gaps,
    )
}

fn redact_feed(feed: &FeedConfig, gaps: &mut Vec<EvidenceGap>) -> EvidenceFeedConfiguration {
    let symbols = if feed.symbols.len() > MAX_CONFIGURATION_SYMBOLS {
        gaps.push(EvidenceGap {
            subject: format!("configuration.feeds.{}.symbols", feed.id),
            reason: "truncated_at_named_limit".to_owned(),
        });
        feed.symbols[..MAX_CONFIGURATION_SYMBOLS].to_vec()
    } else {
        feed.symbols.clone()
    };
    EvidenceFeedConfiguration {
        id: feed.id.clone(),
        name: feed.name.clone(),
        provider: provider_id(feed.provider).to_owned(),
        symbol_count: feed.symbols.len(),
        symbols,
        default_layout: feed.default_layout.and_then(layout_id),
        default_bars: feed.default_bars.clone(),
        bubble_preset_configured: feed.bubble_preset.is_some(),
        symbol_bubble_preset_count: feed.symbol_bubble_presets.len(),
    }
}

/// The port a bridge dials, and whether the address it is bound to is the
/// loopback interface.
///
/// The host itself never travels: on a machine bound to a routable address it
/// names the user's network, and the port plus this flag is everything an
/// investigation into a bridge that will not connect actually needs.
fn split_listen_addr(addr: &str) -> (Option<u16>, bool) {
    // Through the configuration's own splitter, not a second one: two parsers
    // of one format disagree the first time either is touched, and these two
    // already disagreed about an empty host.
    let Some((host, port)) = crate::config::split_host_port(addr) else {
        return (None, false);
    };
    let host = host.trim_matches(['[', ']']);
    let loopback = host.parse::<std::net::IpAddr>().map_or_else(
        |_| host.eq_ignore_ascii_case("localhost"),
        |ip| ip.is_loopback(),
    );
    (Some(port), loopback)
}

/// The configuration file's own name for a provider.
///
/// An exhaustive match rather than a lookup table: adding a provider becomes a
/// compile error here, which is the only way a second list stays in step with
/// the first one. The names are the ones the TOML uses.
const fn provider_id(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Binance => "binance",
        ProviderKind::Hyperliquid => "hyperliquid",
        ProviderKind::MetaTrader => "metatrader",
    }
}

/// The configuration file's own name for a side-source policy, exhaustively
/// matched for the same reason as [`provider_id`].
const fn side_source_id(source: Mt5SideSource) -> &'static str {
    match source {
        Mt5SideSource::TickRule => "tick_rule",
        Mt5SideSource::Flags => "flags",
    }
}

/// The layout's configuration name, taken from the serde vocabulary the config
/// file and the saved workspace already share — `DeclaredLayout` documents
/// that its names live in one place, so this reads them rather than repeating
/// them.
fn layout_id(layout: DeclaredLayout) -> Option<String> {
    serde_json::to_value(layout)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

// ---------------------------------------------------------------------------
// Scope aggregation
// ---------------------------------------------------------------------------

/// What a bundle *always* carries, whatever scopes were named.
///
/// Every capture embeds a page of the semantic event journal and the effective
/// feed configuration, and each of those is somebody's scope: the journal is
/// what `events.read` is gated on, and the configuration names the markets the
/// trader has set up. A bundle that carried them while requiring neither would
/// be exactly the aggregation-as-a-way-in this module says it refuses — and
/// the read-time recheck could never catch it, because a scope missing from
/// the manifest is a scope the recheck is not looking for.
const ALWAYS_AGGREGATED_PERMISSION_IDS: &[&str] = &[
    // The event page, and its cursor.
    "observe.events",
    // The effective configuration: feed IDs, symbol catalogues, the
    // MetaTrader port.
    "observe.market",
    // The build and host identity. Every manifest and every document embeds
    // `system_snapshot()` whatever scopes were named, and that is the `system`
    // module's projection — so a capture of `feed.status` alone still reads
    // it, and must still hold its scope.
    "observe.system",
];

/// The scopes a bundle over these snapshot scopes aggregates.
///
/// The evidence scope itself, every permission the named scopes require, what
/// a bundle always carries, and the screenshot scope when one was asked for. A
/// bundle may not become a way to read something the connection was refused
/// one call earlier.
pub(crate) fn source_scopes<'a>(
    scope_permissions: impl Iterator<Item = &'a BTreeSet<PermissionId>>,
    screenshot: bool,
) -> BTreeSet<PermissionId> {
    let mut scopes: BTreeSet<PermissionId> = scope_permissions.flatten().cloned().collect();
    scopes.insert(permission(EVIDENCE_PERMISSION_ID));
    scopes.extend(
        ALWAYS_AGGREGATED_PERMISSION_IDS
            .iter()
            .map(|id| permission(id)),
    );
    if screenshot {
        scopes.insert(permission(SCREENSHOT_PERMISSION_ID));
    }
    scopes
}

fn permission(id: &str) -> PermissionId {
    PermissionId::new(id).expect("static permission ID is valid")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn instance() -> InstanceId {
        InstanceId::from_bytes([7; 16])
    }

    /// The operator's own words do not travel in a bundle.
    ///
    /// The projections strip user text before it reaches a snapshot, but a
    /// bundle also embeds a page of the *journal*, and the journal records
    /// prose verbatim — the note a trader typed at a mark, the message an
    /// assistant sent, the words on a label.
    #[test]
    fn the_event_page_a_bundle_carries_holds_no_operator_prose() {
        let mut payload = json!({
            "target": { "pane_id": "3" },
            "note": "the absorption I keep seeing here",
            "annotation": { "text": "supply", "tool_id": "text" },
            "nested": [{ "message": "look at this" }],
        });
        redact_prose_value(&mut payload);

        let encoded = payload.to_string();
        for prose in ["the absorption", "supply", "look at this"] {
            assert!(!encoded.contains(prose), "`{prose}` survived in {encoded}");
        }
        // Redacted, and saying so — a withheld note reads differently from a
        // mark that never had one.
        assert_eq!(payload["note"]["redacted"], true);
        assert_eq!(payload["annotation"]["text"]["redacted"], true);
        assert_eq!(payload["nested"][0]["message"]["redacted"], true);
        // And everything that is not prose is untouched.
        assert_eq!(payload["target"]["pane_id"], "3");
        assert_eq!(payload["annotation"]["tool_id"], "text");
    }

    /// Every bundle embeds the build identity, so every bundle needs its scope.
    #[test]
    fn a_bundle_requires_the_system_scope_it_always_embeds() {
        let aggregated = source_scopes(std::iter::empty(), false);
        assert!(
            aggregated.contains(&permission("observe.system")),
            "`system_snapshot()` is in every manifest and every document: {aggregated:?}"
        );
    }

    /// Meeting the budget is reported; *not* meeting it is not.
    ///
    /// The check used to run at the top of the walk, so a scope that found
    /// exactly its budget and still had a scalar queued reported that fields
    /// had been dropped when none had — a false statement in the section whose
    /// whole job is honesty about what is missing.
    #[test]
    fn a_coverage_walk_that_fits_exactly_does_not_claim_it_truncated() {
        let scope = json!({
            "first": { "available": false, "reason": "one" },
            "second": { "available": false, "reason": "two" },
            "plenty": "of other nodes",
            "more": [1, 2, 3],
        });
        let mut found = Vec::new();
        let mut truncated = false;
        walk_unavailable("/snapshot", &scope, 2, &mut found, &mut truncated);
        assert_eq!(found.len(), 2);
        assert!(
            !truncated,
            "both fields fit, so nothing was dropped and nothing should say so"
        );
    }

    /// The renderer a bundle names is the renderer the build links.
    ///
    /// The feature is chosen in a manifest and is invisible to this crate as a
    /// `cfg`, so the constant is checked against that manifest — the repo's own
    /// answer for a rule the compiler cannot see. Switching to `wgpu` without
    /// touching the constant would have every bundle blame the wrong backend.
    ///
    /// The manifest to read is the workspace root's, not this crate's: every
    /// third-party version and feature set is stated once under
    /// `[workspace.dependencies]`, so `crates/app/Cargo.toml` now says only
    /// `eframe = { workspace = true }`. Aimed at the crate manifest this test
    /// does not quietly pass — both `expect`s still find something, and the
    /// assertion below fails naming what it read instead, which is how the
    /// move was caught.
    #[test]
    fn the_reported_graphics_backend_is_the_one_the_manifest_selects() {
        let manifest = include_str!("../../../../Cargo.toml");
        let eframe = manifest
            .split("eframe = ")
            .nth(1)
            .expect("the workspace manifest depends on eframe");
        let features = eframe
            .split_once(']')
            .expect("the eframe workspace dependency lists features")
            .0;
        assert!(
            features.contains(&format!("\"{GRAPHICS_BACKEND}\"")),
            "the bundle reports `{GRAPHICS_BACKEND}` but the manifest selects: {features}"
        );
        // And only that one, or a bundle naming a single renderer is guessing.
        assert!(
            !features.contains("\"wgpu\""),
            "the manifest selects a second renderer, so the reported backend is \
             ambiguous: {features}"
        );
    }

    /// The bind address explains a bridge failure without naming the network
    /// the user is on.
    #[test]
    fn a_listen_address_reduces_to_a_port_and_whether_it_is_loopback() {
        assert_eq!(split_listen_addr("127.0.0.1:9100"), (Some(9100), true));
        assert_eq!(split_listen_addr("localhost:9100"), (Some(9100), true));
        assert_eq!(split_listen_addr("[::1]:9100"), (Some(9100), true));
        assert_eq!(split_listen_addr("192.168.1.20:9100"), (Some(9100), false));
        assert_eq!(split_listen_addr("nonsense"), (None, false));
    }

    /// Every field the walk reports is one a projection said it could not
    /// fill, found by the shape rather than by a list this module keeps — and
    /// a field that *is* available never appears.
    #[test]
    fn the_coverage_walk_finds_unavailable_fields_by_shape_and_names_their_pointer() {
        let scope = json!({
            "coverage": { "available": false, "reason": "regions_not_enumerated" },
            "controls": [
                { "bounds_availability": { "available": true } },
                { "bounds_availability": { "available": false, "reason": "bounds_not_recorded" } },
            ],
        });
        let mut found = Vec::new();
        let mut truncated = false;
        walk_unavailable(
            "/snapshot/scopes/scene.controls/value",
            &scope,
            MAX_UNAVAILABLE_FIELDS,
            &mut found,
            &mut truncated,
        );
        found.sort();
        assert!(!truncated);
        assert_eq!(
            found,
            vec![
                EvidenceUnavailableField {
                    pointer: "/snapshot/scopes/scene.controls/value/controls/1/bounds_availability"
                        .to_owned(),
                    reason: "bounds_not_recorded".to_owned(),
                },
                EvidenceUnavailableField {
                    pointer: "/snapshot/scopes/scene.controls/value/coverage".to_owned(),
                    reason: "regions_not_enumerated".to_owned(),
                },
            ]
        );
    }

    /// The bound is a backstop, and meeting it is reported rather than hidden.
    #[test]
    fn a_coverage_walk_that_meets_its_bound_says_so() {
        let scope = Value::Array(
            (0..MAX_UNAVAILABLE_FIELDS + 8)
                .map(|index| json!({ "available": false, "reason": format!("reason_{index}") }))
                .collect(),
        );
        let mut found = Vec::new();
        let mut truncated = false;
        walk_unavailable(
            "/snapshot",
            &scope,
            MAX_UNAVAILABLE_FIELDS,
            &mut found,
            &mut truncated,
        );
        assert!(truncated);
        assert_eq!(found.len(), MAX_UNAVAILABLE_FIELDS);
    }

    /// One noisy scope does not cost the others their coverage.
    ///
    /// The scene reports "bounds are not recorded" on nearly every control it
    /// names, so a shared budget spent in key order would fill the whole
    /// report from that one scope and drop the real gaps of everything sorting
    /// after it — which, alphabetically, is most of the registry.
    #[test]
    fn a_noisy_scope_cannot_spend_another_scopes_share_of_the_coverage_report() {
        let noisy = Value::Array(
            (0..MAX_UNAVAILABLE_FIELDS * 4)
                .map(|index| json!({ "available": false, "reason": format!("noise_{index}") }))
                .collect(),
        );
        let snapshot = SerializedSnapshotCapture {
            instance_id: instance(),
            capture_revision: WireU64::new(1),
            captured_at_unix_ms: 0,
            module_revisions: Vec::new(),
            capture_elapsed_us: WireU64::new(0),
            capture_budget_us: WireU64::new(0),
            capture_within_budget: true,
            omitted_scopes: Vec::new(),
            scopes: BTreeMap::from([
                (
                    SnapshotScopeId::new("aaa.noisy").unwrap(),
                    scope_value(noisy),
                ),
                (
                    SnapshotScopeId::new("zzz.quiet").unwrap(),
                    scope_value(json!({ "available": false, "reason": "the_one_that_matters" })),
                ),
            ]),
        };

        let (found, truncated) = collect_unavailable_fields(&snapshot);
        assert!(truncated, "the noisy scope spent its share and says so");
        assert!(
            found
                .iter()
                .any(|field| field.reason == "the_one_that_matters"),
            "and the quiet scope's own gap survived: {found:?}"
        );
        assert!(found.len() <= MAX_UNAVAILABLE_FIELDS);
    }

    fn scope_value(value: Value) -> super::super::registry::SerializedScope {
        super::super::registry::SerializedScope {
            module_id: quantick_control::id::ModuleId::new("test").unwrap(),
            schema_version: 1,
            value,
        }
    }
}
