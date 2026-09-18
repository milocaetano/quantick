//! App-local event constructors for existing tests.
use super::{IndicatorEvent, SlotId};
use quantick_indicators::IndicatorDescriptor;

/// A `Rebuilt` carrying only the shape a test cares about: no paint, no
/// bound inputs, not stale, and the row count taken from the columns.
///
/// The row count is a field rather than a derivation in production
/// precisely because a paint-only indicator has no column to count — but
/// every test here declares plots, so deriving it is exact for them and
/// keeps the fixtures readable.
pub(crate) fn rebuilt(
    slot: SlotId,
    descriptor: IndicatorDescriptor,
    columns: Vec<Vec<f64>>,
) -> IndicatorEvent {
    IndicatorEvent::Rebuilt {
        slot,
        descriptor,
        rows: columns.first().map_or(0, Vec::len),
        columns,
        bar_paint: Vec::new(),
        inputs: Vec::new(),
        stale: None,
    }
}

/// An `Appended` row that asks for no candle paint.
pub(crate) fn appended(slot: SlotId, row: Vec<f64>) -> IndicatorEvent {
    IndicatorEvent::Appended {
        slot,
        row,
        paint: None,
    }
}
