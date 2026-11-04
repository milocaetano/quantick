//! Regional emphasis and calibration text, independent of pixels.
use super::constants::LARGE_REGION_REFERENCE_DIVISOR;
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

/// The one caption the flow pane still draws: its name and the notes it may
/// never drop ([`caption_qualifications`]); none without notes.
#[must_use]
pub fn notes_caption(
    frame: Option<&FlowTapeFrame>,
    progress: super::FlowProgress,
) -> Option<String> {
    let notes = caption_qualifications(frame, progress)?;
    Some(format!("FLOW | {notes}"))
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
            // What the spheres drew: the earned area over visible regions,
            // the radius limit's over a typed reference.
            let area = if frame.view.reference.caps_oversized_opening() {
                "capped at the radius limit"
            } else {
                "proportional"
            };
            hints.push(format!(
                "{} first daily regions: total {}, recorded opening {} (faint area {})",
                count,
                quantick_engine::display::display_decimal(total),
                quantick_engine::display::display_decimal(opening),
                area
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
