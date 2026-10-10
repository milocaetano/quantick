//! Regional emphasis and calibration text, independent of pixels.
use super::constants::{FLOW_REGION_WINDOW_MS, LARGE_REGION_REFERENCE_DIVISOR};
use super::{FlowScaleBasis, FlowTapeDot, FlowTapeFrame};
use crate::config::dressing::flow::{CONTEXT_OPACITY, PEAK_OPACITY};
use rust_decimal::{Decimal, prelude::ToPrimitive as _};

impl FlowTapeFrame {
    /// Inclusive ordinary-region placement threshold; brightness rises above it.
    pub fn large_region_threshold(&self) -> Option<Decimal> {
        self.effective_reference
            .filter(|reference| *reference > Decimal::ZERO)
            .map(|reference| reference / Decimal::from(LARGE_REGION_REFERENCE_DIVISOR))
    }

    /// Ordinary volume stays dim through scale/4, then gradually reaches the
    /// existing peak brightness at the reference, independent of camera spacing.
    pub fn ordinary_region_opacity(&self, dot: &FlowTapeDot) -> f32 {
        let reference = self
            .effective_reference
            .filter(|reference| *reference > Decimal::ZERO)
            .or_else(|| {
                self.dots
                    .iter()
                    .filter(|dot| !dot.opening_oversized)
                    .map(|dot| dot.mark.quantity)
                    .max()
                    .filter(|quantity| *quantity > Decimal::ZERO)
            });
        let Some(reference) = reference else {
            return CONTEXT_OPACITY;
        };
        // Convert before division: even MAX / 1e-28 fits f64, unlike Decimal.
        let relative =
            dot.mark.quantity.to_f64().unwrap_or_default() / reference.to_f64().unwrap_or(1.0);
        let start = 1.0 / f64::from(LARGE_REGION_REFERENCE_DIVISOR);
        let emphasis = ((relative - start) / (1.0 - start)).clamp(0.0, 1.0) as f32;
        CONTEXT_OPACITY + (PEAK_OPACITY - CONTEXT_OPACITY) * emphasis
    }
}

/// Current viewport calibration and honest progress/opening qualifications.
pub fn caption_text(
    frame: Option<&FlowTapeFrame>,
    progress: super::FlowProgress,
    legend_visible: bool,
) -> Option<String> {
    let mut hints = Vec::new();
    if legend_visible {
        hints.push(
            frame
                .and_then(|frame| frame.effective_reference)
                .map_or_else(
                    || {
                        format!(
                            "{FLOW_REGION_WINDOW_MS} ms regions · area = regional gross volume · visible scale"
                        )
                    },
                    |reference| {
                        format!(
                            "{FLOW_REGION_WINDOW_MS} ms regions · area = regional gross volume · scale {}",
                            crate::config::labels::format_quantity(reference)
                        )
                    },
                ),
        );
        hints.push("brightness rises with regional volume".to_owned());
    }
    if progress.pending {
        hints.push(
            if frame.is_some_and(|f| f.cache_limit_reached) {
                "Partial regional volume"
            } else {
                "Regional volume updating"
            }
            .to_owned(),
        );
    }
    if let Some(frame) = frame {
        if frame.ineligible_executions > 0 {
            hints.push(format!("{} records excluded", frame.ineligible_executions));
        }
        let (count, opening, total) = frame.dots.iter().filter(|dot| dot.opening_oversized).fold(
            (0, Decimal::ZERO, Decimal::ZERO),
            |(count, opening, total), dot| {
                (
                    count + 1,
                    opening.saturating_add(dot.opening_quantity),
                    total.saturating_add(dot.mark.quantity),
                )
            },
        );
        if count > 0 {
            hints.push(format!(
                "{} first daily regions: total {}, recorded opening {} (faint area proportional)",
                count,
                total.normalize(),
                opening.normalize()
            ));
        }
        if frame.opening_exclusion_effective {
            hints.push("First daily region excluded from scale".to_owned());
        } else if frame.scale_basis == FlowScaleBasis::OpeningOnlyFallback {
            hints.push("Opening-only scale fallback".to_owned());
        }
    }
    (!hints.is_empty()).then(|| hints.join(" | "))
}
