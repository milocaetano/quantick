//! Durable drawing addresses, independent of runtime pane-local counters.
use serde::{Deserialize, Serialize};

/// Why an existing drawing cannot become a durable alert target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetResolveError {
    Unavailable,
    Missing,
    Ambiguous,
    Busy,
    SourceChanged,
    WriteFailed(String),
}

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

/// Stable, unambiguous market/role spelling; mirrors share this address.
#[must_use]
pub fn scope(key: &crate::market_settings::DrawingKey) -> String {
    let legacy = legacy_scope(key);
    key.scope.map_or(legacy.clone(), |scope| {
        format!("v2:{}:{}:{legacy}", scope.workspace, scope.layout.0)
    })
}

fn legacy_scope(key: &crate::market_settings::DrawingKey) -> String {
    let role = String::from(key.role.clone());
    format!(
        "{}:{}{}:{}{}:{}",
        key.market.venue.len(),
        key.market.venue,
        key.market.asset.len(),
        key.market.asset,
        role.len(),
        role
    )
}

/// Issue only from caller-supplied unique entropy. The caller commits before
/// publishing this reference; no runtime local id establishes durability.
pub fn issue(
    workspace: &str,
    key: &crate::market_settings::DrawingKey,
    drawing: u64,
    entropy: &str,
) -> Result<DurableDrawingRef, IdentityUnavailable> {
    if workspace.is_empty() || entropy.is_empty() || drawing == 0 {
        return Err(IdentityUnavailable::Legacy);
    }
    Ok(DurableDrawingRef {
        workspace: workspace.to_owned(),
        origin: entropy.to_owned(),
        pane: scope(key),
        drawing,
        incarnation: entropy.to_owned(),
    })
}

/// A carried identity may be reused only in its original source/market role.
#[must_use]
pub fn belongs_to(target: &DurableDrawingRef, key: &crate::market_settings::DrawingKey) -> bool {
    !target.workspace.is_empty()
        && !target.origin.is_empty()
        && !target.incarnation.is_empty()
        && target.drawing != 0
        && (target.pane == scope(key) || target.pane == legacy_scope(key))
}
