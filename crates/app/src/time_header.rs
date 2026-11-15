//! The time pane's header row (`docs/ux/ui-design-model.md` §11): its bars
//! control and indicators button, and the interval drag its bars menu offers.
//!
//! This row governs the time pane and nothing else, wherever focus sits. It
//! sits in a strip carved off the top of the pane rather than
//! floating over it, so nothing is ever painted across market data. Like every
//! pane's, its bars control reads "Time · 5m" and opens the bars menu, the one
//! place the interval changes.

use eframe::egui;

/// Height of every pane's header strip, in pixels.
pub const HEIGHT_PX: f32 = 34.0;

/// The interval a time pane opens on when the split is first shown: M1, the
/// timeframe a flow trader glances at for context. The engine's default for a
/// time spec, so a fresh `bars -> time` and a fresh time pane open alike.
pub const DEFAULT_INTERVAL_MS: i64 = quantick_engine::DEFAULT_TIME_INTERVAL_MS;

/// Draw the header into `strip`; returns whether the indicators glyph was
/// clicked this frame.
///
/// `salt` namespaces every widget this header registers.
///
/// A canvas may now stack more than one context pane, and each carries its own
/// header. egui keys a widget by its parent's id and its order within it, so
/// two headers built from the same parent would answer to one id — the bars
/// control of the lower pane would drive the upper one. The pane's own id is
/// what makes them distinct, the same rule `ChartPane::interaction_id`
/// follows.
///
/// `bars` paints the pane's bars control at the head of the row. The indicators
/// button follows it, and its room is what a narrow strip takes from the bars
/// control, never from it. `reserve_px` keeps the right end free the same way,
/// for the job slot of a layout with no flow pane; a time pane's layer lives in
/// its capsule.
pub fn draw(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    salt: u64,
    reserve_px: f32,
    bars: impl FnOnce(&mut egui::Ui),
) -> bool {
    crate::pane_header::paint_rule(ui, strip);
    let inner = strip.shrink2(crate::pane_header::CONTENT_PADDING);
    let indicators_px = crate::pane_header::indicators_width(ui, true);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("time_header", salt))
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let limit = (inner.right() - indicators_px - reserve_px).max(strip.left());
    content.set_clip_rect(strip.with_max_x(limit).intersect(ui.clip_rect()));
    bars(&mut content);
    let button_left =
        (content.cursor().left() + quantick_chart::pane_header::GROUP_GAP_PX).min(limit);
    crate::pane_header::indicators_button(ui, button_left, strip, salt, true)
}

/// The custom-interval drag, written in the units the presets and the quick
/// switch use — `90s`, `1d`, `1w` — and read back from them or from a bare
/// millisecond count, so a day reads `1d` rather than `86400000 ms`.
///
/// A typed `1mo` sets the month — the setting its preset sets — rather than
/// being clamped to the drag's four weeks, which would silently make 28-day
/// bars: the drag's own range reaches the longest calendar interval so the
/// typed value arrives whole, and `admit_interval` keeps everything else
/// inside `range`.
pub(crate) fn interval_drag(
    interval_ms: &mut i64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
) -> egui::DragValue<'_> {
    use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, MAX_CALENDAR_MONTHS};
    #[allow(clippy::cast_possible_truncation)]
    let (min, max) = (*range.start() as i64, *range.end() as i64);
    #[allow(clippy::cast_precision_loss)]
    let reach = range
        .end()
        .max((MAX_CALENDAR_MONTHS * CALENDAR_MONTH_MS) as f64);
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    egui::DragValue::from_get_set(move |new| {
        if let Some(new) = new {
            *interval_ms = quantick_engine::time_bucket::admit_interval(new as i64, min, max);
        }
        *interval_ms as f64
    })
    .range(*range.start()..=reach)
    .speed(speed)
    .custom_formatter(|value, _| quantick_engine::fmt_time_interval(value as i64))
    .custom_parser(|text| {
        #[allow(clippy::cast_precision_loss)]
        quantick_engine::parse_time_interval(text).map(|ms| ms as f64)
    })
}

#[cfg(test)]
mod tests;
