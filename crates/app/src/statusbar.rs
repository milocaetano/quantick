//! The status bar: one 28 px line answering "how healthy is it?"
//! (`docs/ux/ui-design-model.md` §8).
//!
//! Three sections replace the perf overlay, the floating timezone pill and
//! the mode text that used to be painted over candles: provenance on the
//! left (state dot, venue, symbol, lag), content in the middle (bar spec,
//! counts, honesty labels), machinery on the right (trades, fps, and the
//! timezone picker). A reading that breaches its threshold turns
//! [`theme::WARN`]; the layout never moves.
//!
//! The timezone picker is the bar's only control. It briefly had company —
//! a `Reconnect`/`Reload` pair in the provenance section — because the notice
//! card refused to cover a working chart, and a terminal that froze
//! mid-session left the trader nothing to press anywhere in the window. The
//! offline chip in the chart's bottom-right corner answers that better: it is
//! in one place whatever the chart holds, so the pair went back out rather
//! than stand as a second way to do the same two things.
//!
//! What the corner did leave behind is a duty. The provenance dot used to read
//! the connection alone, which is a socket's opinion — a frozen terminal had
//! this line saying `live` while the corner said `offline`, about the same
//! feed, at the same moment. So [`draw`] is handed the corner's own colour
//! when there is one, and the dot yields to it. One report, two surfaces; see
//! [`crate::feed_notice`].
//!
//! The section values are computed by the app and handed in as a plain
//! [`StatusModel`], so every text and colour decision here stays pure and
//! unit-testable.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::feed_notice;
use crate::metrics;
use crate::theme;
use crate::timezone::TzOffset;
use quantick_feed::{FeedConnectionState, FeedLatency};
use quantick_orderflow as orderflow;

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
    /// Where that delay is being spent, when the provider can cut its own
    /// chain. `None` on a provider that cannot, and before the first sample.
    ///
    /// The cell shows the hop's name beside the figure and puts the numbers in
    /// the hover: a trader mid-session needs one word to know whether to look
    /// at their terminal or at this chart, and the breakdown only when they ask.
    pub feed_latency: Option<FeedLatency>,
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

/// The delay the tape cell reports, and the sample the hop beside it came from.
///
/// A provider that cut its own chain reports the total *its own halves add up
/// to*, measured where it read the print off the wire. The chart's own
/// end-to-end figure — the one that also counts the queue and the frame — stays
/// available and is named separately in the hover.
///
/// Showing one of those totals with a breakdown of the other is what this
/// exists to prevent: three numbers that do not reconcile, on a readout whose
/// entire selling point is that two of them sum to the third.
#[must_use]
pub fn tape_arrival_ms(arrival_ms: Option<i64>, latency: Option<FeedLatency>) -> Option<i64> {
    latency.map(|split| split.arrival_lag_ms).or(arrival_ms)
}

/// The tape cell: `arrival 123 ms` live, `stale 12 s` / `stale 14 h` when the
/// tape has gone
/// quiet, `10× 45%` while replaying, `arrival —` before the first trade.
///
/// Two different measurements, and the distinction is the point. *Arrival* is
/// how late the newest print was when it reached the chart — an observation
/// stored when that print arrived, which stops ageing the moment prints stop.
/// *Staleness* is wall clock minus the newest event's own timestamp, computed
/// every frame. A socket that stays open and delivers nothing keeps a healthy
/// arrival figure forever, so the honest readout is the age of the tape.
///
/// A late tape that knows *where* the time went says so by **replacing** the
/// word `arrival` with the hop's name: `MT5 4210 ms`. Replacing rather than
/// appending, because this row has no width budget between its three sections —
/// a label that grows in the middle overruns the machinery readouts on the
/// right, which is what the side-note comment in [`draw_content`] has always
/// warned about, and it was reproduced at 1000 px by an earlier version of this
/// cell that appended ` · bridge`. Swapped rather than appended, and swapped
/// for something no wider: `LatencyHop::MAX_LABEL_CHARS` is the width of the
/// word `arrival` itself, so the cell can only get narrower by knowing more.
/// Both ends assert it.
///
/// Nothing is lost by dropping `arrival`: the word was only ever explained by
/// the hover, which still explains both readings and now carries the numbers
/// too. The name appears only once the delay is worth acting on
/// ([`metrics::HIGH_LAG_MS`]) — a healthy tape does not need to tell anyone
/// which of its hops was nominally slowest.
#[must_use]
pub fn tape_text(
    replay: Option<ReplayFigures>,
    arrival_ms: Option<i64>,
    tape_age_ms: Option<i64>,
    latency: Option<FeedLatency>,
) -> String {
    if let Some(figures) = replay {
        return format!(
            "{:.0}× {:>3.0}%",
            figures.speed,
            figures.progress.clamp(0.0, 1.0) * 100.0
        );
    }
    match (tape_age_ms, tape_arrival_ms(arrival_ms, latency)) {
        (Some(age), _) if age > metrics::STALE_TAPE_MS => {
            // The same words the corner's popup and the chart's gap seam use.
            // A tape fourteen hours old is the ordinary state of a chart
            // opened before the open, and `stale 50400 s` is a true sentence
            // nobody reads as "yesterday".
            format!("stale {}", quantick_feed::stall::spoken_ms(age))
        }
        (_, Some(arrival)) => {
            // Gated on the figure shown, which is the figure the hop was
            // chosen from: a hop picked for one measurement and revealed by
            // another can name a culprit for a delay it is not displaying.
            let hop = latency
                .filter(|_| arrival > metrics::HIGH_LAG_MS)
                .and_then(|split| split.hop);
            match hop {
                Some(hop) => format!("{hop} {arrival} ms"),
                None => format!("arrival {arrival} ms"),
            }
        }
        (_, None) => "arrival —".to_owned(),
    }
}

/// What the tape cell always says on hover, split or no split.
const TAPE_TOOLTIP_BASE: &str = "arrival: how late the newest print was when it \
     reached the chart; stale: how long the tape has been silent (a socket can \
     stay open and deliver nothing); replay shows speed and progress instead";

/// The hover text under the tape cell.
///
/// Always explains the two measurements the cell can show, because they are
/// easy to conflate. When the provider has cut its own chain the split follows
/// — the numbers behind the one word on the cell, so a trader who wants to know
/// *how much* of the delay is theirs to fix can read it without leaving the
/// chart.
///
/// Every figure says where it was taken. The feed measures at the socket and
/// the chart measures at the frame, so the two totals are not the same number
/// and must never be printed as though they were; their *difference* is what
/// quantick's own queueing and drawing cost, and that is the one hop neither
/// side can see alone.
#[must_use]
pub fn tape_tooltip(arrival_ms: Option<i64>, latency: Option<FeedLatency>) -> String {
    let mut text = String::from(TAPE_TOOLTIP_BASE);
    let Some(split) = latency else {
        return text;
    };
    let ms = orderflow::format_window_ms;
    // Both halves or neither: they come from the same pair of stamps, and one
    // alone would invite the reader to infer the other by subtracting from a
    // total that has a different sample behind it.
    if let (Some(source), Some(transport)) = (split.source_lag_ms, split.transport_lag_ms) {
        text.push_str(&format!(
            "\n\nthe feed measured {} from the venue's stamp: {} before it was \
             handed over, {} from there to the socket",
            ms(split.arrival_lag_ms),
            ms(source),
            ms(transport),
        ));
        if let Some(hop) = split.hop {
            text.push_str(&format!(" — most of it in {hop}"));
        }
    }
    if let Some(peak) = split.source_lag_peak_ms {
        text.push_str(&format!(
            "\nslowest handover in the last {} prints: {}",
            split.prints,
            ms(peak),
        ));
    }
    // The hop neither side can see alone. Two measurements taken at two
    // instants, so the difference is an estimate — said out loud rather than
    // presented as a third measured figure.
    if let Some(drawn) = arrival_ms {
        let queued = drawn - split.arrival_lag_ms;
        if queued > 0 {
            text.push_str(&format!(
                "\nthe chart drew it {} after the venue's stamp, so roughly {} \
                 of that was the chart's own queue and drawing",
                ms(drawn),
                ms(queued),
            ));
        }
    }
    text
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

/// What a click on the bar asked of the app this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StatusResponse {
    /// The SIM cell was clicked: open the Trading dock tab, where the
    /// position it summarizes is managed.
    pub open_trading_tab: bool,
}

/// Draw the status bar as the window's bottom panel. `tz` is the bar's
/// resident control, and the SIM cell clicks through to the Trading tab.
///
/// `offline` is the accent the chart's corner is wearing this frame, or `None`
/// while the chart is being fed. The provenance dot takes it as an override
/// rather than deciding for itself, which is what keeps the two ends of the
/// window from disagreeing about the same feed.
pub fn draw(
    ctx: &egui::Context,
    model: &StatusModel,
    tz: &mut TzOffset,
    offline: Option<egui::Color32>,
) -> StatusResponse {
    let mut response = StatusResponse::default();
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
                draw_provenance(ui, model, offline);
                ui.separator();
                response.open_trading_tab = draw_content(ui, model);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    draw_machinery(ui, model, tz)
                });
            });
        });
    response
}

/// What the provenance dot says, and in what colour.
///
/// The corner wins whenever there is one. Pure, because the rule it keeps —
/// this line never claims `live` about a chart the corner calls `offline` —
/// is one worth asserting rather than eyeballing.
#[must_use]
pub fn provenance_dot(
    state: FeedState,
    offline: Option<egui::Color32>,
) -> (egui::Color32, &'static str) {
    match offline {
        Some(accent) => (accent, feed_notice::OFFLINE_LABEL),
        None => (state.color(), state.label()),
    }
}

/// Left section: state dot, venue, symbol, lag.
///
/// The dot answers what the corner answers, in the words this line has room
/// for: `offline` and the corner's colour whenever the chart is not being fed,
/// the transport's own state otherwise.
fn draw_provenance(ui: &mut egui::Ui, model: &StatusModel, offline: Option<egui::Color32>) {
    let state = feed_state(model.replay.is_some(), model.connection);
    let (dot_color, dot_label) = provenance_dot(state, offline);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(STATE_DOT_DIAMETER_PX, STATE_DOT_DIAMETER_PX),
        egui::Sense::hover(),
    );
    ui.painter()
        .circle_filled(rect.center(), STATE_DOT_DIAMETER_PX / 2.0, dot_color);
    ui.label(egui::RichText::new(dot_label).small().color(dot_color));
    ui.label(egui::RichText::new(&model.venue).color(theme::TEXT_MUTED));
    ui.label(
        egui::RichText::new(&model.symbol)
            .monospace()
            .color(theme::TEXT_PRIMARY),
    );
    let stale = model
        .tape_age_ms
        .is_some_and(|age| age > metrics::STALE_TAPE_MS);
    // The same figure the cell prints, so a cell reading nine seconds cannot
    // be coloured from a different measurement reading two hundred milliseconds.
    let shown_arrival = tape_arrival_ms(model.feed_arrival_ms, model.feed_latency);
    let tape_color = match (state, shown_arrival) {
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
            model.feed_latency,
        ))
        .monospace()
        .color(tape_color),
    )
    // `on_hover_ui`, not `on_hover_text`: the split's sentences are built with
    // `format!`, and `on_hover_text` evaluates its argument on every frame
    // whether or not anyone is pointing at the cell. A per-frame allocation for
    // a tooltip nobody is reading is exactly the trade this repo does not make.
    .on_hover_ui(|ui| {
        ui.label(tape_tooltip(model.feed_arrival_ms, model.feed_latency));
    });
}

/// Middle section: bar spec, counts, honesty labels and navigation hints.
/// Returns whether the SIM cell was clicked.
fn draw_content(ui: &mut egui::Ui, model: &StatusModel) -> bool {
    let mut sim_clicked = false;
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
        // A click-through, not just a readout: the cell summarizes a
        // position managed two levels away, so it carries the way there.
        let cell = ui
            .add(
                egui::Label::new(egui::RichText::new(text).monospace().color(color))
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(
                "paper-trading, in points - simulated fills, not a broker \
                 account; click to open the Trading tab",
            );
        sim_clicked = cell.clicked();
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
    sim_clicked
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

    /// The rule the corner imposed on this line: two ends of one window may
    /// not answer the same question differently.
    #[test]
    fn the_line_never_claims_live_about_a_chart_the_corner_calls_offline() {
        for state in [
            FeedState::Connecting,
            FeedState::Reconnecting,
            FeedState::Live,
            FeedState::Replay,
        ] {
            let (color, label) = provenance_dot(state, Some(theme::WARN));
            assert_eq!(label, crate::feed_notice::OFFLINE_LABEL, "{state:?}");
            assert_eq!(color, theme::WARN, "the corner's colour, not this line's");
            // And with no corner the line goes back to reporting the transport.
            let (color, label) = provenance_dot(state, None);
            assert_eq!(label, state.label(), "{state:?}");
            assert_eq!(color, state.color(), "{state:?}");
        }
    }

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
        assert_eq!(tape_text(replay, None, None, None), "10×  45%");
        assert_eq!(
            tape_text(None, Some(230), Some(300), None),
            "arrival 230 ms"
        );
        // The tape has gone quiet: the stored arrival figure is stale itself.
        assert_eq!(
            tape_text(None, Some(42), Some(12_000), None),
            "stale 12 s",
            "a wedged socket keeps a healthy-looking arrival forever"
        );
        // A chart opened before the open, on the session that closed the
        // afternoon before. This cell is the always-visible half of how old
        // the tape is; the corner is the other half.
        assert_eq!(
            tape_text(None, Some(42), Some(14 * 3_600_000), None),
            "stale 14 h",
            "an overnight close is not fifty thousand seconds"
        );
        assert_eq!(tape_text(None, None, None, None), "arrival —");
    }

    /// A split as a bridge reports one.
    fn split(arrival: i64, source: i64, transport: i64, hop: &'static str) -> FeedLatency {
        FeedLatency {
            arrival_lag_ms: arrival,
            source_lag_ms: Some(source),
            source_lag_peak_ms: Some(source + 100),
            transport_lag_ms: Some(transport),
            hop: Some(hop),
            prints: 64,
        }
    }

    #[test]
    fn a_late_tape_names_the_hop_and_a_healthy_one_stays_quiet() {
        // One word is what a trader mid-session can act on: look at the
        // terminal, or look at the chart. It appears only when there is
        // something to act on — a cell that always carries a second word stops
        // being read, and a healthy tape naming its nominally slowest hop is
        // noise dressed as a diagnosis.
        let late = split(9_000, 8_800, 200, "MT5");
        assert_eq!(
            tape_text(None, Some(9_600), Some(9_000), Some(late)),
            "MT5 9000 ms"
        );
        let healthy = split(120, 90, 30, "MT5");
        assert_eq!(
            tape_text(None, Some(140), Some(120), Some(healthy)),
            "arrival 120 ms"
        );
    }

    #[test]
    fn the_cell_shows_the_total_its_own_halves_add_up_to() {
        // The regression this shape exists to prevent: the cell showing the
        // chart's end-to-end figure while the hover broke down the feed's, so
        // the trader read a total that its own halves did not sum to. The
        // chart's figure is larger — it also counts the queue and the frame —
        // and the hover is where it is named, as its own measurement.
        let late = split(9_000, 8_800, 200, "MT5");
        assert_eq!(tape_arrival_ms(Some(9_600), Some(late)), Some(9_000));
        assert_eq!(
            late.source_lag_ms.unwrap() + late.transport_lag_ms.unwrap(),
            late.arrival_lag_ms
        );
        // A provider with no split to publish is untouched: the chart's own
        // figure is the only one there is.
        assert_eq!(tape_arrival_ms(Some(120), None), Some(120));
    }

    #[test]
    fn naming_the_hop_never_widens_the_cell() {
        // The three sections of this row share one width with no budget between
        // them, so a cell that grows pushes the machinery readouts off the end.
        // Measured at 1000 px: appending the hop overlapped `bars`/`trades`
        // outright, and swapping in an eight-character name still grazed them.
        // `LatencyHop::MAX_LABEL_CHARS` is therefore the width of `arrival`
        // itself; this is the other half of that bargain, asserted where the
        // cell is built, and it is `<=` with no slack on purpose.
        let plain = tape_text(None, Some(18_112), Some(100), None);
        for hop in quantick_feed_mt5::LatencyHop::ALL.map(quantick_feed_mt5::LatencyHop::label) {
            let named = tape_text(
                None,
                Some(18_112),
                Some(100),
                Some(split(18_112, 17_980, 132, hop)),
            );
            assert!(
                named.chars().count() <= plain.chars().count(),
                "{named:?} is wider than {plain:?}"
            );
        }
    }

    #[test]
    fn a_stale_tape_still_reports_staleness_over_any_split() {
        // A wedged socket is the one state where the arrival figure lies, and
        // a hop name beside it would make the lie more convincing.
        assert_eq!(
            tape_text(
                None,
                Some(9_000),
                Some(60_000),
                Some(split(9_000, 8_800, 200, "MT5"))
            ),
            "stale 60 s"
        );
    }

    #[test]
    fn the_hover_says_where_each_figure_was_taken() {
        // The feed measures at the socket and the chart at the frame, so the
        // two totals are different numbers about different things. Printing
        // them as one is what produced three figures that did not reconcile;
        // naming the gap turns it into the one hop neither side sees alone.
        let text = tape_tooltip(Some(9_600), Some(split(9_000, 8_800, 200, "MT5")));
        assert!(text.starts_with(TAPE_TOOLTIP_BASE), "the base always leads");
        assert!(text.contains("the feed measured 9 s from the venue's stamp"));
        assert!(text.contains("8 s before it was handed over"));
        assert!(text.contains("200 ms from there to the socket"));
        assert!(text.contains("most of it in MT5"));
        assert!(text.contains("slowest handover in the last 64 prints"));
        assert!(
            text.contains("the chart's own queue and drawing"),
            "the hop neither side can see alone is named: {text}"
        );
    }

    #[test]
    fn a_provider_that_cannot_split_says_only_what_it_measured() {
        // Data honesty: no invented zeros, and no breakdown of a chain nobody
        // cut. The hover is exactly what it always was.
        assert_eq!(tape_tooltip(Some(120), None), TAPE_TOOLTIP_BASE);
        let unsplit = FeedLatency {
            arrival_lag_ms: 9_000,
            source_lag_ms: None,
            source_lag_peak_ms: None,
            transport_lag_ms: None,
            hop: None,
            prints: 3,
        };
        assert_eq!(tape_tooltip(Some(9_000), Some(unsplit)), TAPE_TOOLTIP_BASE);
        assert_eq!(
            tape_text(None, Some(9_000), Some(9_000), Some(unsplit)),
            "arrival 9000 ms"
        );
    }

    #[test]
    fn replay_progress_is_clamped_to_a_percentage() {
        let over = Some(ReplayFigures {
            speed: 1.0,
            progress: 1.7,
        });
        assert_eq!(tape_text(over, None, None, None), "1× 100%");
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
                feed_latency: None,
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
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    let response = draw(ctx, &model, &mut tz, None);
                    assert_eq!(
                        response,
                        StatusResponse::default(),
                        "an un-clicked frame asks nothing of the app"
                    );
                });
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
            feed_latency: None,
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
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let _ = draw(ctx, &model, &mut tz, None);
            });
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
