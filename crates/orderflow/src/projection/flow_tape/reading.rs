//! Factual regional inspection and calibration text, independent of pixels.
use super::{FlowScaleBasis, FlowTapeDot, FlowTapeFrame};
use rust_decimal::Decimal;

impl FlowTapeFrame {
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
            "Circle at the pooled regional centre.".to_owned(),
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
        if dot.opening_quantity > rust_decimal::Decimal::ZERO {
            rows.push(format!(
                "Recorded opening {}",
                fmt_decimal(dot.opening_quantity)
            ));
        }
        if dot.opening_capped {
            rows.push(
                "Opening volume included; dashed hollow marker is capped, not proportional.".into(),
            );
        } else if self.scale_basis == FlowScaleBasis::OpeningOnlyFallback {
            rows.push("Only opening volume visible; using full volume for scale.".into());
        } else if self.opening_exclusion_effective {
            rows.push("Opening excluded from reference; quantities unchanged.".into());
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
                    || "Regional sell / buy volume · visible scale".to_owned(),
                    |reference| {
                        format!(
                            "Regional sell / buy volume · scale {}",
                            crate::config::labels::format_quantity(reference)
                        )
                    },
                ),
        );
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
        let (count, opening, total) = frame.dots.iter().filter(|dot| dot.opening_capped).fold(
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
                "{} dashed capped regions: total {}, recorded opening {} (not proportional)",
                count,
                total.normalize(),
                opening.normalize()
            ));
        }
        if frame.opening_exclusion_effective {
            hints.push("First recorded burst excluded from scale".to_owned());
        } else if frame.scale_basis == FlowScaleBasis::OpeningOnlyFallback {
            hints.push("Opening-only scale fallback".to_owned());
        }
    }
    (!hints.is_empty()).then(|| hints.join(" | "))
}
