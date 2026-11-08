//! Version policy shared by compiler, type checking and execution.
use crate::error::{ErrorCode, PineError, Span};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Version {
    #[default]
    V5,
    V6,
}
impl Version {
    pub(crate) fn lazy_boolean(self) -> bool {
        self == Self::V6
    }
    pub(crate) fn dynamic_for_bound(self) -> bool {
        self == Self::V6
    }
    pub(crate) fn two_state_bool(self) -> bool {
        self == Self::V6
    }
    pub(crate) fn truncate_const_division(self) -> bool {
        self == Self::V5
    }
    pub(crate) fn color(self, field: &str, legacy: u32) -> u32 {
        match (self, field) {
            (Self::V6, "teal") => 0x0899_81FF,
            _ => legacy,
        }
    }
    pub(crate) fn bool_coercion(self) -> u8 {
        if self.two_state_bool() { 4 } else { 1 }
    }
}
pub(crate) fn diagnose(
    version: Option<(u32, Span)>,
    errors: &mut Vec<PineError>,
    warnings: &mut Vec<PineError>,
) -> Version {
    match version {
        Some((6, _)) => Version::V6,
        Some((5, _)) => Version::V5,
        Some((version, span)) => {
            errors.push(PineError::new(
                ErrorCode::PineVersion,
                span,
                format!("only Pine v5 and v6 scripts are supported, found version {version}"),
            ));
            Version::V5
        }
        None => {
            warnings.push(PineError::new(
                ErrorCode::PineVersion,
                Span::at(0),
                "no //@version directive; assuming version 5",
            ));
            Version::V5
        }
    }
}
