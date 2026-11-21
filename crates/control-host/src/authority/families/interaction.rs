use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.attention",
    "Semantic cursor and current selection",
    false,
)];
pub(super) fn bind(_: &mut Tables) {}
