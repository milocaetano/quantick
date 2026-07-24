//! The egui application: drains the live feed, renders bars, surfaces metrics,
//! and lets the user switch bar type live.
//!
//! Coordinate math lives in [`crate::chart`] (pure, tested), trade → bar logic
//! and the bar-type dispatch in [`crate::state`] (pure, tested), and metric math
//! in [`crate::metrics`] (pure, tested). This layer owns the clocks, the tracing
//! and the widgets, drains the feed each frame, and turns everything into egui
//! shapes.

use std::time::{Duration, Instant};

use eframe::egui;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use tokio::sync::mpsc;

use quantick_feed_binance::depth::DepthEvent;

use crate::candle_view::{draw_candle, draw_style_window};
use crate::chart::PriceScale;
use crate::config::{AppConfig, ProviderKind};
use crate::feed::{self, FeedCommand, FeedEvent, FeedHandle};
use crate::metrics::{self, FrameStats};
use crate::orderflow_view::OrderflowView;
use crate::price_view::PriceView;
use crate::state::{BarKind, BarSpec, ChartState};
use crate::style::{CandlePreset, ChartStyle};
use crate::timezone::TzOffset;
use crate::viewport::Viewport;

/// Convert an explicit unmultiplied RGBA style colour to egui.
fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

const DIVIDER: egui::Color32 = egui::Color32::from_rgb(240, 185, 11);
const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 160, 175);
const OVERLAY: egui::Color32 = egui::Color32::from_rgb(210, 218, 226);
const WARN: egui::Color32 = egui::Color32::from_rgb(255, 99, 71);
const CROSSHAIR: egui::Color32 = egui::Color32::from_rgb(110, 120, 135);
const TAG_BG: egui::Color32 = egui::Color32::from_rgb(55, 63, 80);

/// Width of the right-hand price-axis gutter, in pixels.
const AXIS_GUTTER: f32 = 60.0;
/// Height of the bottom time-axis strip, in pixels.
const TIME_STRIP: f32 = 22.0;

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);
/// Coalesce slider drags into one diagnostic event after the value settles.
const STYLE_LOG_DEBOUNCE: Duration = Duration::from_millis(350);
/// Each UI capture epoch reserves room for reconnect generations. This keeps
/// late events from an aborted task below the next accepted generation floor.
const BOOK_GENERATION_STRIDE: u64 = 1_000_000;
/// Bound depth work per frame so a burst cannot starve egui input/rendering.
const BOOK_DRAIN_BUDGET: usize = 2_048;

/// Convert a UI `f64` parameter to a positive `Decimal` for a builder threshold.
fn dec_from_f64(x: f64) -> Decimal {
    Decimal::from_f64(x.max(1e-8)).unwrap_or(Decimal::ONE)
}

/// Split the padded plot area into the candle chart, the right price gutter and
/// the bottom time strip, so the input handler and the renderer agree on the
/// boundaries.
fn plot_split(area: egui::Rect) -> PlotAreas {
    let plot = area.shrink(16.0);
    let split_x = (plot.right() - AXIS_GUTTER).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - TIME_STRIP).max(plot.top() + 20.0);
    PlotAreas {
        chart: egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y)),
        price_gutter: egui::Rect::from_min_max(
            egui::pos2(split_x, plot.top()),
            egui::pos2(plot.right(), split_y),
        ),
        time_strip: egui::Rect::from_min_max(
            egui::pos2(plot.left(), split_y),
            egui::pos2(split_x, plot.bottom()),
        ),
    }
}

/// The three interactive regions of the plot.
struct PlotAreas {
    chart: egui::Rect,
    price_gutter: egui::Rect,
    time_strip: egui::Rect,
}

/// Format a UTC epoch-millisecond timestamp as `HH:MM:SS` in the display
/// timezone `tz`, for the time axis.
fn fmt_time(ms: i64, tz: TzOffset) -> String {
    let local = ms.saturating_add(tz.offset_ms());
    let secs = local.div_euclid(1000).rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// The quantick chart window.
pub struct QuantickApp {
    state: ChartState,
    events: mpsc::Receiver<FeedEvent>,
    book_events: mpsc::Receiver<DepthEvent>,
    commands: mpsc::Sender<FeedCommand>,
    orderflow: OrderflowView,
    book_capture_epoch: u64,
    book_channel_closed_reported: bool,

    // Feed & asset selection, driven by the configuration. `feed_id`/`symbol`
    // are what the selectors show (the desired selection); `active` is what the
    // running feed thread is actually streaming. When they diverge, the feed is
    // respawned. Nothing here is hard-coded — it all comes from `config`.
    config: AppConfig,
    feed_id: String,
    symbol: String,
    active: (String, String),

    // How many older trades to pull per "load older" click, and how many
    // trades have been backfilled in total (for the readout).
    history_step: usize,
    history_trades: usize,
    // How many history loads (initial backfill + queued "load older"
    // requests) are still unanswered; the loading indicator shows while > 0.
    // A count, not a flag: several requests can be queued (the command
    // channel holds 16), and the first reply must not hide the indicator
    // while others are still in flight. The feed answers every request with
    // exactly one event, so the count always drains back to zero.
    pending_history_loads: usize,

    // Bar-type selector state (one parameter retained per kind).
    kind: BarKind,
    tick_n: u64,
    volume_units: f64,
    dollar_notional: f64,
    time_interval_ms: i64,
    imbalance_target: u64,

    // Pan/zoom navigation over the bar series.
    viewport: Viewport,
    // Manual price-axis pan/zoom (auto-fit until the user drags vertically).
    price_view: PriceView,
    // Last frame's auto-fit price range and chart height, for pixel↔price maths
    // in the input handler (which runs before the draw computes them).
    last_auto_range: Option<(f64, f64)>,
    last_chart_height: f32,
    // Pointer position over the plot this frame, for the crosshair.
    hover_pos: Option<egui::Pos2>,

    // Candle appearance + whether the style panel is open.
    style: ChartStyle,
    show_style: bool,
    style_revision: u64,
    style_log_pending: bool,
    last_style_change: Option<Instant>,
    // Whether the perf overlay (fps / frame time / feed lag) is drawn.
    show_overlay: bool,

    // Fixed UTC offset the time axis is displayed in (default UTC−03:00).
    tz: TzOffset,

    frames: FrameStats,
    last_frame: Option<Instant>,
    latest_trade_ms: Option<i64>,
    live_trades: u64,
    trades_since_summary: u64,
    last_summary: Instant,
}

impl QuantickApp {
    /// Create the app on `config`, opening on `feed_id`/`symbol` (already
    /// streaming through `feed`) and bar `spec`.
    #[must_use]
    pub fn new(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: BarSpec,
        feed: FeedHandle,
    ) -> Self {
        let feed_id = feed_id.into();
        let symbol = symbol.into();
        // Defaults for every kind, with the initial spec's parameter applied.
        let mut tick_n = 50;
        let mut volume_units = 5.0;
        let mut dollar_notional = 500_000.0;
        let mut time_interval_ms = 1_000;
        let mut imbalance_target = 100;
        match &spec {
            BarSpec::Tick(n) => tick_n = *n,
            BarSpec::Volume(u) => volume_units = u.to_f64().unwrap_or(volume_units),
            BarSpec::Dollar(d) => dollar_notional = d.to_f64().unwrap_or(dollar_notional),
            BarSpec::Time(ms) => time_interval_ms = *ms,
            BarSpec::Imbalance(target) => imbalance_target = *target,
        }

        let mut app = Self {
            kind: spec.kind(),
            state: ChartState::new(spec),
            events: feed.events,
            book_events: feed.book_events,
            commands: feed.commands,
            orderflow: OrderflowView::new(symbol.clone()),
            book_capture_epoch: 0,
            book_channel_closed_reported: false,
            active: (feed_id.clone(), symbol.clone()),
            config,
            feed_id,
            symbol,
            history_step: 2000,
            history_trades: 0,
            // The feed starts backfilling the moment it is spawned, so the
            // chart opens with that one load already in flight.
            pending_history_loads: 1,
            tick_n,
            volume_units,
            dollar_notional,
            time_interval_ms,
            imbalance_target,
            viewport: Viewport::new(),
            price_view: PriceView::new(),
            last_auto_range: None,
            last_chart_height: 1.0,
            hover_pos: None,
            style: ChartStyle::default(),
            show_style: false,
            style_revision: 0,
            style_log_pending: false,
            last_style_change: None,
            show_overlay: true,
            tz: TzOffset::default(),
            frames: FrameStats::new(120),
            last_frame: None,
            latest_trade_ms: None,
            live_trades: 0,
            trades_since_summary: 0,
            last_summary: Instant::now(),
        };
        // Dev/ops convenience: start L2 capture without a click. Same code
        // path as the UI toggle, so provider support and command
        // acknowledgement rules stay identical.
        if std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|value| value == "1") {
            app.request_book_capture(true);
        }
        app
    }

    /// The bar spec implied by the current selector state.
    fn current_spec(&self) -> BarSpec {
        match self.kind {
            BarKind::Tick => BarSpec::Tick(self.tick_n.max(1)),
            BarKind::Volume => BarSpec::Volume(dec_from_f64(self.volume_units)),
            BarKind::Dollar => BarSpec::Dollar(dec_from_f64(self.dollar_notional)),
            BarKind::Time => BarSpec::Time(self.time_interval_ms.max(1)),
            BarKind::Imbalance => BarSpec::Imbalance(self.imbalance_target.max(1)),
        }
    }

    /// The display name of the currently selected feed, or its id as a fallback.
    fn feed_display_name(&self) -> String {
        self.config
            .feed(&self.feed_id)
            .map_or_else(|| self.feed_id.clone(), |f| f.name.clone())
    }

    /// Keep `symbol` valid for the selected feed: if the feed changed and no
    /// longer offers the current symbol, fall back to its first symbol.
    fn ensure_symbol_valid(&mut self) {
        let valid = self
            .config
            .feed(&self.feed_id)
            .is_some_and(|f| f.symbols.contains(&self.symbol));
        if !valid
            && let Some(first) = self
                .config
                .feed(&self.feed_id)
                .and_then(|f| f.symbols.first())
                .cloned()
        {
            self.symbol = first;
        }
    }

    /// The feed + symbol selectors, both populated from the configuration.
    fn draw_feed_selectors(&mut self, ui: &mut egui::Ui) {
        // Pre-collect owned option lists so the combo closures don't borrow
        // `self.config` while they mutate `self.feed_id` / `self.symbol`.
        // Providers that aren't streaming yet are labelled "(soon)" so the menu
        // is honest about what actually connects.
        let feeds: Vec<(String, String)> = self
            .config
            .feeds
            .iter()
            .map(|f| {
                let label = if f.provider.is_implemented() {
                    f.name.clone()
                } else {
                    format!("{} (soon)", f.name)
                };
                (f.id.clone(), label)
            })
            .collect();
        ui.label("feed:");
        egui::ComboBox::from_id_salt("feed_sel")
            .selected_text(self.feed_display_name())
            .show_ui(ui, |ui| {
                for (id, name) in &feeds {
                    ui.selectable_value(&mut self.feed_id, id.clone(), name);
                }
            });
        // A newly picked feed may not offer the current symbol.
        self.ensure_symbol_valid();

        let symbols: Vec<String> = self
            .config
            .feed(&self.feed_id)
            .map(|f| f.symbols.clone())
            .unwrap_or_default();
        ui.label("symbol:");
        egui::ComboBox::from_id_salt("symbol_sel")
            .selected_text(&self.symbol)
            .show_ui(ui, |ui| {
                for s in &symbols {
                    ui.selectable_value(&mut self.symbol, s.clone(), s);
                }
            });
    }

    /// The bar-type selector: a combo for the kind and a drag for its parameter.
    fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            self.draw_feed_selectors(ui);
            ui.separator();
            ui.label("bar type:");
            egui::ComboBox::from_id_salt("bar_kind")
                .selected_text(self.kind.label())
                .show_ui(ui, |ui| {
                    for kind in BarKind::ALL {
                        ui.selectable_value(&mut self.kind, kind, kind.label());
                    }
                });
            ui.separator();
            match self.kind {
                BarKind::Tick => {
                    ui.label("N trades");
                    ui.add(egui::DragValue::new(&mut self.tick_n).range(1.0..=5000.0));
                }
                BarKind::Volume => {
                    ui.label("units");
                    ui.add(
                        egui::DragValue::new(&mut self.volume_units)
                            .range(0.1..=1000.0)
                            .speed(0.1),
                    );
                }
                BarKind::Dollar => {
                    ui.label("notional");
                    ui.add(
                        egui::DragValue::new(&mut self.dollar_notional)
                            .range(1000.0..=1_000_000_000.0)
                            .speed(1000.0),
                    );
                }
                BarKind::Time => {
                    ui.label("interval ms");
                    ui.add(
                        egui::DragValue::new(&mut self.time_interval_ms)
                            .range(100.0..=600_000.0)
                            .speed(100.0),
                    );
                }
                BarKind::Imbalance => {
                    ui.label("target trades");
                    ui.add(egui::DragValue::new(&mut self.imbalance_target).range(2.0..=5000.0))
                        .on_hover_text(
                            "expected trades per bar in balanced flow; \
                             one-sided aggression closes bars sooner",
                        );
                }
            }
            ui.separator();
            // History: pull older trades on demand, N per click.
            ui.label("history +");
            ui.add(
                egui::DragValue::new(&mut self.history_step)
                    .range(500.0..=50_000.0)
                    .speed(100.0),
            );
            if ui
                .button("⟲ load older")
                .on_hover_text("fetch older trades and prepend them")
                .clicked()
            {
                self.request_older_history();
            }
            ui.label(format!("({} trades)", self.history_trades));
            ui.separator();
            if ui
                .button("🎨 candle")
                .on_hover_text("candle transparency, outlines, colours and presets")
                .clicked()
            {
                self.show_style = !self.show_style;
            }
            ui.separator();
            let supports_book = matches!(
                self.config.provider_of(&self.feed_id),
                Some(ProviderKind::Binance)
            );
            let mut book_enabled = self.orderflow.enabled();
            let toggle = ui.add_enabled(
                supports_book,
                egui::Checkbox::new(&mut book_enabled, "book heatmap"),
            );
            if toggle
                .on_hover_text(if supports_book {
                    "capture synchronized live L2 depth; history starts when enabled"
                } else {
                    "order-book capture is not available for this provider"
                })
                .changed()
            {
                self.request_book_capture(book_enabled);
            }
            if ui
                .add_enabled(
                    supports_book,
                    egui::Button::new(self.orderflow.settings_button_label()),
                )
                .on_hover_text(
                    "Bookmap palette, non-destructive price ranges, bubbles and liquidity response",
                )
                .clicked()
            {
                self.orderflow.toggle_settings();
            }
            ui.separator();
            ui.checkbox(&mut self.show_overlay, "📈 perf")
                .on_hover_text("show fps / frame time / feed lag (bottom-left)");
        });
    }

    /// Ask the feed thread to fetch and prepend `history_step` older trades.
    /// Non-blocking: if a request is already queued, this frame's click is
    /// dropped rather than piling up commands.
    fn request_older_history(&mut self) {
        match self.commands.try_send(FeedCommand::LoadOlder {
            count: self.history_step.max(1),
        }) {
            Ok(()) => {
                self.pending_history_loads += 1;
                tracing::info!(
                    target: "quantick::app",
                    count = self.history_step,
                    "requested older history"
                );
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!(target: "quantick::app", "older-history request already pending");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(target: "quantick::app", "feed command channel closed");
            }
        }
    }

    /// Allocate a capture generation well above all reconnect generations from
    /// the previous UI capture epoch.
    fn next_book_generation(&mut self) -> u64 {
        self.book_capture_epoch = self.book_capture_epoch.saturating_add(1);
        self.book_capture_epoch
            .saturating_mul(BOOK_GENERATION_STRIDE)
    }

    /// Start or stop the independent depth pipeline without touching aggTrades
    /// or candle construction. UI state changes only if the command is queued.
    fn request_book_capture(&mut self, enabled: bool) {
        if !matches!(
            self.config.provider_of(&self.feed_id),
            Some(ProviderKind::Binance)
        ) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROVIDER_UNSUPPORTED",
                feed = self.feed_id.as_str(),
                symbol = self.symbol.as_str(),
                enabled,
                action = "leave_capture_disabled",
                "selected provider has no order-book pipeline"
            );
            return;
        }

        let generation = self.next_book_generation();
        let command = FeedCommand::SetBookCapture {
            enabled,
            initial_generation: generation,
        };
        match self.commands.try_send(command) {
            Ok(()) => self.orderflow.set_enabled(enabled, generation),
            Err(mpsc::error::TrySendError::Full(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_COMMAND_BACKPRESSURE",
                symbol = self.symbol.as_str(),
                enabled,
                generation,
                action = "retry_on_next_user_action",
                "book capture command channel is full"
            ),
            Err(mpsc::error::TrySendError::Closed(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_COMMAND_CHANNEL_CLOSED",
                symbol = self.symbol.as_str(),
                enabled,
                generation,
                action = "keep_current_capture_state",
                "book capture command channel is closed"
            ),
        }
    }

    /// Restart capture after a semantic configuration change such as base
    /// price grouping. The view commits its staged reset only after this
    /// command is accepted, preserving current history on backpressure.
    fn restart_book_capture(&mut self) {
        if !self.orderflow.enabled() {
            return;
        }
        let generation = self.next_book_generation();
        match self.commands.try_send(FeedCommand::RestartBookCapture {
            initial_generation: generation,
        }) {
            Ok(()) => self.orderflow.accept_capture_grouping_restart(generation),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.orderflow
                    .reject_capture_grouping_restart("command_channel_full");
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_RESTART_BACKPRESSURE",
                    symbol = self.symbol.as_str(),
                    generation,
                    action = "keep_existing_capture",
                    "book restart command channel is full"
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.orderflow
                    .reject_capture_grouping_restart("command_channel_closed");
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_RESTART_CHANNEL_CLOSED",
                    symbol = self.symbol.as_str(),
                    generation,
                    action = "keep_existing_capture",
                    "book restart command channel is closed"
                );
            }
        }
    }

    /// Respawn the feed and reset the chart when the selected feed or symbol
    /// differs from what is currently streaming. A no-op otherwise.
    fn maybe_switch_feed(&mut self) {
        if self.active == (self.feed_id.clone(), self.symbol.clone()) {
            return;
        }
        let Some(provider) = self.config.provider_of(&self.feed_id) else {
            tracing::warn!(
                target: "quantick::app",
                feed = %self.feed_id,
                "selected feed is not in the config; ignoring switch"
            );
            // Snap the selection back to what is actually running.
            (self.feed_id, self.symbol) = self.active.clone();
            return;
        };
        let resume_book_capture = self.orderflow.enabled() && provider == ProviderKind::Binance;

        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_SWITCH",
            feed = %self.feed_id,
            symbol = %self.symbol,
            provider = ?provider,
            resume_book_capture,
            action = "reset_market_state",
            "switching feed/symbol; resetting chart"
        );

        // Dropping the old handle stops the old feed thread. The new feed starts
        // with a fresh backfill in flight.
        let handle = feed::spawn(provider, &self.symbol, &self.config);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.commands = handle.commands;
        self.book_channel_closed_reported = false;

        // Rebuild the chart from scratch for the new stream, keeping the current
        // bar spec. Retained trades from the old symbol must not leak in.
        self.state = ChartState::new(self.current_spec());
        self.viewport = Viewport::new();
        self.price_view = PriceView::new();
        self.last_auto_range = None;
        self.hover_pos = None;
        self.history_trades = 0;
        self.pending_history_loads = 1;
        self.latest_trade_ms = None;
        self.orderflow.reset_for_symbol(self.symbol.clone());

        self.active = (self.feed_id.clone(), self.symbol.clone());
        if resume_book_capture {
            self.request_book_capture(true);
        }
    }

    /// The current background colour as an egui `Color32`.
    fn bg(&self) -> egui::Color32 {
        color32(self.style.canvas.background_rgba())
    }

    /// Window chrome stays opaque even when the plot canvas is transparent;
    /// otherwise toolbar labels would depend on the platform clear colour.
    fn chrome_bg(&self) -> egui::Color32 {
        let [r, g, b] = self.style.canvas.background;
        egui::Color32::from_rgb(r, g, b)
    }

    /// The current chart-grid colour. `TRANSPARENT` disables grid painting
    /// without branching throughout the axis code.
    fn grid(&self) -> egui::Color32 {
        self.style
            .canvas
            .grid_rgba()
            .map_or(egui::Color32::TRANSPARENT, color32)
    }

    /// Draw the modular candle-appearance panel and debounce its diagnostic
    /// event so dragging a slider cannot flood logs at frame rate.
    fn draw_style_panel(&mut self, ctx: &egui::Context, now: Instant) {
        let response = draw_style_window(ctx, &mut self.show_style, &mut self.style);
        if response.changed {
            self.style_revision = self.style_revision.saturating_add(1);
            self.style_log_pending = true;
            self.last_style_change = Some(now);
        }

        let settled = self
            .last_style_change
            .is_some_and(|changed| now.saturating_duration_since(changed) >= STYLE_LOG_DEBOUNCE);
        if self.style_log_pending && (settled || !self.show_style) {
            self.emit_style_changed(response.applied_preset);
        }
    }

    fn emit_style_changed(&mut self, applied_preset: Option<CandlePreset>) {
        let candles = &self.style.candles;
        let preset = applied_preset
            .or_else(|| CandlePreset::detect(candles))
            .map_or("custom", CandlePreset::log_value);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANDLE_STYLE_CHANGED",
            revision = self.style_revision,
            preset,
            body_mode = ?candles.body_mode,
            fill_opacity = candles.fill_opacity,
            outline_opacity = candles.outline_opacity,
            outline_width_px = candles.outline_width,
            body_width_fraction = candles.body_width_frac,
            wick_mode = ?candles.wick_color_mode,
            wick_width_px = candles.wick_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            action = "redraw_only",
            "candle appearance changed"
        );
        self.style_log_pending = false;
    }

    /// A floating timezone picker anchored to the bottom-right corner. The time
    /// axis relabels immediately on selection; the engine is untouched.
    fn draw_timezone_selector(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("tz_selector"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -8.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(egui::Color32::from_black_alpha(150))
                    .show(ui, |ui| {
                        ui.visuals_mut().override_text_color = Some(OVERLAY);
                        ui.horizontal(|ui| {
                            ui.label("🕑");
                            egui::ComboBox::from_id_salt("tz_combo")
                                .selected_text(self.tz.label())
                                .show_ui(ui, |ui| {
                                    for tz in TzOffset::ALL {
                                        ui.selectable_value(&mut self.tz, tz, tz.label());
                                    }
                                });
                        });
                    });
            });
    }

    /// Drain every feed event available this frame into the engine, tracking the
    /// latest trade timestamp and live-trade counts for the metrics.
    fn drain_feed(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(FeedEvent::Backfilled(trades)) => {
                    self.pending_history_loads = self.pending_history_loads.saturating_sub(1);
                    if let Some(last) = trades.last() {
                        self.latest_trade_ms = Some(last.timestamp_ms);
                    }
                    self.history_trades += trades.len();
                    self.state.ingest_backfill(&trades);
                }
                Ok(FeedEvent::HistoryPrepended(trades)) => {
                    // The reply — even an empty one — answers exactly one
                    // pending load; the indicator survives until the last one.
                    self.pending_history_loads = self.pending_history_loads.saturating_sub(1);
                    // Older bars shift every index up; keep the view steady.
                    self.history_trades += trades.len();
                    let added = self.state.prepend_history(&trades);
                    self.viewport.shift_right_edge(added);
                }
                Ok(FeedEvent::Live(trade)) => {
                    self.latest_trade_ms = Some(trade.timestamp_ms);
                    self.live_trades += 1;
                    self.trades_since_summary += 1;
                    self.orderflow.record_trade(&trade);
                    self.state.ingest_live(&trade);
                }
                Err(_) => break,
            }
        }
    }

    /// Drain a bounded number of synchronized depth events. The separate
    /// channel and budget ensure heatmap work cannot block candle ingestion.
    fn drain_book_feed(&mut self) {
        for _ in 0..BOOK_DRAIN_BUDGET {
            match self.book_events.try_recv() {
                Ok(event) => self.orderflow.handle_depth_event(event),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if self.orderflow.enabled() && !self.book_channel_closed_reported {
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "HEATMAP_EVENT_CHANNEL_CLOSED",
                            symbol = self.symbol.as_str(),
                            action = "retain_last_book_and_wait_for_feed_switch",
                            "depth event channel closed"
                        );
                        self.book_channel_closed_reported = true;
                    }
                    break;
                }
            }
        }
    }

    /// Periodically log a perf summary and warn on threshold breaches.
    fn maybe_emit_summary(&mut self, now: Instant) {
        let elapsed = now - self.last_summary;
        if elapsed < SUMMARY_INTERVAL {
            return;
        }
        let rate = self.trades_since_summary as f64 / elapsed.as_secs_f64();
        let lag = metrics::feed_lag_ms(metrics::wall_clock_ms(), self.latest_trade_ms);
        let avg = self.frames.avg_ms().unwrap_or(0.0);
        let worst = self.frames.worst_ms().unwrap_or(0.0);
        let fps = self.frames.fps().unwrap_or(0.0);
        let book = self.orderflow.health();
        let book_lag = metrics::feed_lag_ms(metrics::wall_clock_ms(), book.last_event_ms);
        let book_rate = book.depth_updates_since_summary as f64 / elapsed.as_secs_f64();
        let book_queue_len = self.book_events.len();
        let candle_preset =
            CandlePreset::detect(&self.style.candles).map_or("custom", CandlePreset::log_value);

        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "APP_HEALTH_SUMMARY",
            fps = fps as i64,
            frame_avg_ms = avg,
            frame_worst_ms = worst,
            feed_lag_ms = lag,
            trades_per_s = rate,
            live_trades = self.live_trades,
            bar_spec = self.state.spec().summary(),
            book_enabled = book.enabled,
            book_status = book.status,
            book_generation = book.generation,
            book_last_update_id = book.last_update_id,
            book_last_event_ms = book.last_event_ms,
            book_snapshot_observed_ms = book.last_snapshot_observed_ms,
            book_lag_ms = book_lag,
            book_updates_per_s = book_rate,
            book_updates_total = book.depth_updates,
            book_queue_len,
            book_channel_closed = self.book_channel_closed_reported,
            book_bid_levels = book.bid_levels,
            book_ask_levels = book.ask_levels,
            heatmap_active_levels = book.active_levels,
            heatmap_archived_runs = book.archived_runs,
            aggression_count = book.aggression_count,
            heatmap_history_bytes = book.history_bytes,
            heatmap_cells = book.projection_cells,
            heatmap_aggressions = book.projection_aggressions,
            heatmap_liquidity_events = book.projection_liquidity_events,
            heatmap_effective_grouping = %book.effective_grouping,
            heatmap_effective_grouping_multiple = book.effective_grouping_multiple,
            heatmap_dropped_cells = book.dropped_cells,
            heatmap_dropped_aggressions = book.dropped_aggressions,
            heatmap_dropped_liquidity_events = book.dropped_liquidity_events,
            heatmap_projection_ms = book.projection_ms,
            heatmap_projection_builds = book.projection_builds,
            heatmap_projection_cache_hits = book.projection_cache_hits,
            heatmap_config_revision = book.config_revision,
            heatmap_snapshots = book.snapshots,
            heatmap_gaps = book.gaps,
            candle_style_revision = self.style_revision,
            candle_preset,
            candle_body_mode = ?self.style.candles.body_mode,
            candle_fill_opacity = self.style.candles.fill_opacity,
            candle_outline_opacity = self.style.candles.outline_opacity,
            candle_outline_width_px = self.style.candles.outline_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            action = "observe",
            "application health summary"
        );
        if avg > metrics::SLOW_FRAME_MS {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_SLOW_FRAMES",
                frame_avg_ms = avg,
                threshold_ms = metrics::SLOW_FRAME_MS,
                heatmap_enabled = book.enabled,
                heatmap_projection_ms = book.projection_ms,
                heatmap_cells = book.projection_cells,
                action = "inspect_render_budget",
                "slow frames: the chart is not keeping up"
            );
        }
        if let Some(l) = lag
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_HIGH_TRADE_LAG",
                feed_lag_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                action = "inspect_trade_connection",
                "high feed lag: trades are arriving well behind their timestamps"
            );
        }
        if let Some(l) = book_lag
            && book.enabled
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_HIGH_LAG",
                symbol = self.symbol.as_str(),
                book_lag_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                book_status = book.status,
                action = "inspect_depth_connection",
                "order-book events are behind wall clock"
            );
        }
        if book.dropped_cells > 0
            || book.dropped_aggressions > 0
            || book.dropped_liquidity_events > 0
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROJECTION_CAPPED",
                symbol = self.symbol.as_str(),
                dropped_cells = book.dropped_cells,
                dropped_aggressions = book.dropped_aggressions,
                dropped_liquidity_events = book.dropped_liquidity_events,
                action = "increase_grouping_or_reduce_retention",
                "heatmap primitive cap was reached"
            );
        }

        self.trades_since_summary = 0;
        self.orderflow.reset_summary_counters();
        self.last_summary = now;
    }

    /// Handle mouse navigation, TradingView-style:
    /// - drag the chart → pan time (x, moves the whole chart) and price (y);
    /// - scroll over the chart → zoom time;
    /// - drag the bottom time strip left/right → zoom time (spread candles);
    /// - drag the right price gutter up/down → zoom the price scale;
    /// - scroll over either axis → zoom that axis;
    /// - double-click → reset to the live edge and auto-fit price.
    fn handle_navigation(&mut self, ui: &egui::Ui, area: egui::Rect) {
        let areas = plot_split(area);
        let auto = self.last_auto_range;
        let height = self.last_chart_height;
        let total = self.state.bars().len() + usize::from(self.state.partial().is_some());

        // Chart body: drag pans both axes; scroll zooms time.
        let chart = ui.interact(
            areas.chart,
            egui::Id::new("chart_nav"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = chart.hover_pos();
        if total > 0 && chart.dragged() {
            let drag = chart.drag_delta();
            self.viewport.pan_pixels(drag.x, total);
            if let Some(auto) = auto
                && drag.y != 0.0
                && height > 1.0
            {
                let (lo, hi) = self.price_view.resolve(auto);
                let price_per_px = (hi - lo) / f64::from(height);
                self.price_view.pan(f64::from(drag.y) * price_per_px, auto);
            }
        }
        if chart.double_clicked() {
            self.viewport.snap_to_live();
            self.price_view.reset();
        }
        if chart.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                // Scroll up (positive) zooms in — wider candles.
                self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
            }
        }

        // Bottom time strip: drag or scroll to zoom the candle spacing.
        let time = ui.interact(
            areas.time_strip,
            egui::Id::new("time_nav"),
            egui::Sense::click_and_drag(),
        );
        if time.dragged() {
            // Drag right → wider candles (zoom in); left → narrower (zoom out).
            self.viewport.zoom((time.drag_delta().x / 120.0).exp());
        }
        if time.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
            }
        }

        // Right price gutter: drag or scroll to zoom the price scale.
        let price = ui.interact(
            areas.price_gutter,
            egui::Id::new("price_nav"),
            egui::Sense::click_and_drag(),
        );
        if let Some(auto) = auto {
            if price.dragged() {
                // Drag up → compress span (bigger candles); down → expand.
                self.price_view
                    .zoom(f64::from(price.drag_delta().y / 150.0).exp(), auto);
            }
            if price.double_clicked() {
                self.price_view.reset();
            }
            if price.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    self.price_view.zoom(f64::from(-scroll / 200.0).exp(), auto);
                }
            }
        }
    }

    fn draw_chart(&mut self, painter: &egui::Painter, area: egui::Rect) {
        painter.rect_filled(area, egui::Rounding::ZERO, self.bg());

        let closed = self.state.bars();
        let partial = self.state.partial();
        let total = closed.len() + usize::from(partial.is_some());
        let areas = plot_split(area);
        let chart_rect = areas.chart;
        if total == 0 {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                format!("connecting to {} …", self.symbol),
                egui::FontId::proportional(16.0),
                MUTED,
            );
            self.orderflow.draw_status_badge(painter, chart_rect);
            return;
        }

        // Live tail: while following a live book, grow the forming bar to the
        // right on a real-time scale so its order flow rolls (events stay put)
        // instead of recompressing every depth update. Collapses to one slot the
        // moment the bar closes.
        let live_span = if self.viewport.follows_live()
            && let Some(bar) = partial
            && let Some(now) = self.orderflow.live_end_ms()
        {
            let elapsed = now - bar.open_time;
            let ref_dur = closed
                .last()
                .map(|b| (b.close_time - b.open_time).max(1))
                .unwrap_or(1_000);
            // Use up to ~55% of the chart for the live book, reach that width
            // over ~60% of the previous bar's lifetime, and floor it generously
            // so the right side is always well used (not a thin sliver).
            let max_span =
                (chart_rect.width() / self.viewport.candle_width() * 0.55).clamp(8.0, 18.0);
            let grow_over = (ref_dur * 3 / 5).max(1);
            crate::orderflow_render::live_span_for(elapsed, grow_over, max_span)
                .clamp(8.0, max_span)
        } else {
            1.0
        };
        self.viewport.set_live_tail(live_span - 1.0);

        let (start, end) = self.viewport.visible_range(chart_rect.width(), total);

        // The visible closed bars, plus the partial if it falls in view.
        let closed_start = start.min(closed.len());
        let closed_end = end.min(closed.len());
        let visible_closed = &closed[closed_start..closed_end];
        let partial_visible = partial.filter(|_| closed.len() >= start && closed.len() < end);

        // Auto-fit the visible bars, then apply any manual price pan/zoom.
        let Some(auto_scale) = PriceScale::auto(
            visible_closed,
            partial_visible,
            chart_rect.top(),
            chart_rect.bottom(),
            0.05,
        ) else {
            return;
        };
        let auto_range = auto_scale.range();
        let (lo, hi) = self.price_view.resolve(auto_range);
        let scale = PriceScale::from_range(lo, hi, chart_rect.top(), chart_rect.bottom());

        let cw = self.viewport.candle_width();
        let half = (cw * self.style.candles.clamped_width_frac() / 2.0).max(0.5);
        let right = chart_rect.right();

        // Resting liquidity is the bottom visual layer. Projection is pure with
        // respect to candles and uses the same bar-warped viewport coordinates.
        let orderflow_frame = self.orderflow.project_visible(
            closed_start,
            visible_closed,
            partial_visible,
            end == total,
            scale.range(),
        );
        let canvas_background = self.bg();
        if let Some(frame) = &orderflow_frame {
            self.orderflow.draw_background(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                live_span,
            );
        }

        // Grid + price labels first, behind the candles.
        self.draw_price_axis(painter, chart_rect, &scale);

        // Candles, clipped to the chart body so they don't spill into the axes.
        let clip = painter.with_clip_rect(chart_rect);
        // Clear the heat behind each candle's high–low span so a translucent
        // candle stays a clean divider — no liquidity band shows through it.
        // Where the price swept, the wall reads as consumed; bands survive only
        // in the gaps between candles and above/below each bar.
        if orderflow_frame.is_some() {
            let clear_bar = |xc: f32, bar: &quantick_engine::Bar| {
                let top = scale.y(bar.high.to_f64().unwrap_or(0.0));
                let bottom = scale.y(bar.low.to_f64().unwrap_or(0.0));
                clip.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - half, top),
                        egui::pos2(xc + half, bottom),
                    ),
                    egui::Rounding::ZERO,
                    canvas_background,
                );
            };
            for (offset, bar) in visible_closed.iter().enumerate() {
                clear_bar(
                    self.viewport.x_center(closed_start + offset, right, total),
                    bar,
                );
            }
            if let Some(partial) = partial_visible {
                clear_bar(self.viewport.x_center(closed.len(), right, total), partial);
            }
        }
        for (offset, bar) in visible_closed.iter().enumerate() {
            let index = closed_start + offset;
            let xc = self.viewport.x_center(index, right, total);
            draw_candle(&clip, xc, half, &scale, bar, false, &self.style.candles);
        }
        if let Some(partial) = partial_visible {
            let xc = self.viewport.x_center(closed.len(), right, total);
            draw_candle(&clip, xc, half, &scale, partial, true, &self.style.candles);
        }
        if let Some(frame) = &orderflow_frame {
            self.orderflow.draw_aggressions(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                live_span,
            );
        }

        self.draw_backfill_divider(painter, chart_rect, total, cw);
        self.draw_time_strip(painter, areas.time_strip, closed, start, end, total);
        self.draw_crosshair(painter, chart_rect, &scale);
        self.draw_header(painter, chart_rect);
        self.orderflow.draw_status_badge(painter, chart_rect);

        // Cache the auto range + height for next frame's input handler, which
        // runs before the draw and needs them for pixel↔price conversion.
        self.last_auto_range = Some(auto_range);
        self.last_chart_height = chart_rect.height();
    }

    /// Bottom time strip: a top border and a few `HH:MM:SS` labels for the
    /// visible bars. Draggable left/right to zoom the candle spacing.
    fn draw_time_strip(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        closed: &[quantick_engine::Bar],
        start: usize,
        end: usize,
        total: usize,
    ) {
        painter.line_segment(
            [
                egui::pos2(strip.left(), strip.top()),
                egui::pos2(strip.right(), strip.top()),
            ],
            egui::Stroke::new(1.0_f32, self.grid()),
        );
        let font = egui::FontId::monospace(10.0);
        let y = strip.center().y;
        // Up to ~6 evenly-spaced labels across the visible closed bars.
        let visible = end.saturating_sub(start);
        if visible == 0 {
            return;
        }
        let step = (visible / 6).max(1);
        let mut index = start;
        while index < end {
            if let Some(bar) = closed.get(index) {
                let x = self.viewport.x_center(index, strip.right(), total);
                if strip.x_range().contains(x) {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        fmt_time(bar.open_time, self.tz),
                        font.clone(),
                        MUTED,
                    );
                }
            }
            index += step;
        }
    }

    /// Right-hand price axis: round-number gridlines and labels.
    fn draw_price_axis(&self, painter: &egui::Painter, chart_rect: egui::Rect, scale: &PriceScale) {
        let (lo, hi) = scale.range();
        let font = egui::FontId::monospace(11.0);
        for tick in crate::chart::nice_ticks(lo, hi, 8) {
            let y = scale.y(tick);
            if y < chart_rect.top() || y > chart_rect.bottom() {
                continue;
            }
            painter.line_segment(
                [
                    egui::pos2(chart_rect.left(), y),
                    egui::pos2(chart_rect.right(), y),
                ],
                egui::Stroke::new(1.0_f32, self.grid()),
            );
            painter.text(
                egui::pos2(chart_rect.right() + 6.0, y),
                egui::Align2::LEFT_CENTER,
                format!("{tick:.2}"),
                font.clone(),
                MUTED,
            );
        }
        // The axis dividing line.
        painter.line_segment(
            [
                egui::pos2(chart_rect.right(), chart_rect.top()),
                egui::pos2(chart_rect.right(), chart_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, self.grid()),
        );
    }

    /// Crosshair following the pointer, with the price shown on the axis.
    fn draw_crosshair(&self, painter: &egui::Painter, chart_rect: egui::Rect, scale: &PriceScale) {
        let Some(pos) = self.hover_pos else {
            return;
        };
        if !chart_rect.contains(pos) {
            return;
        }
        let stroke = egui::Stroke::new(1.0_f32, CROSSHAIR);
        painter.line_segment(
            [
                egui::pos2(pos.x, chart_rect.top()),
                egui::pos2(pos.x, chart_rect.bottom()),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(chart_rect.left(), pos.y),
                egui::pos2(chart_rect.right(), pos.y),
            ],
            stroke,
        );

        // Price tag on the axis at the cursor height.
        let price = scale.price_at(pos.y);
        let galley = painter.layout_no_wrap(
            format!("{price:.2}"),
            egui::FontId::monospace(11.0),
            egui::Color32::WHITE,
        );
        let text_pos = egui::pos2(chart_rect.right() + 6.0, pos.y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect_filled(bg, egui::Rounding::same(2.0), TAG_BG);
        painter.galley(text_pos, galley, egui::Color32::WHITE);
    }

    /// A vertical marker separating backfilled history (left) from live (right),
    /// drawn only when the boundary falls inside the chart body.
    fn draw_backfill_divider(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        total: usize,
        candle_width: f32,
    ) {
        let Some(boundary) = self.state.backfill_boundary() else {
            return;
        };
        if boundary == 0 {
            return; // nothing backfilled
        }
        // The divider sits at the left edge of the first live bar.
        let x = self.viewport.x_center(boundary, chart_rect.right(), total) - candle_width / 2.0;
        if x < chart_rect.left() || x > chart_rect.right() {
            return; // off-screen
        }
        painter.line_segment(
            [
                egui::pos2(x, chart_rect.top()),
                egui::pos2(x, chart_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, DIVIDER),
        );
        let font = egui::FontId::proportional(11.0);
        painter.text(
            egui::pos2(x - 4.0, chart_rect.bottom() - 4.0),
            egui::Align2::RIGHT_BOTTOM,
            "backfill",
            font.clone(),
            MUTED,
        );
        painter.text(
            egui::pos2(x + 4.0, chart_rect.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "live",
            font,
            DIVIDER,
        );
    }

    fn draw_header(&self, painter: &egui::Painter, plot: egui::Rect) {
        let bars = self.state.bars();
        let (backfilled, live) = match self.state.backfill_boundary() {
            Some(b) => (b, bars.len().saturating_sub(b)),
            None => (0, bars.len()),
        };
        let mode = if self.viewport.follows_live() {
            "● live"
        } else {
            "history · double-click for live"
        };
        let price_mode = if self.price_view.is_auto() {
            ""
        } else {
            " · price: manual (double-click to auto-fit)"
        };
        let header = format!(
            "{} · {} · {} backfilled + {} live bars · {}{}",
            self.symbol,
            self.state.spec().summary(),
            backfilled,
            live,
            mode,
            price_mode
        );
        painter.text(
            egui::pos2(plot.left(), plot.top()),
            egui::Align2::LEFT_TOP,
            header,
            egui::FontId::proportional(13.0),
            MUTED,
        );
    }

    /// A bottom-left overlay with FPS, frame time and feed lag; values that
    /// breach a threshold are drawn in the warning colour. Sits in the empty
    /// lower-left of the chart so it doesn't cover the candles, and is toggled by
    /// the "perf" checkbox in the controls bar.
    fn draw_overlay(&self, painter: &egui::Painter, area: egui::Rect) {
        if !self.show_overlay {
            return;
        }
        let avg = self.frames.avg_ms();
        let lag = metrics::feed_lag_ms(metrics::wall_clock_ms(), self.latest_trade_ms);

        let fps_color = if avg.is_some_and(|a| a > metrics::SLOW_FRAME_MS) {
            WARN
        } else {
            OVERLAY
        };
        let lag_color = if lag.is_some_and(|l| l > metrics::HIGH_LAG_MS) {
            WARN
        } else {
            OVERLAY
        };
        let lag_text = match lag {
            Some(l) => format!("feed lag {l} ms"),
            None => "feed lag —".to_string(),
        };

        let lines: [(String, egui::Color32); 4] = [
            (
                format!(
                    "{:>4.0} fps  {:>5.1} ms",
                    self.frames.fps().unwrap_or(0.0),
                    avg.unwrap_or(0.0)
                ),
                fps_color,
            ),
            (
                format!("worst {:>5.1} ms", self.frames.worst_ms().unwrap_or(0.0)),
                OVERLAY,
            ),
            (lag_text, lag_color),
            (format!("{} live trades", self.live_trades), OVERLAY),
        ];

        let font = egui::FontId::monospace(12.0);
        let pad = 8.0;
        let line_h = 16.0;
        let box_w = 180.0;
        let box_h = lines.len() as f32 * line_h + pad;
        // Anchor to the empty lower-left of the chart body, above the time strip.
        let chart = plot_split(area).chart;
        let bottom_left = egui::pos2(chart.left() + 8.0, chart.bottom() - 8.0 - box_h);
        let backdrop = egui::Rect::from_min_size(bottom_left, egui::vec2(box_w, box_h));
        painter.rect_filled(
            backdrop,
            egui::Rounding::same(4.0),
            egui::Color32::from_black_alpha(150),
        );

        let mut y = backdrop.top() + pad / 2.0;
        let left = backdrop.left() + pad;
        for (text, color) in &lines {
            painter.text(
                egui::pos2(left, y),
                egui::Align2::LEFT_TOP,
                text,
                font.clone(),
                *color,
            );
            y += line_h;
        }
    }

    /// A discreet "history is loading" indicator centred at the chart's top: a
    /// small spinner plus a tiny label, shown while the initial backfill or an
    /// on-demand "load older" request is in flight. Centred so it never covers
    /// the symbol tag at the top-left or the perf overlay at the top-right.
    fn draw_history_loader(&self, ui: &mut egui::Ui, area: egui::Rect) {
        if self.pending_history_loads == 0 {
            return;
        }
        let size = 12.0;
        let gap = 6.0;
        let galley = ui.painter().layout_no_wrap(
            "loading history…".to_owned(),
            egui::FontId::proportional(10.0),
            MUTED,
        );
        // Centre spinner + gap + label as one group on the chart's mid-line.
        let total_w = size + gap + galley.size().x;
        let left = area.center().x - total_w / 2.0;
        let top = area.top() + 8.0;
        let spinner_rect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(size, size));
        ui.put(spinner_rect, egui::Spinner::new().size(size).color(MUTED));
        let text_pos = egui::pos2(left + size + gap, top + (size - galley.size().y) / 2.0);
        ui.painter().galley(text_pos, galley, MUTED);
    }
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            self.frames.record((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);

        self.drain_feed();
        self.drain_book_feed();
        self.maybe_emit_summary(now);

        let bg = self.bg();
        egui::TopBottomPanel::top("controls")
            .frame(egui::Frame::none().fill(self.chrome_bg()).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(OVERLAY);
                self.draw_controls(ui);
            });
        // Respawn the feed if the feed/symbol selection changed (resets the
        // chart), then apply any bar-type change (no-op if unchanged).
        self.maybe_switch_feed();
        self.state.set_spec(self.current_spec());
        self.draw_style_panel(ctx, now);
        if self.orderflow.draw_settings(ctx) {
            self.restart_book_capture();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                self.handle_navigation(ui, area);
                self.draw_chart(ui.painter(), area);
                self.draw_overlay(ui.painter(), area);
                self.draw_history_loader(ui, area);
            });
        self.draw_timezone_selector(ctx);
        // Live feed: keep polling the channel ~60×/s without busy-spinning.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::{FeedConfig, ProviderKind};

    /// A minimal one-feed, two-symbol config for the app tests.
    fn test_config() -> AppConfig {
        AppConfig {
            default_feed: "binance".to_string(),
            default_symbol: "TESTUSDT".to_string(),
            feeds: vec![FeedConfig {
                id: "binance".to_string(),
                name: "Binance".to_string(),
                provider: ProviderKind::Binance,
                symbols: vec!["TESTUSDT".to_string(), "ETHUSDT".to_string()],
            }],
            metatrader: Default::default(),
        }
    }

    /// An app wired to in-memory channels, plus the test's ends of them: send
    /// feed events in, observe feed commands out. No egui, no network.
    fn test_app() -> (
        QuantickApp,
        mpsc::Sender<FeedEvent>,
        mpsc::Receiver<FeedCommand>,
        mpsc::Sender<DepthEvent>,
    ) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                commands: cmd_tx,
            },
        );
        (app, evt_tx, cmd_rx, book_tx)
    }

    fn enable_heatmap_with_snapshot(
        app: &mut QuantickApp,
        commands: &mut mpsc::Receiver<FeedCommand>,
    ) {
        use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

        app.request_book_capture(true);
        let generation = match commands.try_recv().expect("capture command") {
            FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation,
            } => initial_generation,
            _ => panic!("unexpected command"),
        };
        app.orderflow.handle_depth_event(DepthEvent::Snapshot {
            symbol: "TESTUSDT".to_owned(),
            generation,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        });
        app.orderflow.flush_for_test();
        assert_eq!(app.orderflow.health().active_levels, 2);
    }

    #[test]
    fn loader_survives_until_every_pending_load_is_answered() {
        // Two "load older" clicks land while the initial backfill is still in
        // flight: three loads pending. The first reply must NOT hide the
        // indicator - only the last one may.
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        assert_eq!(app.pending_history_loads, 1, "backfill in flight at start");

        app.request_older_history();
        app.request_older_history();
        assert_eq!(app.pending_history_loads, 3);

        evt_tx.try_send(FeedEvent::Backfilled(Vec::new())).unwrap();
        app.drain_feed();
        assert_eq!(app.pending_history_loads, 2, "older loads still pending");

        evt_tx
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.drain_feed();
        assert_eq!(app.pending_history_loads, 1, "one reply answers one load");

        evt_tx
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.drain_feed();
        assert_eq!(app.pending_history_loads, 0, "last reply hides the loader");
    }

    #[test]
    fn rejected_request_does_not_arm_the_loader() {
        // With the command channel closed the request never reaches the feed,
        // so no reply will ever come - the count must not grow.
        let (mut app, _evt_tx, cmd_rx, _book_tx) = test_app();
        drop(cmd_rx);
        app.request_older_history();
        assert_eq!(app.pending_history_loads, 1, "only the initial backfill");
    }

    #[test]
    fn changing_feed_falls_back_to_a_valid_symbol() {
        // Two feeds with disjoint symbol lists: switching to a feed that does
        // not offer the current symbol must snap to that feed's first symbol.
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let config = AppConfig {
            default_feed: "a".to_string(),
            default_symbol: "AAA".to_string(),
            feeds: vec![
                FeedConfig {
                    id: "a".to_string(),
                    name: "A".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["AAA".to_string()],
                },
                FeedConfig {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["BBB".to_string()],
                },
            ],
            metatrader: Default::default(),
        };
        let mut app = QuantickApp::new(
            config,
            "a",
            "AAA",
            BarSpec::Tick(10),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                commands: cmd_tx,
            },
        );

        app.feed_id = "b".to_string();
        app.ensure_symbol_valid();
        assert_eq!(app.symbol, "BBB", "symbol snaps to feed b's first symbol");

        // A symbol already valid for the feed is left untouched.
        app.symbol = "BBB".to_string();
        app.ensure_symbol_valid();
        assert_eq!(app.symbol, "BBB");
    }

    #[test]
    fn capture_toggle_commits_only_after_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        assert!(!app.orderflow.enabled());

        app.request_book_capture(true);
        let command = cmd_rx.try_recv().expect("capture command");
        let generation = match command {
            FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation,
            } => initial_generation,
            _ => panic!("unexpected command"),
        };
        assert_eq!(generation, BOOK_GENERATION_STRIDE);
        assert!(app.orderflow.enabled());

        drop(cmd_rx);
        app.request_book_capture(false);
        assert!(
            app.orderflow.enabled(),
            "closed command channel must preserve current capture state"
        );
    }

    #[test]
    fn grouping_restart_commits_only_after_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let grouping = Decimal::new(5, 2);

        assert!(app.orderflow.stage_capture_grouping_for_test(grouping));
        assert_eq!(app.orderflow.health().active_levels, 2);
        app.restart_book_capture();

        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(FeedCommand::RestartBookCapture { .. })
        ));
        assert_eq!(app.orderflow.base_capture_grouping_for_test(), grouping);
        assert_eq!(app.orderflow.health().active_levels, 0);
        assert_eq!(app.orderflow.health().status, "connecting");
    }

    #[test]
    fn closed_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app.orderflow.base_capture_grouping_for_test();

        assert!(
            app.orderflow
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        drop(cmd_rx);
        app.restart_book_capture();

        assert_eq!(app.orderflow.base_capture_grouping_for_test(), original);
        assert_eq!(app.orderflow.health().active_levels, 2);
    }

    #[test]
    fn full_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app.orderflow.base_capture_grouping_for_test();
        let (full_tx, mut full_rx) = mpsc::channel(1);
        app.commands = full_tx;
        app.commands
            .try_send(FeedCommand::LoadOlder { count: 1 })
            .unwrap();

        assert!(
            app.orderflow
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        app.restart_book_capture();

        assert!(matches!(
            full_rx.try_recv(),
            Ok(FeedCommand::LoadOlder { count: 1 })
        ));
        assert_eq!(app.orderflow.base_capture_grouping_for_test(), original);
        assert_eq!(app.orderflow.health().active_levels, 2);
    }

    #[test]
    fn depth_channel_updates_heatmap_without_mutating_candles() {
        use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

        let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
        app.request_book_capture(true);
        let generation = match cmd_rx.try_recv().unwrap() {
            FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation,
            } => initial_generation,
            _ => panic!("unexpected command"),
        };
        let bars_before = app.state.bars().len();
        book_tx
            .try_send(DepthEvent::Snapshot {
                symbol: "TESTUSDT".to_owned(),
                generation,
                observed_at_ms: 1_100,
                effective_at_ms: 999,
                snapshot: BookSnapshot::new(
                    10,
                    vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                    vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                    BookCoverage::Limited {
                        levels_per_side: 1_000,
                    },
                ),
            })
            .unwrap();

        app.drain_book_feed();
        app.orderflow.flush_for_test();
        let book = app.orderflow.health();
        assert_eq!(book.bid_levels, 1);
        assert_eq!(book.ask_levels, 1);
        assert_eq!(app.state.bars().len(), bars_before);
    }

    #[test]
    fn candle_appearance_change_is_render_only() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        app.request_book_capture(true);
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(FeedCommand::SetBookCapture { enabled: true, .. })
        ));
        let capture_epoch = app.book_capture_epoch;
        let bar_spec = app.state.spec().clone();

        app.style.candles = CandlePreset::OutlineOnly.style();
        app.style_revision = app.style_revision.saturating_add(1);
        app.emit_style_changed(Some(CandlePreset::OutlineOnly));

        assert_eq!(app.state.spec(), &bar_spec);
        assert!(app.orderflow.enabled());
        assert_eq!(app.book_capture_epoch, capture_epoch);
        assert!(
            matches!(cmd_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "appearance changes must not restart or reconfigure market data"
        );
    }

    #[test]
    fn closed_depth_channel_is_reported_once_per_feed_handle() {
        let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
        app.request_book_capture(true);
        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(FeedCommand::SetBookCapture { enabled: true, .. })
        ));
        drop(book_tx);

        app.drain_book_feed();
        assert!(app.book_channel_closed_reported);
        app.drain_book_feed();
        assert!(
            app.book_channel_closed_reported,
            "subsequent frames keep the one-shot diagnostic latched"
        );
    }

    #[test]
    fn fmt_time_in_utc() {
        // Epoch: 1970-01-01 00:00:00 UTC, then +1h 2m 3s.
        assert_eq!(fmt_time(0, TzOffset::new(0)), "00:00:00");
        assert_eq!(fmt_time(3_723_000, TzOffset::new(0)), "01:02:03");
    }

    #[test]
    fn fmt_time_applies_the_offset() {
        // UTC midnight shown in UTC−03:00 is 21:00 of the previous day.
        assert_eq!(fmt_time(0, TzOffset::new(-180)), "21:00:00");
        // UTC midnight in UTC+05:30 is 05:30.
        assert_eq!(fmt_time(0, TzOffset::new(330)), "05:30:00");
    }
}
