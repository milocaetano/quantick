//! Read the opening sidecar through the existing chart state owner.
use super::ChartState;
use quantick_engine::BarFootprint;
use std::collections::BTreeMap;

#[must_use]
pub fn closed(state: &ChartState) -> &BTreeMap<usize, BarFootprint> {
    state.footprints.opening_closed()
}
#[must_use]
pub fn partial(state: &ChartState) -> Option<&BarFootprint> {
    state.footprints.opening_partial()
}
#[must_use]
pub fn recorded_windows(state: &ChartState) -> &[i64] {
    state.footprints.recorded_openings()
}
