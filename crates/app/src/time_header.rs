//! The time pane's inline timeframe selector
//! (`docs/ux/ui-design-model.md` §11).
//!
//! This row governs the time pane and nothing else, wherever focus sits. It
//! sits in a strip carved off the top of the pane rather than
//! floating over it, so nothing is ever painted across market data.
//!
//! The presets are the timeframes a context chart is actually read at, from a
//! minute to a calendar month; the drag beside them accepts the same fixed
//! interval domain the pane's bars menu does
//! ([`crate::state::MIN_TIME_INTERVAL_MS`]..=[`crate::state::MAX_TIME_INTERVAL_MS`]),
//! so a timeframe this row does not name is still one gesture away — and every
//! preset it does name is a value the other control accepts. A calendar month
//! has no millisecond length to drag, so while one is chosen the drag gives way
//! to its name.

use eframe::egui;

use crate::theme;

/// Height of every pane's header strip, in pixels.
pub const HEIGHT_PX: f32 = 34.0;

/// The named timeframes, in the order they are offered.
pub const PRESETS: [(&str, i64); quantick_engine::bar_registry::TIME_PRESETS.len()] =
    quantick_engine::bar_registry::TIME_PRESETS;

/// The interval a time pane opens on when the split is first shown: M1, the
/// timeframe a flow trader glances at for context. The engine's default for a
/// time spec, so a fresh `bars -> time` and a fresh time pane open alike.
pub const DEFAULT_INTERVAL_MS: i64 = quantick_engine::DEFAULT_TIME_INTERVAL_MS;

/// The presets' text, as the mock sets them.
const PRESET_SIZE: f32 = 12.0;

/// The free interval field's room; narrower, the chips already say enough.
const MIN_INTERVAL_FIELD_PX: f32 = 44.0;

/// What the header laid out this frame.
pub struct HeaderLayout {
    /// Whether the interval changed — a chip click or a drag on the interval.
    pub changed: bool,
    /// Whether the indicators glyph was clicked this frame.
    pub indicators: bool,
    /// Where each preset's chip landed, in [`PRESETS`] order — see
    /// [`Self::chips`].
    #[cfg(test)]
    chips: [egui::Rect; PRESETS.len()],
    /// Where the interval control landed. Recorded for the same reason the
    /// chips are: the geometry a test asserts on has to be the geometry egui
    /// produced, not one derived from font metrics this code does not own.
    #[cfg(test)]
    interval: egui::Rect,
}

impl HeaderLayout {
    /// Where each preset's chip landed, in [`PRESETS`] order.
    ///
    /// Recorded rather than guessed: a test that clicks a timeframe has to
    /// hit the pixels a user would, not a position derived from font metrics
    /// it does not control.
    #[cfg(test)]
    pub(crate) fn chips(&self) -> [egui::Rect; PRESETS.len()] {
        self.chips
    }

    /// Where the interval control landed.
    #[cfg(test)]
    fn interval(&self) -> egui::Rect {
        self.interval
    }
}

/// Draw the header into `strip`.
///
/// `interval_ms` is the pane's own `BarSpec::Time` parameter; the caller
/// applies it through the same deferred spec path a toolbar change takes, so
/// the rebuild is armed and painted exactly once either way.
/// `salt` namespaces every widget this header registers.
///
/// A canvas may now stack more than one context pane, and each carries its own
/// header. egui keys a widget by its parent's id and its order within it, so
/// two headers built from the same parent would answer to one id — the chips
/// of the lower pane would drive the upper one's interval. The pane's own id
/// is what makes them distinct, the same rule `ChartPane::interaction_id`
/// follows.
///
/// `bars` paints the pane's bars control at the head of the row; `presets` says
/// whether the timeframe controls follow it. The indicators button follows those,
/// and its room is what a narrow strip takes from the presets, never from it.
/// Nothing stands at the right end: a time pane's layer lives in its capsule.
pub fn draw(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    interval_ms: &mut i64,
    salt: u64,
    presets: bool,
    bars: impl FnOnce(&mut egui::Ui),
) -> HeaderLayout {
    let mut changed = false;
    #[cfg(test)]
    let mut chips = [egui::Rect::NOTHING; PRESETS.len()];
    #[cfg(test)]
    let mut recorded = chips.iter_mut();
    crate::pane_header::paint_rule(ui, strip);
    let inner = strip.shrink2(crate::pane_header::CONTENT_PADDING);
    let indicators_px = crate::pane_header::indicators_width(ui);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("time_header", salt))
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let limit = (inner.right() - indicators_px).max(strip.left());
    content.set_clip_rect(strip.with_max_x(limit).intersect(ui.clip_rect()));
    bars(&mut content);
    let interval_rect = if presets {
        for (label, preset_ms) in PRESETS {
            let selected = *interval_ms == preset_ms;
            let ink = if selected {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            };
            let chip = content.add(
                egui::Button::new(
                    egui::RichText::new(label)
                        .font(egui::FontId::monospace(PRESET_SIZE))
                        .color(ink),
                )
                .frame(false),
            );
            #[cfg(test)]
            if let Some(slot) = recorded.next() {
                *slot = chip.rect;
            }
            if chip.clicked() && !selected {
                *interval_ms = preset_ms;
                changed = true;
            }
        }
        // A month is a calendar unit, not a duration: dragging its nominal
        // millisecond size would walk into values no chart accepts, and its
        // chip already names it, so nothing stands beside it.
        let fits = content.available_width() >= indicators_px + MIN_INTERVAL_FIELD_PX;
        let interval = (fits
            && quantick_engine::time_bucket::calendar_months(*interval_ms).is_none())
        .then(|| {
            content
                .add(interval_drag(
                    interval_ms,
                    crate::state::MIN_TIME_INTERVAL_MS as f64
                        ..=crate::state::MAX_TIME_INTERVAL_MS as f64,
                    crate::state::TIME_INTERVAL_DRAG_SPEED,
                ))
                .on_hover_text("custom interval for this pane")
        });
        changed |= interval.as_ref().is_some_and(egui::Response::changed);
        interval.map_or(egui::Rect::NOTHING, |interval| interval.rect)
    } else {
        egui::Rect::NOTHING
    };
    #[cfg(not(test))]
    let _ = interval_rect;
    let button_left =
        (content.cursor().left() + quantick_chart::pane_header::GROUP_GAP_PX).min(limit);
    let indicators = crate::pane_header::indicators_button(ui, button_left, strip, salt);
    HeaderLayout {
        changed,
        indicators,
        #[cfg(test)]
        chips,
        #[cfg(test)]
        interval: interval_rect,
    }
}

/// The custom-interval drag, written in the units the chips and the quick
/// switch use — `90s`, `1d`, `1w` — and read back from them or from a bare
/// millisecond count, so a day reads `1d` rather than `86400000 ms`.
///
/// A typed `1mo` sets the month — the setting its chip sets — rather than
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
mod tests {
    use super::*;

    /// The default has to be one of the offered presets, or the pane opens
    /// with no chip lit and the row reads as broken.
    #[test]
    fn the_default_interval_is_one_of_the_named_presets() {
        assert!(PRESETS.iter().any(|(_, ms)| *ms == DEFAULT_INTERVAL_MS));
    }

    /// A strip too narrow for the presets cuts them and drops the free interval
    /// field: the indicators button after them stays whole, or a hand could not
    /// reach it.
    #[test]
    fn a_narrow_strip_keeps_the_indicators_button_whole() {
        let ctx = egui::Context::default();
        let mut interval_ms = DEFAULT_INTERVAL_MS;
        let mut strip = egui::Rect::NOTHING;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 200.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                strip = egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(160.0, HEIGHT_PX));
                let header = draw(ui, strip, &mut interval_ms, 0, true, |_| {});
                assert_eq!(
                    header.interval(),
                    egui::Rect::NOTHING,
                    "no room for the field"
                );
            });
        });
        let button: egui::Rect = ctx
            .data(|data| data.get_temp(crate::pane_header::indicators_id(0)))
            .expect("the button drew");
        assert!(strip.contains_rect(button), "{button:?} left {strip:?}");
    }

    /// Every preset has to be reachable by *both* time-bar controls: the two
    /// set the same `BarSpec::Time`, and 1h used to sit outside what the
    /// bars menu would accept.
    #[test]
    fn every_preset_lies_inside_the_shared_interval_domain() {
        for (label, ms) in PRESETS {
            assert!(
                quantick_engine::bar_registry::is_time_interval(ms),
                "{label} is outside the interval domain both controls share"
            );
            assert_eq!(
                quantick_engine::fmt_time_interval(ms),
                label,
                "a chip reads as the spec it sets"
            );
        }
    }

    /// A typed `1mo` or `3mo` sets the month, never four weeks; a drag past
    /// four weeks stops there, and below the shortest interval at it.
    #[test]
    fn a_typed_month_sets_the_month_and_never_four_weeks() {
        use quantick_engine::time_bucket::admit_interval;
        let (min, max) = (
            crate::state::MIN_TIME_INTERVAL_MS,
            crate::state::MAX_TIME_INTERVAL_MS,
        );
        for typed in ["1mo", "3mo", "12mo"] {
            let ms = quantick_engine::parse_time_interval(typed).expect("an interval");
            assert_eq!(admit_interval(ms, min, max), ms, "{typed}");
            assert_eq!(quantick_engine::fmt_time_interval(ms), typed);
        }
        assert_eq!(admit_interval(max + 1, min, max), max, "four weeks at most");
        assert_eq!(admit_interval(0, min, max), min);
        let week = quantick_engine::time_bucket::WEEK_MS;
        assert_eq!(admit_interval(week, min, max), week);
    }

    /// The day, week and month charts are one click away, and choosing the
    /// month chip lights it rather than leaving a drag holding a value no
    /// chart accepts.
    #[test]
    fn the_calendar_chips_are_offered_and_a_month_stays_a_month() {
        for wanted in ["1d", "1w", "1mo"] {
            assert!(
                PRESETS.iter().any(|(label, _)| *label == wanted),
                "{wanted} is a chip"
            );
        }
        let ctx = egui::Context::default();
        let mut interval_ms = quantick_engine::time_bucket::CALENDAR_MONTH_MS;
        let mut changed = None;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let strip =
                        egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(600.0, HEIGHT_PX));
                    changed = Some(draw(ui, strip, &mut interval_ms, 0, true, |_| {}).changed);
                });
            });
        }
        assert_eq!(changed, Some(false), "drawing a month changes nothing");
        assert_eq!(
            interval_ms,
            quantick_engine::time_bucket::CALENDAR_MONTH_MS,
            "no drag clamps the month to a fixed interval"
        );
    }
}
