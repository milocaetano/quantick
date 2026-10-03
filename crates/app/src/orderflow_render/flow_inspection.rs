//! Passive inspection of the FLOW regions actually painted this frame.
use crate::{paper_chrome::fmt_decimal, theme, timezone::TzOffset};
use eframe::egui;
use quantick_civil::{CivilDate, fmt_offset_millisecond};
use quantick_orderflow::projection::flow_tape::{FlowProgress, FlowTapeDot, FlowTapeFrame};

pub(crate) struct FlowInspection<'a> {
    pub painter: &'a egui::Painter,
    pub history: egui::Rect,
    pub pointer: Option<egui::Pos2>,
    pub frame: &'a FlowTapeFrame,
    pub presentation: &'a quantick_chart::flow_execution::FlowPresentation,
    pub progress: FlowProgress,
    pub prefix_len: usize,
    pub tz: TzOffset,
    pub side_inferred: bool,
}

/// The cached display plan is shared with this frame's regional painter.
/// No interaction widgets, retained selection, source reads, or member expansion.
pub(crate) fn draw_flow_inspection(view: FlowInspection<'_>) -> Option<egui::Rect> {
    let pointer = view.pointer.filter(|point| view.history.contains(*point))?;
    let index = view.presentation.hit(
        [view.history.min.into(), view.history.max.into()],
        pointer.into(),
    )?;
    let dot = &view.frame.dots[index];
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

fn details(dot: &FlowTapeDot, view: &FlowInspection<'_>) -> Vec<String> {
    let mark = &dot.mark;
    let dates = CivilDate::from_ms(mark.first_timestamp_ms, view.tz)
        != CivilDate::from_ms(mark.last_timestamp_ms, view.tz);
    let mut rows = view.frame.inspection_details(
        dot,
        view.progress.pending,
        view.prefix_len,
        view.side_inferred,
        fmt_decimal,
    );
    rows[0] = format!(
        "Symbol separated above source; pooled price {}",
        fmt_decimal(mark.price)
    );
    rows.insert(
        3,
        format!(
            "Time {}–{} ({})",
            fmt_offset_millisecond(mark.first_timestamp_ms, view.tz, dates),
            fmt_offset_millisecond(mark.last_timestamp_ms, view.tz, dates),
            view.tz.label(),
        ),
    );
    rows
}

#[cfg(test)]
#[path = "tests/flow_inspection.rs"]
mod tests;
