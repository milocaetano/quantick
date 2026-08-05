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

use crate::feed::FeedConnectionState;
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
    /// The first live transport is not established yet.
    Connecting,
    /// A previously established live transport is reconnecting.
    Reconnecting,
    /// The provider reports an established live transport.
    Live,
    /// A recorded session is the source.
    Replay,
}

impl FeedState {
    /// Dot colour: green live, amber replay/reconnect, faint connecting.
    #[must_use]
    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Connecting => theme::TEXT_FAINT,
            Self::Reconnecting => theme::WARN,
            Self::Live => theme::BUY,
            Self::Replay => theme::AMBER,
        }
    }

    /// The word next to the dot.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Reconnecting => "reconnecting",
            Self::Live => "live",
            Self::Replay => "replay",
        }
    }
}

/// Classify the feed for the provenance dot.
#[must_use]
pub fn feed_state(replaying: bool, connection: FeedConnectionState) -> FeedState {
    if replaying {
        return FeedState::Replay;
    }
    match connection {
        FeedConnectionState::Connecting => FeedState::Connecting,
        FeedConnectionState::Reconnecting => FeedState::Reconnecting,
        FeedConnectionState::Connected => FeedState::Live,
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
    /// Provider-neutral live transport state, reported by the reconnect loop.
    pub connection: FeedConnectionState,
    /// Exchange-to-screen delay observed when the newest trade arrived; `None`
    /// before the first live trade or while replaying (a recording has no lag
    /// to report).
    /// How late the newest print was when it reached the UI, as observed
    /// when it arrived. Frozen between prints — see [`tape_text`].
    pub feed_arrival_ms: Option<i64>,
    /// Wall clock minus the newest event's own timestamp, recomputed every
    /// frame. This is what catches a transport that stays open and stops
    /// delivering, which no error and no connection state reports.
    pub tape_age_ms: Option<i64>,
    /// The bar spec, e.g. `tick(50)`.
    pub spec_summary: String,
    /// How far the forming bar is from closing, e.g. `37/50 ticks`, when its
    /// rule counts toward a fixed threshold.
    ///
    /// Alternative bars close on activity, not on a clock, so without this the
    /// chart never says whether the next print completes the bar or the
    /// fiftieth does.
    pub bar_progress: Option<String>,
    /// Bars that came from the venue's own candle history, in front of
    /// everything this app built from prints. Zero on any pane without a
    /// venue prefix, which is every flow pane.
    pub venue_bars: usize,
    /// Closed bars built from backfilled history.
    pub backfilled_bars: usize,
    /// Closed bars built live.
    pub live_bars: usize,
    /// Data-honesty label such as `side: inferred (tick rule)`, when the
    /// aggressor side is not venue truth.
    ///
    /// Short enough to sit beside the machinery readouts without pushing into
    /// them — the full story goes in [`StatusModel::side_detail`].
    pub side_note: Option<String>,
    /// The hover text behind [`StatusModel::side_note`], where the disclosure
    /// can be as long as it needs to be. `None` reuses the label itself.
    pub side_detail: Option<String>,
    /// The paper-trading cell: `SIM ±N pts` plus its sign for color, `None`
    /// while the simulator has never been touched (an idle chart owes no
    /// account line).
    pub sim_pnl: Option<(String, std::cmp::Ordering)>,
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

/// The tape cell: `arrival 123 ms` live, `stale 12 s` when the tape has gone
/// quiet, `10× 45%` while replaying, `arrival —` before the first trade.
///
/// Two different measurements, and the distinction is the point. *Arrival* is
/// how late the newest print was when it reached the UI — an observation
/// stored when that print arrived, which stops ageing the moment prints stop.
/// *Staleness* is wall clock minus the newest event's own timestamp, computed
/// every frame. A socket that stays open and delivers nothing keeps a healthy
/// arrival figure forever, so the honest readout is the age of the tape.
#[must_use]
pub fn tape_text(
    replay: Option<ReplayFigures>,
    arrival_ms: Option<i64>,
    tape_age_ms: Option<i64>,
) -> String {
    if let Some(figures) = replay {
        return format!(
            "{:.0}× {:>3.0}%",
            figures.speed,
            figures.progress.clamp(0.0, 1.0) * 100.0
        );
    }
    match (tape_age_ms, arrival_ms) {
        (Some(age), _) if age > metrics::STALE_TAPE_MS => {
            format!("stale {} s", age / 1_000)
        }
        (_, Some(arrival)) => format!("arrival {arrival} ms"),
        (_, None) => "arrival —".to_owned(),
    }
}

/// The bar-count cell, keeping the backfilled+live split: `240+61 bars`.
#[must_use]
pub fn bars_text(venue: usize, backfilled: usize, live: usize) -> String {
    // Three sources, three counts, in the order they sit on the chart. The
    // venue term is dropped rather than shown as zero: a chart with no venue
    // prefix has nothing to disclose about one, and "0+" would read as a
    // failed fetch.
    if venue == 0 {
        return format!("{backfilled}+{live} bars");
    }
    format!("{venue}v+{backfilled}+{live} bars")
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
    let state = feed_state(model.replay.is_some(), model.connection);
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
    let stale = model
        .tape_age_ms
        .is_some_and(|age| age > metrics::STALE_TAPE_MS);
    let tape_color = match (state, model.feed_arrival_ms) {
        (FeedState::Replay, _) => theme::AMBER,
        // A quiet tape and a late print are both worth a warning, and a
        // wedged socket only shows up as the first.
        _ if stale => theme::WARN,
        (_, Some(arrival)) if arrival > metrics::HIGH_LAG_MS => theme::WARN,
        (FeedState::Connecting | FeedState::Reconnecting | FeedState::Live, _) => theme::TEXT_MUTED,
    };
    // The two measurements this cell can show are easy to conflate, so the
    // cell explains itself (audit: every readout answers on hover).
    ui.label(
        egui::RichText::new(tape_text(
            model.replay,
            model.feed_arrival_ms,
            model.tape_age_ms,
        ))
        .monospace()
        .color(tape_color),
    )
    .on_hover_text(
        "arrival: how late the newest print was when it reached the chart; \
         stale: how long the tape has been silent (a socket can stay open \
         and deliver nothing); replay shows speed and progress instead",
    );
}

/// Middle section: bar spec, counts, honesty labels and navigation hints.
fn draw_content(ui: &mut egui::Ui, model: &StatusModel) {
    ui.label(
        egui::RichText::new(&model.spec_summary)
            .monospace()
            .color(theme::TEXT_PRIMARY),
    );
    if let Some(progress) = &model.bar_progress {
        ui.label(
            egui::RichText::new(progress)
                .monospace()
                .color(theme::AMBER),
        )
        .on_hover_text("how far the forming bar is from closing");
    }
    ui.label(
        egui::RichText::new(bars_text(
            model.venue_bars,
            model.backfilled_bars,
            model.live_bars,
        ))
        .monospace()
        .color(theme::TEXT_MUTED),
    )
    .on_hover_text(
        "closed bars by source, oldest first: venue candles (v), bars built \
         from backfilled prints, bars built live",
    );
    if let Some(note) = &model.side_note {
        // Inferred data wears the amber thread, like every not-quite-venue
        // truth on this chart. Kept short on purpose: this section shares its
        // row with the machinery readouts, and a long label overruns them.
        ui.label(egui::RichText::new(note).small().color(theme::AMBER))
            .on_hover_text(model.side_detail.as_deref().unwrap_or(note.as_str()));
    }
    if let Some((text, sign)) = &model.sim_pnl {
        let color = match sign {
            std::cmp::Ordering::Greater => theme::BUY,
            std::cmp::Ordering::Less => theme::SELL,
            std::cmp::Ordering::Equal => theme::TEXT_MUTED,
        };
        ui.label(egui::RichText::new(text).monospace().color(color))
            .on_hover_text(
                "paper-trading P&L (realized + open) in points - simulated fills, \
                 not a broker account",
            );
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
        assert_eq!(
            feed_state(false, FeedConnectionState::Connecting),
            FeedState::Connecting
        );
        assert_eq!(
            feed_state(false, FeedConnectionState::Reconnecting),
            FeedState::Reconnecting
        );
        assert_eq!(
            feed_state(false, FeedConnectionState::Connected),
            FeedState::Live
        );
        // A recording is replay, whatever the displaced live transport says.
        assert_eq!(
            feed_state(true, FeedConnectionState::Connecting),
            FeedState::Replay
        );
        assert_eq!(
            feed_state(true, FeedConnectionState::Connected),
            FeedState::Replay
        );
    }

    #[test]
    fn dot_colours_follow_the_tokens() {
        assert_eq!(FeedState::Live.color(), theme::BUY);
        assert_eq!(FeedState::Replay.color(), theme::AMBER);
        assert_eq!(FeedState::Reconnecting.color(), theme::WARN);
        assert_eq!(FeedState::Connecting.color(), theme::TEXT_FAINT);
    }

    #[test]
    fn lag_cell_reports_replay_live_and_waiting() {
        let replay = Some(ReplayFigures {
            speed: 10.0,
            progress: 0.45,
        });
        assert_eq!(tape_text(replay, None, None), "10×  45%");
        assert_eq!(tape_text(None, Some(230), Some(300)), "arrival 230 ms");
        // The tape has gone quiet: the stored arrival figure is stale itself.
        assert_eq!(
            tape_text(None, Some(42), Some(12_000)),
            "stale 12 s",
            "a wedged socket keeps a healthy-looking arrival forever"
        );
        assert_eq!(tape_text(None, None, None), "arrival —");
    }

    #[test]
    fn replay_progress_is_clamped_to_a_percentage() {
        let over = Some(ReplayFigures {
            speed: 1.0,
            progress: 1.7,
        });
        assert_eq!(tape_text(over, None, None), "1× 100%");
    }

    #[test]
    fn bar_counts_keep_the_backfilled_live_split() {
        assert_eq!(bars_text(0, 240, 61), "240+61 bars");
        assert_eq!(bars_text(0, 0, 0), "0+0 bars");
    }

    /// A time pane standing on venue history says so, in the same cell and the
    /// same order the chart puts the three sources in.
    #[test]
    fn a_venue_prefix_becomes_a_third_term() {
        assert_eq!(bars_text(26_000, 240, 61), "26000v+240+61 bars");
        assert_eq!(
            bars_text(0, 240, 61),
            "240+61 bars",
            "a chart with no prefix discloses nothing about one; `0v+` would read as a fetch that failed"
        );
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
                connection: FeedConnectionState::Connected,
                feed_arrival_ms: (!replaying).then_some(120),
                tape_age_ms: (!replaying).then_some(200),
                spec_summary: "tick(50)".to_owned(),
                bar_progress: Some("37/50 ticks".to_owned()),
                venue_bars: 0,
                backfilled_bars: 240,
                live_bars: 61,
                side_note: replaying.then(|| "side: inferred (tick rule)".to_owned()),
                side_detail: None,
                sim_pnl: Some(("SIM +12.5 pts".to_owned(), std::cmp::Ordering::Greater)),
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

    /// The quote-driven disclosure has to reach actual pixels, not just the
    /// model: a chart of one-unit prints that says nothing reads as a real
    /// tape where every trade happened to be the same size.
    #[test]
    fn the_quote_driven_disclosure_is_really_painted() {
        let ctx = egui::Context::default();
        let mut tz = TzOffset::default();
        let model = StatusModel {
            venue: "MetaTrader 5".to_owned(),
            symbol: "US500".to_owned(),
            replay: None,
            connection: FeedConnectionState::Connected,
            feed_arrival_ms: Some(106),
            tape_age_ms: Some(150),
            spec_summary: "tick(50)".to_owned(),
            bar_progress: Some("37/50 ticks".to_owned()),
            venue_bars: 0,
            backfilled_bars: 3_999,
            live_bars: 17,
            side_note: Some("prints: quote-derived".to_owned()),
            side_detail: Some(
                "this venue quotes prices but prints no trades: every candle is built \n                 from one synthetic print per tick"
                    .to_owned(),
            ),
            sim_pnl: None,
            follows_live: true,
            price_auto: true,
            live_trades: 846,
            fps: Some(60.0),
            frame_avg_ms: Some(16.7),
            frame_cpu_ms: Some(4.2),
            show_perf: false,
        };
        // Two passes: egui settles its layout on the second.
        let mut painted = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| draw(ctx, &model, &mut tz));
            painted.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(text) = shape.shape {
                    painted.push_str(text.galley.text());
                    painted.push(' ');
                }
            }
        }
        assert!(
            painted.contains("prints: quote-derived"),
            "the honesty label never reached the screen; painted: {painted}"
        );
        assert!(painted.contains("US500"), "painted: {painted}");
    }
}
