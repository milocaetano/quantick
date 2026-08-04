//! Paper trading: the app-side host of the deterministic `quantick-sim`
//! simulator. UX contract: `docs/ux/paper-trading.md`.
//!
//! Ownership rules this module enforces:
//!
//! - Simulated order state lives *here*, never in the drawings overlay — a
//!   bar-spec change clears annotations, but orders belong to the session.
//! - The simulator taps the exact per-trade ingestion point the bar engine
//!   uses, so live feeds and replay behave identically.
//! - Closed trades are journaled to the history folder the moment they
//!   close, one self-contained CSV row per trade (`quantick_sim::history`);
//!   nothing else survives a session, and every surface says "SIM".

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use quantick_engine::{Side, Trade};
use quantick_sim::{
    Bracket, ClosedTrade, Command, EntryKind, OrderId, PerformanceReport, QueuedAction, SimEvent,
    Simulator, history,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::chart::PriceScale;
use crate::theme;

/// Overrides the history folder (cwd-relative otherwise, like every
/// quantick path).
const TRADES_DIR_ENV: &str = "QUANTICK_TRADES_DIR";
/// Default history folder, relative to the working directory.
const TRADES_DIR: &str = "paper-trades";
/// How long a paper toast stays on screen.
const TOAST_MS: u64 = 4_000;
/// Grab distance for order lines — the drawings' select radius, so the two
/// grammars feel identical under the pointer.
const LINE_GRAB_RADIUS_PX: f32 = 10.0;
/// Dash geometry of a pending order's line (the last-price line's rhythm).
const ORDER_DASH_PX: f32 = 4.0;
/// Gap between dashes of a pending order's line.
const ORDER_GAP_PX: f32 = 4.0;
/// How far above the bottom chrome the paper toast floats — clear of the
/// drawings toast so the two never overlap.
const TOAST_LIFT_PX: f32 = 96.0;
/// Price precision for snapped drags before any print reveals the
/// instrument's own (two decimals, the crypto-major default).
const SNAP_FALLBACK_DECIMALS: u32 = 2;
/// Text color inside colored gutter chips (the last-price chip's near-black).
const CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(0x14, 0x18, 0x1F);

/// The next chart click places this entry (`Limit` or `Stop` only — a
/// market order needs no price and fires straight from its button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmedPlacement {
    side: Side,
    kind: EntryKind,
}

/// Which simulated line the pointer is dragging.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum PaperDrag {
    #[default]
    None,
    StopLoss,
    TakeProfit,
    Order(OrderId),
    /// The press landed on the entry line: an average entry is history, not
    /// an order, so the geometry stays put — but the gesture still belongs
    /// to the line (the chart must not pan under it).
    Blocked,
}

/// One transient message; rejections double as the tutorial.
struct Toast {
    message: String,
    shown_at: Instant,
}

/// Report scope: one symbol's history or the whole folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportScope {
    Symbol,
    All,
}

/// What the report window shows, loaded fresh from disk when opened.
struct ReportData {
    report: PerformanceReport,
    trades: usize,
    files: usize,
    /// Files that were not readable quantick-trades files.
    unreadable_files: usize,
    /// Rows the parser had to report as unreadable (torn tails and such).
    problem_rows: usize,
}

/// Everything `handle_chart_input` needs from the frame, gathered by the
/// caller so this module never reads raw input state itself.
pub struct ChartInput<'a> {
    pub chart: egui::Rect,
    pub scale: Option<&'a PriceScale>,
    pub pointer: Option<egui::Pos2>,
    pub primary_pressed: bool,
    pub primary_down: bool,
    pub primary_released: bool,
    pub escape: bool,
}

/// The app-side paper-trading host: simulator, order-entry form state,
/// chart-layer interaction, journal and report.
pub struct PaperTrading {
    sim: Simulator,
    /// Symbol the journal writes under; follows the app's active symbol.
    symbol: String,
    dir: PathBuf,
    /// Current session file, named after the first closed trade.
    journal_path: Option<PathBuf>,
    /// A failed journal write warns once, not once per trade.
    journal_warned: bool,
    // Order-entry form.
    side: Side,
    qty_text: String,
    order_type: EntryKind,
    stop_offset_text: String,
    profit_offset_text: String,
    armed: Option<ArmedPlacement>,
    // Chart-layer drag.
    drag: PaperDrag,
    drag_price: Option<f64>,
    toast: Option<Toast>,
    report_open: bool,
    report_scope: ReportScope,
    report: Option<ReportData>,
}

impl Default for PaperTrading {
    fn default() -> Self {
        Self::new()
    }
}

impl PaperTrading {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sim: Simulator::new(),
            symbol: String::new(),
            dir: std::env::var_os(TRADES_DIR_ENV)
                .map_or_else(|| PathBuf::from(TRADES_DIR), PathBuf::from),
            journal_path: None,
            journal_warned: false,
            side: Side::Buy,
            qty_text: "1".to_owned(),
            order_type: EntryKind::Market,
            stop_offset_text: String::new(),
            profit_offset_text: String::new(),
            armed: None,
            drag: PaperDrag::None,
            drag_price: None,
            toast: None,
            report_open: false,
            report_scope: ReportScope::Symbol,
            report: None,
        }
    }

    /// Point the journal at a scratch folder (tests only).
    #[cfg(test)]
    pub(crate) fn redirect_history_dir(&mut self, dir: PathBuf) {
        self.dir = dir;
    }

    /// Follow the app's active symbol. A change retargets the journal; the
    /// simulator itself was already flattened by the timeline reset that
    /// every switch performs.
    pub fn set_symbol(&mut self, symbol: &str) {
        if self.symbol != symbol {
            self.symbol = symbol.to_owned();
            self.journal_path = None;
        }
    }

    /// Seed the mark from backfilled history — never fills (look-ahead).
    pub fn seed(&mut self, trade: &Trade) {
        self.sim.seed(trade);
    }

    /// Feed one live print through the simulator and act on what it did.
    pub fn on_trade(&mut self, trade: &Trade) {
        let events = self.sim.on_trade(trade);
        self.handle_events(events);
    }

    /// The source rebuilt its timeline (replay seek, feed/symbol switch,
    /// restart): pending orders are swept and the position flattens at the
    /// last mark, labeled `reset` — never silently.
    pub fn on_timeline_reset(&mut self) {
        let had_position = self.sim.position().is_some();
        let had_orders = !self.sim.orders().is_empty() || !self.sim.queued().is_empty();
        let events = self.sim.reset();
        for event in &events {
            if let SimEvent::Closed(trade) = event {
                self.journal(&trade.clone());
            }
        }
        self.armed = None;
        self.drag = PaperDrag::None;
        self.drag_price = None;
        if had_position {
            self.show_toast(
                "SIM position flattened - the timeline was rebuilt under it.".to_owned(),
            );
        } else if had_orders {
            self.show_toast(
                "SIM orders cancelled - the timeline was rebuilt under them.".to_owned(),
            );
        }
    }

    /// Whether the simulator has a price to trade against — the toolbar
    /// buttons disable themselves (with the reason) until this is true.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.sim.mark_price().is_some()
    }

    /// A toolbar/panel market order using the form's quantity and offsets.
    pub fn market(&mut self, side: Side) {
        let Some(quantity) = self.parse_quantity() else {
            return;
        };
        let reference = self.sim.mark_price().unwrap_or_default();
        let Some(bracket) = self.parse_bracket(side, reference) else {
            return;
        };
        let events = self.sim.apply(Command::PlaceMarket {
            side,
            quantity,
            bracket,
        });
        self.handle_events(events);
    }

    /// The status-bar cell: `SIM ±N pts` (realized + open), and the sign
    /// for its color. `None` while the simulator has never been touched.
    #[must_use]
    pub fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
        let untouched = self.sim.position().is_none()
            && self.sim.closed_trades().is_empty()
            && self.sim.orders().is_empty()
            && self.sim.queued().is_empty();
        if untouched {
            return None;
        }
        let open = self
            .sim
            .position()
            .and_then(|position| self.sim.mark_price().map(|mark| position.open_points(mark)));
        let total = self
            .sim
            .realized_points()
            .saturating_add(open.unwrap_or_default());
        Some((
            format!("SIM {} pts", fmt_signed_points(total)),
            total.cmp(&Decimal::ZERO),
        ))
    }

    // ------------------------------------------------------------------
    // Chart layer
    // ------------------------------------------------------------------

    /// Paint the simulated lines: pending orders (dashed, accent), then the
    /// position's entry / stop-loss / take-profit (solid, semantic colors).
    /// Chips share the last-price chip geometry so prices never disagree
    /// about their pixel.
    pub fn draw_layer(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
    ) {
        for order in self.sim.orders() {
            let Some(level) = order.price else { continue };
            let price = if self.drag == PaperDrag::Order(order.id) {
                self.drag_price
                    .unwrap_or_else(|| level.to_f64().unwrap_or_default())
            } else {
                level.to_f64().unwrap_or_default()
            };
            let label = format!(
                "#{} {} {} {} @ {}",
                order.id.0,
                side_word(order.side),
                kind_word(order.kind),
                fmt_decimal(order.quantity),
                fmt_decimal(level),
            );
            draw_price_line(
                painter,
                chart_rect,
                axis_x,
                scale,
                price,
                theme::ACCENT,
                true,
                &label,
            );
        }
        if let Some(position) = self.sim.position() {
            let entry_color = match position.side {
                Side::Buy => theme::BUY,
                Side::Sell => theme::SELL,
            };
            let entry = position.avg_price.to_f64().unwrap_or_default();
            let label = format!(
                "SIM {} {} @ {}",
                position_word(position.side),
                fmt_decimal(position.quantity),
                fmt_decimal(position.avg_price),
            );
            draw_price_line(
                painter,
                chart_rect,
                axis_x,
                scale,
                entry,
                entry_color,
                false,
                &label,
            );
            if let Some(stop) = position.stop_loss {
                let price = if self.drag == PaperDrag::StopLoss {
                    self.drag_price
                        .unwrap_or_else(|| stop.to_f64().unwrap_or_default())
                } else {
                    stop.to_f64().unwrap_or_default()
                };
                let label = format!(
                    "SL {} {} pts",
                    fmt_decimal(stop),
                    fmt_signed_points(position.open_points(stop)),
                );
                draw_price_line(
                    painter,
                    chart_rect,
                    axis_x,
                    scale,
                    price,
                    theme::SELL,
                    false,
                    &label,
                );
            }
            if let Some(target) = position.take_profit {
                let price = if self.drag == PaperDrag::TakeProfit {
                    self.drag_price
                        .unwrap_or_else(|| target.to_f64().unwrap_or_default())
                } else {
                    target.to_f64().unwrap_or_default()
                };
                let label = format!(
                    "TP {} {} pts",
                    fmt_decimal(target),
                    fmt_signed_points(position.open_points(target)),
                );
                draw_price_line(
                    painter,
                    chart_rect,
                    axis_x,
                    scale,
                    price,
                    theme::BUY,
                    false,
                    &label,
                );
            }
        }
        if let Some(armed) = self.armed {
            let hint = format!(
                "click a price to place your {} {} - Esc cancels",
                side_word(armed.side),
                kind_word(armed.kind),
            );
            painter.text(
                chart_rect.left_top() + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                hint,
                egui::FontId::proportional(12.0),
                theme::ACCENT,
            );
        }
    }

    /// Route pointer input to the simulated lines. Returns true when paper
    /// trading owns the gesture this frame — the chart must not pan and the
    /// drawings must not select under it.
    pub fn handle_chart_input(&mut self, input: &ChartInput<'_>) -> bool {
        if input.escape {
            self.armed = None;
        }

        // An armed placement takes the next chart click.
        if let Some(armed) = self.armed
            && input.primary_pressed
            && let Some(pointer) = input.pointer
            && input.chart.contains(pointer)
            && let Some(scale) = input.scale
        {
            self.place_armed(armed, scale.price_at(pointer.y));
            return true;
        }

        // Grab a line.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(pointer) = input.pointer
            && input.chart.contains(pointer)
            && let Some(scale) = input.scale
            && let Some(target) = self.line_at(pointer, scale)
        {
            self.drag = target;
            self.drag_price = Some(scale.price_at(pointer.y));
            return true;
        }

        // Follow the pointer while dragging.
        if input.primary_down && self.drag != PaperDrag::None {
            if let (Some(pointer), Some(scale)) = (input.pointer, input.scale) {
                let y = pointer.y.clamp(input.chart.top(), input.chart.bottom());
                self.drag_price = Some(scale.price_at(y));
            }
            return true;
        }

        // Drop: submit the new price; the simulator answers (a rejection
        // snaps the line back and the toast explains why).
        if input.primary_released && self.drag != PaperDrag::None {
            let drag = std::mem::take(&mut self.drag);
            if let Some(price) = self.drag_price.take() {
                let price = self.snap(price);
                let command = match drag {
                    PaperDrag::StopLoss => {
                        self.sim.position().map(|position| Command::SetBracket {
                            stop_loss: Some(price),
                            take_profit: position.take_profit,
                        })
                    }
                    PaperDrag::TakeProfit => {
                        self.sim.position().map(|position| Command::SetBracket {
                            stop_loss: position.stop_loss,
                            take_profit: Some(price),
                        })
                    }
                    PaperDrag::Order(id) => Some(Command::ModifyOrder { id, price }),
                    PaperDrag::None | PaperDrag::Blocked => None,
                };
                if let Some(command) = command {
                    let events = self.sim.apply(command);
                    self.handle_events(events);
                }
            }
            return true;
        }

        self.drag != PaperDrag::None
    }

    /// Which line sits under the pointer, in draw-stack priority: pending
    /// orders first (they draw on top), then take profit, stop loss, and
    /// the (blocked) entry line.
    fn line_at(&self, pointer: egui::Pos2, scale: &PriceScale) -> Option<PaperDrag> {
        let near = |price: Decimal| {
            let y = scale.y(price.to_f64().unwrap_or_default());
            (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        };
        for order in self.sim.orders().iter().rev() {
            if let Some(level) = order.price
                && near(level)
            {
                return Some(PaperDrag::Order(order.id));
            }
        }
        let position = self.sim.position()?;
        if let Some(target) = position.take_profit
            && near(target)
        {
            return Some(PaperDrag::TakeProfit);
        }
        if let Some(stop) = position.stop_loss
            && near(stop)
        {
            return Some(PaperDrag::StopLoss);
        }
        if near(position.avg_price) {
            return Some(PaperDrag::Blocked);
        }
        None
    }

    /// The armed click: build the command at the clicked price and let the
    /// simulator answer. Stays armed on a rejection — the toast explains
    /// where the order may sit, and the user clicks again.
    fn place_armed(&mut self, armed: ArmedPlacement, raw_price: f64) {
        let Some(quantity) = self.parse_quantity() else {
            return;
        };
        let price = self.snap(raw_price);
        let Some(bracket) = self.parse_bracket(armed.side, price) else {
            return;
        };
        let command = match armed.kind {
            EntryKind::Limit => Command::PlaceLimit {
                side: armed.side,
                quantity,
                price,
                bracket,
            },
            EntryKind::Stop => Command::PlaceStop {
                side: armed.side,
                quantity,
                trigger: price,
                bracket,
            },
            // Market never arms; the form fires it directly.
            EntryKind::Market => return,
        };
        let events = self.sim.apply(command);
        let placed = events
            .iter()
            .any(|event| matches!(event, SimEvent::Placed(_)));
        self.handle_events(events);
        if placed {
            self.armed = None;
        }
    }

    /// Round a pointer price to the precision the tape itself uses (the
    /// mark's decimal places), so a dragged line lands on a price the
    /// instrument can actually print.
    fn snap(&self, price: f64) -> Decimal {
        let places = self
            .sim
            .mark_price()
            .map_or(SNAP_FALLBACK_DECIMALS, |mark| mark.scale());
        Decimal::from_f64_retain(price)
            .unwrap_or_default()
            .round_dp(places)
            .normalize()
    }

    // ------------------------------------------------------------------
    // Dock tab
    // ------------------------------------------------------------------

    /// The Trading dock tab: position card, order entry, pending orders,
    /// session summary. See `docs/ux/paper-trading.md` §3.
    pub fn draw_trading_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Simulated fills from the tape - no broker, points not currency.")
                .color(theme::TEXT_MUTED)
                .small(),
        )
        .on_hover_text(
            "A market order fills at the next print; a limit at its own price when \
             the tape trades at or through it; a stop at the print that triggers it. \
             Nothing here touches a real account.",
        );
        ui.add_space(6.0);

        self.draw_position_card(ui);
        ui.separator();
        self.draw_order_entry(ui);
        ui.separator();
        self.draw_pending_orders(ui);
        ui.separator();
        self.draw_session_summary(ui);
    }

    fn draw_position_card(&mut self, ui: &mut egui::Ui) {
        let Some(position) = self.sim.position().cloned() else {
            ui.label(egui::RichText::new("No open position.").color(theme::TEXT_MUTED));
            return;
        };
        let color = match position.side {
            Side::Buy => theme::BUY,
            Side::Sell => theme::SELL,
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} {} @ {}",
                    position_word(position.side),
                    fmt_decimal(position.quantity),
                    fmt_decimal(position.avg_price),
                ))
                .color(color)
                .strong(),
            );
            if let Some(mark) = self.sim.mark_price() {
                let open = position.open_points(mark);
                ui.label(
                    egui::RichText::new(format!("{} pts", fmt_signed_points(open)))
                        .color(points_color(open)),
                )
                .on_hover_text("open profit at the last print, in points (price units × quantity)");
            }
        });
        let mut bracket_change = None;
        ui.horizontal(|ui| match position.stop_loss {
            Some(stop) => {
                ui.label(format!("stop loss {}", fmt_decimal(stop)));
                if ui
                    .small_button("clear")
                    .on_hover_text("remove the protective stop")
                    .clicked()
                {
                    bracket_change = Some(Command::SetBracket {
                        stop_loss: None,
                        take_profit: position.take_profit,
                    });
                }
            }
            None => {
                ui.label(
                    egui::RichText::new(
                        "stop loss - (drag one on the chart, or use the offsets below)",
                    )
                    .color(theme::TEXT_MUTED),
                );
            }
        });
        ui.horizontal(|ui| match position.take_profit {
            Some(target) => {
                ui.label(format!("take profit {}", fmt_decimal(target)));
                if ui
                    .small_button("clear")
                    .on_hover_text("remove the profit target")
                    .clicked()
                {
                    bracket_change = Some(Command::SetBracket {
                        stop_loss: position.stop_loss,
                        take_profit: None,
                    });
                }
            }
            None => {
                ui.label(
                    egui::RichText::new(
                        "take profit - (drag one on the chart, or use the offsets below)",
                    )
                    .color(theme::TEXT_MUTED),
                );
            }
        });
        if let Some(command) = bracket_change {
            let events = self.sim.apply(command);
            self.handle_events(events);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .button("Close")
                .on_hover_text("exit the position at the next print")
                .clicked()
            {
                let events = self.sim.apply(Command::ClosePosition);
                self.handle_events(events);
            }
            if ui
                .button("Flatten")
                .on_hover_text("close the position and cancel every pending order")
                .clicked()
            {
                let events = self.sim.apply(Command::Flatten);
                self.handle_events(events);
            }
        });
    }

    fn draw_order_entry(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.side,
                Side::Buy,
                egui::RichText::new("BUY").color(theme::BUY).strong(),
            );
            ui.selectable_value(
                &mut self.side,
                Side::Sell,
                egui::RichText::new("SELL").color(theme::SELL).strong(),
            );
            ui.label(egui::RichText::new("qty").color(theme::TEXT_MUTED));
            ui.add(egui::TextEdit::singleline(&mut self.qty_text).desired_width(48.0));
            egui::ComboBox::from_id_salt("paper_order_type")
                .selected_text(kind_word(self.order_type))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.order_type, EntryKind::Market, "market");
                    ui.selectable_value(&mut self.order_type, EntryKind::Limit, "limit");
                    ui.selectable_value(&mut self.order_type, EntryKind::Stop, "stop");
                });
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("stop -pts").color(theme::TEXT_MUTED))
                .on_hover_text(
                    "optional protective stop, this many points on the losing side of the \
                     entry; empty places no stop",
                );
            ui.add(egui::TextEdit::singleline(&mut self.stop_offset_text).desired_width(48.0));
            ui.label(egui::RichText::new("profit +pts").color(theme::TEXT_MUTED))
                .on_hover_text(
                    "optional profit target, this many points on the winning side of the \
                     entry; empty places no target",
                );
            ui.add(egui::TextEdit::singleline(&mut self.profit_offset_text).desired_width(48.0));
        });
        ui.add_space(4.0);
        let side = self.side;
        let side_color = match side {
            Side::Buy => theme::BUY,
            Side::Sell => theme::SELL,
        };
        match self.order_type {
            EntryKind::Market => {
                let label = format!(
                    "{} {} at market",
                    side_word_upper(side),
                    self.qty_text.trim()
                );
                let button = ui
                    .add_enabled(
                        self.ready(),
                        egui::Button::new(egui::RichText::new(label).color(side_color).strong()),
                    )
                    .on_hover_text("simulated: fills at the next print of the tape")
                    .on_disabled_hover_text("waiting for the first print - there is no market yet");
                if button.clicked() {
                    self.market(side);
                }
            }
            kind @ (EntryKind::Limit | EntryKind::Stop) => {
                if self.armed.is_some() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("click the chart at your price…")
                                .color(theme::ACCENT),
                        );
                        if ui.small_button("cancel").clicked() {
                            self.armed = None;
                        }
                    });
                } else {
                    let label = format!(
                        "Place {} {} on the chart…",
                        side_word(side),
                        kind_word(kind)
                    );
                    let button = ui
                        .add_enabled(
                            self.ready(),
                            egui::Button::new(egui::RichText::new(label).color(side_color)),
                        )
                        .on_hover_text(
                            "arms a click: the next chart click rests the order at that price \
                             (Esc cancels)",
                        )
                        .on_disabled_hover_text(
                            "waiting for the first print - there is no market yet",
                        );
                    if button.clicked() {
                        self.armed = Some(ArmedPlacement { side, kind });
                    }
                }
            }
        }
    }

    fn draw_pending_orders(&mut self, ui: &mut egui::Ui) {
        let queued_entries = self
            .sim
            .queued()
            .iter()
            .filter(|action| matches!(action, QueuedAction::Entry(_)))
            .count();
        let queued_closes = self.sim.queued().len() - queued_entries;
        if queued_entries > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{queued_entries} market order(s) await the next print"
                ))
                .color(theme::TEXT_MUTED)
                .small(),
            );
        }
        if queued_closes > 0 {
            ui.label(
                egui::RichText::new("closing at the next print…")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
        }
        let orders: Vec<_> = self.sim.orders().to_vec();
        if orders.is_empty() && queued_entries == 0 {
            ui.label(egui::RichText::new("No pending orders.").color(theme::TEXT_MUTED));
            return;
        }
        for order in orders {
            ui.horizontal(|ui| {
                let price = order.price.map_or_else(String::new, fmt_decimal);
                ui.label(format!(
                    "#{} {} {} {} @ {}",
                    order.id.0,
                    side_word(order.side),
                    kind_word(order.kind),
                    fmt_decimal(order.quantity),
                    price,
                ));
                if ui
                    .small_button("×")
                    .on_hover_text("cancel this order")
                    .clicked()
                {
                    let events = self.sim.apply(Command::CancelOrder { id: order.id });
                    self.handle_events(events);
                }
            });
        }
    }

    fn draw_session_summary(&mut self, ui: &mut egui::Ui) {
        ui.label(format!(
            "session: {} pts realized · {} closed trade(s)",
            fmt_signed_points(self.sim.realized_points()),
            self.sim.closed_trades().len(),
        ));
        ui.horizontal(|ui| {
            if ui
                .button("Report…")
                .on_hover_text("performance metrics computed from the saved history")
                .clicked()
            {
                self.open_report();
            }
        });
        ui.label(
            egui::RichText::new(format!("history: {}", self.dir.display()))
                .color(theme::TEXT_MUTED)
                .small(),
        );
    }

    // ------------------------------------------------------------------
    // Report window
    // ------------------------------------------------------------------

    fn open_report(&mut self) {
        self.report_open = true;
        self.reload_report();
    }

    fn reload_report(&mut self) {
        let symbol = match self.report_scope {
            ReportScope::Symbol => Some(self.symbol.as_str()),
            ReportScope::All => None,
        };
        self.report = Some(load_report(&self.dir, symbol));
    }

    /// The performance report, computed from what is actually on disk.
    pub fn draw_report_window(&mut self, ctx: &egui::Context) {
        if !self.report_open {
            return;
        }
        let mut open = true;
        let mut scope_changed = false;
        egui::Window::new("Simulated performance")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("scope").color(theme::TEXT_MUTED));
                    scope_changed |= ui
                        .selectable_value(
                            &mut self.report_scope,
                            ReportScope::Symbol,
                            format!("this symbol ({})", self.symbol),
                        )
                        .clicked();
                    scope_changed |= ui
                        .selectable_value(&mut self.report_scope, ReportScope::All, "all symbols")
                        .clicked();
                });
                ui.separator();
                match &self.report {
                    Some(data) if data.trades > 0 => draw_report_body(ui, data),
                    _ => {
                        ui.label(
                            egui::RichText::new(
                                "No saved trades yet - close a simulated trade and it lands here.",
                            )
                            .color(theme::TEXT_MUTED),
                        );
                    }
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "All figures are simulated, in points (price units × quantity) - \
                         the workspace knows no per-instrument currency value.",
                    )
                    .color(theme::TEXT_MUTED)
                    .small(),
                );
            });
        if scope_changed {
            self.reload_report();
        }
        if !open {
            self.report_open = false;
        }
    }

    // ------------------------------------------------------------------
    // Toast
    // ------------------------------------------------------------------

    /// The transient message slot; newest wins, expires on its own.
    pub fn draw_toast(&mut self, ctx: &egui::Context, now: Instant) {
        let Some(toast) = &self.toast else {
            return;
        };
        if now.saturating_duration_since(toast.shown_at) >= Duration::from_millis(TOAST_MS) {
            self.toast = None;
            return;
        }
        let message = toast.message.clone();
        egui::Area::new(egui::Id::new("paper_toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -TOAST_LIFT_PX))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::TAG_BG)
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(message).color(theme::TEXT_PRIMARY));
                    });
            });
    }

    fn show_toast(&mut self, message: String) {
        self.toast = Some(Toast {
            message,
            shown_at: Instant::now(),
        });
    }

    // ------------------------------------------------------------------
    // Events, journal, parsing
    // ------------------------------------------------------------------

    /// One funnel for everything the simulator reports: closures are
    /// journaled, fills and closures toast, rejections teach.
    fn handle_events(&mut self, events: Vec<SimEvent>) {
        for event in events {
            match event {
                SimEvent::Rejected(reason) => self.show_toast(format!("SIM: {reason}")),
                SimEvent::Filled(fill) => {
                    if matches!(fill.role, quantick_sim::FillRole::Entry(_)) {
                        self.show_toast(format!(
                            "SIM fill: {} {} @ {}",
                            side_word(fill.side),
                            fmt_decimal(fill.quantity),
                            fmt_decimal(fill.price),
                        ));
                    }
                }
                SimEvent::Closed(trade) => {
                    self.journal(&trade);
                    self.show_toast(format!(
                        "SIM closed: {} {} → {} pts ({})",
                        position_word(trade.side),
                        fmt_decimal(trade.quantity),
                        fmt_signed_points(trade.pnl_points),
                        trade.exit_reason.as_str().replace('_', " "),
                    ));
                }
                _ => {}
            }
        }
    }

    /// Append one closed trade to the session's history file, creating the
    /// file (with its header) on the first close. A failed write warns once
    /// and never crashes a trading session.
    fn journal(&mut self, trade: &ClosedTrade) {
        if self.symbol.is_empty() {
            return;
        }
        let folder = self.dir.join(sanitize_symbol(&self.symbol));
        let path = self.journal_path.get_or_insert_with(|| {
            folder.join(format!(
                "{}.{}",
                utc_compact(trade.closed_ms),
                history::FILE_EXTENSION
            ))
        });
        let mut text = String::new();
        if !path.exists() {
            text.push_str(&history::write_header(&self.symbol));
        }
        text.push_str(&history::write_trade(trade));
        let written = std::fs::create_dir_all(&folder).and_then(|()| {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&*path)
                .and_then(|mut file| file.write_all(text.as_bytes()))
        });
        if let Err(error) = written
            && !self.journal_warned
        {
            self.journal_warned = true;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_TRADE_JOURNAL_FAILED",
                path = %path.display(),
                %error,
                action = "trade_not_saved",
                "could not append to the paper-trading history"
            );
            self.show_toast(
                "SIM: could not save the trade history - see the log for the path.".to_owned(),
            );
        }
    }

    fn parse_quantity(&mut self) -> Option<Decimal> {
        match self.qty_text.trim().parse::<Decimal>() {
            Ok(quantity) if quantity > Decimal::ZERO => Some(quantity),
            _ => {
                self.show_toast(format!(
                    "SIM: quantity must be a positive number - got `{}`",
                    self.qty_text.trim(),
                ));
                None
            }
        }
    }

    /// Turn the optional offset fields into absolute protective prices
    /// around `reference` (the entry's own price, or the mark for market
    /// orders). `None` means a field failed to parse and was toasted.
    fn parse_bracket(&mut self, side: Side, reference: Decimal) -> Option<Bracket> {
        let stop_offset = match parse_offset(&self.stop_offset_text) {
            Ok(value) => value,
            Err(got) => {
                self.show_toast(format!(
                    "SIM: the stop offset must be a positive number of points - got `{got}`",
                ));
                return None;
            }
        };
        let profit_offset = match parse_offset(&self.profit_offset_text) {
            Ok(value) => value,
            Err(got) => {
                self.show_toast(format!(
                    "SIM: the profit offset must be a positive number of points - got `{got}`",
                ));
                return None;
            }
        };
        let (stop_loss, take_profit) = match side {
            Side::Buy => (
                stop_offset.map(|offset| reference.saturating_sub(offset)),
                profit_offset.map(|offset| reference.saturating_add(offset)),
            ),
            Side::Sell => (
                stop_offset.map(|offset| reference.saturating_add(offset)),
                profit_offset.map(|offset| reference.saturating_sub(offset)),
            ),
        };
        Some(Bracket {
            stop_loss,
            take_profit,
        })
    }
}

/// The report body: one metric per row, the explanation on hover, honest
/// blanks (`—`) where a ratio has no denominator.
fn draw_report_body(ui: &mut egui::Ui, data: &ReportData) {
    let report = &data.report;
    egui::Grid::new("paper_report_grid")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String, explain: &str| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED))
                    .on_hover_text(explain.to_owned());
                ui.label(value);
                ui.end_row();
            };
            row(
                "net P&L",
                format!("{} pts", fmt_signed_points(report.net_points)),
                "every closed trade's points, summed",
            );
            row(
                "trades",
                format!(
                    "{} ({} long / {} short)",
                    report.trades, report.long_trades, report.short_trades
                ),
                "closed round trips in the saved history",
            );
            row(
                "win rate",
                report
                    .win_rate_pct
                    .map_or_else(|| "—".to_owned(), |rate| format!("{}%", fmt_points(rate))),
                "winning trades over all trades; a scratch counts as a trade but not a win",
            );
            row(
                "profit factor",
                report
                    .profit_factor
                    .map_or_else(|| "—".to_owned(), fmt_points),
                "gross profit divided by gross loss; above 1 means the wins outweigh — \
                 blank with no losses, because an undefined ratio is not infinity",
            );
            row(
                "max drawdown",
                format!("{} pts", fmt_points(report.max_drawdown_points)),
                "deepest drop of realized equity below its running peak, in closing order",
            );
            row(
                "gross profit / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.gross_profit),
                    fmt_points(report.gross_loss)
                ),
                "winners' points and losers' magnitude, before netting",
            );
            row(
                "avg win / loss",
                format!(
                    "{} / {} pts",
                    report.avg_win.map_or_else(|| "—".to_owned(), fmt_points),
                    report.avg_loss.map_or_else(|| "—".to_owned(), fmt_points),
                ),
                "mean winner and mean loser magnitude",
            );
            row(
                "largest win / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.largest_win),
                    fmt_points(report.largest_loss)
                ),
                "best single trade and worst single trade magnitude",
            );
        });
    if data.unreadable_files > 0 || data.problem_rows > 0 {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "{} file(s) unreadable, {} row(s) skipped - counted, never silently dropped.",
                data.unreadable_files, data.problem_rows,
            ))
            .color(theme::AMBER)
            .small(),
        );
    }
    ui.label(
        egui::RichText::new(format!(
            "{} trade(s) across {} file(s)",
            data.trades, data.files
        ))
        .color(theme::TEXT_MUTED)
        .small(),
    );
}

/// Read every history file under `dir` (one symbol's folder, or all of
/// them), merge chronologically, and aggregate. Missing folders are simply
/// empty — the report says "no saved trades", not an error.
fn load_report(dir: &Path, symbol: Option<&str>) -> ReportData {
    let mut folders = Vec::new();
    match symbol {
        Some(symbol) => folders.push(dir.join(sanitize_symbol(symbol))),
        None => {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        folders.push(path);
                    }
                }
                folders.sort();
            }
        }
    }
    let mut trades = Vec::new();
    let mut files = 0usize;
    let mut unreadable_files = 0usize;
    let mut problem_rows = 0usize;
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == history::FILE_EXTENSION)
            })
            .collect();
        paths.sort();
        for path in paths {
            files += 1;
            match std::fs::read_to_string(&path).map_err(|error| error.to_string()) {
                Ok(text) => match history::parse(&text) {
                    Ok(parsed) => {
                        problem_rows += parsed.problems.len();
                        trades.extend(parsed.trades);
                    }
                    Err(_) => unreadable_files += 1,
                },
                Err(_) => unreadable_files += 1,
            }
        }
    }
    // Files are per-session; merge into one closing-order timeline so the
    // drawdown walk is honest across sessions.
    trades.sort_by_key(|trade| (trade.closed_ms, trade.opened_ms));
    ReportData {
        report: PerformanceReport::from_trades(&trades),
        trades: trades.len(),
        files,
        unreadable_files,
        problem_rows,
    }
}

/// One price line with its gutter chip — the last-price chip geometry, so
/// prices never disagree about their pixel.
#[expect(clippy::too_many_arguments, reason = "a paint helper, not an API")]
fn draw_price_line(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    axis_x: f32,
    scale: &PriceScale,
    price: f64,
    color: egui::Color32,
    dashed: bool,
    label: &str,
) {
    let y = scale.y(price);
    if y < chart_rect.top() || y > chart_rect.bottom() {
        return;
    }
    let stroke = egui::Stroke::new(1.0_f32, color);
    if dashed {
        painter.extend(egui::Shape::dashed_line(
            &[egui::pos2(chart_rect.left(), y), egui::pos2(axis_x, y)],
            stroke,
            ORDER_DASH_PX,
            ORDER_GAP_PX,
        ));
    } else {
        painter.line_segment(
            [egui::pos2(chart_rect.left(), y), egui::pos2(axis_x, y)],
            stroke,
        );
    }
    let galley = painter.layout_no_wrap(label.to_owned(), egui::FontId::monospace(11.0), CHIP_TEXT);
    let text_pos = egui::pos2(axis_x + 6.0, y - galley.size().y / 2.0);
    let bg = egui::Rect::from_min_size(
        text_pos - egui::vec2(3.0, 1.0),
        galley.size() + egui::vec2(6.0, 2.0),
    );
    painter.rect_filled(bg, egui::Rounding::same(2.0), color);
    painter.galley(text_pos, galley, CHIP_TEXT);
}

/// Empty means "none"; otherwise a strictly positive decimal.
fn parse_offset(text: &str) -> Result<Option<Decimal>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<Decimal>() {
        Ok(value) if value > Decimal::ZERO => Ok(Some(value)),
        _ => Err(trimmed.to_owned()),
    }
}

fn side_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn side_word_upper(side: Side) -> &'static str {
    match side {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

fn position_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "LONG",
        Side::Sell => "SHORT",
    }
}

fn kind_word(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "market",
        EntryKind::Limit => "limit",
        EntryKind::Stop => "stop",
    }
}

fn points_color(points: Decimal) -> egui::Color32 {
    match points.cmp(&Decimal::ZERO) {
        std::cmp::Ordering::Greater => theme::BUY,
        std::cmp::Ordering::Less => theme::SELL,
        std::cmp::Ordering::Equal => theme::TEXT_MUTED,
    }
}

/// Exact value, trailing zeros stripped — prices and quantities.
fn fmt_decimal(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Points rounded to two places for display (the stored value stays exact).
fn fmt_points(value: Decimal) -> String {
    value.round_dp(2).normalize().to_string()
}

/// Signed points: an explicit `+` on gains so a green `12` can never be
/// misread as a count.
fn fmt_signed_points(value: Decimal) -> String {
    if value > Decimal::ZERO {
        format!("+{}", fmt_points(value))
    } else {
        fmt_points(value)
    }
}

/// Keep the characters real venue symbols use (`WDO$`, `WIN@N`… stay
/// recognizable); anything else becomes `_` so a symbol can never traverse
/// paths.
fn sanitize_symbol(symbol: &str) -> String {
    let cleaned: String = symbol
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "-_.$#".contains(character) {
                character
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "_".to_owned()
    } else {
        cleaned
    }
}

/// `YYYYMMDD-HHMMSS` in UTC from epoch milliseconds — session file names
/// derive from venue time, so the same replay run names the same file.
/// Civil-from-days per Howard Hinnant's algorithm; no clock, no chrono.
fn utc_compact(timestamp_ms: i64) -> String {
    let seconds = timestamp_ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(agg_id: u64, price: i64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    #[test]
    fn utc_compact_matches_known_timestamps() {
        // 2026-03-16 13:01:08 UTC.
        assert_eq!(utc_compact(1_773_666_068_000), "20260316-130108");
        // The epoch itself.
        assert_eq!(utc_compact(0), "19700101-000000");
    }

    #[test]
    fn symbols_sanitize_without_losing_venue_spellings() {
        assert_eq!(sanitize_symbol("WDO$"), "WDO$");
        assert_eq!(sanitize_symbol("BTCUSDT"), "BTCUSDT");
        assert_eq!(sanitize_symbol("../evil"), ".._evil");
        assert_eq!(sanitize_symbol(""), "_");
    }

    #[test]
    fn signed_points_always_carry_their_sign() {
        assert_eq!(fmt_signed_points(Decimal::from(12)), "+12");
        assert_eq!(fmt_signed_points(Decimal::from(-3)), "-3");
        assert_eq!(fmt_signed_points(Decimal::ZERO), "0");
    }

    #[test]
    fn closed_trades_journal_to_one_session_file_and_reload() {
        let dir = std::env::temp_dir().join("quantick-paper-journal-test");
        let _ = std::fs::remove_dir_all(&dir);
        let mut paper = PaperTrading::new();
        paper.dir.clone_from(&dir);
        paper.set_symbol("TESTUSDT");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(2, 105));
        // A second round trip appends to the same session file.
        paper.market(Side::Sell);
        paper.on_trade(&print(3, 105));
        let events = paper.sim.apply(Command::ClosePosition);
        paper.handle_events(events);
        paper.on_trade(&print(4, 103));

        let folder = dir.join("TESTUSDT");
        let files: Vec<_> = std::fs::read_dir(&folder)
            .expect("the symbol folder exists")
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "one session, one file");
        let text = std::fs::read_to_string(files[0].path()).expect("readable");
        let parsed = history::parse(&text).expect("valid history");
        assert_eq!(parsed.symbol.as_deref(), Some("TESTUSDT"));
        assert_eq!(parsed.trades.len(), 2);
        assert!(parsed.problems.is_empty());
        assert_eq!(parsed.trades[0].pnl_points, Decimal::from(5));
        assert_eq!(parsed.trades[1].pnl_points, Decimal::from(2));

        let data = load_report(&dir, Some("TESTUSDT"));
        assert_eq!(data.trades, 2);
        assert_eq!(data.report.net_points, Decimal::from(7));
        assert_eq!(data.files, 1);
        assert_eq!(data.unreadable_files, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_timeline_reset_journals_the_flatten_and_clears_the_form_state() {
        let dir = std::env::temp_dir().join("quantick-paper-reset-test");
        let _ = std::fs::remove_dir_all(&dir);
        let mut paper = PaperTrading::new();
        paper.dir.clone_from(&dir);
        paper.set_symbol("RESETX");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        paper.on_timeline_reset();
        assert!(paper.sim.position().is_none());
        assert!(
            paper.armed.is_none(),
            "an armed click dies with the timeline"
        );
        assert!(paper.toast.is_some(), "the flatten is never silent");
        let data = load_report(&dir, Some("RESETX"));
        assert_eq!(data.trades, 1);
        assert_eq!(
            data.report.trades, 1,
            "the reset exit is a real, journaled trade"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn market_offsets_become_a_bracket_around_the_reference() {
        let mut paper = PaperTrading::new();
        paper.stop_offset_text = "5".to_owned();
        paper.profit_offset_text = "10".to_owned();
        let bracket = paper
            .parse_bracket(Side::Buy, Decimal::from(100))
            .expect("both parse");
        assert_eq!(bracket.stop_loss, Some(Decimal::from(95)));
        assert_eq!(bracket.take_profit, Some(Decimal::from(110)));
        let bracket = paper
            .parse_bracket(Side::Sell, Decimal::from(100))
            .expect("both parse");
        assert_eq!(bracket.stop_loss, Some(Decimal::from(105)));
        assert_eq!(bracket.take_profit, Some(Decimal::from(90)));
    }

    #[test]
    fn a_bad_offset_toasts_and_blocks_the_order() {
        let mut paper = PaperTrading::new();
        paper.stop_offset_text = "abc".to_owned();
        assert!(paper.parse_bracket(Side::Buy, Decimal::from(100)).is_none());
        assert!(paper.toast.is_some(), "the refusal teaches, never silent");
    }

    /// A 800×400 chart over the given price range, plus the input for one
    /// pointer frame at `(x, y)`.
    fn chart_and_scale(lo: f64, hi: f64) -> (egui::Rect, PriceScale) {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        (chart, PriceScale::from_range(lo, hi, 0.0, 400.0))
    }

    fn frame<'a>(
        chart: egui::Rect,
        scale: &'a PriceScale,
        y: f32,
        pressed: bool,
        down: bool,
        released: bool,
    ) -> ChartInput<'a> {
        ChartInput {
            chart,
            scale: Some(scale),
            pointer: Some(egui::pos2(400.0, y)),
            primary_pressed: pressed,
            primary_down: down,
            primary_released: released,
            escape: false,
        }
    }

    #[test]
    fn an_armed_click_places_the_order_at_the_clicked_price_and_disarms() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        let (chart, scale) = chart_and_scale(90.0, 110.0);
        // y = 300 sits at price 95 on this scale.
        let consumed = paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false));
        assert!(consumed, "the armed click never reaches the chart pan");
        assert!(paper.armed.is_none(), "a successful placement disarms");
        assert_eq!(paper.sim.orders().len(), 1);
        assert_eq!(paper.sim.orders()[0].price, Some(Decimal::from(95)));
    }

    #[test]
    fn a_rejected_armed_click_stays_armed_and_teaches() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        let (chart, scale) = chart_and_scale(90.0, 110.0);
        // y = 100 sits at price 105 — a buy limit above the market.
        let consumed = paper.handle_chart_input(&frame(chart, &scale, 100.0, true, true, false));
        assert!(consumed);
        assert!(
            paper.armed.is_some(),
            "the user clicks again after the toast"
        );
        assert!(paper.sim.orders().is_empty());
        assert!(paper.toast.is_some(), "the refusal explains itself");
    }

    #[test]
    fn dragging_the_stop_loss_reprices_it_on_release() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.stop_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        assert_eq!(
            paper.sim.position().expect("long").stop_loss,
            Some(Decimal::from(90)),
        );
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // The stop at 90 sits at y = 300; grab it, pull to 95 (y = 250), drop.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, false, true)));
        assert_eq!(
            paper.sim.position().expect("still long").stop_loss,
            Some(Decimal::from(95)),
            "the drop resubmitted the bracket at the dragged price"
        );
    }

    #[test]
    fn the_entry_line_blocks_the_gesture_but_never_moves() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // The entry at 100 sits at y = 200: grabbing it consumes the gesture
        // (the chart must not pan under it) but repositions nothing.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, false, true)));
        assert_eq!(
            paper.sim.position().expect("long").avg_price,
            Decimal::from(100),
            "an average entry is history, not an order"
        );
        // Empty space is not ours: the press falls through to the chart.
        assert!(
            !paper.handle_chart_input(&frame(chart, &scale, 40.0, true, true, false)),
            "a press far from every line belongs to the pan"
        );
    }

    #[test]
    fn snapping_uses_the_marks_own_precision() {
        let mut paper = PaperTrading::new();
        paper.seed(&Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: Decimal::new(10325, 2), // 103.25 → two decimal places
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert_eq!(paper.snap(101.23456), Decimal::new(10123, 2));
        // An integer-printing instrument snaps drags to whole points.
        paper.seed(&print(2, 182_035));
        assert_eq!(paper.snap(182_036.7), Decimal::from(182_037));
    }
}
