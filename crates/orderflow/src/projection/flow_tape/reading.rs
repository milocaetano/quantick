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

/// Where the caption's discs are drawn, after the Buy / Sell key: regional
/// FLOW sits at a fixed offset up and left of its execution so candles stay
/// readable; tick spheres stand on their executions.
#[must_use]
pub fn placement_note(tick_spheres: bool) -> &'static str {
    if tick_spheres {
        " \u{00b7} spheres at executions"
    } else {
        " \u{00b7} offset \u{2196}"
    }
}

/// Current viewport calibration and honest progress/opening qualifications.
pub fn caption_text(
    frame: Option<&FlowTapeFrame>,
    progress: super::FlowProgress,
    legend_visible: bool,
) -> Option<String> {
    styled_caption_text(frame, progress, legend_visible, false, None)
}

/// [`caption_text`] for the look drawn: tick spheres count in `unit`, the
/// quantity unit the asset's config mode names, and claim none without one.
pub fn styled_caption_text(
    frame: Option<&FlowTapeFrame>,
    progress: super::FlowProgress,
    legend_visible: bool,
    tick_spheres: bool,
    unit: Option<&str>,
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
        hints.push(match (tick_spheres, unit) {
            (true, Some(unit)) => format!("quantity in {unit}; sphere area follows gross volume"),
            (true, None) => "sphere area follows gross volume".to_owned(),
            (false, _) => "brightness rises with regional volume".to_owned(),
        });
    }
    hints.extend(qualifications(frame, progress));
    (!hints.is_empty()).then(|| hints.join(" | "))
}

/// The notes a FLOW caption may never drop, without its key: volume still
/// updating or partial, records left out, and how the opening is scaled. A
/// caption that cannot be placed still shows these.
#[must_use]
pub fn caption_qualifications(
    frame: Option<&FlowTapeFrame>,
    progress: super::FlowProgress,
) -> Option<String> {
    let notes = qualifications(frame, progress);
    (!notes.is_empty()).then(|| notes.join(" | "))
}

fn qualifications(frame: Option<&FlowTapeFrame>, progress: super::FlowProgress) -> Vec<String> {
    let mut hints = Vec::new();
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
                quantick_engine::display::display_decimal(total),
                quantick_engine::display::display_decimal(opening)
            ));
        }
        if frame.opening_exclusion_effective {
            hints.push("First daily region excluded from scale".to_owned());
        } else if frame.scale_basis == FlowScaleBasis::OpeningOnlyFallback {
            hints.push("Opening-only scale fallback".to_owned());
        }
    }
    hints
}
