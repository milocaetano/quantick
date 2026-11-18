//! Honest display vocabulary for measurements made against venue wall clocks,
//! and the provider's own split of a feed's delay.

/// A signed local-minus-venue stamp difference is clock evidence, not measured
/// transport delay. Preserve the sign, including a venue clock ahead of local.
#[must_use]
pub fn stamp_offset_label(offset_ms: i64) -> String {
    format!("clock {offset_ms} ms")
}

pub const STAMP_OFFSET_TOOLTIP: &str = "clock: local wall clock minus the newest venue timestamp; a negative value means the venue timestamp is ahead of the local clock. Clock disagreement and delivery time both affect this difference, so it is not measured transport delay. Local worker residence and processing use monotonic timing in diagnostics.";

/// Where a feed's delay is being spent, as far as the provider can tell.
///
/// The chart has always been able to say *how* late a print was — its timestamp
/// against the local clock, one number. That number cannot be acted on: a tape
/// running seconds behind looks the same whether the venue's own adapter got it
/// late, the transport carried it late, or the chart drained it late, and those
/// have different fixes. This is that number cut into the pieces the provider
/// can actually measure.
///
/// Every split is optional, and the whole struct is absent for a provider that
/// cannot cut its own chain. That is deliberate: an invented zero would read as
/// "this hop is instant", which is a measurement nobody took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedLatency {
    /// Signed provider receipt stamp minus venue stamp. Clock disagreement can
    /// contribute; an unsplit reading is not a measurement of transport latency.
    /// Chart drain reads another receipt stamp. Their separately sampled
    /// difference is diagnostic, not precise local queue residence.
    pub arrival_lag_ms: i64,
    /// Venue stamp to the source handing the print over — everything upstream
    /// of quantick.
    pub source_lag_ms: Option<i64>,
    /// Worst `source_lag_ms` over the sample.
    ///
    /// The only peak here, because it is the only one a provider can take
    /// without a clock: both stamps come from the source, per print. A peak on
    /// the arrival or wire figures would need the reader's clock applied to a
    /// print that arrived earlier, which measures that print's *age* rather
    /// than its delay — and on a quiet tape that is the sampling interval,
    /// reported as latency.
    pub source_lag_peak_ms: Option<i64>,
    /// The source handing it over to quantick reading it: the wire.
    pub transport_lag_ms: Option<i64>,
    /// The provider's own name for the hop that owns most of the delay.
    ///
    /// A borrowed name rather than a shared enum, because the chains differ:
    /// MetaTrader's runs venue → terminal → bridge → socket, a web-socket
    /// venue's does not have a terminal at all, and a recorded session has no
    /// chain to speak of. One enum covering all of them would either lose the
    /// detail that makes the reading actionable or grow a variant per provider,
    /// and this readout exists to be acted on.
    pub hop: Option<&'static str>,
    /// How many live prints the sample covers.
    pub prints: u32,
}

impl FeedLatency {
    /// The arrival figure as a measured delay: only when the provider cut it
    /// into both halves and it is not negative. Otherwise it is a clock
    /// difference, which the tape cell says as `clock N ms`.
    #[must_use]
    pub fn measured_ms(&self) -> Option<i64> {
        let split = self.source_lag_ms.is_some() && self.transport_lag_ms.is_some();
        (split && self.arrival_lag_ms >= 0).then_some(self.arrival_lag_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_clock_difference_is_never_clamped_or_called_latency() {
        assert_eq!(stamp_offset_label(-920), "clock -920 ms");
        assert_eq!(stamp_offset_label(120), "clock 120 ms");
        assert_eq!(stamp_offset_label(0), "clock 0 ms");
    }
}
