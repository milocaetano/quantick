//! Peak spheres: a zoom region's volume-by-price profile, smoothed, divided
//! at the valleys between its significant peaks, one sphere per peak.
use super::{FlowTapeDot, FlowTapeFrame};
use rust_decimal::Decimal;

impl FlowTapeFrame {
    /// The least quantity that carries a label under peak spheres; `None`
    /// when the frame does not divide its regions at peaks.
    #[must_use]
    pub fn peak_label_floor(&self) -> Option<Decimal> {
        None
    }

    /// How strongly a peak sphere's fill is drawn, from its buy/sell
    /// imbalance; 1 when the frame does not divide its regions at peaks.
    #[must_use]
    pub fn peak_fill_strength(&self, _dot: &FlowTapeDot) -> f32 {
        1.0
    }
}
