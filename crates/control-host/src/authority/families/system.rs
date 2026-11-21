use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.system",
    "Application build and runtime identity",
    false,
)];
pub(super) fn bind(_: &mut Tables) {}
