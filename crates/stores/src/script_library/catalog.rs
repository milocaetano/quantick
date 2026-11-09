//! Embedded Pine assets and their stable file identities.

/// Embedded starter scripts: (menu name, source). Each doubles as a
/// conformance fixture in the pine crate's corpus.
pub const EMBEDDED_SCRIPTS: &[(&str, &str)] = &[
    ("ema.pine", include_str!("../../../app/scripts/ema.pine")),
    ("cvd.pine", include_str!("../../../app/scripts/cvd.pine")),
    (
        "delta_histogram.pine",
        include_str!("../../../app/scripts/delta_histogram.pine"),
    ),
    (
        "vwap_cumulative.pine",
        include_str!("../../../app/scripts/vwap_cumulative.pine"),
    ),
    (
        "zigzag.pine",
        include_str!("../../../app/scripts/zigzag.pine"),
    ),
    (
        "range_box.pine",
        include_str!("../../../app/scripts/range_box.pine"),
    ),
    (
        "copilot.pine",
        include_str!("../../../app/scripts/copilot.pine"),
    ),
    (
        "force_bar.pine",
        include_str!("../../../app/scripts/force_bar.pine"),
    ),
    (
        "exhaustion_reversal.pine",
        include_str!("../../../app/scripts/exhaustion_reversal.pine"),
    ),
];
