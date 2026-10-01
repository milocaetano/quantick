//! Passive inspection of the FLOW regions actually painted this frame.
use crate::{paper_chrome::fmt_decimal, theme, timezone::TzOffset};
use eframe::egui;
use quantick_civil::CivilDate;
use quantick_orderflow::projection::flow_tape::{
    FlowProgress, FlowScaleBasis, FlowTapeDot, FlowTapeFrame,
};

pub(crate) struct FlowInspection<'a> {
    pub painter: &'a egui::Painter,
    pub history: egui::Rect,
    pub pointer: Option<egui::Pos2>,
    pub frame: &'a FlowTapeFrame,
    pub progress: FlowProgress,
    pub prefix_len: usize,
    pub tz: TzOffset,
    pub side_inferred: bool,
}

/// `center` is the same mapping used by the bubble painter, with current axes.
/// No interaction widgets, retained selection, source reads, or member expansion.
pub(crate) fn draw_flow_inspection(
    view: FlowInspection<'_>,
    center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) -> Option<egui::Rect> {
    let pointer = view.pointer.filter(|point| view.history.contains(*point))?;
    let dot = hit_region(&view.frame.dots, view.history, pointer, center)?;
    let painter = view.painter.with_clip_rect(view.history);
    let width_limit = view.history.width() - 16.0;
    if width_limit < 80.0 {
        return None;
    }
    let heading = painter.layout(
        format!("Regional executions · {} prints", dot.mark.trade_count),
        egui::FontId::monospace(11.0),
        theme::TEXT_PRIMARY,
        width_limit,
    );
    let detail = painter.layout(
        details(dot, &view).join("\n"),
        egui::FontId::monospace(10.0),
        theme::TEXT_MUTED,
        width_limit,
    );
    let size = egui::vec2(
        heading.size().x.max(detail.size().x) + 16.0,
        heading.size().y + detail.size().y + 14.0,
    );
    if size.x > view.history.width() || size.y > view.history.height() {
        return None;
    }
    let above = pointer.y - size.y - 8.0;
    let y = if above >= view.history.top() {
        above
    } else {
        pointer.y + 12.0
    };
    let origin = egui::pos2(
        (pointer.x + 12.0).clamp(view.history.left(), view.history.right() - size.x),
        y.clamp(view.history.top(), view.history.bottom() - size.y),
    );
    let rect = egui::Rect::from_min_size(origin, size);
    painter.rect_filled(rect, egui::Rounding::same(4.0), theme::TAG_BG);
    let detail_y = heading.size().y + 8.0;
    painter.galley(
        rect.min + egui::vec2(8.0, 4.0),
        heading,
        theme::TEXT_PRIMARY,
    );
    painter.galley(
        rect.min + egui::vec2(8.0, detail_y),
        detail,
        theme::TEXT_MUTED,
    );
    Some(rect)
}

fn hit_region(
    dots: &[FlowTapeDot],
    history: egui::Rect,
    pointer: egui::Pos2,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) -> Option<&FlowTapeDot> {
    dots.iter()
        .filter_map(|dot| {
            if !dot.radius.is_finite() || dot.radius <= 0.0 {
                return None;
            }
            let at = center(dot)?;
            if !at.x.is_finite() || !at.y.is_finite() {
                return None;
            }
            let edge = egui::pos2(
                at.x.clamp(history.left(), history.right()),
                at.y.clamp(history.top(), history.bottom()),
            );
            // A wholly clipped disc is never inspectable through the hit padding.
            if at.distance_sq(edge) > dot.radius * dot.radius {
                return None;
            }
            let distance = at.distance_sq(pointer);
            (distance <= dot.radius.max(6.0).powi(2)).then_some((distance, dot))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, dot)| dot)
}

fn details(dot: &FlowTapeDot, view: &FlowInspection<'_>) -> Vec<String> {
    let mark = &dot.mark;
    let dates = CivilDate::from_ms(mark.first_timestamp_ms, view.tz)
        != CivilDate::from_ms(mark.last_timestamp_ms, view.tz);
    let mut rows = vec![
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
            "Time {}–{} ({})",
            stamp(mark.first_timestamp_ms, view.tz, dates),
            stamp(mark.last_timestamp_ms, view.tz, dates),
            view.tz.label()
        ),
        format!(
            "Candle span {}–{}",
            dot.first_slot
                .saturating_add(view.prefix_len)
                .saturating_add(1),
            dot.end_slot.saturating_add(view.prefix_len)
        ),
    ];
    if let Some(reference) = view.frame.effective_reference {
        rows.push(format!("Area reference {}", fmt_decimal(reference)));
    }
    if dot.opening_quantity > rust_decimal::Decimal::ZERO {
        rows.push(format!(
            "Recorded opening {}",
            fmt_decimal(dot.opening_quantity)
        ));
    }
    if dot.opening_capped {
        rows.push("Opening volume included; bubble area capped.".into());
    } else if view.frame.scale_basis == FlowScaleBasis::OpeningOnlyFallback {
        rows.push("Only opening volume visible; using full volume for scale.".into());
    } else if view.frame.opening_exclusion_effective {
        rows.push("Opening excluded from reference; quantities unchanged.".into());
    }
    if view.progress.pending {
        rows.push("Updating: showing last computed regions.".into());
    }
    if view.frame.omitted_executions > 0 {
        rows.push(format!(
            "Partial coverage: {} of {} records loaded{}.",
            view.frame.loaded_executions,
            view.frame.requested_ordinals.len(),
            if view.frame.cache_limit_reached {
                " (display capacity)"
            } else {
                ""
            }
        ));
    }
    if view.frame.ineligible_executions > 0 {
        rows.push(format!(
            "{} source records excluded.",
            view.frame.ineligible_executions
        ));
    }
    if view.side_inferred {
        rows.push("Aggressor side inferred.".into());
    }
    rows
}

fn stamp(ms: i64, tz: TzOffset, date: bool) -> String {
    let local = ms.saturating_add(tz.offset_ms());
    let (year, month, day, hour, minute, second) = quantick_civil::civil_utc(local);
    let clock = format!(
        "{hour:02}:{minute:02}:{second:02}.{:03}",
        local.rem_euclid(1000)
    );
    if date {
        format!("{year:04}-{month:02}-{day:02} {clock}")
    } else {
        clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_preserves_milliseconds_and_display_dates() {
        let tz = TzOffset::new(-180);
        assert_eq!(stamp(10_799_999, tz, true), "1969-12-31 23:59:59.999");
        assert_eq!(stamp(10_800_001, tz, true), "1970-01-01 00:00:00.001");
        assert_eq!(stamp(10_800_001, tz, false), "00:00:00.001");
    }
}
