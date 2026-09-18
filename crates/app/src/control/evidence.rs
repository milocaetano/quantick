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

pub(crate) use quantick_control_schema::evidence::*;

use std::collections::BTreeSet;

use quantick_control::{
    error::ControlError,
    id::{EvidenceId, PermissionId, ResourceId},
};

use super::{contract::UiReadContext, gateway::runtime_id_bytes};

// Re-exported at the visibility they had: the gateway hands a frame over as
// `RawScreenshot`, and the contract and the gateway share the store by path.

pub(crate) use quantick_control_host::evidence::{EvidenceChunkPage, EvidenceStore};

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
        system: super::system::snapshot(),
        configuration,
        screenshot,
        pending_gaps,
        source_scopes,
        store: context.evidence.clone(),
        store_epoch: context.evidence.epoch(),
        captured_at_unix_ms: crate::metrics::wall_clock_ms(),
    })
}
