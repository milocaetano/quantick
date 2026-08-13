//! Native reference indicators.
//!
//! Hand-written Rust implementations of the [`Indicator`] contract. They
//! exist for three reasons: they prove the whole pipe (host -> worker ->
//! render) before the script frontend lands (M1 of the plan), they are the
//! performance reference a scripted equivalent is benchmarked against, and
//! they double as living documentation of how an indicator is written.
//!
//! Each lives in its own file and is registered here with one `pub use`, so
//! a third native is a new file plus one line rather than an edit to the
//! behaviour of the two that already ship. Both follow the preview
//! discipline the trait demands: kernels are cloned for the preview push so
//! committed state never advances inside a forming bar.

mod avwap;
mod cvd;
mod ema;

pub use avwap::{AVWAP_BAND_PAIRS, AnchoredVwap};
pub use cvd::Cvd;
pub use ema::Ema;

use crate::output::Rgba8;

/// Stroke width of a native's plot, in points (`PlotSpec::width`'s unit).
pub(crate) const PLOT_WIDTH_PT: f32 = 1.5;

/// Default EMA line color.
pub(crate) const EMA_COLOR: Rgba8 = Rgba8::opaque(255, 179, 0);

/// Length a fresh EMA declares in its input spec.
pub(crate) const EMA_DEFAULT_LEN: i64 = 9;

/// CVD pane color.
pub(crate) const CVD_COLOR: Rgba8 = Rgba8::opaque(41, 182, 246);

/// Anchored VWAP center line color (deep purple — distinct from EMA amber
/// and CVD blue).
pub(crate) const AVWAP_COLOR: Rgba8 = Rgba8::opaque(124, 77, 255);

/// Band line colors, pair 1..=3: same hue, fading with distance from vwap.
pub(crate) const AVWAP_LINE_COLORS: [Rgba8; 3] = [
    Rgba8::new(124, 77, 255, 150),
    Rgba8::new(124, 77, 255, 110),
    Rgba8::new(124, 77, 255, 80),
];

/// Translucent fills between each band pair; alphas chosen to stay legible
/// when all three stack over candles.
pub(crate) const AVWAP_FILL_COLORS: [Rgba8; 3] = [
    Rgba8::new(124, 77, 255, 20),
    Rgba8::new(124, 77, 255, 13),
    Rgba8::new(124, 77, 255, 8),
];

/// Default σ multipliers a fresh Anchored VWAP declares for its three bands.
pub(crate) const AVWAP_BAND_MULTS: [f64; 3] = [1.0, 2.0, 3.0];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::Indicator;
    use crate::input::SourceId;

    #[test]
    fn ema_overlay_follows_the_source_scale() {
        assert!(Ema::new(9, SourceId::Close).descriptor().overlay);
        assert!(Ema::new(9, SourceId::Hl2).descriptor().overlay);
        assert!(
            !Ema::new(9, SourceId::Delta).descriptor().overlay,
            "an EMA of delta lives on the flow scale, not the price scale"
        );
        assert!(!Ema::new(9, SourceId::Cvd).descriptor().overlay);
    }

    #[test]
    fn ema_title_names_non_default_sources() {
        assert_eq!(Ema::new(9, SourceId::Close).descriptor().title, "EMA(9)");
        assert_eq!(
            Ema::new(21, SourceId::Delta).descriptor().title,
            "EMA(21, delta)"
        );
    }

    #[test]
    fn descriptors_declare_their_settings() {
        let ema = Ema::default();
        assert_eq!(ema.descriptor().inputs.len(), 2);
        assert_eq!(ema.descriptor().inputs[0].name(), "len");
        assert!(Cvd::new().descriptor().inputs.is_empty());
    }
}
