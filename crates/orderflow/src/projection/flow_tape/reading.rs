//! Factual regional inspection and calibration text, independent of pixels.
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

    /// Exact facts for any renderer; time-zone formatting stays with its caller.
    pub fn inspection_details(
        &self,
        dot: &FlowTapeDot,
        pending: bool,
        prefix_len: usize,
        side_inferred: bool,
        fmt_decimal: impl Fn(Decimal) -> String,
    ) -> Vec<String> {
        let mark = &dot.mark;
        let mut rows = vec![
            "Symbol may be separated above its source; executed prices remain factual.".to_owned(),
            format!(
                "Buy {} · Sell {} · Total {}",
                fmt_decimal(mark.buy_quantity),
                fmt_decimal(mark.quantity - mark.buy_quantity),
                fmt_decimal(mark.quantity)
            ),
            format!(
                "Executed prices {}–{}",
                fmt_decimal(mark.price_bucket),
                fmt_decimal(mark.price_bucket + mark.price_span)
            ),
            format!(
                "Candle span {}–{}",
                dot.first_slot.saturating_add(prefix_len).saturating_add(1),
                dot.end_slot.saturating_add(prefix_len)
            ),
        ];
        if let Some(reference) = self.effective_reference {
            rows.push(format!("Volume reference {}", fmt_decimal(reference)));
        }
        if let Some(threshold) = self.large_region_threshold() {
            rows.push(format!(
                "Ordinary regions: brightness rises gradually above total {} (scale / 4), reaching full colour at the volume reference; separation alone does not promote small regions.",
                fmt_decimal(threshold)
            ));
        }
        rows.push("Dim context keeps full volume and area; brightness is emphasis.".into());
        rows.push(
            "Circle volume totals this price/time region; footprint totals cover a candle price row."
                .into(),
        );
        rows.push(
            "Dim context is drawn beneath footprint; inspection retains exact quantities.".into(),
        );
        rows.push(format!(
            "Fixed {FLOW_REGION_WINDOW_MS} ms price/time regions; not individual orders."
        ));
        if dot.opening_quantity > rust_decimal::Decimal::ZERO {
            rows.push(format!(
                "Recorded opening {}",
                fmt_decimal(dot.opening_quantity)
            ));
        }
        if dot.opening_oversized {
            rows.push("First daily region: uncapped area proportional to its total volume.".into());
            rows.push("Lighter fill preserves the candles beneath this oversized region.".into());
        } else if self.scale_basis == FlowScaleBasis::OpeningOnlyFallback {
            rows.push("Only opening volume visible; using full volume for scale.".into());
        } else if self.opening_exclusion_effective {
            rows.push("First daily region excluded from scale; quantities unchanged.".into());
        }
        if pending {
            rows.push("Updating: showing last computed regions.".into());
        }
        if self.omitted_executions > 0 {
            rows.push(format!(
                "Partial coverage: {} of {} records loaded{}.",
                self.loaded_executions,
                self.requested_ordinals.len(),
                if self.cache_limit_reached {
                    " (display capacity)"
                } else {
                    ""
                }
            ));
        }
        if self.ineligible_executions > 0 {
            rows.push(format!(
                "{} source records excluded.",
                self.ineligible_executions
            ));
        }
        if side_inferred {
            rows.push("Aggressor side inferred.".into());
        }
        rows
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
