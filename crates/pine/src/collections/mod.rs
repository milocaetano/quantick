//! Existing rejection policy extracted without enabling new behavior.
use crate::error::ErrorCode;

/// The baseline rejection; the owning child may replace it with supported lowering.
pub(crate) fn rejection() -> (ErrorCode, &'static str) {
    (
        ErrorCode::PineNoCollections,
        "collection namespaces (array/matrix/map/table) are not in the v1 dialect",
    )
}
