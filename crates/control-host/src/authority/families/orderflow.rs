use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.orderflow",
    "Order-flow and local depth state",
    false,
)];
pub(super) fn bind(_: &mut Tables) {}
