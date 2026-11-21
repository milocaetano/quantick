use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.workspace",
    "Open tabs, layout, focus, and appearance configuration",
    false,
)];
pub(super) fn bind(_: &mut Tables) {}
