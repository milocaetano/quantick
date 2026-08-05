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
    Bracket, ClosedTrade, Command, EntryKind, OrderId, PerformanceReport, Position, QueuedAction,
    SimEvent, Simulator, history,
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
/// The position's entry line leads the paper lines: it is the one that is
/// history rather than an order, and it matches the drawings' default width.
const POSITION_LINE_WIDTH_PX: f32 = 1.5;
/// Resting width of every other paper line (orders, stop, target).
const LINE_WIDTH_PX: f32 = 1.0;
/// Width of a paper line while the pointer is within grab range — the same
/// emphasis step the drawings use.
const LINE_HOVER_WIDTH_PX: f32 = 1.5;
/// Width of a paper line while it is being dragged.
const LINE_DRAG_WIDTH_PX: f32 = 2.0;
/// The drawings' selection-halo treatment, mirrored for a dragged paper
/// line so a grabbed stop feels identical to a grabbed drawing (the
/// originals are private to `drawings`).
const DRAG_HALO_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(40, 40, 40, 40);
/// How much wider than the line the halo pass paints.
const DRAG_HALO_EXTRA_WIDTH_PX: f32 = 3.5;
/// Height of an in-plot tag (fits mono 11 plus its padding).
const TAG_HEIGHT_PX: f32 = 20.0;
/// Gap between a tag's right edge and the plot's right edge — the inside
/// mirror of the gutter chips' `AXIS_LABEL_GAP_PX`.
const TAG_GAP_PX: f32 = 6.0;
/// Horizontal padding inside a tag.
const TAG_PAD_X: f32 = 6.0;
/// Width of the ✕ zone a hovered tag reveals. An overlay convenience —
/// every action here has a ≥ 28 px twin in the chrome.
const TAG_BUTTON_PX: f32 = 20.0;
/// How far around a tag the hover that reveals its ✕ still counts.
const TAG_HOVER_SLACK_PX: f32 = 4.0;
/// Size of a labelled SL/TP bracket handle on the entry line.
const HANDLE_SIZE: egui::Vec2 = egui::vec2(20.0, 14.0);
/// Vertical clearance between the entry line and a bracket handle.
const HANDLE_GAP_PX: f32 = 4.0;
/// Horizontal gap between the position tag and its bracket handles.
const HANDLE_TAG_GAP_PX: f32 = 8.0;
/// How far (in pixels) a press on the entry line must travel before it
/// commits to creating one bracket leg — the drawings' drag threshold.
const CREATE_DECIDE_THRESHOLD_PX: f32 = 4.0;
/// Text color inside colored gutter chips — the same ink as the last-price
/// chip (`LAST_PRICE_CHIP_TEXT` is private to `app.rs`, so the value is
/// duplicated here; keep the two identical).
const CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(0x0E, 0x12, 0x1A);
/// Vertical clearance between a paper chip's centre and the last-price
/// chip's, in pixels — just over one chip height, so the two can never
/// overprint. At the instant a market order fills, the entry price *is* the
/// last price, and without this the one persistent "you are long" statement
/// is born unreadable.
const CHIP_CLEAR_PX: f32 = 16.0;

/// The next chart click places this entry (`Limit` or `Stop` only — a
/// market order needs no price and fires straight from its button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmedPlacement {
    side: Side,
    kind: EntryKind,
}

/// The open position, read-only, as every chrome surface reports it — the
/// HUD, the dock badge and the status cell all describe the same trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionSummary {
    /// Which way the position points.
    pub side: Side,
    /// Contracts/units held.
    pub quantity: Decimal,
    /// Average entry price.
    pub avg_price: Decimal,
    /// Open profit at the current mark; `None` before any mark exists.
    pub open_points: Option<Decimal>,
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
    /// to the line (the chart must not pan under it). This is the state for
    /// a fully bracketed position, whose legs are their own handles.
    Blocked,
    /// The press landed on the entry line and at least one bracket leg is
    /// missing: the first committed pull decides which leg the drag creates
    /// (profit side → take profit, losing side → stop loss).
    CreatePending,
    /// Dragging a stop loss into existence from the entry line or its
    /// handle; release submits it, exactly like repricing an existing one.
    CreateStopLoss,
    /// Dragging a take profit into existence (see `CreateStopLoss`).
    CreateTakeProfit,
}

/// A painted overlay control from the last paint pass: a tag's ✕ or a
/// bracket handle. Hit rects are cached one frame behind the paint — the
/// input pass runs before the draw, and an immediate-mode overlay control is
/// pressed against where it was actually painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaperControl {
    /// ✕ on the position tag: exit at the next print.
    ClosePosition,
    /// ✕ on the stop-loss tag: clear the protective stop.
    ClearStopLoss,
    /// ✕ on the take-profit tag: clear the profit target.
    ClearTakeProfit,
    /// ✕ on a working order's tag: cancel it.
    CancelOrder(OrderId),
    /// Labelled `SL` handle on the entry line: press starts a create-drag.
    HandleStopLoss,
    /// Labelled `TP` handle on the entry line.
    HandleTakeProfit,
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
    /// Interactive rects painted by the last `draw_layer` pass (tag ✕s,
    /// bracket handles), pressed against on the next input pass.
    controls: Vec<(PaperControl, egui::Rect)>,
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
            controls: Vec::new(),
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

    /// The status-bar cell, honest about open versus flat: `SIM LONG 1 ·
    /// +2 pts` while a position is open (side, size and its open profit),
    /// `SIM +7 pts · flat` otherwise (the session's realized points). `None`
    /// while the simulator has never been touched.
    #[must_use]
    pub fn status_cell(&self) -> Option<(String, std::cmp::Ordering)> {
        if let Some(position) = self.sim.position() {
            let open = self
                .sim
                .mark_price()
                .map(|mark| position.open_points(mark))
                .unwrap_or_default();
            return Some((
                format!(
                    "SIM {} {} · {} pts",
                    position_word(position.side),
                    fmt_decimal(position.quantity),
                    fmt_signed_points(open),
                ),
                open.cmp(&Decimal::ZERO),
            ));
        }
        let untouched = self.sim.closed_trades().is_empty()
            && self.sim.orders().is_empty()
            && self.sim.queued().is_empty();
        if untouched {
            return None;
        }
        let realized = self.sim.realized_points();
        Some((
            format!("SIM {} pts · flat", fmt_signed_points(realized)),
            realized.cmp(&Decimal::ZERO),
        ))
    }

    /// The open position as the chrome reports it: side, size, entry, and
    /// the open profit at the current mark. `None` while flat.
    #[must_use]
    pub fn position_summary(&self) -> Option<PositionSummary> {
        let position = self.sim.position()?;
        Some(PositionSummary {
            side: position.side,
            quantity: position.quantity,
            avg_price: position.avg_price,
            open_points: self.sim.mark_price().map(|mark| position.open_points(mark)),
        })
    }

    /// Exit the open position at the next print — the toolbar's close
    /// button, the HUD's, and the Trading tab's all funnel here.
    pub fn close_position(&mut self) {
        let events = self.sim.apply(Command::ClosePosition);
        self.handle_events(events);
    }

    /// Close the position and cancel every pending order.
    pub fn flatten(&mut self) {
        let events = self.sim.apply(Command::Flatten);
        self.handle_events(events);
    }

    /// Flip the open position: one market order for twice its size, which
    /// closes it and opens the opposite side at the same quantity. The
    /// form's protective offsets apply to the new entry, exactly as they do
    /// to any market order.
    pub fn reverse_position(&mut self) {
        let Some(position) = self.sim.position().cloned() else {
            return;
        };
        let side = match position.side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        let reference = self.sim.mark_price().unwrap_or_default();
        let Some(bracket) = self.parse_bracket(side, reference) else {
            return;
        };
        let events = self.sim.apply(Command::PlaceMarket {
            side,
            quantity: position.quantity.saturating_add(position.quantity),
            bracket,
        });
        self.handle_events(events);
    }

    /// The form quantity, parsed without side effects — the label builders
    /// peek at it every frame and must not toast.
    fn quantity_preview(&self) -> Option<Decimal> {
        match self.qty_text.trim().parse::<Decimal>() {
            Ok(quantity) if quantity > Decimal::ZERO => Some(quantity),
            _ => None,
        }
    }

    /// State-aware entry label: what pressing this side's button would do to
    /// the open position — `SELL 1 (closes)`, `SELL 5 (reverses to short
    /// 4)`. Whether a press closes or flips hangs on a quantity field the
    /// toolbar never shows, so the button itself must say. Falls back to the
    /// bare side word while the form quantity does not parse — the click
    /// will toast the correction.
    #[must_use]
    pub fn entry_label(&self, side: Side) -> String {
        let word = side_word_upper(side);
        let Some(qty) = self.quantity_preview() else {
            return word.to_owned();
        };
        let qty_text = fmt_decimal(qty);
        let Some(position) = self.sim.position() else {
            return format!("{word} {qty_text}");
        };
        if position.side == side {
            return format!(
                "{word} {qty_text} (adds to {})",
                fmt_decimal(position.quantity.saturating_add(qty)),
            );
        }
        match qty.cmp(&position.quantity) {
            std::cmp::Ordering::Less => format!(
                "{word} {qty_text} (closes {qty_text} of {})",
                fmt_decimal(position.quantity),
            ),
            std::cmp::Ordering::Equal => format!("{word} {qty_text} (closes)"),
            std::cmp::Ordering::Greater => format!(
                "{word} {qty_text} (reverses to {} {})",
                match side {
                    Side::Buy => "long",
                    Side::Sell => "short",
                },
                fmt_decimal(qty.saturating_sub(position.quantity)),
            ),
        }
    }

    /// The entry buttons' hover text: the quantity and protective offsets
    /// the press will use, which the toolbar itself has no widgets for.
    #[must_use]
    pub fn entry_hover(&self, side: Side) -> String {
        let quantity = match self.quantity_preview() {
            Some(qty) => format!("quantity {}", fmt_decimal(qty)),
            None => "the quantity is not a positive number".to_owned(),
        };
        let bracket = match (
            parse_offset(&self.stop_offset_text),
            parse_offset(&self.profit_offset_text),
        ) {
            (Ok(None), Ok(None)) => "no protective bracket set".to_owned(),
            (Ok(stop), Ok(profit)) => {
                let mut parts = Vec::new();
                if let Some(stop) = stop {
                    parts.push(format!("stop {} pts", fmt_decimal(stop)));
                }
                if let Some(profit) = profit {
                    parts.push(format!("target {} pts", fmt_decimal(profit)));
                }
                format!("{} on fill", parts.join(" / "))
            }
            _ => "an offset field needs fixing".to_owned(),
        };
        format!(
            "simulated market {} - fills at the next print; {quantity}, {bracket} (Trading tab)",
            side_word(side),
        )
    }

    /// `Close 1 LONG` while a position is open — the toolbar's exit button
    /// label. `None` while flat, which is what removes the button.
    #[must_use]
    pub fn close_button_label(&self) -> Option<String> {
        let position = self.sim.position()?;
        Some(format!(
            "Close {} {}",
            fmt_decimal(position.quantity),
            position_word(position.side),
        ))
    }

    // ------------------------------------------------------------------
    // Chart layer
    // ------------------------------------------------------------------

    /// Paint the simulated lines and their in-plot tags: pending orders
    /// (dashed, accent), then the position's entry / stop-loss / take-profit
    /// (solid, semantic colors). Gutter chips keep the last-price chip's
    /// geometry and carry *the price and nothing else*, so prices never
    /// disagree about their pixel; the words and the controls live in tags
    /// right-anchored inside the plot. `pointer` is `Some` only on the pane
    /// that owns paper input, so hover affordances (a tag's ✕, the bracket
    /// handles) paint nowhere else. The interactive rects painted here are
    /// cached for the next input pass — immediate mode presses against what
    /// was actually drawn.
    #[expect(
        clippy::too_many_arguments,
        reason = "the pane hands over its frame geometry; bundling it here would rename, not simplify"
    )]
    pub fn draw_layer(
        &mut self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        tag_right: f32,
        axis_x: f32,
        scale: &PriceScale,
        reserved_chip_y: Option<f32>,
        pointer: Option<egui::Pos2>,
    ) {
        let ctx = PaintCtx {
            painter,
            chart_rect,
            tag_right,
            axis_x,
            scale,
            reserved_chip_y,
            pointer,
        };
        let mut controls = Vec::new();

        for order in self.sim.orders() {
            let Some(level) = order.price else { continue };
            let dragged = self.drag == PaperDrag::Order(order.id);
            let price = if dragged {
                self.drag_price
                    .unwrap_or_else(|| level.to_f64().unwrap_or_default())
            } else {
                level.to_f64().unwrap_or_default()
            };
            let y = ctx.scale.y(price);
            if !ctx.in_range(y) {
                continue;
            }
            let hovered = ctx.hovers_line(y);
            let shown = if dragged { self.snap(price) } else { level };
            ctx.level_line(y, theme::ACCENT, true, LINE_WIDTH_PX, hovered, dragged);
            ctx.gutter_chip(y, theme::ACCENT, &fmt_decimal(shown));
            let text = format!(
                "#{} {} {} {} @ {}",
                order.id.0,
                side_word_upper(order.side),
                kind_short(order.kind),
                fmt_decimal(order.quantity),
                fmt_decimal(shown),
            );
            if let Some(close) = ctx.chip_tag(y, theme::ACCENT, &text, hovered, !dragged) {
                controls.push((PaperControl::CancelOrder(order.id), close));
            }
        }

        if let Some(position) = self.sim.position().cloned() {
            let color = side_color(position.side);
            let entry_y = ctx.scale.y(position.avg_price.to_f64().unwrap_or_default());
            if ctx.in_range(entry_y) {
                ctx.level_line(entry_y, color, false, POSITION_LINE_WIDTH_PX, false, false);
                ctx.gutter_chip(entry_y, color, &fmt_decimal(position.avg_price));
                let side_text = format!(
                    "SIM {} {}",
                    position_word(position.side),
                    fmt_decimal(position.quantity),
                );
                let points = self
                    .sim
                    .mark_price()
                    .map(|mark| position.open_points(mark))
                    .map(|open| {
                        (
                            format!("{} pts", fmt_signed_points(open)),
                            points_color(open),
                        )
                    });
                let line_hovered = ctx.hovers_line(entry_y);
                let (close, tag_rect) =
                    ctx.position_tag(entry_y, color, &side_text, points, line_hovered);
                if let Some(close) = close {
                    controls.push((PaperControl::ClosePosition, close));
                }

                // Labelled handles for the missing bracket legs, revealed
                // while the pointer is on the entry line or its tag: the
                // affordance behind "drag from the position line to create".
                let over_tag = ctx
                    .pointer
                    .is_some_and(|pointer| tag_rect.expand(TAG_HOVER_SLACK_PX).contains(pointer));
                if self.drag == PaperDrag::None && (line_hovered || over_tag) {
                    let anchor = tag_rect.left() - HANDLE_TAG_GAP_PX;
                    let legs = [
                        (
                            position.stop_loss.is_none(),
                            PaperControl::HandleStopLoss,
                            "SL",
                            position.side == Side::Buy,
                            theme::SELL,
                        ),
                        (
                            position.take_profit.is_none(),
                            PaperControl::HandleTakeProfit,
                            "TP",
                            position.side == Side::Sell,
                            theme::BUY,
                        ),
                    ];
                    for (absent, control, label, below, leg_color) in legs {
                        if !absent {
                            continue;
                        }
                        let rect = handle_rect(anchor, entry_y, below);
                        ctx.bracket_handle(rect, label, leg_color);
                        controls.push((control, rect));
                    }
                }
            }

            self.draw_bracket_leg(
                &ctx,
                &LegPaint {
                    position: &position,
                    level: position.stop_loss,
                    other_level: position.take_profit,
                    word: "SL",
                    color: theme::SELL,
                    amend: PaperDrag::StopLoss,
                    create: PaperDrag::CreateStopLoss,
                    clear: PaperControl::ClearStopLoss,
                },
                &mut controls,
            );
            self.draw_bracket_leg(
                &ctx,
                &LegPaint {
                    position: &position,
                    level: position.take_profit,
                    other_level: position.stop_loss,
                    word: "TP",
                    color: theme::BUY,
                    amend: PaperDrag::TakeProfit,
                    create: PaperDrag::CreateTakeProfit,
                    clear: PaperControl::ClearTakeProfit,
                },
                &mut controls,
            );
        }

        if let Some(armed) = self.armed {
            let hint = format!(
                "click a price to place your {} {} - Esc cancels",
                side_word(armed.side),
                kind_word(armed.kind),
            );
            // Bottom-left: the top-left corner belongs to the position HUD,
            // and arming an entry with a position open is a normal flow.
            painter.text(
                chart_rect.left_bottom() + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                hint,
                egui::FontId::proportional(12.0),
                theme::ACCENT,
            );
        }
        self.controls = controls;
    }

    /// One protective leg: its resting line and tag, the drag that reprices
    /// it, or — while a create-drag runs and the leg does not exist yet —
    /// the dashed preview of where release would put it. The tag gains the
    /// live R:R read once both legs are known, which is what turns the drag
    /// into a decision.
    fn draw_bracket_leg(
        &self,
        ctx: &PaintCtx<'_>,
        leg: &LegPaint<'_>,
        controls: &mut Vec<(PaperControl, egui::Rect)>,
    ) {
        let amending = self.drag == leg.amend;
        let creating = self.drag == leg.create && leg.level.is_none();
        let resting = leg.level.map(|level| level.to_f64().unwrap_or_default());
        let price = if amending || creating {
            self.drag_price.or(resting)
        } else {
            resting
        };
        let Some(price) = price else { return };
        let y = ctx.scale.y(price);
        if !ctx.in_range(y) {
            return;
        }
        let dragging = amending || creating;
        let hovered = ctx.hovers_line(y);
        let shown = if dragging {
            self.snap(price)
        } else {
            leg.level.unwrap_or_else(|| self.snap(price))
        };
        // A leg being created previews dashed — it is not an order yet.
        ctx.level_line(y, leg.color, creating, LINE_WIDTH_PX, hovered, dragging);
        ctx.gutter_chip(y, leg.color, &fmt_decimal(shown));
        let mut text = format!(
            "{} {} {} pts",
            leg.word,
            fmt_decimal(shown),
            fmt_signed_points(leg.position.open_points(shown)),
        );
        if dragging && let Some(ratio) = rr_ratio(leg, shown) {
            text.push_str(&format!(" · R:R {ratio}"));
        }
        if let Some(close) = ctx.chip_tag(y, leg.color, &text, hovered, !dragging) {
            controls.push((leg.clear, close));
        }
    }

    /// Route pointer input to the simulated lines. Returns true when paper
    /// trading owns the gesture this frame — the chart must not pan and the
    /// drawings must not select under it.
    /// Cancel the transient chart interaction — an armed placement or a
    /// grabbed line (dropped without submitting). Called from the app's
    /// escape stack; returns true when there was something to cancel, so
    /// the stack spends exactly one layer on it.
    pub fn cancel_interaction(&mut self) -> bool {
        if self.armed.take().is_some() {
            return true;
        }
        if self.drag != PaperDrag::None {
            self.drag = PaperDrag::None;
            self.drag_price = None;
            return true;
        }
        false
    }

    pub fn handle_chart_input(&mut self, input: &ChartInput<'_>) -> bool {
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

        // A painted overlay control (a tag's ✕, a bracket handle) takes the
        // press before any line under it — it was drawn on top.
        if input.primary_pressed
            && self.drag == PaperDrag::None
            && let Some(pointer) = input.pointer
            && let Some(scale) = input.scale
            && let Some(control) = self.control_at(pointer)
        {
            match control {
                PaperControl::ClosePosition => self.close_position(),
                PaperControl::ClearStopLoss => self.clear_bracket_leg(true),
                PaperControl::ClearTakeProfit => self.clear_bracket_leg(false),
                PaperControl::CancelOrder(id) => {
                    let events = self.sim.apply(Command::CancelOrder { id });
                    self.handle_events(events);
                }
                PaperControl::HandleStopLoss => {
                    self.drag = PaperDrag::CreateStopLoss;
                    self.drag_price = Some(scale.price_at(pointer.y));
                }
                PaperControl::HandleTakeProfit => {
                    self.drag = PaperDrag::CreateTakeProfit;
                    self.drag_price = Some(scale.price_at(pointer.y));
                }
            }
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

        // Follow the pointer while dragging. A press that started on the
        // entry line commits to one bracket leg on its first real pull:
        // towards the profit side it creates the take profit, towards the
        // losing side the stop — and a side whose leg already exists stays
        // blocked (that leg's own line is its handle).
        if input.primary_down && self.drag != PaperDrag::None {
            if let (Some(pointer), Some(scale)) = (input.pointer, input.scale) {
                let y = pointer.y.clamp(input.chart.top(), input.chart.bottom());
                self.drag_price = Some(scale.price_at(y));
                if self.drag == PaperDrag::CreatePending {
                    self.decide_pending_leg(y, scale);
                }
            }
            return true;
        }

        // Drop: submit the new price; the simulator answers (a rejection
        // snaps the line back and the toast explains why). Creating a leg
        // and repricing it are the same command — the bracket is replaced
        // wholesale either way.
        if input.primary_released && self.drag != PaperDrag::None {
            let drag = std::mem::take(&mut self.drag);
            if let Some(price) = self.drag_price.take() {
                let price = self.snap(price);
                let command = match drag {
                    PaperDrag::StopLoss | PaperDrag::CreateStopLoss => {
                        self.sim.position().map(|position| Command::SetBracket {
                            stop_loss: Some(price),
                            take_profit: position.take_profit,
                        })
                    }
                    PaperDrag::TakeProfit | PaperDrag::CreateTakeProfit => {
                        self.sim.position().map(|position| Command::SetBracket {
                            stop_loss: position.stop_loss,
                            take_profit: Some(price),
                        })
                    }
                    PaperDrag::Order(id) => Some(Command::ModifyOrder { id, price }),
                    PaperDrag::None | PaperDrag::Blocked | PaperDrag::CreatePending => None,
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

    /// Remove one protective leg, keeping the other — the tag ✕'s command.
    fn clear_bracket_leg(&mut self, stop: bool) {
        let Some(position) = self.sim.position() else {
            return;
        };
        let command = if stop {
            Command::SetBracket {
                stop_loss: None,
                take_profit: position.take_profit,
            }
        } else {
            Command::SetBracket {
                stop_loss: position.stop_loss,
                take_profit: None,
            }
        };
        let events = self.sim.apply(command);
        self.handle_events(events);
    }

    /// The overlay control under the pointer, from the last paint pass.
    fn control_at(&self, pointer: egui::Pos2) -> Option<PaperControl> {
        self.controls
            .iter()
            .find(|(_, rect)| rect.contains(pointer))
            .map(|(control, _)| *control)
    }

    /// Turn a pending entry-line press into the leg the pull chose, once it
    /// travelled far enough to mean it.
    fn decide_pending_leg(&mut self, pointer_y: f32, scale: &PriceScale) {
        let Some(position) = self.sim.position() else {
            self.drag = PaperDrag::Blocked;
            return;
        };
        let entry_y = scale.y(position.avg_price.to_f64().unwrap_or_default());
        let delta = pointer_y - entry_y;
        if delta.abs() < CREATE_DECIDE_THRESHOLD_PX {
            return;
        }
        // On screen, up is negative y; up is the profit side for a long.
        let profit_side = match position.side {
            Side::Buy => delta < 0.0,
            Side::Sell => delta > 0.0,
        };
        self.drag = if profit_side {
            if position.take_profit.is_none() {
                PaperDrag::CreateTakeProfit
            } else {
                PaperDrag::Blocked
            }
        } else if position.stop_loss.is_none() {
            PaperDrag::CreateStopLoss
        } else {
            PaperDrag::Blocked
        };
    }

    /// The cursor that announces what is under the pointer (audit M3/M4):
    /// a pointing hand over a painted control, vertical-resize over a
    /// draggable order/stop/target line and over an entry line with a
    /// missing bracket leg (dragging away from it creates that leg),
    /// not-allowed over a fully bracketed entry — the average entry itself
    /// is history and never moves. `None` away from everything, so the
    /// caller falls through to the drawings' own cursors.
    #[must_use]
    pub fn hover_cursor(
        &self,
        pointer: egui::Pos2,
        scale: &PriceScale,
    ) -> Option<egui::CursorIcon> {
        if self.control_at(pointer).is_some() {
            return Some(egui::CursorIcon::PointingHand);
        }
        match self.line_at(pointer, scale)? {
            PaperDrag::Blocked => Some(egui::CursorIcon::NotAllowed),
            PaperDrag::StopLoss
            | PaperDrag::TakeProfit
            | PaperDrag::Order(_)
            | PaperDrag::CreatePending => Some(egui::CursorIcon::ResizeVertical),
            PaperDrag::None | PaperDrag::CreateStopLoss | PaperDrag::CreateTakeProfit => None,
        }
    }

    /// Which line sits under the pointer, in draw-stack priority: pending
    /// orders first (they draw on top), then take profit, stop loss, and
    /// the entry line — which starts a bracket-creating drag while a leg is
    /// missing, and blocks the gesture once both exist.
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
            let creatable = position.stop_loss.is_none() || position.take_profit.is_none();
            return Some(if creatable {
                PaperDrag::CreatePending
            } else {
                PaperDrag::Blocked
            });
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
                self.close_position();
            }
            if ui
                .button("Flatten")
                .on_hover_text("close the position and cancel every pending order")
                .clicked()
            {
                self.flatten();
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
                SimEvent::BracketDropped { reason } => {
                    self.show_toast(format!("SIM: dropped at the fill - {reason}"));
                }
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
            .color(theme::WARN)
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

/// The frame geometry every paper paint helper reads: one struct so a tag,
/// a chip and a line can never disagree about where the plot ends.
struct PaintCtx<'a> {
    painter: &'a egui::Painter,
    chart_rect: egui::Rect,
    /// Right edge of the interactive plot (the lane divider when a live
    /// lane is up, the chart's edge otherwise) — tags anchor inside it.
    tag_right: f32,
    /// Left edge of the price axis — lines run to it, gutter chips sit past
    /// it.
    axis_x: f32,
    scale: &'a PriceScale,
    /// The last-price chip's row, which every gutter chip dodges.
    reserved_chip_y: Option<f32>,
    /// The pointer, `Some` only on the pane that owns paper input.
    pointer: Option<egui::Pos2>,
}

/// One protective leg's paint inputs (see `draw_bracket_leg`).
struct LegPaint<'a> {
    position: &'a Position,
    level: Option<Decimal>,
    /// The other leg's level, for the R:R read while dragging.
    other_level: Option<Decimal>,
    word: &'static str,
    color: egui::Color32,
    amend: PaperDrag,
    create: PaperDrag,
    clear: PaperControl,
}

impl PaintCtx<'_> {
    fn in_range(&self, y: f32) -> bool {
        y >= self.chart_rect.top() && y <= self.chart_rect.bottom()
    }

    /// Whether the pointer is within grab range of the line at `y`.
    fn hovers_line(&self, y: f32) -> bool {
        self.pointer.is_some_and(|pointer| {
            self.chart_rect.contains(pointer) && (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        })
    }

    /// The line itself: hover thickens it, a drag thickens it further and
    /// paints the drawings' halo treatment beneath, so a grabbed stop feels
    /// identical to a grabbed drawing.
    fn level_line(
        &self,
        y: f32,
        color: egui::Color32,
        dashed: bool,
        base_width: f32,
        hovered: bool,
        dragged: bool,
    ) {
        let width = if dragged {
            LINE_DRAG_WIDTH_PX
        } else if hovered {
            LINE_HOVER_WIDTH_PX.max(base_width)
        } else {
            base_width
        };
        let points = [
            egui::pos2(self.chart_rect.left(), y),
            egui::pos2(self.axis_x, y),
        ];
        if dragged {
            self.painter.line_segment(
                points,
                egui::Stroke::new(width + DRAG_HALO_EXTRA_WIDTH_PX, DRAG_HALO_COLOR),
            );
        }
        let stroke = egui::Stroke::new(width, color);
        if dashed {
            self.painter.extend(egui::Shape::dashed_line(
                &points,
                stroke,
                ORDER_DASH_PX,
                ORDER_GAP_PX,
            ));
        } else {
            self.painter.line_segment(points, stroke);
        }
    }

    /// The gutter chip: the price and nothing else, on the last-price
    /// chip's geometry. The line stays at its true price; only the chip
    /// dodges the reserved last-price row.
    fn gutter_chip(&self, y: f32, color: egui::Color32, text: &str) {
        let chip_y = dodged_chip_y(
            y,
            self.reserved_chip_y,
            self.chart_rect.top(),
            self.chart_rect.bottom(),
        );
        let galley =
            self.painter
                .layout_no_wrap(text.to_owned(), egui::FontId::monospace(11.0), CHIP_TEXT);
        let text_pos = egui::pos2(self.axis_x + 6.0, chip_y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        self.painter
            .rect_filled(bg, egui::Rounding::same(2.0), color);
        self.painter.galley(text_pos, galley, CHIP_TEXT);
    }

    /// A solid tag on a line, right-anchored inside the plot. While the
    /// pointer is on the line or the tag (and `with_close` allows it), the
    /// tag grows leftward to reveal a ✕ zone; the returned rect is that
    /// button. Overlay ✕s carry no tooltip of their own — each has a
    /// full-size, fully labelled twin in the chrome.
    fn chip_tag(
        &self,
        y: f32,
        fill: egui::Color32,
        text: &str,
        line_hovered: bool,
        with_close: bool,
    ) -> Option<egui::Rect> {
        let galley =
            self.painter
                .layout_no_wrap(text.to_owned(), egui::FontId::monospace(11.0), CHIP_TEXT);
        let half = TAG_HEIGHT_PX / 2.0;
        let center_y = y.clamp(
            self.chart_rect.top() + half,
            self.chart_rect.bottom() - half,
        );
        let content_w = galley.size().x + 2.0 * TAG_PAD_X;
        let resting = egui::Rect::from_min_max(
            egui::pos2(self.tag_right - TAG_GAP_PX - content_w, center_y - half),
            egui::pos2(self.tag_right - TAG_GAP_PX, center_y + half),
        );
        let hovered = with_close
            && (line_hovered
                || self
                    .pointer
                    .is_some_and(|pointer| resting.expand(TAG_HOVER_SLACK_PX).contains(pointer)));
        let full = if hovered {
            egui::Rect::from_min_max(resting.min - egui::vec2(TAG_BUTTON_PX, 0.0), resting.max)
        } else {
            resting
        };
        self.painter
            .rect_filled(full, egui::Rounding::same(3.0), fill);
        self.painter.galley(
            egui::pos2(resting.left() + TAG_PAD_X, center_y - galley.size().y / 2.0),
            galley,
            CHIP_TEXT,
        );
        if !hovered {
            return None;
        }
        let button = egui::Rect::from_min_size(full.min, egui::vec2(TAG_BUTTON_PX, TAG_HEIGHT_PX));
        self.painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::monospace(11.0),
            CHIP_TEXT,
        );
        Some(button)
    }

    /// The position's tag wears the card grammar, not a chip: a position is
    /// a fact about the account, not an order that will fire. Returns the
    /// hover-revealed ✕ rect and the resting rect (the bracket handles
    /// anchor left of it).
    fn position_tag(
        &self,
        y: f32,
        side_color: egui::Color32,
        side_text: &str,
        points: Option<(String, egui::Color32)>,
        line_hovered: bool,
    ) -> (Option<egui::Rect>, egui::Rect) {
        let font = egui::FontId::monospace(11.0);
        let side_galley =
            self.painter
                .layout_no_wrap(side_text.to_owned(), font.clone(), side_color);
        let points_galley = points.map(|(text, color)| {
            (
                self.painter.layout_no_wrap(text, font.clone(), color),
                color,
            )
        });
        let rail = 3.0;
        let mut content_w = rail + TAG_PAD_X + side_galley.size().x + TAG_PAD_X;
        if let Some((galley, _)) = &points_galley {
            content_w += galley.size().x + TAG_PAD_X;
        }
        let half = TAG_HEIGHT_PX / 2.0;
        let center_y = y.clamp(
            self.chart_rect.top() + half,
            self.chart_rect.bottom() - half,
        );
        let resting = egui::Rect::from_min_max(
            egui::pos2(self.tag_right - TAG_GAP_PX - content_w, center_y - half),
            egui::pos2(self.tag_right - TAG_GAP_PX, center_y + half),
        );
        let hovered = line_hovered
            || self
                .pointer
                .is_some_and(|pointer| resting.expand(TAG_HOVER_SLACK_PX).contains(pointer));
        let full = if hovered {
            egui::Rect::from_min_max(resting.min - egui::vec2(TAG_BUTTON_PX, 0.0), resting.max)
        } else {
            resting
        };
        self.painter
            .rect_filled(full, egui::Rounding::same(3.0), theme::INSET);
        self.painter.rect_stroke(
            full,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
        // The side rail rides the card's left edge, ✕ zone and all.
        self.painter.rect_filled(
            egui::Rect::from_min_max(
                full.min + egui::vec2(1.0, 1.0),
                egui::pos2(full.min.x + 1.0 + rail, full.max.y - 1.0),
            ),
            egui::Rounding {
                nw: 2.0,
                sw: 2.0,
                ne: 0.0,
                se: 0.0,
            },
            side_color,
        );
        let mut x = resting.left() + rail + TAG_PAD_X;
        let side_size = side_galley.size();
        self.painter.galley(
            egui::pos2(x, center_y - side_size.y / 2.0),
            side_galley,
            side_color,
        );
        x += side_size.x + TAG_PAD_X;
        if let Some((galley, color)) = points_galley {
            let size = galley.size();
            self.painter
                .galley(egui::pos2(x, center_y - size.y / 2.0), galley, color);
        }
        if !hovered {
            return (None, resting);
        }
        let button = egui::Rect::from_min_size(
            full.min + egui::vec2(1.0 + rail, 0.0),
            egui::vec2(TAG_BUTTON_PX - rail, TAG_HEIGHT_PX),
        );
        let over_button = self.pointer.is_some_and(|pointer| button.contains(pointer));
        self.painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::monospace(11.0),
            if over_button {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
        // A hairline between the ✕ zone and the words.
        self.painter.line_segment(
            [
                egui::pos2(button.right(), full.top() + 3.0),
                egui::pos2(button.right(), full.bottom() - 3.0),
            ],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
        (Some(button), resting)
    }

    /// A labelled `SL`/`TP` handle on the entry line: quiet control fill,
    /// the leg's own colour on hover.
    fn bracket_handle(&self, rect: egui::Rect, label: &str, leg_color: egui::Color32) {
        let hovered = self.pointer.is_some_and(|pointer| rect.contains(pointer));
        self.painter
            .rect_filled(rect, egui::Rounding::same(3.0), theme::CONTROL);
        self.painter.rect_stroke(
            rect,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, if hovered { leg_color } else { theme::BORDER }),
        );
        self.painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(9.0),
            if hovered {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
    }
}

/// Where a bracket handle sits: right-anchored at `anchor_right`, one gap
/// off the entry line, on the side its leg protects.
fn handle_rect(anchor_right: f32, line_y: f32, below: bool) -> egui::Rect {
    let x = anchor_right - HANDLE_SIZE.x;
    let y = if below {
        line_y + HANDLE_GAP_PX
    } else {
        line_y - HANDLE_GAP_PX - HANDLE_SIZE.y
    };
    egui::Rect::from_min_size(egui::pos2(x, y), HANDLE_SIZE)
}

/// Reward over risk at the dragged level, against the other leg — the read
/// that turns a drag into a decision. `None` until both legs are known or
/// while the risk is zero.
fn rr_ratio(leg: &LegPaint<'_>, dragged: Decimal) -> Option<String> {
    let other = leg.other_level?;
    let entry = leg.position.avg_price;
    let (stop, target) = if leg.word == "SL" {
        (dragged, other)
    } else {
        (other, dragged)
    };
    let risk = entry.saturating_sub(stop).abs();
    let reward = target.saturating_sub(entry).abs();
    (risk > Decimal::ZERO).then(|| fmt_points(reward / risk))
}

/// The side's chrome colour — one mapping for every paper surface.
fn side_color(side: Side) -> egui::Color32 {
    match side {
        Side::Buy => theme::BUY,
        Side::Sell => theme::SELL,
    }
}

/// Three-letter order kind for the compact chart tags (`LMT`, `STP`, `MKT`).
fn kind_short(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "MKT",
        EntryKind::Limit => "LMT",
        EntryKind::Stop => "STP",
    }
}

/// Keep a gutter chip legible when it would land on the last-price chip:
/// push it just clear of the reserved row, towards its own side of the
/// price, clamped into the pane. At the exact fill price (no distance at
/// all) the chip steps down, below the last-price chip. When the reserved
/// row itself hugs a pane edge the clamp can land the chip back inside the
/// band — accepted: a chip pinned at the edge beats one pushed out of the
/// pane, and the next print separates them.
fn dodged_chip_y(y: f32, reserved: Option<f32>, top: f32, bottom: f32) -> f32 {
    let Some(reserved) = reserved else {
        return y;
    };
    let delta = y - reserved;
    if delta.abs() >= CHIP_CLEAR_PX {
        return y;
    }
    let dodged = if delta >= 0.0 {
        reserved + CHIP_CLEAR_PX
    } else {
        reserved - CHIP_CLEAR_PX
    };
    dodged.clamp(top, bottom)
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

/// `LONG`/`SHORT` — shared with the HUD so every surface uses one register.
pub(crate) fn position_word(side: Side) -> &'static str {
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

/// Green gains, red losses, muted zero — shared with the HUD.
pub(crate) fn points_color(points: Decimal) -> egui::Color32 {
    match points.cmp(&Decimal::ZERO) {
        std::cmp::Ordering::Greater => theme::BUY,
        std::cmp::Ordering::Less => theme::SELL,
        std::cmp::Ordering::Equal => theme::TEXT_MUTED,
    }
}

/// Exact value, trailing zeros stripped — prices and quantities.
pub(crate) fn fmt_decimal(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Points rounded to two places for display (the stored value stays exact).
fn fmt_points(value: Decimal) -> String {
    value.round_dp(2).normalize().to_string()
}

/// Signed points: an explicit `+` on gains so a green `12` can never be
/// misread as a count.
pub(crate) fn fmt_signed_points(value: Decimal) -> String {
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
        let dir = std::env::temp_dir().join(format!(
            "quantick-paper-journal-test-{}",
            std::process::id()
        ));
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
        let dir =
            std::env::temp_dir().join(format!("quantick-paper-reset-test-{}", std::process::id()));
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
        }
    }

    /// Escape (routed through the app's escape stack) cancels exactly one
    /// paper interaction per press, and a cancelled drag submits nothing.
    #[test]
    fn escape_cancels_the_armed_placement_then_the_grabbed_line() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.armed = Some(ArmedPlacement {
            side: Side::Buy,
            kind: EntryKind::Limit,
        });
        assert!(paper.cancel_interaction(), "the armed placement dies first");
        assert!(paper.armed.is_none());
        assert!(!paper.cancel_interaction(), "nothing left to cancel");

        paper.stop_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // Grab the stop at 90 (y = 300), cancel, then let go: no submit.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false)));
        assert!(paper.cancel_interaction(), "the grabbed line is released");
        assert!(!paper.handle_chart_input(&frame(chart, &scale, 250.0, false, false, true)));
        assert_eq!(
            paper.sim.position().expect("still long").stop_loss,
            Some(Decimal::from(90)),
            "a cancelled drag never moves the stop"
        );
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

    /// The lines say what a press would do before it happens (audit M3/M4):
    /// draggable levels wear the resize cursor, an entry line with a
    /// missing leg offers the create-drag, and empty tape asks nothing.
    #[test]
    fn hover_cursors_announce_draggable_and_creatable_lines() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.stop_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (_chart, scale) = chart_and_scale(80.0, 120.0);
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 300.0), &scale),
            Some(egui::CursorIcon::ResizeVertical),
            "the stop at 90 sits at y 300 and drags"
        );
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 200.0), &scale),
            Some(egui::CursorIcon::ResizeVertical),
            "the entry at 100 offers the missing take profit by drag"
        );
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 40.0), &scale),
            None,
            "empty tape belongs to the chart"
        );
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

    /// The TradingView gesture: pull away from the entry line and the
    /// missing bracket leg is born on release — the profit side makes a
    /// take profit, the losing side a stop.
    #[test]
    fn dragging_from_the_entry_line_creates_the_missing_leg() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // Grab the entry at 100 (y = 200), pull up to 105 (y = 150), drop.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, false, true)));
        let position = paper.sim.position().expect("still long");
        assert_eq!(
            position.take_profit,
            Some(Decimal::from(105)),
            "above a long is the profit side"
        );
        assert_eq!(
            position.avg_price,
            Decimal::from(100),
            "the entry itself never moves"
        );
        // The same pull downward births the stop.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true)));
        let position = paper.sim.position().expect("still long");
        assert_eq!(position.stop_loss, Some(Decimal::from(90)));
        assert_eq!(position.take_profit, Some(Decimal::from(105)), "untouched");
    }

    #[test]
    fn a_fully_bracketed_entry_line_blocks_the_gesture_but_never_moves() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        paper.stop_offset_text = "10".to_owned();
        paper.profit_offset_text = "10".to_owned();
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        assert_eq!(
            paper.hover_cursor(egui::pos2(400.0, 200.0), &scale),
            Some(egui::CursorIcon::NotAllowed),
            "both legs exist, so their own lines are the handles"
        );
        // Grabbing the entry still consumes the gesture (the chart must not
        // pan under it) but repositions nothing.
        assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, false, true)));
        let position = paper.sim.position().expect("long");
        assert_eq!(position.avg_price, Decimal::from(100));
        assert_eq!(position.stop_loss, Some(Decimal::from(90)), "untouched");
        assert_eq!(position.take_profit, Some(Decimal::from(110)), "untouched");
        // Empty space is not ours: the press falls through to the chart.
        assert!(
            !paper.handle_chart_input(&frame(chart, &scale, 40.0, true, true, false)),
            "a press far from every line belongs to the pan"
        );
    }

    /// Painted overlay controls (cached from the last paint pass) take the
    /// press before the lines under them.
    #[test]
    fn painted_controls_take_the_press_before_the_lines() {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let events = paper.sim.apply(Command::PlaceLimit {
            side: Side::Buy,
            quantity: Decimal::ONE,
            price: Decimal::from(95),
            bracket: Bracket::none(),
        });
        paper.handle_events(events);
        let id = paper.sim.orders()[0].id;
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        // A ✕ painted last frame at (700, 120) — far from any line.
        let button = egui::Rect::from_center_size(egui::pos2(700.0, 120.0), egui::vec2(20.0, 20.0));
        paper.controls = vec![(PaperControl::CancelOrder(id), button)];
        let input = ChartInput {
            chart,
            scale: Some(&scale),
            pointer: Some(egui::pos2(700.0, 120.0)),
            primary_pressed: true,
            primary_down: true,
            primary_released: false,
        };
        assert!(paper.handle_chart_input(&input), "the ✕ owns the press");
        assert!(paper.sim.orders().is_empty(), "and it cancelled the order");

        // A bracket handle press starts the create-drag for its leg.
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let handle = egui::Rect::from_center_size(egui::pos2(700.0, 210.0), HANDLE_SIZE);
        paper.controls = vec![(PaperControl::HandleStopLoss, handle)];
        let press = ChartInput {
            chart,
            scale: Some(&scale),
            pointer: Some(egui::pos2(700.0, 210.0)),
            primary_pressed: true,
            primary_down: true,
            primary_released: false,
        };
        assert!(paper.handle_chart_input(&press));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false)));
        assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true)));
        assert_eq!(
            paper.sim.position().expect("long").stop_loss,
            Some(Decimal::from(90)),
            "the handle drag placed the stop"
        );
    }

    /// One long, then every label the entry buttons can wear: the button
    /// must disclose close-or-reverse, because the quantity deciding it
    /// lives in a tab the toolbar never shows.
    #[test]
    fn entry_labels_disclose_what_the_press_would_do() {
        let mut paper = PaperTrading::new();
        assert_eq!(
            paper.entry_label(Side::Buy),
            "BUY 1",
            "flat is a plain entry"
        );
        paper.qty_text = "x".to_owned();
        assert_eq!(
            paper.entry_label(Side::Sell),
            "SELL",
            "an unparseable quantity promises nothing"
        );
        paper.qty_text = "2".to_owned();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));

        paper.qty_text = "1".to_owned();
        assert_eq!(paper.entry_label(Side::Buy), "BUY 1 (adds to 3)");
        assert_eq!(paper.entry_label(Side::Sell), "SELL 1 (closes 1 of 2)");
        paper.qty_text = "2".to_owned();
        assert_eq!(paper.entry_label(Side::Sell), "SELL 2 (closes)");
        paper.qty_text = "5".to_owned();
        assert_eq!(
            paper.entry_label(Side::Sell),
            "SELL 5 (reverses to short 3)"
        );
    }

    /// The status cell answers the reported question — "am I in a trade?" —
    /// not just "how many points".
    #[test]
    fn the_status_cell_distinguishes_open_from_flat() {
        let mut paper = PaperTrading::new();
        assert!(paper.status_cell().is_none(), "untouched owes no line");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let (text, _) = paper.status_cell().expect("a position is state");
        assert_eq!(text, "SIM LONG 1 · 0 pts");
        paper.on_trade(&print(2, 105));
        let (text, sign) = paper.status_cell().expect("still open");
        assert_eq!(text, "SIM LONG 1 · +5 pts");
        assert_eq!(sign, std::cmp::Ordering::Greater);

        paper.close_position();
        paper.on_trade(&print(3, 107));
        let (text, sign) = paper.status_cell().expect("history keeps the cell");
        assert_eq!(text, "SIM +7 pts · flat");
        assert_eq!(sign, std::cmp::Ordering::Greater);
        assert!(paper.close_button_label().is_none(), "flat has no close");
    }

    #[test]
    fn the_close_button_names_the_position_it_exits() {
        let mut paper = PaperTrading::new();
        paper.qty_text = "3".to_owned();
        paper.seed(&print(0, 100));
        paper.market(Side::Sell);
        paper.on_trade(&print(1, 100));
        assert_eq!(paper.close_button_label().as_deref(), Some("Close 3 SHORT"));
        let summary = paper.position_summary().expect("open");
        assert_eq!(summary.side, Side::Sell);
        assert_eq!(summary.quantity, Decimal::from(3));
        assert_eq!(summary.avg_price, Decimal::from(100));
    }

    /// Reverse flips side and size in one market order, and the form's
    /// protective offsets ride along to the new entry.
    #[test]
    fn reverse_flips_the_position_with_the_forms_bracket() {
        let mut paper = PaperTrading::new();
        paper.qty_text = "2".to_owned();
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        paper.stop_offset_text = "5".to_owned();
        paper.reverse_position();
        paper.on_trade(&print(2, 100));
        let position = paper.sim.position().expect("reversed, not flat");
        assert_eq!(position.side, Side::Sell);
        assert_eq!(position.quantity, Decimal::from(2));
        assert_eq!(
            position.stop_loss,
            Some(Decimal::from(105)),
            "the new short is protected by the form's offset"
        );
    }

    /// The chip dodge: lines keep their price, chips clear the last-price
    /// row by the minimum, and the fill-moment tie steps down.
    #[test]
    fn paper_chips_dodge_the_last_price_chip_never_the_line() {
        // No reservation, or far enough away: the chip stays at its line.
        assert_eq!(dodged_chip_y(100.0, None, 0.0, 400.0), 100.0);
        assert_eq!(dodged_chip_y(100.0, Some(200.0), 0.0, 400.0), 100.0);
        // Inside the band: pushed just clear, towards its own side.
        assert_eq!(
            dodged_chip_y(210.0, Some(200.0), 0.0, 400.0),
            200.0 + CHIP_CLEAR_PX
        );
        assert_eq!(
            dodged_chip_y(190.0, Some(200.0), 0.0, 400.0),
            200.0 - CHIP_CLEAR_PX
        );
        // The fill moment: entry == last price, and the chip steps down.
        assert_eq!(
            dodged_chip_y(200.0, Some(200.0), 0.0, 400.0),
            200.0 + CHIP_CLEAR_PX
        );
        // Never dodged out of the pane.
        assert_eq!(dodged_chip_y(398.0, Some(399.0), 0.0, 400.0), 383.0);
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
