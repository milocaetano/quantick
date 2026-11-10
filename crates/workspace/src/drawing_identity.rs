//! Durable drawing addresses, independent of runtime pane-local counters.
use serde::{Deserialize, Serialize};

/// Caller-issued identities travel with the drawing, including mirrored copies.
/// `origin` is a unique document incarnation, not a mutable layout name.
/// Hosts must persist it before permitting a new reference. Replacing or
/// importing a different document must not reuse its incarnation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DurableDrawingRef {
    pub workspace: String,
    pub origin: String,
    pub pane: String,
    pub drawing: u64,
    /// Distinguishes a newly created mark from a deleted local id on restart.
    pub incarnation: String,
}

/// Absence is intentional: loading a legacy local id cannot bind an alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityUnavailable {
    Legacy,
    Missing,
    Ambiguous,
}

/// Exact composite lookup. Duplicate identities are refused even if geometry
/// happens to match. A caller may collapse known mirrors before resolving.
pub fn resolve<'a>(
    target: &DurableDrawingRef,
    candidates: impl IntoIterator<Item = &'a DurableDrawingRef>,
) -> Result<&'a DurableDrawingRef, IdentityUnavailable> {
    let mut matches = candidates
        .into_iter()
        .filter(|candidate| *candidate == target);
    let first = matches.next().ok_or(IdentityUnavailable::Missing)?;
    if matches.next().is_some() {
        return Err(IdentityUnavailable::Ambiguous);
    }
    Ok(first)
}
