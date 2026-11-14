//! The status bar: wrapping 28 px rows answering "how healthy is it?"
//! (`docs/ux/ui-design-model.md` §8).
//!
//! Three sections replace the perf overlay, the floating timezone pill and
//! the mode text that used to be painted over candles: provenance on the
//! left (state dot, venue, symbol, lag), content in the middle (honesty
//! labels), machinery on the right (fps and the timezone picker). What the
//! forming bar has left lives on its own price tag, not here. A reading
//! that breaches its threshold turns
//! the `warn` token; narrow windows wrap cells without hiding their facts.
//!
//! The bar's controls share one raised hover. The workspace moved to the
//! toolbar's capsule; the timezone picker sits at the right end. A
//! `Reconnect`/`Reload` pair once stood in the provenance section, because
//! the notice card refused to cover a working
//! chart, and a terminal that froze mid-session left the trader nothing to
//! press anywhere in the window. The offline chip in the chart's bottom-right
//! corner answers that better: it is in one place whatever the chart holds, so
//! the pair went back out rather than stand as a second way to do the same two
//! things.
//!
//! At the left end stands the window's one word on the feed's global
//! state, in grey, and it is the recovery popup's door: see
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
    /// The deal recorder's cell — `REC 2 301 455 · 09:00:00 · file`, `REC
    /// off`, `RECORDED · day` — or none where the feed has no counter.
    pub deal_recording: Option<DealCell>,
    /// What the recorder has to say about the flow pane, beside that cell.
    pub deal_chip: Option<crate::deal_recording_ui::DealChip>,
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
    /// Whether the price axis is auto-fitting.
    pub price_auto: bool,
    /// Rolling frames per second.
    pub fps: Option<f32>,
    /// Rolling mean frame time, in milliseconds.
    pub frame_avg_ms: Option<f32>,
    /// Rolling mean CPU cost per frame (update + tessellation + paint, no
    /// vsync wait), in milliseconds.
    pub frame_cpu_ms: Option<f32>,
    /// Whether the perf readings (fps, frame time, cpu) are shown
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

impl DealCell {
    /// The cell for the recorder the chrome sees, or none where the view
    /// has no cell to show.
    #[must_use]
    pub fn of(view: Option<crate::deal_recording::RecordingView>) -> Option<Self> {
        let view = view?;
        Some(Self {
            text: view.status_cell()?,
            tone: view.state,
        })
    }
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

/// What a click on the bar asked of the app this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct StatusResponse {
    /// The SIM cell was clicked: open the Trading dock tab, where the
    /// position it summarizes is managed.
    pub open_trading_tab: bool,
    /// The clock was clicked: Settings opens on Display.
    pub open_display_settings: bool,
    /// The word was clicked: it opens and closes the recovery popup.
    pub word_clicked: bool,
    /// The word's rect while it is a door, which is the popup's anchor.
    pub word_rect: Option<egui::Rect>,
    /// The one-click control beside the word was pressed.
    pub reconnect: bool,
}

/// What the feed-state cell shows this frame.
#[derive(Debug, Clone, Default)]
pub struct StatusLead<'a> {
    pub word: Option<StatusWord>,
    /// The word opens the recovery popup: there is a report to read.
    pub door: bool,
    /// The popup it opens is up.
    pub open: bool,
    /// The reason, on the word's hover, so reading it takes no click.
    pub report: Option<crate::feed_notice::Report<'a>>,
    /// What the one-click control beside the word asks: the popup's first.
    pub action: crate::feed_notice::NoticeAction,
    /// Why the tab shows another contract than it was opened on.
    pub note: Option<String>,
}

impl<'a> StatusLead<'a> {
    /// What the feed-state cell shows for this tab, and the reason behind the word.
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
            report: crate::feed_notice::report(&tab.link.notice, stall)
                .filter(crate::feed_notice::Report::is_offline)
                .map(|report| report.with_roll(tab.link.roll_note())),
            action: tab.link.quick_action(),
            note: tab
                .link
                .rolled
                .as_ref()
                .map(quantick_feed::link::Rolled::sentence),
        }
    }
}

/// Draw the status bar as the window's bottom panel. The clock clicks through
/// to Settings, and the SIM cell to the Trading tab.
pub fn draw(
    ctx: &egui::Context,
    model: &StatusModel,
    tz: TzOffset,
    lead: &StatusLead<'_>,
) -> StatusResponse {
    let palette = theme::get(ctx);
    let mut response = StatusResponse::default();
    // PanelState lives in the shared Context, not in its native viewport.
    egui::TopBottomPanel::bottom(egui::Id::new((ctx.viewport_id(), "status_bar")))
        .min_height(STATUS_BAR_HEIGHT)
        .frame(
            egui::Frame::none()
                .fill(palette.chrome())
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
                response.open_display_settings = draw_machinery(ui, model, tz);
            });
        });
    response
}

/// The word: a grey dot and what is true of the feed. Only an attempt pulses.
fn draw_lead(ui: &mut egui::Ui, lead: &StatusLead<'_>, response: &mut StatusResponse) {
    let palette = theme::get(ui.ctx());
    if let Some(note) = &lead.note {
        ui.label(
            egui::RichText::new(note)
                .small()
                .color(palette.loading_meta()),
        );
    }
    let Some(word) = &lead.word else {
        return;
    };
    let colour = match word.word {
        "offline" | "no data" | "quiet" => palette.loading_faint(),
        _ if word.pulses => palette
            .loading_meta()
            .gamma_multiply(crate::loading::pulse(ui.ctx())),
        _ => palette.loading_meta(),
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
            palette.loading_meta(),
        )
    });
    let size = egui::vec2(
        STATE_DOT_DIAMETER_PX + DOT_WORD_GAP_PX + galley.size().x,
        galley.size().y.max(STATE_DOT_DIAMETER_PX),
    );
    let (rect, cell) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();
    if lead.door && (lead.open || cell.hovered()) {
        painter.rect_filled(
            rect.expand(2.0),
            egui::Rounding::same(3.0),
            palette.control(),
        );
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
        palette.loading_meta(),
    );
    if lead.door {
        response.word_rect = Some(rect);
        let cell = cell
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_ui(|ui| {
                if let Some(report) = &lead.report {
                    ui.label(report.headline);
                    if let Some(step) = report.next_step_text() {
                        ui.label(egui::RichText::new(step).color(palette.text_muted()));
                    }
                }
            });
        response.word_clicked = cell.clicked();
    }
    if word.reconnect && !lead.open {
        let reconnect = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(lead.action.label())
                        .small()
                        .color(palette.loading_meta()),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(lead.action.cost());
        response.reconnect = reconnect.clicked();
    }
}

/// Venue, symbol, lag.
fn draw_provenance(ui: &mut egui::Ui, model: &StatusModel) {
    let palette = theme::get(ui.ctx());
    ui.label(egui::RichText::new(&model.venue).color(palette.text_muted()));
    ui.label(
        egui::RichText::new(&model.symbol)
            .monospace()
            .color(palette.text_primary()),
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
        .color(palette.text_muted()),
    )
    // `on_hover_ui`, not `on_hover_text`: the split's sentences are built with
    // `format!`, and `on_hover_text` evaluates its argument on every frame
    // whether or not anyone is pointing at the cell. A per-frame allocation for
    // a tooltip nobody is reading is exactly the trade this repo does not make.
    .on_hover_ui(|ui| {
        ui.label(tape_tooltip(model.feed_arrival_ms, model.feed_latency));
    });
}

/// Middle section: honesty labels, the paper account and the axis hint.
/// Returns whether the SIM cell was clicked.
fn draw_content(ui: &mut egui::Ui, model: &StatusModel) -> bool {
    let palette = theme::get(ui.ctx());
    let mut sim_clicked = false;
    if let Some(reason) = &model.saves_off {
        ui.label(
            egui::RichText::new(SAVES_OFF_LABEL)
                .monospace()
                .color(palette.amber()),
        )
        .on_hover_text(reason);
    }
    if let Some(cell) = &model.deal_recording {
        let color = match cell.tone {
            RecState::Recording => palette.rec(),
            RecState::Stale => palette.amber(),
            RecState::Off | RecState::Recorded | RecState::Unsupported => palette.text_muted(),
        };
        ui.label(egui::RichText::new(&cell.text).monospace().color(color))
            .on_hover_text("the venue's deal counter: counted live and written to disk");
    }
    if let Some(chip) = &model.deal_chip {
        let color = match chip.tone {
            RecState::Recording => palette.rec(),
            RecState::Stale => palette.amber(),
            RecState::Off | RecState::Recorded | RecState::Unsupported => palette.text_muted(),
        };
        ui.label(egui::RichText::new(&chip.text).small().color(color))
            .on_hover_text(&chip.hover);
    }
    if let Some(note) = &model.side_note {
        // Inferred data wears the amber thread, like every not-quite-venue
        // truth on this chart. Kept short on purpose: this section shares its
        // row with the machinery readouts, and a long label overruns them.
        ui.label(egui::RichText::new(note).small().color(palette.amber()))
            .on_hover_text(model.side_detail.as_deref().unwrap_or(note.as_str()));
    }
    if let Some((text, sign)) = &model.sim_pnl {
        let color = crate::paper_chrome::points_color(&palette, (*sign as i8).into());
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
    if !model.price_auto {
        ui.label(
            egui::RichText::new("price: manual · double-click the axis to auto-fit")
                .small()
                .color(palette.text_faint()),
        );
    }
    sim_clicked
}

/// Machinery cells share the same wrapping flow as the provenance and content.
fn draw_machinery(ui: &mut egui::Ui, model: &StatusModel, tz: TzOffset) -> bool {
    let palette = theme::get(ui.ctx());
    let clock =
        egui::RichText::new(format!("{} {}", tz.label(), icons::CLOCK)).color(palette.text_muted());
    let clicked = ui
        .add(egui::Button::new(clock).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Timezone and display settings")
        .clicked();
    if !model.show_perf {
        return clicked;
    }
    ui.separator();
    let slow = model
        .frame_avg_ms
        .is_some_and(|avg| avg > metrics::SLOW_FRAME_MS);
    let fps_color = if slow {
        palette.warn()
    } else {
        palette.text_muted()
    };
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
    clicked
}

#[cfg(test)]
#[path = "statusbar/tests/native_layout_tests.rs"]
mod native_layout_tests;

#[cfg(test)]
mod tests;
