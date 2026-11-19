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
use quantick_session::paper::TradeCommand;
use quantick_sim::EntryKind;
use rust_decimal::Decimal;

use quantick_paper::desk::ticket::parse_offset;

use super::{
    ArmedPlacement, PaperTrading, TradingTabAction, cmd, kind_short, kind_word, reveal_folder,
    side_word_upper, strategies,
};
use crate::paper_chrome::{caption, financial_text, fmt_decimal, pill_toggle, points_color};
use crate::paper_report::LedgerAction;
use crate::theme;
use crate::timezone::TzOffset;

mod entry_pair;
mod position_card;

use entry_pair::{EntryPress, entry_pair};
use position_card::{CardCommand, PositionCard};

impl PaperTrading {
    // ------------------------------------------------------------------
    // Chart layer
    // ------------------------------------------------------------------

    /// The chart context menu's resting-order section, anchored at the
    /// clicked price. Invalid types stay visible but disabled, wearing the
    /// sim core's own rejection text — the same curriculum the toasts teach.
    /// Market entry remains in the trading ticket and its hotkeys.
    pub fn context_trade_actions(&mut self, ui: &mut egui::Ui, raw_price: f64) {
        let palette = theme::get(ui.ctx());
        ui.label(
            egui::RichText::new("trade")
                .font(palette.body_font(11.0))
                .color(palette.text_muted()),
        );
        let Some(mark) = self.paper.account.venue().mark_price() else {
            ui.label(
                egui::RichText::new("no print yet - there is no market to trade against")
                    .color(palette.text_muted()),
            );
            return;
        };
        let quantity = self
            .paper
            .desk
            .ticket
            .quantity_preview()
            .map_or_else(|| "?".to_owned(), fmt_decimal);
        let price = self.paper.account.snap(raw_price);
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
        let palette = theme::get(ui.ctx());
        self.paper.desk.gesture.hovered_order = None;
        // Said only while orders go to the simulator; a broker route says its own.
        if !self.paper.account.route_kind().is_broker() {
            ui.label(
                egui::RichText::new("Simulated fills from the tape - no broker.")
                    .color(palette.text_muted()),
            )
            .on_hover_text(
                "A market order fills at the next print; a limit at its own price when \
             the tape trades at or through it; a stop at the print that triggers it. \
             Nothing here touches a real account. Money comes from the point value \
             you declared, else from instruments.toml.",
            );
        }
        let pnl_mode_changed = ui.horizontal(|ui| self.draw_pnl_mode_switch(ui)).inner;
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
            if changed.risk || pnl_mode_changed {
                return Some(TradingTabAction::RiskSettingsChanged);
            }
        }
        action
    }

    /// The position block, drawn by [`PositionCard`] from a snapshot; its
    /// press is applied here, before the order form reads the account.
    fn draw_position_card(&mut self, ui: &mut egui::Ui) {
        let venue = self.paper.account.venue();
        let position = venue.position().cloned();
        let card = PositionCard {
            chip: self.paper.account.position_chip().unwrap_or_default(),
            position: position.as_ref(),
            mark: venue.mark_price(),
            realized: self.paper.account.realized_pnl_tint(),
            realized_text: self.paper.account.realized_pnl_text(),
            open_text: self.paper.account.open_pnl_text(),
            brackets: self.paper.account.bracket_pnl(),
            has_working_orders: !venue.working_orders().is_empty(),
            stop_offset: parse_offset(&self.paper.desk.ticket.stop_offset_text)
                .ok()
                .flatten(),
            profit_offset: parse_offset(&self.paper.desk.ticket.profit_offset_text)
                .ok()
                .flatten(),
            actions: self.paper.position_actions(),
        };
        match card.show(ui) {
            None => {}
            Some(CardCommand::CancelAllOrders) => self.paper.send(TradeCommand::CancelAll),
            Some(CardCommand::Venue(command)) => {
                let events = self.paper.account.dispatch(*command);
                self.paper.account.handle_events(events);
            }
            Some(CardCommand::Close) => self.paper.send(TradeCommand::Close),
            Some(CardCommand::Reverse) => self.paper.send(TradeCommand::Reverse),
            Some(CardCommand::Flatten) => self.paper.send(TradeCommand::Flatten),
        }
    }

    fn draw_order_entry(&mut self, ui: &mut egui::Ui) -> OrderEntryChanges {
        let palette = theme::get(ui.ctx());
        ui.label(caption(&palette, "ORDER"));
        // What the risk per trade makes of the entry the ticket is holding.
        // Read once for the whole form: the quantity field, the support line
        // and the entry pair must all be talking about the same aim.
        //
        // Side::Buy stands for both. Every stop this reads is symmetric in
        // distance - the ruler walks both legs the same number of points,
        // and a typed offset is a distance rather than a price - so the risk
        // is the side-independent half of the answer.
        let risk_reference = self.paper.account.venue().mark_price().unwrap_or_default();
        let risk_state = self.risk_state(Side::Buy, risk_reference);
        let derived_quantity = risk_state.derived_quantity();
        let risk_blocks = risk_state.blocks_entry(self.paper.account.risk_settings().lock);
        if let Some(quantity) = derived_quantity {
            // The mode writes into the field the whole form already reads,
            // rather than adding a second number beside it: one quantity on
            // screen is the quantity that will be sent.
            let derived = crate::paper_chrome::fmt_exact(quantity);
            if self.paper.desk.ticket.qty_text != derived {
                self.paper.desk.ticket.qty_text = derived;
            }
        }
        self.draw_quantity_row(ui, derived_quantity.is_none());
        // The discreet line: what the number means, or why there is none.
        // Supporting body text keeps the size explanation readable.
        let sentence = risk_state.sentence();
        if !sentence.is_empty() {
            let colour = if risk_blocks {
                palette.warn()
            } else {
                palette.text_support()
            };
            ui.label(egui::RichText::new(sentence).color(colour));
        }
        self.draw_order_fields(ui);
        let strategies_changed = strategies::draw_strategy_row(
            ui,
            &mut self.paper.account,
            &mut self.paper.desk.strategy_editor,
        );
        // The whole risk surface, in its own module: this file already
        // carries the order form, and a second feature inside it is how the
        // trunk grew the first time.
        let editor = self.paper.account.risk_editor();
        let risk_changed = crate::risk_sizing::draw_risk_block(
            ui,
            crate::risk_sizing::RiskBlock {
                symbol: editor.symbol,
                settings: editor.settings,
                capital: editor.capital,
                book: editor.book,
                seeded: editor.seeded,
                amount_text: &mut self.paper.desk.ticket.risk_amount_text,
                percent_text: &mut self.paper.desk.ticket.risk_percent_text,
                capital_text: &mut self.paper.desk.ticket.capital_text,
                point_value_text: &mut self.paper.desk.ticket.point_value_text,
                size_step_text: &mut self.paper.desk.ticket.size_step_text,
                currency_text: &mut self.paper.desk.ticket.currency_text,
            },
        );
        ui.add_space(4.0);

        // The lock, enforced on the surface as well as at the click: a
        // ceiling the trader can still press through is one they will press
        // through by accident on a fast tape.
        let ready = self.paper.account.entry_ready() && !risk_blocks;
        match entry_pair(ui, self, ready) {
            None => {}
            Some(EntryPress::Disarm) => self.paper.desk.armed = None,
            Some(EntryPress::Fire(side)) => self.market(side),
            Some(EntryPress::Arm(side)) => {
                self.paper.desk.armed = Some(ArmedPlacement {
                    side,
                    kind: self.paper.desk.ticket.order_type,
                });
            }
        }
        ui.label(
            egui::RichText::new(if self.paper.desk.armed.is_some() {
                "Click the chart at your price. Esc cancels."
            } else if self.paper.desk.ticket.order_type == EntryKind::Market {
                "Market orders fill at the next print."
            } else {
                "The button arms a click; the next chart click rests the order there."
            })
            .color(palette.text_support()),
        );
        OrderEntryChanges {
            cmd_trading: cmd::draw_cmd_trading_settings(
                ui,
                &mut self.paper.desk.cmd_trading,
                &mut self.paper.desk.ruler,
                &self.paper.account,
            ),
            strategies: strategies_changed,
            risk: risk_changed,
        }
    }

    /// Qty: free decimal text (empty must keep meaning "fix me"), with
    /// steppers beside it; Shift steps by ten. Derived and read-only while
    /// the risk per trade is deciding it (`typed` false).
    fn draw_quantity_row(&mut self, ui: &mut egui::Ui, typed: bool) {
        let palette = theme::get(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Qty").color(palette.text_muted()));
            let step = if ui.input(|input| input.modifiers.shift) {
                Decimal::TEN
            } else {
                Decimal::ONE
            };
            let hint = self.paper.account.quantity_step_hint(step);
            ui.add_enabled_ui(typed, |ui| {
                if ui
                    .small_button("−")
                    .on_hover_text(format!("{hint} less (Shift: ten steps)"))
                    .clicked()
                {
                    self.step_quantity(-step);
                }
                let width = ui
                    .painter()
                    .layout_no_wrap(
                        self.paper.desk.ticket.qty_text.clone(),
                        egui::TextStyle::Monospace.resolve(ui.style()),
                        palette.text_primary(),
                    )
                    .size()
                    .x
                    + 12.0;
                ui.add(
                    egui::TextEdit::singleline(&mut self.paper.desk.ticket.qty_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(width.max(56.0).min(ui.available_width() - 28.0)),
                );
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
        let palette = theme::get(ui.ctx());
        // Type: three pills. Picking Limit or Stop is a promise of an
        // accent line on the chart, so the selected pill wears the accent.
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Type").color(palette.text_muted()));
            for kind in [EntryKind::Market, EntryKind::Limit, EntryKind::Stop] {
                let on = self.paper.desk.ticket.order_type == kind;
                if pill_toggle(ui, kind_word(kind), on, "how the entry meets the market").clicked()
                    && !on
                {
                    self.paper.desk.ticket.order_type = kind;
                    self.paper.desk.armed = None;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Stop").color(palette.text_muted()))
                .on_hover_text(
                    "optional protective stop, this many points on the losing side of the \
                     entry; empty places no stop",
                );
            ui.add(
                egui::TextEdit::singleline(&mut self.paper.desk.ticket.stop_offset_text)
                    .desired_width(52.0),
            );
            ui.label(egui::RichText::new("Target").color(palette.text_muted()))
                .on_hover_text(
                    "optional profit target, this many points on the winning side of the \
                 entry; empty places no target",
                );
            ui.add(
                egui::TextEdit::singleline(&mut self.paper.desk.ticket.profit_offset_text)
                    .desired_width(52.0),
            );
            ui.label(
                egui::RichText::new("pts")
                    .color(palette.text_faint())
                    .small(),
            );
        });
    }

    fn draw_pending_orders(&mut self, ui: &mut egui::Ui) {
        let palette = theme::get(ui.ctx());
        let mut in_flight = Vec::new();
        self.paper.account.venue().in_flight_entries(&mut in_flight);
        let queued_entries = in_flight.len();
        // Saturating, because the two reads cross a trait boundary and are
        // documented independently: a venue whose `in_flight_entries` says
        // more than its `in_flight` counts must not panic the render loop.
        let queued_closes = self
            .paper
            .account
            .venue()
            .in_flight()
            .saturating_sub(queued_entries);
        let orders: Vec<_> = self.paper.account.working_orders().to_vec();
        ui.label(caption(
            &palette,
            &format!("WORKING ORDERS · {}", orders.len()),
        ));
        if queued_entries > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{queued_entries} market order(s) await the next print"
                ))
                .color(palette.text_muted()),
            );
        }
        if queued_closes > 0 {
            ui.label(egui::RichText::new("closing at the next print…").color(palette.text_muted()));
        }
        if orders.is_empty() && queued_entries == 0 {
            ui.label(egui::RichText::new("No working orders.").color(palette.text_muted()));
            ui.label(
                egui::RichText::new("Pick Limit or Stop, then click a price on the chart.")
                    .color(palette.text_support()),
            );
            return;
        }
        for order in orders {
            let response = ui.horizontal_wrapped(|ui| {
                // A short accent dash, echoing the dashed chart line.
                let (dash, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
                ui.painter().line_segment(
                    [
                        egui::pos2(dash.left(), dash.center().y),
                        egui::pos2(dash.right(), dash.center().y),
                    ],
                    egui::Stroke::new(2.0_f32, palette.accent()),
                );
                ui.label(
                    egui::RichText::new(format!("#{}", order.id.0))
                        .color(palette.text_faint())
                        .small(),
                );
                ui.label(
                    egui::RichText::new(side_word_upper(order.side))
                        .monospace()
                        .color(palette.side_color(order.side)),
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
                        .color(palette.text_primary()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text("cancel this order")
                        .clicked()
                    {
                        self.paper.send(TradeCommand::Cancel { id: order.id });
                    }
                });
            });
            // One hover, two surfaces: the row lifts its chart line.
            if response.response.hovered() {
                self.paper.desk.gesture.hovered_order = Some(order.id);
            }
        }
    }

    /// The P&L mode switch: which figure leads every readout; the other
    /// always follows it. True when the trader picked another one.
    fn draw_pnl_mode_switch(&mut self, ui: &mut egui::Ui) -> bool {
        let current = self.paper.account.pnl_primary();
        let mut picked = None;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (primary, label) in self.paper.account.pnl_modes().into_iter().rev() {
                let hover = format!("lead P&L with {label} - the other figure stays beside it (session.pnl_display.set)");
                if ui.selectable_label(primary == current, egui::RichText::new(label).small()).on_hover_text(hover).clicked() && primary != current {
                    picked = Some(primary);
                }
            }
        });
        picked
            .inspect(|primary| self.paper.account.set_pnl_primary(*primary))
            .is_some()
    }

    fn draw_session_summary(&mut self, ui: &mut egui::Ui) -> Option<TradingTabAction> {
        let palette = theme::get(ui.ctx());
        let mut action = None;
        let realized = self.paper.account.realized_pnl_tint();
        ui.label(financial_text(
            &palette,
            &self.paper.account.realized_pnl_text(),
            palette.mono_font(12.0),
            points_color(&palette, realized),
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "realized · {} trades",
                    self.paper.account.venue().closed_trades().len()
                ))
                .color(palette.text_muted()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Report…")
                    .on_hover_text("performance metrics computed from the saved history")
                    .clicked()
                {
                    let (report, env) = self.paper.account.report_parts();
                    report.open(&env);
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
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
                            self.paper.account.trades_dir().display()
                        ))
                        .color(palette.text_muted()),
                    )
                    .fill(palette.control())
                    .stroke(egui::Stroke::new(1.0_f32, palette.border()))
                    .rounding(egui::Rounding::same(3.0))
                    .wrap_mode(egui::TextWrapMode::Wrap),
                )
                .on_hover_text(format!(
                    "click to open the folder — {}\nthe folder button beside this picks a \
                     new one; [paper] trades_dir in quantick.toml sets the base and \
                     QUANTICK_TRADES_DIR overrides it for one run. Anything writing the \
                     quantick-trades format here (a future bot included) shows up in the \
                     ledger, the report and the export.",
                    std::path::absolute(self.paper.account.trades_dir())
                        .unwrap_or_else(|_| self.paper.account.trades_dir().to_path_buf())
                        .display()
                ))
                .clicked()
            {
                reveal_folder(self.paper.account.trades_dir());
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

    /// The strategy editor window, drawn from the app's own frame - a
    /// window that lives inside a dock tab disappears the moment the trader
    /// looks at another panel. Returns true when anything changed, so the
    /// app can persist it.
    pub(crate) fn draw_strategy_editor(&mut self, ctx: &egui::Context) -> bool {
        strategies::draw_strategy_editor(
            ctx,
            &mut self.paper.account,
            &mut self.paper.desk.strategy_editor,
        )
    }

    /// The trades ledger tab. Returns what the ledger asked of the host.
    pub fn draw_trades_tab(
        &mut self,
        ui: &mut egui::Ui,
        tz: TzOffset,
        live: &[std::path::PathBuf],
    ) -> Option<LedgerAction> {
        let (report, env) = self.paper.account.report_parts();
        report.draw_trades_tab(ui, tz, &env, live)
    }

    /// The performance report window, computed from what is on disk.
    pub fn draw_report_window(&mut self, ctx: &egui::Context, tz: TzOffset) {
        let (report, env) = self.paper.account.report_parts();
        let asked = report.draw_window(ctx, tz, &env);
        // The report can decide a folder picker should open; opening one is
        // this host's job, because the import copies into *its* journal.
        if asked.start_import {
            self.paper.account.start_import();
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
        self.paper.desk.gesture.hovered_order = None;
        self.paper.account.settle();
    }
}

#[cfg(test)]
impl PaperTrading {
    /// The report's state and its environment, together.
    ///
    /// Test-only, and a delegation: the parts are the account's now.
    pub(crate) fn report_parts(
        &mut self,
    ) -> (
        &mut crate::paper_report::ReportState,
        crate::paper_report::ReportEnv<'_>,
    ) {
        self.paper.account.report_parts()
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
