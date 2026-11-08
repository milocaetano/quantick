//! Version diagnostics, owned by child 8 after skeleton integration.
use crate::error::{ErrorCode, PineError, Span};

/// Preserve the existing v5 policy; callers own both diagnostic collections.
pub(crate) fn diagnose(
    version: Option<(u32, Span)>,
    errors: &mut Vec<PineError>,
    warnings: &mut Vec<PineError>,
) {
    if let Some((version, span)) = version {
        if version != 5 {
            errors.push(PineError::new(
                ErrorCode::PineVersion,
                span,
                format!("only Pine v5 scripts are supported, found version {version}"),
            ));
        }
    } else {
        warnings.push(PineError::new(
            ErrorCode::PineVersion,
            Span::at(0),
            "no //@version directive; assuming version 5",
        ));
    }
}
