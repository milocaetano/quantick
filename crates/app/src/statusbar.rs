//! The status bar: wrapping 28 px rows answering "how healthy is it?"
//! (`docs/ux/ui-design-model.md` §8).
//!
//! Three sections replace the perf overlay, the floating timezone pill and
//! the mode text that used to be painted over candles: provenance on the
//! left (state dot, venue, symbol, lag), content in the middle (bar spec,
//! counts, honesty labels), machinery on the right (trades, fps, and the
//! timezone picker). A reading that breaches its threshold turns
//! [`theme::WARN`]; narrow windows wrap cells without hiding their facts.
//!
//! The timezone picker is the bar's only control. It briefly had company —
//! a `Reconnect`/`Reload` pair in the provenance section — because the notice
//! card refused to cover a working chart, and a terminal that froze
//! mid-session left the trader nothing to press anywhere in the window. The
//! offline chip in the chart's bottom-right corner answers that better: it is
//! in one place whatever the chart holds, so the pair went back out rather
//! than stand as a second way to do the same two things.
//!
//! The first cell is the window's one word on the feed's global state, in grey,
//! and it is the recovery popup's door: see
//! [`quantick_chart::connection_state`] for what it says and [`crate::feed_notice`]
//! for what it opens.
//!
//! The section values are computed by the app and handed in as a plain
//! [`StatusModel`], so every text and colour decision here stays pure and
//! unit-testable.

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::deal_recording::RecState;
use crate::metrics;
use crate::theme;
use crate::timezone::TzOffset;
use quantick_chart::connection_state::StatusWord;
use quantick_feed::FeedLatency;
use quantick_orderflow as orderflow;

/// Height of the status line, in pixels (§5 zone 7).
pub const STATUS_BAR_HEIGHT: f32 = 28.0;
/// Diameter of the provenance state dot, in pixels.
const STATE_DOT_DIAMETER_PX: f32 = 8.0;
/// Gap between neighbouring cells on the line, in pixels.
const CELL_SPACING_PX: f32 = 8.0;
/// Gap between the state dot and its word, in pixels.
const DOT_WORD_GAP_PX: f32 = 4.0;

/// Replay figures for the provenance section, read from the transport link.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayFigures {
    /// Playback speed multiplier.
    pub speed: f32,
    /// Played fraction in `[0, 1]`.
    pub progress: f32,
}

/// The status cell a session that writes no store wears for its whole run.
pub const SAVES_OFF_LABEL: &str = "SAVES OFF";

/// Everything the bar shows this frame, precomputed by the app.
pub struct StatusModel {
    /// Display name of the venue (or the session, while replaying).
    pub venue: String,
    /// The streamed symbol.
    pub symbol: String,
    /// Replay speed/progress while a recording plays, `None` when live.
    pub replay: Option<ReplayFigures>,
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
    /// The deal recorder's cell — `REC 2 301 455 · 09:00:00 · file`, `REC
    /// off`, `RECORDED · day` — or none where the feed has no counter.
    pub deal_recording: Option<DealCell>,
    /// What the recorder has to say about the flow pane, beside that cell.
    pub deal_chip: Option<crate::deal_recording_ui::DealChip>,
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
    /// Why nothing is saved this session, when nothing is (decision DS7): a
    /// cell that stays on the line for as long as the session runs.
    pub saves_off: Option<String>,
}

/// The recording cell, with the state that colours it — red while the
/// counter is being written, amber while it is stale, quiet otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealCell {
    pub text: String,
    pub tone: RecState,
}

/// The tape cell: `clock 123 ms` for an unsplit wall-clock difference,
/// `arrival 123 ms` for provider breakdowns, `stale 12 s` / `stale 14 h` when the
/// tape has gone
/// quiet, `10× 45%` while replaying, `arrival —` before the first trade.
///
/// A signed clock difference retains source/local disagreement and is not
/// measured transport delay. Provider breakdowns retain their own provenance.
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
    quantick_feed::timing::live_tape_text(
        arrival_ms,
        tape_age_ms,
        latency,
        metrics::STALE_TAPE_MS,
        metrics::HIGH_LAG_MS,
    )
}

#[cfg(test)]
use quantick_feed::timing::{TAPE_TOOLTIP_BASE, tape_arrival_ms};

pub fn tape_tooltip(arrival_ms: Option<i64>, latency: Option<FeedLatency>) -> String {
    quantick_feed::timing::tape_tooltip(arrival_ms, latency, orderflow::format_window_ms)
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
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct StatusResponse {
    /// The SIM cell was clicked: open the Trading dock tab, where the
    /// position it summarizes is managed.
    pub open_trading_tab: bool,
    /// The word was clicked: it opens and closes the recovery popup.
    pub word_clicked: bool,
    /// The word's rect while it is a door, which is the popup's anchor.
    pub word_rect: Option<egui::Rect>,
    /// The one-click Reconnect beside the word was pressed.
    pub reconnect: bool,
}

/// What the status bar's first cell shows this frame.
#[derive(Debug, Clone, Default)]
pub struct StatusLead<'a> {
    pub word: Option<StatusWord>,
    /// The word opens the recovery popup: there is a report to read.
    pub door: bool,
    /// The popup it opens is up.
    pub open: bool,
    /// The reason, on the word's hover, so reading it takes no click.
    pub report: Option<crate::feed_notice::Report<'a>>,
}

impl<'a> StatusLead<'a> {
    /// What the first cell shows for this tab, and the reason behind the word.
    #[must_use]
    pub fn of(
        tab: &'a crate::tab::Tab,
        stall: Option<&'a quantick_feed::stall::Stall>,
        now_ms: i64,
        open: bool,
    ) -> Self {
        Self {
            word: tab.connection.status_word(now_ms),
            door: tab.connection.has_recovery_door(),
            open,
            report: crate::feed_notice::report(&tab.notice, stall)
                .filter(crate::feed_notice::Report::is_offline),
        }
    }
}

/// Draw the status bar as the window's bottom panel. `tz` is the bar's
/// resident control, and the SIM cell clicks through to the Trading tab.
pub fn draw(
    ctx: &egui::Context,
    model: &StatusModel,
    tz: &mut TzOffset,
    lead: &StatusLead<'_>,
) -> StatusResponse {
    let mut response = StatusResponse::default();
    egui::TopBottomPanel::bottom("status_bar")
        .min_height(STATUS_BAR_HEIGHT)
        .frame(
            egui::Frame::none()
                .fill(theme::CHROME)
                .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                // Wrap whole facts as cells, keeping their words together.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.spacing_mut().item_spacing.x = CELL_SPACING_PX;
                draw_lead(ui, lead, &mut response);
                draw_provenance(ui, model);
                ui.separator();
                response.open_trading_tab = draw_content(ui, model);
                draw_machinery(ui, model, tz);
            });
        });
    response
}

/// The word: a grey dot and what is true of the feed. Only an attempt pulses.
fn draw_lead(ui: &mut egui::Ui, lead: &StatusLead<'_>, response: &mut StatusResponse) {
    let Some(word) = &lead.word else {
        return;
    };
    let colour = match word.word {
        "offline" | "no data" | "quiet" => crate::loading::GREY_FAINT,
        _ if word.pulses => {
            crate::loading::GREY_MID.gamma_multiply(crate::loading::pulse(ui.ctx()))
        }
        _ => crate::loading::GREY_MID,
    };
    let text = match &word.age {
        Some(age) => format!("{} · {age}", word.word),
        None => word.word.to_owned(),
    };
    let sense = if lead.door {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let galley = ui.fonts(|fonts| {
        fonts.layout_no_wrap(
            text,
            egui::TextStyle::Small.resolve(ui.style()),
            crate::loading::GREY_MID,
        )
    });
    let size = egui::vec2(
        STATE_DOT_DIAMETER_PX + DOT_WORD_GAP_PX + galley.size().x,
        galley.size().y.max(STATE_DOT_DIAMETER_PX),
    );
    let (rect, cell) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();
    if lead.door && (lead.open || cell.hovered()) {
        painter.rect_filled(rect.expand(2.0), egui::Rounding::same(3.0), theme::CONTROL);
    }
    painter.circle_filled(
        egui::pos2(rect.left() + STATE_DOT_DIAMETER_PX / 2.0, rect.center().y),
        STATE_DOT_DIAMETER_PX / 2.0,
        colour,
    );
    painter.galley(
        egui::pos2(
            rect.left() + STATE_DOT_DIAMETER_PX + DOT_WORD_GAP_PX,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        crate::loading::GREY_MID,
    );
    if lead.door {
        response.word_rect = Some(rect);
        let cell = cell
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_ui(|ui| {
                if let Some(report) = &lead.report {
                    ui.label(report.headline);
                    if let Some(step) = report.next_step {
                        ui.label(egui::RichText::new(step).color(theme::TEXT_MUTED));
                    }
                }
            });
        response.word_clicked = cell.clicked();
    }
    if word.reconnect && !lead.open {
        let reconnect = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(crate::loading::RECONNECT_LABEL)
                        .small()
                        .color(crate::loading::GREY_MID),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Keeps the chart as it is");
        response.reconnect = reconnect.clicked();
    }
}

/// Venue, symbol, lag.
fn draw_provenance(ui: &mut egui::Ui, model: &StatusModel) {
    ui.label(egui::RichText::new(&model.venue).color(theme::TEXT_MUTED));
    ui.label(
        egui::RichText::new(&model.symbol)
            .monospace()
            .color(theme::TEXT_PRIMARY),
    );
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
        .color(theme::TEXT_MUTED),
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
    if let Some(reason) = &model.saves_off {
        ui.label(
            egui::RichText::new(SAVES_OFF_LABEL)
                .monospace()
                .color(theme::AMBER),
        )
        .on_hover_text(reason);
    }
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
    if let Some(cell) = &model.deal_recording {
        let color = match cell.tone {
            RecState::Recording => theme::REC,
            RecState::Stale => theme::AMBER,
            RecState::Off | RecState::Recorded | RecState::Unsupported => theme::TEXT_MUTED,
        };
        ui.label(egui::RichText::new(&cell.text).monospace().color(color))
            .on_hover_text("the venue's deal counter: counted live and written to disk");
    }
    if let Some(chip) = &model.deal_chip {
        let color = match chip.tone {
            RecState::Recording => theme::REC,
            RecState::Stale => theme::AMBER,
            RecState::Off | RecState::Recorded | RecState::Unsupported => theme::TEXT_MUTED,
        };
        ui.label(egui::RichText::new(&chip.text).small().color(color))
            .on_hover_text(&chip.hover);
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

/// Machinery cells share the same wrapping flow as the provenance and content.
fn draw_machinery(ui: &mut egui::Ui, model: &StatusModel, tz: &mut TzOffset) {
    // Reserve the picker as one cell before it paints its internal button;
    // the button's own late allocation cannot move that paint to a new row.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.spacing().combo_width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            egui::ComboBox::from_id_salt("tz_combo")
                .selected_text(tz.label())
                .show_ui(ui, |ui| {
                    for offset in TzOffset::ALL {
                        ui.selectable_value(tz, offset, offset.label());
                    }
                });
        },
    );
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
    fn lag_cell_reports_replay_live_and_waiting() {
        let replay = Some(ReplayFigures {
            speed: 10.0,
            progress: 0.45,
        });
        assert_eq!(tape_text(replay, None, None, None), "10×  45%");
        assert_eq!(tape_text(None, Some(230), Some(300), None), "clock 230 ms");
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
        assert!(tape_tooltip(Some(120), None).starts_with(TAPE_TOOLTIP_BASE));
        let unsplit = FeedLatency {
            arrival_lag_ms: 9_000,
            source_lag_ms: None,
            source_lag_peak_ms: None,
            transport_lag_ms: None,
            hop: None,
            prints: 3,
        };
        assert!(tape_tooltip(Some(9_000), Some(unsplit)).starts_with(TAPE_TOOLTIP_BASE));
        assert_eq!(
            tape_text(None, Some(9_000), Some(9_000), Some(unsplit)),
            "clock 9000 ms"
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
                feed_arrival_ms: (!replaying).then_some(120),
                feed_latency: None,
                tape_age_ms: (!replaying).then_some(200),
                spec_summary: "tick(50)".to_owned(),
                bar_progress: Some("37/50 ticks".to_owned()),
                deal_recording: None,
                deal_chip: None,
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
                saves_off: None,
            };
            for width in [650.0, 1000.0, 1500.0] {
                for frame in 0..2 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 800.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            let response = draw(ctx, &model, &mut tz, &StatusLead::default());
                            assert_eq!(
                                response,
                                StatusResponse::default(),
                                "an un-clicked frame asks nothing of the app"
                            );
                        },
                    );
                    let labels: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|shape| {
                            if let egui::Shape::Text(text) = &shape.shape {
                                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                                if frame > 0 {
                                    assert!(
                                        shape.clip_rect.contains_rect(rect),
                                        "clipped {}: {rect:?} outside {:?}",
                                        text.galley.text(),
                                        shape.clip_rect
                                    );
                                }
                                Some((text.galley.text(), rect))
                            } else {
                                None
                            }
                        })
                        .collect();
                    assert!(
                        labels
                            .iter()
                            .any(|(label, _)| label.contains("SIM +12.5 pts"))
                    );
                    for (index, (label, rect)) in labels.iter().enumerate() {
                        assert!(
                            rect.left() >= 0.0 && rect.right() <= width,
                            "clipped {label}: {rect:?}"
                        );
                        for (other, other_rect) in &labels[index + 1..] {
                            assert!(
                                !rect.intersects(*other_rect),
                                "overlap {label} / {other}: {rect:?} / {other_rect:?}"
                            );
                        }
                    }
                }
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
            feed_arrival_ms: Some(106),
            feed_latency: None,
            tape_age_ms: Some(150),
            spec_summary: "tick(50)".to_owned(),
            bar_progress: Some("37/50 ticks".to_owned()),
            deal_recording: None,
            deal_chip: None,
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
            saves_off: None,
        };
        // Two passes: egui settles its layout on the second.
        let mut painted = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let _ = draw(ctx, &model, &mut tz, &StatusLead::default());
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
