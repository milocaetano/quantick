use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[
    (
        "observe.health",
        "Bounded structured health diagnostics",
        false,
    ),
    (
        "observe.diagnostic_logs",
        "Redacted diagnostic log records",
        true,
    ),
];
pub(super) fn bind(_: &mut Tables) {}
