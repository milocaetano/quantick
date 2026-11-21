use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.market",
    "Feed, symbol, and visible market data",
    false,
)];
pub(super) fn bind(_: &mut Tables) {}
