//! UI conversions and compatibility imports; series ownership lives below app.
#[cfg(test)]
pub use quantick_engine::ImbalanceUnit;
pub use quantick_engine::bar_registry::BarConfiguration;
pub use quantick_engine::bar_registry::TIME_INTERVAL_DRAG_SPEED;
pub use quantick_engine::bar_selection::BarSelection as SpecSelector;
pub use quantick_engine::{BarKind, BarSpec, MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS};
pub use quantick_series::RetainedSeries;
use rust_decimal::Decimal;

/// Any UI `f64` as a positive [`Decimal`].
///
/// Two kinds of number pass through here and they are not the same kind: a bar
/// rule's threshold from the toolbar, and — via `pane::strategy_region` — a
/// drawing's anchor *prices*. The floor below is its own constant for exactly
/// that reason: it is the positivity floor this conversion has always applied,
/// not the engine's bar-parameter floor
/// ([`DECIMAL_PARAM_FLOOR`](quantick_engine::DECIMAL_PARAM_FLOOR)), and tuning
/// that floor must not silently retune where a strategy region's bounds land.
///
/// The price case is why that floor is a wart rather than a guard: a region
/// drawn on a negative-valued band (a CVD) has both bounds collapsed onto it.
/// Pre-existing and outside this change; written down here so the next reader
/// finds it rather than trusting the constant's name.
#[must_use]
pub fn dec_from_f64(x: f64) -> Decimal {
    use rust_decimal::prelude::FromPrimitive as _;
    /// The smallest positive `Decimal` this conversion produces. A separate
    /// number from the engine's
    /// [`DECIMAL_PARAM_FLOOR`](quantick_engine::DECIMAL_PARAM_FLOOR), which it
    /// happens to equal: they answer to different callers.
    const POSITIVE_FLOOR: Decimal = Decimal::from_parts(1, 0, 0, false, 8);
    Decimal::from_f64(x)
        .unwrap_or(Decimal::ONE)
        .max(POSITIVE_FLOOR)
}
