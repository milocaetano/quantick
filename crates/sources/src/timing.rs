//! Honest display vocabulary for measurements made against venue wall clocks.

/// A signed local-minus-venue stamp difference is clock evidence, not measured
/// transport delay. Preserve the sign, including a venue clock ahead of local.
#[must_use]
pub fn stamp_offset_label(offset_ms: i64) -> String {
    format!("clock {offset_ms} ms")
}

pub const STAMP_OFFSET_TOOLTIP: &str = "clock: local wall clock minus the newest venue timestamp; a negative value means the venue timestamp is ahead of the local clock. Clock disagreement and delivery time both affect this difference, so it is not measured transport delay. Local worker residence and processing use monotonic timing in diagnostics.";

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
