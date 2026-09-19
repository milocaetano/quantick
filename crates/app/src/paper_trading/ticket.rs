//! The trading ticket: the panel beside the chart.
//!
//! Everything the trader reads and presses that is not on the chart itself -
//! the position card, the order form, the resting-order list, the session
//! summary and the two dock tabs. None of it decides anything: every button
//! resolves its text here and then calls [`super::PaperTrading`]'s account,
//! which is where the money path lives.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
use quantick_sim::{Command, EntryKind, Position};
use rust_decimal::Decimal;

use super::{
    ArmedPlacement, PaperTrading, TradingTabAction, kind_short, kind_word, reveal_folder,
    side_word, side_word_upper,
};
use crate::paper_chrome::{caption, fmt_decimal, fmt_signed_points, pill_toggle, points_color};
use crate::paper_report::LedgerAction;
use crate::theme;
use crate::timezone::TzOffset;

mod entry_pair;
mod position_card;

use entry_pair::{EntryPress, entry_pair};
use position_card::{CardCommand, PositionCard};

impl PaperTrading {
    /// The form quantity, parsed without side effects — the label builders
    /// peek at it every frame and must not toast.
    pub(super) fn quantity_preview(&self) -> Option<Decimal> {
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
        // The size the press would actually send. The risk-derived quantity
        // is written into the field by the ticket, so a toolbar reading the
        // field while the dock is closed promised a size the click did not
        // send - the button naming one number and the order carrying
        // another is the plainest kind of lie this surface can tell.
        let derived = self.risk_report().0.derived_quantity();
        let Some(qty) = derived.or_else(|| self.quantity_preview()) else {
            return word.to_owned();
        };
        let qty_text = fmt_decimal(qty);
        let Some(position) = self.account.venue().position() else {
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
        let hotkey = match side {
            Side::Buy => "Shift+B",
            Side::Sell => "Shift+S",
        };
        format!(
            "simulated market {} - fills at the next print; {quantity}, {bracket} \
             (Trading tab) · {hotkey}",
            side_word(side),
        )
    }

    // ------------------------------------------------------------------
    // Chart layer
    // ------------------------------------------------------------------

    /// The chart context menu's resting-order section, anchored at the
    /// clicked price. Invalid types stay visible but disabled, wearing the
    /// sim core's own rejection text — the same curriculum the toasts teach.
    /// Market entry remains in the trading ticket and its hotkeys.
    pub fn context_trade_actions(&mut self, ui: &mut egui::Ui, raw_price: f64) {
        ui.label(
            egui::RichText::new("trade")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        let Some(mark) = self.account.venue().mark_price() else {
            ui.label(
                egui::RichText::new("no print yet - there is no market to trade against")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            return;
        };
        let quantity = self
            .quantity_preview()
            .map_or_else(|| "?".to_owned(), fmt_decimal);
        let price = self.account.snap(raw_price);
        let entries = [
            (Side::Buy, EntryKind::Limit, price < mark),
            (Side::Buy, EntryKind::Stop, price > mark),
            (Side::Sell, EntryKind::Limit, price > mark),
            (Side::Sell, EntryKind::Stop, price < mark),
        ];
        for (side, kind, valid) in entries {
            let label = format!(
                "{} {quantity} {} @ {}",
                side_word_upper(side),
                kind_word(kind),
                fmt_decimal(price),
            );
            let reason = match kind {
                EntryKind::Limit => quantick_sim::RejectReason::LimitOnWrongSide(side),
                _ => quantick_sim::RejectReason::StopOnWrongSide(side),
            };
            let response = ui
                .add_enabled(valid, egui::Button::new(label))
                .on_disabled_hover_text(reason.to_string());
            if response.clicked() {
                self.place_resting(side, kind, raw_price);
                ui.close_menu();
            }
        }
    }

    // ------------------------------------------------------------------
    // Dock tab
    // ------------------------------------------------------------------

    /// The Trading dock tab: position, ticket, working orders, session
    /// strip. See `docs/ux/paper-trading.md` §3.
    pub fn draw_trading_tab(&mut self, ui: &mut egui::Ui) -> Option<TradingTabAction> {
        self.hovered_order = None;
        ui.label(
            egui::RichText::new(
                "Simulated fills from the tape - no broker. Results are in points; a currency here is the point value you declared.",
            )
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
        let changed = self.draw_order_entry(ui);
        ui.separator();
        self.draw_pending_orders(ui);
        ui.separator();
        let action = self.draw_session_summary(ui);
        if action.is_none() {
            // Same-frame collision with another action is a picker click;
            // the settings change persists on its next touch.
            if changed.strategies {
                return Some(TradingTabAction::OrderStrategiesChanged);
            }
            if changed.cmd_trading {
                return Some(TradingTabAction::CmdTradingChanged);
            }
            if changed.risk {
                return Some(TradingTabAction::RiskSettingsChanged);
            }
        }
        action
    }

    /// The position block, drawn by [`PositionCard`] from a snapshot; its
    /// press is applied here, before the order form reads the account.
    fn draw_position_card(&mut self, ui: &mut egui::Ui) {
        let venue = self.account.venue();
        let position = venue.position().cloned();
        let card = PositionCard {
            position: position.as_ref(),
            mark: venue.mark_price(),
            realized: venue.realized_points(),
            has_working_orders: !venue.working_orders().is_empty(),
            stop_offset: parse_offset(&self.stop_offset_text).ok().flatten(),
            profit_offset: parse_offset(&self.profit_offset_text).ok().flatten(),
        };
        match card.show(ui) {
            None => {}
            Some(CardCommand::CancelAllOrders) => self.account.cancel_all_orders(),
            Some(CardCommand::Venue(command)) => {
                let events = self.account.dispatch(*command);
                self.account.handle_events(events);
            }
            Some(CardCommand::Close) => self.account.close_position(),
            Some(CardCommand::Reverse) => self.reverse_position(),
            Some(CardCommand::Flatten) => self.account.flatten(),
        }
    }

    fn draw_order_entry(&mut self, ui: &mut egui::Ui) -> OrderEntryChanges {
        ui.label(caption("ORDER"));
        // What the risk per trade makes of the entry the ticket is holding.
        // Read once for the whole form: the quantity field, the support line
        // and the entry pair must all be talking about the same aim.
        //
        // Side::Buy stands for both. Every stop this reads is symmetric in
        // distance - the ruler walks both legs the same number of points,
        // and a typed offset is a distance rather than a price - so the risk
        // is the side-independent half of the answer.
        let risk_reference = self.account.venue().mark_price().unwrap_or_default();
        let risk_state = self.risk_state(Side::Buy, risk_reference);
        let derived_quantity = risk_state.derived_quantity();
        let risk_blocks = risk_state.blocks_entry(self.account.risk_settings().lock);
        if let Some(quantity) = derived_quantity {
            // The mode writes into the field the whole form already reads,
            // rather than adding a second number beside it: one quantity on
            // screen is the quantity that will be sent.
            let derived = fmt_decimal(quantity);
            if self.qty_text != derived {
                self.qty_text = derived;
            }
        }
        self.draw_quantity_row(ui, derived_quantity.is_none());
        // The discreet line: what the number means, or why there is none.
        // Small and quiet on purpose - it explains the size without taking
        // the screen away from the chart.
        let sentence = risk_state.sentence();
        if !sentence.is_empty() {
            let colour = if risk_blocks {
                theme::WARN
            } else {
                theme::TEXT_FAINT
            };
            ui.label(egui::RichText::new(sentence).color(colour).small());
        }
        self.draw_order_fields(ui);
        let strategies_changed = self.draw_strategy_row(ui);
        // The whole risk surface, in its own module: this file already
        // carries the order form, and a second feature inside it is how the
        // trunk grew the first time.
        let editor = self.account.risk_editor();
        let risk_changed = crate::risk_sizing::draw_risk_block(
            ui,
            crate::risk_sizing::RiskBlock {
                symbol: editor.symbol,
                settings: editor.settings,
                capital: editor.capital,
                book: editor.book,
                amount_text: &mut self.risk_amount_text,
                percent_text: &mut self.risk_percent_text,
                capital_text: &mut self.capital_text,
                point_value_text: &mut self.point_value_text,
                size_step_text: &mut self.size_step_text,
                currency_text: &mut self.currency_text,
            },
        );
        ui.add_space(4.0);

        // The lock, enforced on the surface as well as at the click: a
        // ceiling the trader can still press through is one they will press
        // through by accident on a fast tape.
        let ready = self.account.ready() && !risk_blocks;
        match entry_pair(ui, self, ready) {
            None => {}
            Some(EntryPress::Disarm) => self.account.armed = None,
            Some(EntryPress::Fire(side)) => self.market(side),
            Some(EntryPress::Arm(side)) => {
                self.account.armed = Some(ArmedPlacement {
                    side,
                    kind: self.order_type,
                });
            }
        }
        ui.label(
            egui::RichText::new(if self.account.armed.is_some() {
                "Click the chart at your price. Esc cancels."
            } else if self.order_type == EntryKind::Market {
                "Market orders fill at the next print."
            } else {
                "The button arms a click; the next chart click rests the order there."
            })
            .color(theme::TEXT_SUPPORT)
            .small(),
        );
        OrderEntryChanges {
            cmd_trading: self.draw_cmd_trading_settings(ui),
            strategies: strategies_changed,
            risk: risk_changed,
        }
    }

    /// Qty: free decimal text (empty must keep meaning "fix me"), with
    /// steppers beside it; Shift steps by ten. Derived and read-only while
    /// the risk per trade is deciding it (`typed` false).
    fn draw_quantity_row(&mut self, ui: &mut egui::Ui, typed: bool) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Qty").color(theme::TEXT_MUTED).small());
            let step = if ui.input(|input| input.modifiers.shift) {
                Decimal::TEN
            } else {
                Decimal::ONE
            };
            let hint = self.account.quantity_step_hint(step);
            ui.add_enabled_ui(typed, |ui| {
                if ui
                    .small_button("−")
                    .on_hover_text(format!("{hint} less (Shift: ten steps)"))
                    .clicked()
                {
                    self.step_quantity(-step);
                }
                ui.add(egui::TextEdit::singleline(&mut self.qty_text).desired_width(56.0));
                if ui
                    .small_button("+")
                    .on_hover_text(format!("{hint} more (Shift: ten steps)"))
                    .clicked()
                {
                    self.step_quantity(step);
                }
            })
            .response
            .on_disabled_hover_text(
                "the size is derived from your risk per trade - switch the mode off to type one",
            );
        });
    }

    /// The order's shape: the type pills and the protective offsets.
    fn draw_order_fields(&mut self, ui: &mut egui::Ui) {
        // Type: three pills. Picking Limit or Stop is a promise of an
        // accent line on the chart, so the selected pill wears the accent.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Type").color(theme::TEXT_MUTED).small());
            for kind in [EntryKind::Market, EntryKind::Limit, EntryKind::Stop] {
                let on = self.order_type == kind;
                if pill_toggle(ui, kind_word(kind), on, "how the entry meets the market").clicked()
                    && !on
                {
                    self.order_type = kind;
                    self.account.armed = None;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Stop").color(theme::TEXT_MUTED).small())
                .on_hover_text(
                    "optional protective stop, this many points on the losing side of the \
                     entry; empty places no stop",
                );
            ui.add(egui::TextEdit::singleline(&mut self.stop_offset_text).desired_width(52.0));
            ui.label(
                egui::RichText::new("Target")
                    .color(theme::TEXT_MUTED)
                    .small(),
            )
            .on_hover_text(
                "optional profit target, this many points on the winning side of the \
                 entry; empty places no target",
            );
            ui.add(egui::TextEdit::singleline(&mut self.profit_offset_text).desired_width(52.0));
            ui.label(egui::RichText::new("pts").color(theme::TEXT_FAINT).small());
        });
    }

    /// Walk the typed quantity by `notches` of the instrument's own size
    /// step.
    ///
    /// The step is the instrument's, not one. A hard-coded 1 is already
    /// wrong on any instrument whose lot is fractional — a press moved a
    /// crypto size by a hundred thousand steps — and the floor is the
    /// instrument's minimum rather than "anything above zero", so the
    /// steppers can only ever land on a size the venue would take.
    pub(super) fn step_quantity(&mut self, notches: Decimal) {
        let (unit, floor) = self
            .account
            .instrument_money()
            .get(self.account.symbol())
            .map_or((Decimal::ONE, Decimal::ONE), |money| {
                (money.size_step, money.min_size)
            });
        let current = self
            .qty_text
            .trim()
            .parse::<Decimal>()
            .ok()
            .filter(|quantity| *quantity > Decimal::ZERO)
            .unwrap_or(floor);
        let next = current.saturating_add(notches.saturating_mul(unit));
        if next >= floor {
            self.qty_text = fmt_decimal(next);
        }
    }

    fn draw_pending_orders(&mut self, ui: &mut egui::Ui) {
        let mut in_flight = Vec::new();
        self.account.venue().in_flight_entries(&mut in_flight);
        let queued_entries = in_flight.len();
        // Saturating, because the two reads cross a trait boundary and are
        // documented independently: a venue whose `in_flight_entries` says
        // more than its `in_flight` counts must not panic the render loop.
        let queued_closes = self
            .account
            .venue()
            .in_flight()
            .saturating_sub(queued_entries);
        let orders: Vec<_> = self.account.working_orders().to_vec();
        ui.label(caption(&format!("WORKING ORDERS · {}", orders.len())));
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
        if orders.is_empty() && queued_entries == 0 {
            ui.label(egui::RichText::new("No working orders.").color(theme::TEXT_MUTED));
            ui.label(
                egui::RichText::new("Pick Limit or Stop, then click a price on the chart.")
                    .color(theme::TEXT_SUPPORT)
                    .small(),
            );
            return;
        }
        for order in orders {
            let response = ui.horizontal(|ui| {
                // A short accent dash, echoing the dashed chart line.
                let (dash, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
                ui.painter().line_segment(
                    [
                        egui::pos2(dash.left(), dash.center().y),
                        egui::pos2(dash.right(), dash.center().y),
                    ],
                    egui::Stroke::new(2.0_f32, theme::ACCENT),
                );
                ui.label(
                    egui::RichText::new(format!("#{}", order.id.0))
                        .color(theme::TEXT_FAINT)
                        .small(),
                );
                ui.label(
                    egui::RichText::new(side_word_upper(order.side))
                        .monospace()
                        .color(theme::side_color(order.side)),
                );
                let mut line = format!(
                    "{} {} @ {}",
                    kind_short(order.kind),
                    fmt_decimal(order.quantity),
                    order.price.map_or_else(String::new, fmt_decimal),
                );
                // The self-cancel level rides the row — an order that can
                // vanish on its own never does so unannounced.
                if let Some(cancel) = order.cancel_at {
                    line.push_str(&format!(" · cancels @ {}", fmt_decimal(cancel)));
                }
                ui.label(
                    egui::RichText::new(line)
                        .monospace()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text("cancel this order")
                        .clicked()
                    {
                        let events = self.account.dispatch(Command::CancelOrder { id: order.id });
                        self.account.handle_events(events);
                    }
                });
            });
            // One hover, two surfaces: the row lifts its chart line.
            if response.response.hovered() {
                self.hovered_order = Some(order.id);
            }
        }
    }

    fn draw_session_summary(&mut self, ui: &mut egui::Ui) -> Option<TradingTabAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            let realized = self.account.venue().realized_points();
            ui.label(
                egui::RichText::new(format!("{} pts", fmt_signed_points(realized)))
                    .monospace()
                    .strong()
                    .color(points_color(realized)),
            );
            ui.label(
                egui::RichText::new(format!(
                    "realized · {} trades",
                    self.account.venue().closed_trades().len()
                ))
                .color(theme::TEXT_MUTED)
                .small(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Report…")
                    .on_hover_text("performance metrics computed from the saved history")
                    .clicked()
                {
                    let (report, env) = self.account.report_parts();
                    report.open(&env);
                }
            });
        });
        ui.horizontal(|ui| {
            if ui
                .small_button(icons::FOLDER_OPEN)
                .on_hover_text(
                    "choose where trades are saved — applies to every tab and is \
                     remembered across restarts",
                )
                .clicked()
            {
                action = Some(TradingTabAction::PickTradesDir);
            }
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!(
                            "trades saved to: {}",
                            self.account.trades_dir().display()
                        ))
                        .color(theme::TEXT_MUTED)
                        .small(),
                    )
                    .fill(theme::CONTROL)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(egui::Rounding::same(3.0))
                    .min_size(egui::vec2(ui.available_width(), 20.0)),
                )
                .on_hover_text(format!(
                    "click to open the folder — {}\nthe folder button beside this picks a \
                     new one; [paper] trades_dir in quantick.toml sets the base and \
                     QUANTICK_TRADES_DIR overrides it for one run. Anything writing the \
                     quantick-trades format here (a future bot included) shows up in the \
                     ledger, the report and the export.",
                    std::path::absolute(self.account.trades_dir())
                        .unwrap_or_else(|_| self.account.trades_dir().to_path_buf())
                        .display()
                ))
                .clicked()
            {
                reveal_folder(self.account.trades_dir());
            }
        });
        action
    }

    // ------------------------------------------------------------------
    // Report and ledger
    //
    // The window, the calendar and the trades tab live in `paper_report`.
    // What stays here is the seam: this host owns the journal folder, the
    // symbol and the venue, so it gathers those into a `ReportEnv` and
    // hands them over. Every wrapper below is one line for that reason and
    // not because a layer was added for its own sake - the control plane
    // and the harness hooks call these names, and a name the operator
    // already knows must not move because the code behind it did.
    // ------------------------------------------------------------------

    /// The report's state and its environment, together.
    ///
    /// Test-only, and a delegation: the parts are the account's now.
    #[cfg(test)]
    pub(crate) fn report_parts(
        &mut self,
    ) -> (
        &mut crate::paper_report::ReportState,
        crate::paper_report::ReportEnv<'_>,
    ) {
        self.account.report_parts()
    }

    /// The trades ledger tab. Returns what the ledger asked of the host.
    pub fn draw_trades_tab(&mut self, ui: &mut egui::Ui, tz: TzOffset) -> Option<LedgerAction> {
        let (report, env) = self.account.report_parts();
        report.draw_trades_tab(ui, tz, &env)
    }

    /// The performance report window, computed from what is on disk.
    pub fn draw_report_window(&mut self, ctx: &egui::Context, tz: TzOffset) {
        let (report, env) = self.account.report_parts();
        let asked = report.draw_window(ctx, tz, &env);
        // The report can decide a folder picker should open; opening one is
        // this host's job, because the import copies into *its* journal.
        if asked.start_import {
            self.account.start_import();
        }
        // And it can refuse a typed period. That message goes to the one
        // outbox every paper acknowledgement uses - dropping it here would
        // swallow a refusal the trader earned, which is what "a typed 2x
        // must never do nothing quietly" was written against.
        if let Some(message) = asked.toast {
            self.show_toast(message);
        }
    }

    // ------------------------------------------------------------------
    // End of frame
    // ------------------------------------------------------------------

    /// Settle this panel's per-frame handshakes. Runs once a frame, before
    /// the report window paints, for **every** tab, not only the one shown.
    ///
    /// Here the dock-hover link is cleared (the chart has already read it),
    /// an open report re-reads a close the journal took since, and the export
    /// and import pickers are polled for a background job that finished. Both
    /// jobs belong to the tab that started them, and a trader who starts an
    /// export and then looks at another chart must not have to come back for
    /// it to land — which is what running this only for the active tab used
    /// to mean.
    ///
    /// It no longer draws anything. The message it produces goes to the
    /// window's one toast, through [`Self::take_toast`].
    pub fn settle(&mut self) {
        self.hovered_order = None;
        self.account.settle();
    }

    /// Take the acknowledgement waiting to be shown, if there is one.
    ///
    /// This panel used to draw its own: the same `CENTER_BOTTOM` anchor as
    /// the window's `ToastSurface`, 96px up instead of 44, on a 4-second
    /// clock instead of 8 — so two acknowledgements could sit in one lane, at
    /// two heights, disagreeing about how long an acknowledgement lasts. It
    /// posts to an outbox now and the host drains it into the one surface
    /// that owns the clock and the position.
    ///
    /// Newest wins, as the toast it replaces did: a slot, not a queue, since
    /// a trader reading a stale acknowledgement while the current one waits
    /// behind it is worse than missing the stale one. Taking rather than
    /// reading, so one message is handed over once however many frames pass.
    pub(crate) fn take_toast(&mut self) -> Option<String> {
        self.account.take_toast()
    }

    /// Post an acknowledgement for the window to show. Newest wins.
    ///
    /// No clock is read here, unlike the toast this replaces: the surface is
    /// told the frame's `Instant` by the host, which is what makes an
    /// acknowledgement's lifetime as testable as the engine's arithmetic.
    pub(crate) fn show_toast(&mut self, message: String) {
        self.account.set_toast(message);
    }
}

/// What the order form changed this frame; both are app-wide settings the
/// host persists and fans out to every tab.
#[derive(Debug, Clone, Copy, Default)]
struct OrderEntryChanges {
    cmd_trading: bool,
    strategies: bool,
    /// The risk per trade, the capital or an instrument's money moved, so
    /// the sidecar wants writing and the other tabs want telling.
    risk: bool,
}

/// Empty means "none"; otherwise a strictly positive decimal.
pub(super) fn parse_offset(text: &str) -> Result<Option<Decimal>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<Decimal>() {
        Ok(value) if value > Decimal::ZERO => Ok(Some(value)),
        _ => Err(trimmed.to_owned()),
    }
}

/// A protective price the ticket's offset away from the average entry:
/// the losing side for a stop, the winning side for a target. A long's
/// stop and a short's target sit below the entry; the other two above.
fn offset_price(position: &Position, offset: Decimal, stop: bool) -> Decimal {
    let below = (position.side == Side::Buy) == stop;
    if below {
        position.avg_price.saturating_sub(offset)
    } else {
        position.avg_price.saturating_add(offset)
    }
}
