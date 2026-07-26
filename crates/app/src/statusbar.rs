//! The status bar: one 28 px line answering "how healthy is it?"
//! (`docs/ux/ui-design-model.md` §8).
//!
//! Three sections replace the perf overlay, the floating timezone pill and
//! the mode text that used to be painted over candles: provenance on the
//! left (state dot, venue, symbol, lag), content in the middle (bar spec,
//! counts, honesty labels), machinery on the right (trades, fps, and the
//! bar's only control — the timezone picker). A reading that breaches its
//! threshold turns [`theme::WARN`]; the layout never moves.
//!
//! The section values are computed by the app and handed in as a plain
//! [`StatusModel`], so every text and colour decision here stays pure and
//! unit-testable.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::metrics;
use crate::theme;
use crate::timezone::TzOffset;

/// Height of the status line, in pixels (§5 zone 7).
pub const STATUS_BAR_HEIGHT: f32 = 28.0;
/// Diameter of the provenance state dot, in pixels.
const STATE_DOT_DIAMETER_PX: f32 = 8.0;
/// Gap between neighbouring cells on the line, in pixels.
const CELL_SPACING_PX: f32 = 8.0;

/// What the provenance dot reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedState {
    /// No trade has arrived yet.
    Connecting,
    /// Live and within the lag threshold.
    Live,
    /// Live but the newest trade is older than [`metrics::HIGH_LAG_MS`].
    Stalled,
    /// A recorded session is the source.
    Replay,
}

impl FeedState {
    /// Dot colour: green live, amber replay, red stalled, faint connecting.
    #[must_use]
    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Connecting => theme::TEXT_FAINT,
            Self::Live => theme::BUY,
            Self::Stalled => theme::WARN,
            Self::Replay => theme::AMBER,
        }
    }

    /// The word next to the dot.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Live => "live",
            Self::Stalled => "stalled",
            Self::Replay => "replay",
        }
    }
}

/// Classify the feed for the provenance dot.
#[must_use]
pub fn feed_state(replaying: bool, lag_ms: Option<i64>) -> FeedState {
    if replaying {
        return FeedState::Replay;
    }
    match lag_ms {
        None => FeedState::Connecting,
        Some(lag) if lag > metrics::HIGH_LAG_MS => FeedState::Stalled,
        Some(_) => FeedState::Live,
    }
}

/// Replay figures for the provenance section, read from the transport link.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayFigures {
    /// Playback speed multiplier.
    pub speed: f32,
    /// Played fraction in `[0, 1]`.
    pub progress: f32,
}

/// Everything the bar shows this frame, precomputed by the app.
pub struct StatusModel {
    /// Display name of the venue (or the session, while replaying).
    pub venue: String,
    /// The streamed symbol.
    pub symbol: String,
    /// Replay speed/progress while a recording plays, `None` when live.
    pub replay: Option<ReplayFigures>,
    /// Exchange-to-screen lag of the newest trade; `None` before the first
    /// trade or while replaying (a recording has no lag to report).
    pub feed_lag_ms: Option<i64>,
    /// The bar spec, e.g. `tick(50)`.
    pub spec_summary: String,
    /// Closed bars built from backfilled history.
    pub backfilled_bars: usize,
    /// Closed bars built live.
    pub live_bars: usize,
    /// Data-honesty label such as `side: inferred (tick rule)`, when the
    /// aggressor side is not venue truth.
    pub side_note: Option<String>,
    /// Whether the viewport follows the live edge.
    pub follows_live: bool,
    /// Whether the price axis is auto-fitting.
    pub price_auto: bool,
    /// Live trades ingested since the feed started.
    pub live_trades: u64,
    /// Rolling frames per second.
    pub fps: Option<f32>,
    /// Rolling mean frame time, in milliseconds.
    pub frame_avg_ms: Option<f32>,
    /// Rolling mean CPU cost per frame (update + tessellation + paint, no
    /// vsync wait), in milliseconds.
    pub frame_cpu_ms: Option<f32>,
    /// Whether the perf readings (fps, frame time, trades) are shown
    /// (View → perf readings).
    pub show_perf: bool,
}

/// The lag/progress cell: `lag 123 ms` live, `10× 45%` while replaying,
/// `lag —` before the first trade.
#[must_use]
pub fn lag_text(replay: Option<ReplayFigures>, lag_ms: Option<i64>) -> String {
    match (replay, lag_ms) {
        (Some(figures), _) => format!(
            "{:.0}× {:>3.0}%",
            figures.speed,
            figures.progress.clamp(0.0, 1.0) * 100.0
        ),
        (None, Some(lag)) => format!("lag {lag} ms"),
        (None, None) => "lag —".to_owned(),
    }
}

/// The bar-count cell, keeping the backfilled+live split: `240+61 bars`.
#[must_use]
pub fn bars_text(backfilled: usize, live: usize) -> String {
    format!("{backfilled}+{live} bars")
}

/// Draw the status bar as the window's bottom panel. `tz` is the bar's only
/// control; picking an offset relabels the time axis immediately.
pub fn draw(ctx: &egui::Context, model: &StatusModel, tz: &mut TzOffset) {
    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(STATUS_BAR_HEIGHT)
        .frame(
            egui::Frame::none()
                .fill(theme::CHROME)
                .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = CELL_SPACING_PX;
                draw_provenance(ui, model);
                ui.separator();
                draw_content(ui, model);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    draw_machinery(ui, model, tz)
                });
            });
        });
}

/// Left section: state dot, venue, symbol, lag.
fn draw_provenance(ui: &mut egui::Ui, model: &StatusModel) {
    let state = feed_state(model.replay.is_some(), model.feed_lag_ms);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(STATE_DOT_DIAMETER_PX, STATE_DOT_DIAMETER_PX),
        egui::Sense::hover(),
    );
    ui.painter()
        .circle_filled(rect.center(), STATE_DOT_DIAMETER_PX / 2.0, state.color());
    ui.label(
        egui::RichText::new(state.label())
            .small()
            .color(state.color()),
    );
    ui.label(egui::RichText::new(&model.venue).color(theme::TEXT_MUTED));
    ui.label(
        egui::RichText::new(&model.symbol)
            .monospace()
            .color(theme::TEXT_PRIMARY),
    );
    let lag_color = match state {
        FeedState::Stalled => theme::WARN,
        FeedState::Replay => theme::AMBER,
        FeedState::Live | FeedState::Connecting => theme::TEXT_MUTED,
    };
    ui.label(
        egui::RichText::new(lag_text(model.replay, model.feed_lag_ms))
            .monospace()
            .color(lag_color),
    );
}

/// Middle section: bar spec, counts, honesty labels and navigation hints.
fn draw_content(ui: &mut egui::Ui, model: &StatusModel) {
    ui.label(
        egui::RichText::new(&model.spec_summary)
            .monospace()
            .color(theme::TEXT_PRIMARY),
    );
    ui.label(
        egui::RichText::new(bars_text(model.backfilled_bars, model.live_bars))
            .monospace()
            .color(theme::TEXT_MUTED),
    );
    if let Some(note) = &model.side_note {
        // Inferred data wears the amber thread, like every not-quite-venue
        // truth on this chart.
        ui.label(egui::RichText::new(note).small().color(theme::AMBER));
    }
    if !model.follows_live {
        ui.label(
            egui::RichText::new("history · double-click for live")
                .small()
                .color(theme::TEXT_FAINT),
        );
    }
    if !model.price_auto {
        ui.label(
            egui::RichText::new("price: manual · double-click the axis to auto-fit")
                .small()
                .color(theme::TEXT_FAINT),
        );
    }
}

/// Right section, laid out right-to-left: timezone picker at the far edge,
/// then the perf readings.
fn draw_machinery(ui: &mut egui::Ui, model: &StatusModel, tz: &mut TzOffset) {
    egui::ComboBox::from_id_salt("tz_combo")
        .selected_text(tz.label())
        .show_ui(ui, |ui| {
            for offset in TzOffset::ALL {
                ui.selectable_value(tz, offset, offset.label());
            }
        });
    ui.label(egui::RichText::new(icons::CLOCK).color(theme::TEXT_MUTED));
    if !model.show_perf {
        return;
    }
    ui.separator();
    let slow = model
        .frame_avg_ms
        .is_some_and(|avg| avg > metrics::SLOW_FRAME_MS);
    let fps_color = if slow { theme::WARN } else { theme::TEXT_MUTED };
    // The cpu cost rides along because frame time alone hides behind vsync:
    // a real incident needed exactly this split to tell "we are slow" from
    // "we are waiting for the display".
    ui.label(
        egui::RichText::new(format!(
            "{:>4.0} fps · {:>5.1} ms · cpu {:>4.1} ms",
            model.fps.unwrap_or(0.0),
            model.frame_avg_ms.unwrap_or(0.0),
            model.frame_cpu_ms.unwrap_or(0.0)
        ))
        .monospace()
        .color(fps_color),
    );
    ui.separator();
    ui.label(
        egui::RichText::new(format!("{} trades", model.live_trades))
            .monospace()
            .color(theme::TEXT_MUTED),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dot_reads_the_feed_honestly() {
        assert_eq!(feed_state(false, None), FeedState::Connecting);
        assert_eq!(feed_state(false, Some(120)), FeedState::Live);
        assert_eq!(
            feed_state(false, Some(metrics::HIGH_LAG_MS + 1)),
            FeedState::Stalled
        );
        // A recording is replay, whatever its (meaningless) lag would be.
        assert_eq!(feed_state(true, None), FeedState::Replay);
        assert_eq!(feed_state(true, Some(999_999)), FeedState::Replay);
    }

    #[test]
    fn dot_colours_follow_the_tokens() {
        assert_eq!(FeedState::Live.color(), theme::BUY);
        assert_eq!(FeedState::Replay.color(), theme::AMBER);
        assert_eq!(FeedState::Stalled.color(), theme::WARN);
        assert_eq!(FeedState::Connecting.color(), theme::TEXT_FAINT);
    }

    #[test]
    fn lag_cell_reports_replay_live_and_waiting() {
        let replay = Some(ReplayFigures {
            speed: 10.0,
            progress: 0.45,
        });
        assert_eq!(lag_text(replay, None), "10×  45%");
        assert_eq!(lag_text(None, Some(230)), "lag 230 ms");
        assert_eq!(lag_text(None, None), "lag —");
    }

    #[test]
    fn replay_progress_is_clamped_to_a_percentage() {
        let over = Some(ReplayFigures {
            speed: 1.0,
            progress: 1.7,
        });
        assert_eq!(lag_text(over, None), "1× 100%");
    }

    #[test]
    fn bar_counts_keep_the_backfilled_live_split() {
        assert_eq!(bars_text(240, 61), "240+61 bars");
        assert_eq!(bars_text(0, 0), "0+0 bars");
    }

    /// Lay the bar out for real, off-screen, in its live and replay shapes —
    /// every section drawn, no duplicated widget id, no panicking widget.
    #[test]
    fn the_status_bar_lays_out_against_a_real_context() {
        let ctx = egui::Context::default();
        let mut tz = TzOffset::default();
        for replaying in [false, true] {
            let model = StatusModel {
                venue: "Binance".to_owned(),
                symbol: "BTCUSDT".to_owned(),
                replay: replaying.then_some(ReplayFigures {
                    speed: 10.0,
                    progress: 0.4,
                }),
                feed_lag_ms: (!replaying).then_some(120),
                spec_summary: "tick(50)".to_owned(),
                backfilled_bars: 240,
                live_bars: 61,
                side_note: replaying.then(|| "side: inferred (tick rule)".to_owned()),
                follows_live: false,
                price_auto: false,
                live_trades: 12_345,
                fps: Some(60.0),
                frame_avg_ms: Some(16.7),
                frame_cpu_ms: Some(4.2),
                show_perf: true,
            };
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| draw(ctx, &model, &mut tz));
            }
        }
        assert_eq!(
            tz,
            TzOffset::default(),
            "an un-clicked frame changes nothing"
        );
    }
}
