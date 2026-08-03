//! The time pane's inline timeframe selector (§11).
//!
//! Two selectors, two panes, no modes: the toolbar's BARS group governs the
//! flow pane and this row governs the time pane. It sits in a strip carved off
//! the top of the pane rather than floating over it, so nothing is ever
//! painted across market data.
//!
//! The presets are the four timeframes a context chart is actually read at;
//! the drag beside them is the same custom interval the BARS group offers, so
//! a timeframe this row does not name is still one gesture away.

use eframe::egui;

use crate::theme;

/// Height of the header strip, in pixels. Matches the bottom time axis, so the
/// two horizontal rules framing a time pane read as one pair.
pub const HEIGHT_PX: f32 = 24.0;

/// The named timeframes, in the order they are offered.
pub const PRESETS: [(&str, i64); 4] = [
    ("1m", 60_000),
    ("5m", 300_000),
    ("15m", 900_000),
    ("1h", 3_600_000),
];

/// The interval a time pane opens on when the split is first shown: M1, the
/// timeframe a flow trader glances at for context.
pub const DEFAULT_INTERVAL_MS: i64 = 60_000;

/// Bounds of the custom interval drag. One second is the finest a *context*
/// chart is worth reading at; a day is the coarsest that still fits a session.
const MIN_INTERVAL_MS: f64 = 1_000.0;
/// See [`MIN_INTERVAL_MS`].
const MAX_INTERVAL_MS: f64 = 86_400_000.0;
/// Milliseconds per drag point — a minute per two points, so a whole preset
/// range is reachable without the value running away.
const INTERVAL_DRAG_SPEED: f64 = 500.0;

/// What the header laid out this frame.
pub struct HeaderLayout {
    /// Whether the interval changed — a chip click or a drag on the interval.
    pub changed: bool,
    /// Where each preset's chip landed, in [`PRESETS`] order.
    ///
    /// Test-only, and recorded rather than guessed: a test that clicks a
    /// timeframe has to hit the pixels a user would, not a position derived
    /// from font metrics it does not control.
    #[cfg(test)]
    pub chips: [egui::Rect; PRESETS.len()],
}

/// Draw the header into `strip`.
///
/// `interval_ms` is the pane's own `BarSpec::Time` parameter; the caller
/// applies it through the same deferred spec path a toolbar change takes, so
/// the rebuild is armed and painted exactly once either way.
pub fn draw(ui: &mut egui::Ui, strip: egui::Rect, interval_ms: &mut i64) -> HeaderLayout {
    let mut changed = false;
    #[cfg(test)]
    let mut chips = [egui::Rect::NOTHING; PRESETS.len()];
    #[cfg(test)]
    let mut recorded = chips.iter_mut();
    ui.painter().line_segment(
        [
            egui::pos2(strip.left(), strip.bottom()),
            egui::pos2(strip.right(), strip.bottom()),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip.shrink2(egui::vec2(8.0, 2.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    content.label(egui::RichText::new("time").small().color(theme::TEXT_FAINT));
    for (label, preset_ms) in PRESETS {
        let selected = *interval_ms == preset_ms;
        let chip = content.selectable_label(selected, label);
        #[cfg(test)]
        if let Some(slot) = recorded.next() {
            *slot = chip.rect;
        }
        if chip.clicked() && !selected {
            *interval_ms = preset_ms;
            changed = true;
        }
    }
    changed |= content
        .add(
            egui::DragValue::new(interval_ms)
                .range(MIN_INTERVAL_MS..=MAX_INTERVAL_MS)
                .speed(INTERVAL_DRAG_SPEED)
                .suffix(" ms"),
        )
        .on_hover_text("custom interval for this pane")
        .changed();
    HeaderLayout {
        changed,
        #[cfg(test)]
        chips,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default has to be one of the offered presets, or the pane opens
    /// with no chip lit and the row reads as broken.
    #[test]
    fn the_default_interval_is_one_of_the_named_presets() {
        assert!(PRESETS.iter().any(|(_, ms)| *ms == DEFAULT_INTERVAL_MS));
    }

    /// Every preset has to be reachable by the drag too, otherwise clicking a
    /// chip would set a value the custom control then refuses to show.
    #[test]
    fn every_preset_lies_inside_the_custom_drag_range() {
        for (label, ms) in PRESETS {
            let ms = ms as f64;
            assert!(
                (MIN_INTERVAL_MS..=MAX_INTERVAL_MS).contains(&ms),
                "{label} is outside the custom interval range"
            );
        }
    }
}
