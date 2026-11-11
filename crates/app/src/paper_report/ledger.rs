//! The trades ledger: every closed trade the journal folder holds.
//!
//! The tab beside the report, and the scoping that decides which sessions
//! feed it. The report answers "how did the account do?"; the ledger answers
//! "which trades, in what order, from which file?", so the two read the same
//! journal for different questions and keep their state apart.

use eframe::egui;
use egui_phosphor::regular as icons;
pub(crate) use quantick_paper::report::ledger_rows::LedgerScope;
use quantick_paper::report::ledger_rows::{LedgerPage, RowNest, ledger_rows, nesting, saved_items};
use quantick_paper::report::ledger_state::{
    LedgerEdit, LedgerView, LedgerViewChange, SessionPress,
};
use quantick_paper::report::load_history;
use quantick_paper::report::readout::LedgerScopeTotals;
use quantick_paper::report::session_edit::JournalEdit;
use quantick_paper::report::sessions::{JournaledTrade, SessionKey, TradeRef, live_keys};

use super::rows::{
    GroupView, Item, LedgerRow, TradeMeta, draw_day_header, draw_group_header, draw_ledger_row,
    draw_more_row, draw_open_row, framed,
};
use super::sessions::{draw_hidden_live, draw_inline, draw_session_head};
use super::{
    LEDGER_ROW_HEIGHT_PX, LEDGER_SCOPE_COMBO_PX, LedgerTotals, ReportEnv, ReportState,
    TOTALS_STRIP_PX,
};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::{caption, financial_text, list_symbol_folders, points_color};
use crate::theme;
use crate::timezone::TzOffset;
use quantick_appearance::paper::{CARD_GAP_PX, ledger_frame};

impl ReportState {
    // ------------------------------------------------------------------
    // Trades ledger tab
    // ------------------------------------------------------------------

    /// Load the earlier sessions' rows for the ledger, scoped to the
    /// current symbol or the whole folder. The live session's own file is
    /// excluded — its trades are already in the simulator.
    pub(crate) fn reload_ledger(&mut self, env: &ReportEnv<'_>) {
        self.ledger_layout_revision = self.ledger_layout_revision.wrapping_add(1);
        let symbol = self.ledger_scope.folder(env.symbol);
        let history = load_history(env.dir, symbol, env.session_journal_paths);
        // The strip under the list sums every saved trade, not the revealed
        // page, so it is summed once here rather than on every frame.
        self.saved_totals = LedgerTotals::of_rows(env.pnl, &history.rows);
        self.sessions.reloaded(&history);
        self.history_cache = Some(history);
        self.ledger_symbols = list_symbol_folders(env.dir);
    }

    /// Set what the ledger shows from outside its filter row — the
    /// `session.journal.set` view action — and read it back.
    pub(crate) fn ledger_view(&mut self, change: &LedgerViewChange) -> (LedgerView, bool) {
        self.sessions.view(change)
    }

    /// A session file was renamed, tagged or deleted — by a gesture here, in
    /// another tab, or through the control plane: re-read the folder.
    pub(crate) fn journal_edited(&mut self, env: &ReportEnv<'_>) {
        self.history_cache = None;
        if self.report_open {
            self.reload_report(env);
        }
    }

    /// Fold or unfold one civil day in the ledger. A named action taking
    /// data, like every other capability here: the header click calls it,
    /// and so does anything else that ever wants to.
    pub(crate) fn set_day_collapsed(&mut self, day: CivilDate, collapsed: bool) {
        if collapsed {
            self.collapsed_days.insert(day.day_number());
        } else {
            self.collapsed_days.remove(&day.day_number());
        }
    }

    /// Whether every day currently in the ledger is folded shut. Read from
    /// the loaded history, so the control can name what it will do.
    pub(super) fn all_days_collapsed(&self, env: &ReportEnv<'_>) -> bool {
        let mut any = false;
        for day in self.ledger_days(env) {
            any = true;
            if !self.collapsed_days.contains(&day) {
                return false;
            }
        }
        any
    }

    /// Every civil day the ledger currently lists, saved and live alike.
    fn ledger_days(&self, env: &ReportEnv<'_>) -> std::collections::BTreeSet<i64> {
        let tz = self.ledger_tz;
        let saved = self
            .history_cache
            .iter()
            .flat_map(|cache| cache.rows.iter().map(|row| &row.trade));
        saved
            .chain(env.session_trades.iter())
            .map(|trade| CivilDate::from_ms(trade.closed_ms, tz).day_number())
            .collect()
    }

    /// Fold every day shut, or open every one back up.
    pub(crate) fn toggle_all_days(&mut self, expand: bool, env: &ReportEnv<'_>) {
        if expand {
            self.collapsed_days.clear();
        } else {
            self.collapsed_days = self.ledger_days(env);
        }
    }

    /// Re-read the folder for a *changed scope* — the refresh button and
    /// the symbol/scope switches. Unlike [`Self::reload_ledger`] this also
    /// drops back to the first page: a deep page count cannot survive a
    /// list it was never counted against.
    pub(crate) fn rescope_ledger(&mut self, env: &ReportEnv<'_>) {
        self.ledger_pages = 1;
        self.reload_ledger(env);
    }

    /// The Trades dock tab: the ledger of closed simulated trades — the
    /// open position pinned on top, this session under it, the saved
    /// history under that, each day split by session where it holds
    /// several, and a totals strip that never scrolls away.
    pub(crate) fn draw_trades_tab(
        &mut self,
        ui: &mut egui::Ui,
        tz: TzOffset,
        env: &ReportEnv<'_>,
    ) -> Option<LedgerAction> {
        let palette = theme::get(ui.ctx());
        if self.history_cache.is_none() {
            self.reload_ledger(env);
        }
        self.ledger_tz = tz;

        crate::panel_chrome::filter_bar(ui, |ui| {
            self.draw_scope_row(ui, env);
            self.sessions
                .draw_filter_row(ui, self.history_cache.as_ref(), tz);
        });
        self.sessions
            .state
            .ensure_cut(self.history_cache.as_ref(), env.live_closes(), tz);
        ui.horizontal(|ui| {
            ui.label(caption(&palette, "TRADE"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(caption(&palette, "P&L"));
            });
        });
        ui.separator();

        // The open position rides above the scroll — panning through
        // history must never hide the trade you are in.
        if let Some(open) = &env.open {
            ui.label(caption(&palette, "OPEN"));
            draw_open_row(ui, open, env.pnl.of(env.symbol), env.symbol);
        }

        let saved_len = self
            .history_cache
            .as_ref()
            .map_or(0, |cache| cache.rows.len());
        if env.session_trades.is_empty() && saved_len == 0 {
            let action = if env.open.is_none() {
                self.draw_empty_state(ui, env)
            } else {
                None
            };
            self.draw_ledger_disclosure(ui);
            return action;
        }

        let cache = self.history_cache.as_ref();
        let cut = self.sessions.state.cut.as_ref()?;
        // Each close's session and file trade were recorded as it was
        // written; only the set of files still being written is read here.
        let live_keys = live_keys(env.dir, env.session_journal_paths);
        // This session's trades go through the same filter the saved rows
        // were cut with, a handful per frame.
        let (live, hidden_live) = self.sessions.state.live_trades(
            env.session_trades,
            env.session_trade_at,
            env.session_trade_sources,
            cache,
            tz,
        );
        let pnl = env.pnl.of(env.symbol);
        let live: Vec<Item<'_>> = live
            .into_iter()
            .map(|live| Item::live(live, env.symbol, pnl))
            .collect();
        // Only the revealed pages are turned into rows; the rest stay
        // loaded and counted, which is what the control below them says.
        let page = LedgerPage::of(cut.kept.len(), self.ledger_pages);
        let saved = saved_items(cache, &cut.kept, page.shown, env.pnl);

        // Rows are cut newest first, so day headers open each day as the
        // list walks back in time.
        let view = GroupView {
            collapsed_days: &self.collapsed_days,
            collapsed_sessions: &self.sessions.state.view.folded,
            focused: self.sessions.state.focused.as_ref(),
            editing: self.sessions.state.edit.as_ref().map(LedgerEdit::session),
            labels: &cut.labels,
            live: &live_keys,
            tz,
        };
        let (rows, inline_placed) = ledger_rows(
            &live,
            hidden_live,
            &saved,
            cut.kept.len(),
            page.remaining,
            &view,
        );

        let scope = if self.sessions.state.view.filter.is_default() {
            let own = env.session_trades;
            LedgerScopeTotals::Whole {
                symbol: env.symbol,
                own,
            }
        } else {
            let live = live.iter().map(|item| (item.meta.pnl, item.trade));
            LedgerScopeTotals::Cut {
                kept: &cut.kept,
                live: live.collect(),
            }
        };
        let rows_in = cache.map(|cache| cache.rows.as_slice());
        let totals = self.saved_totals.in_scope(env.pnl, rows_in, scope);
        let shown = live.len() + cut.kept.len();
        let rows_listed = live.len() + saved.len();
        let list_height = (ui.available_height() - palette.layout(TOTALS_STRIP_PX) * 3.0)
            .max(palette.layout(LEDGER_ROW_HEIGHT_PX));
        if rows.is_empty() {
            ui.label(
                egui::RichText::new("Nothing matches the filter.")
                    .color(palette.text_muted())
                    .small(),
            );
        }
        let target = self.sessions.state.tag_target(self.live_pick(env), cache);
        let delete_key = self.sessions.state.key_press(false, &live_keys, cache);
        let presses = RowList {
            layout_key: self.ledger_layout_key(env, tz, rows.len()),
            rows: &rows,
            selected: self.selected_trade,
            selected_saved: self.sessions.state.selected_saved.as_ref(),
            chart: env.symbol,
            tz,
            deletes: delete_key.is_some(),
            edit: &mut self.sessions.state.edit,
            focus_edit: &mut self.sessions.state.focus_edit,
        }
        .show(ui, list_height);
        self.sessions.state.edit_placed(inline_placed);
        let focus_tags = presses.tag || (presses.hovered && presses.hash);
        let keyed = if presses.f2 {
            self.sessions.state.key_press(true, &live_keys, cache)
        } else if presses.delete {
            delete_key
        } else {
            None
        };
        let mut action = self.apply_row_presses(presses);
        if let Some(press) = keyed {
            action = action.or(self.apply_session_press(press));
        }

        ui.separator();
        let target = target
            .as_ref()
            .map(|(key, trade, tags)| (key.clone(), *trade, tags.as_slice()));
        if let Some(edit) = self.sessions.draw_tag_strip(ui, target, focus_tags) {
            action = Some(LedgerAction::Edit(edit));
        }
        self.sessions
            .draw_filter_status(ui, shown, env.session_trades.len() + saved_len);
        crate::panel_chrome::panel_footer(ui, |ui| {
            draw_totals_strip(ui, env.pnl.worded(&totals.net), &totals, page, rows_listed);
        });
        self.draw_ledger_disclosure(ui);
        action
    }

    /// The row list's layout identity: anything that moves a row changes it.
    fn ledger_layout_key(&self, env: &ReportEnv<'_>, tz: TzOffset, rows: usize) -> egui::Id {
        egui::Id::new((
            self.ledger_layout_revision,
            env.symbol,
            env.dir,
            env.session_journal_paths,
            env.session_trades.len(),
            rows,
            self.ledger_pages,
            format!(
                "{:?}{:?}{:?}{:?}{:?}",
                self.sessions.state.view,
                self.collapsed_days,
                env.pnl.key(),
                tz,
                self.sessions
                    .state
                    .edit
                    .as_ref()
                    .map(|edit| (std::mem::discriminant(edit), edit.session()))
            ),
        ))
    }

    /// The selected live trade's journal entry — `Some(None)` for one no
    /// file holds — or `None` when no live trade is selected.
    fn live_pick<'e>(&self, env: &ReportEnv<'e>) -> Option<Option<&'e JournaledTrade>> {
        let index = self.selected_trade?;
        Some(env.session_trade_at.get(index).and_then(Option::as_ref))
    }

    /// Hand a session gesture to the session layer; an edit leaves for the
    /// host, which applies it for every tab.
    fn apply_session_press(&mut self, press: SessionPress) -> Option<LedgerAction> {
        let cache = self.history_cache.as_ref();
        self.sessions
            .state
            .apply(press, cache)
            .map(LedgerAction::Edit)
    }

    /// Scope row: which instrument's saved history the ledger lists, the
    /// re-read button and the fold-every-day switch.
    fn draw_scope_row(&mut self, ui: &mut egui::Ui, env: &ReportEnv<'_>) {
        let palette = theme::get(ui.ctx());
        let mut reload = false;
        let mut picked: Option<LedgerScope> = None;
        ui.horizontal(|ui| {
            let chart = env.symbol.to_owned();
            egui::ComboBox::from_id_salt("paper_ledger_scope")
                .width(LEDGER_SCOPE_COMBO_PX)
                .selected_text(
                    egui::RichText::new(self.ledger_scope.label(&chart))
                        .font(palette.mono_font(11.0)),
                )
                .show_ui(ui, |ui| {
                    let mut option = |ui: &mut egui::Ui, scope: LedgerScope| {
                        let on = self.ledger_scope == scope;
                        if ui.selectable_label(on, scope.label(&chart)).clicked() && !on {
                            picked = Some(scope);
                        }
                    };
                    option(ui, LedgerScope::Chart);
                    option(ui, LedgerScope::All);
                    if !self.ledger_symbols.is_empty() {
                        ui.separator();
                    }
                    for symbol in self.ledger_symbols.clone() {
                        option(ui, LedgerScope::Symbol(symbol));
                    }
                })
                .response
                .on_hover_text(
                    "which instrument's saved history this list shows - the chart's, one you \
                     name, or all of them mixed into one timeline",
                );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icons::ARROWS_CLOCKWISE)
                    .on_hover_text("re-read the history folder")
                    .clicked()
                {
                    reload = true;
                }
                // Folding every earlier day at once: the list becomes one
                // line per day, which is how a week is read rather than
                // scrolled.
                let folded = self.all_days_collapsed(env);
                let (icon, hover) = if folded {
                    (icons::ARROWS_OUT_LINE_VERTICAL, "open every day back up")
                } else {
                    (
                        icons::ARROWS_IN_LINE_VERTICAL,
                        "fold every day shut - each keeps its date, count and net",
                    )
                };
                if ui.small_button(icon).on_hover_text(hover).clicked() {
                    self.toggle_all_days(folded, env);
                }
            });
        });
        if let Some(scope) = picked {
            self.ledger_scope = scope;
            reload = true;
        }
        if reload {
            self.rescope_ledger(env);
        }
    }

    /// Nothing closed and nothing open: say so, and offer the ticket.
    fn draw_empty_state(&self, ui: &mut egui::Ui, env: &ReportEnv<'_>) -> Option<LedgerAction> {
        let palette = theme::get(ui.ctx());
        ui.add_space(12.0);
        let headline = match self.ledger_scope.folder(env.symbol) {
            Some(symbol) if !symbol.is_empty() => {
                format!("No trades for {symbol}.")
            }
            _ => "No simulated trades yet.".to_owned(),
        };
        ui.label(egui::RichText::new(headline).color(palette.text_primary()));
        ui.label(
            egui::RichText::new(
                "Close a position and it lands here - this session and every saved one.",
            )
            .color(palette.text_support()),
        );
        ui.add_space(4.0);
        let open_ticket = ui
            .button("Open the ticket")
            .on_hover_text("switch to the Trading tab and place an order")
            .clicked();
        open_ticket.then_some(LedgerAction::OpenTicket)
    }

    /// Apply what the list was pressed for, after it drew. The navigation,
    /// if any, is the host's to perform and is handed back.
    fn apply_row_presses(&mut self, presses: RowPresses) -> Option<LedgerAction> {
        if let Some(selection) = presses.select {
            self.selected_trade = selection;
            if selection.is_some() {
                self.sessions.state.selected_saved = None;
                self.sessions.state.trade_selected();
            }
        }
        if let Some(selection) = presses.select_saved {
            if selection.is_some() {
                self.selected_trade = None;
                self.sessions.state.trade_selected();
            }
            self.sessions.state.selected_saved = selection;
        }
        if let Some(press) = presses.session
            && let Some(action) = self.apply_session_press(press)
        {
            return Some(action);
        }
        if presses.reveal_more {
            self.ledger_pages = self.ledger_pages.saturating_add(1);
        }
        if let Some((day, collapsed)) = presses.fold {
            self.set_day_collapsed(day, collapsed);
        }
        presses.navigate
    }

    /// The honesty line under the ledger: unreadable files and skipped rows
    /// are counted, never silently dropped.
    fn draw_ledger_disclosure(&self, ui: &mut egui::Ui) {
        let palette = theme::get(ui.ctx());
        let Some(cache) = &self.history_cache else {
            return;
        };
        if cache.unreadable_files == 0 && cache.problem_rows == 0 {
            return;
        }
        ui.label(
            egui::RichText::new(format!(
                "{} file(s) unreadable, {} row(s) skipped - counted, never silently dropped.",
                cache.unreadable_files, cache.problem_rows,
            ))
            .color(palette.warn())
            .small(),
        );
    }
}

/// The ledger's scrolling list as a view: the rows already cut, and the
/// readings a row needs to paint itself. It reports what was pressed as
/// [`RowPresses`] and changes nothing but the inline edit's own text.
struct RowList<'a, 'r> {
    layout_key: egui::Id,
    rows: &'a [LedgerRow<'r>],
    selected: Option<usize>,
    selected_saved: Option<&'a (SessionKey, TradeRef)>,
    /// The chart's instrument: this session's rows are on it, and a row
    /// names its own only where it is another.
    chart: &'a str,
    tz: TzOffset,
    /// Del over the list would start a delete, so the list takes the key.
    deletes: bool,
    edit: &'a mut Option<LedgerEdit>,
    /// The edit just started: its inline row takes the keyboard, once.
    focus_edit: &'a mut bool,
}

/// What the ledger's rows were pressed for in one frame.
#[derive(Default)]
struct RowPresses {
    /// A session row's selection changed: `Some(index)` selects, `None`
    /// clears.
    select: Option<Option<usize>>,
    /// A saved row's selection changed.
    select_saved: Option<Option<(SessionKey, TradeRef)>>,
    /// Center the chart on a round trip.
    navigate: Option<LedgerAction>,
    /// Reveal one more page of saved history.
    reveal_more: bool,
    /// Fold (`true`) or unfold one day.
    fold: Option<(CivilDate, bool)>,
    /// A session row, its menu or an inline row asked for something.
    session: Option<SessionPress>,
    /// A trade's menu asked for the tag strip.
    tag: bool,
    /// The pointer is over the list, so its keys are the ledger's.
    hovered: bool,
    f2: bool,
    delete: bool,
    hash: bool,
}

impl RowList<'_, '_> {
    fn show(mut self, ui: &mut egui::Ui, list_height: f32) -> RowPresses {
        let mut presses = RowPresses::default();
        let nests = nesting(self.rows);
        let output = egui::ScrollArea::vertical()
            .id_salt("paper_trades_ledger")
            .auto_shrink([false, false])
            .max_height(list_height)
            .show_viewport(ui, |ui, viewport| {
                let palette = theme::get(ui.ctx());
                let key = self.layout_key.with((
                    ui.available_width().to_bits(),
                    ui.ctx().pixels_per_point().to_bits(),
                    format!(
                        "{:?}{:?}{:?}",
                        palette.mono_font(11.0),
                        ui.style().text_styles,
                        ui.spacing().item_spacing
                    ),
                ));
                // Rows meet edge to edge, so a card's border runs unbroken;
                // [`Self::row`] spaces the rows outside the cards.
                ui.spacing_mut().item_spacing.y = 0.0;
                let cache_id = ui.id().with("measured_ledger_rows");
                let cached =
                    ui.data(|data| data.get_temp::<(egui::Id, std::sync::Arc<Vec<f32>>)>(cache_id));
                if let Some((_, offsets)) = cached.filter(|(held_key, _)| *held_key == key) {
                    let total = offsets.last().copied().unwrap_or_default();
                    ui.set_height((total - ui.spacing().item_spacing.y).max(0.0));
                    let start = offsets
                        .partition_point(|y| *y <= viewport.min.y)
                        .saturating_sub(1)
                        .min(self.rows.len());
                    let end = offsets
                        .partition_point(|y| *y <= viewport.max.y)
                        .min(self.rows.len());
                    let top = ui.max_rect().top();
                    let rect = egui::Rect::from_x_y_ranges(
                        ui.max_rect().x_range(),
                        top + offsets[start]..=top + offsets[end],
                    );
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                        for (index, nest) in nests.iter().enumerate().take(end).skip(start) {
                            ui.push_id(index, |ui| self.row(ui, index, *nest, &mut presses));
                        }
                    });
                } else {
                    // Measure once per content/layout revision. Warm frames walk
                    // only the visible range, even after revealing many pages.
                    let offsets = ui
                        .allocate_new_ui(egui::UiBuilder::new().max_rect(ui.max_rect()), |ui| {
                            let top = ui.cursor().top();
                            let mut offsets = Vec::with_capacity(self.rows.len() + 1);
                            offsets.push(0.0);
                            for (index, nest) in nests.iter().enumerate() {
                                ui.push_id(index, |ui| {
                                    self.row(ui, index, *nest, &mut presses);
                                });
                                offsets.push(ui.cursor().top() - top);
                            }
                            offsets
                        })
                        .inner;
                    ui.data_mut(|data| {
                        data.insert_temp(cache_id, (key, std::sync::Arc::new(offsets)))
                    });
                }
            });
        // Keys belong to the ledger only while the pointer is over it and
        // nothing is being typed: a trading hotkey is never taken. The
        // chart's Del and Backspace stand down while the pointer is on the
        // dock (`Dock::holds_pointer`); Esc is the inline row's only while
        // its field or button holds the keyboard.
        presses.hovered =
            ui.rect_contains_pointer(output.inner_rect) && !ui.ctx().wants_keyboard_input();
        if presses.hovered {
            ui.input_mut(|input| {
                presses.f2 = input.consume_key(egui::Modifiers::NONE, egui::Key::F2);
                presses.delete =
                    self.deletes && input.consume_key(egui::Modifiers::NONE, egui::Key::Delete);
                presses.hash = input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Text(text) if text == "#"));
            });
        }
        presses
    }

    /// One row inside the day card and session box `nest` places it in;
    /// a spaced one takes a card's gap under the last card, else the style's.
    fn row(&mut self, ui: &mut egui::Ui, index: usize, nest: RowNest, presses: &mut RowPresses) {
        #[cfg(test)]
        layout_cache_tests::record(ui);
        framed(ui, &ledger_frame(nest.day, nest.session), |ui| {
            self.row_body(ui, index, presses)
        });
        let gap = ui.ctx().style().spacing.item_spacing.y;
        if nest.spaced {
            ui.add_space(if nest.day.is_some() { CARD_GAP_PX } else { gap });
        }
    }

    fn row_body(&mut self, ui: &mut egui::Ui, index: usize, presses: &mut RowPresses) {
        let rows = self.rows;
        match &rows[index] {
            LedgerRow::Header(label, count) => draw_group_header(ui, label, *count),
            LedgerRow::Day(date, count, net, folded) => {
                if draw_day_header(ui, *date, *count, net, *folded) {
                    presses.fold = Some((*date, !*folded));
                }
            }
            LedgerRow::SessionHead(head) => {
                if let Some(press) = draw_session_head(ui, head) {
                    presses.session = Some(press);
                }
            }
            LedgerRow::Inline => {
                if let Some(edit) = self.edit.as_mut()
                    && let Some(press) = draw_inline(ui, edit, self.focus_edit)
                {
                    presses.session = Some(press);
                }
            }
            LedgerRow::HiddenLive(hidden) => draw_hidden_live(ui, *hidden),
            LedgerRow::Session(trade_index, trade, meta) => {
                let is_selected = self.selected == Some(*trade_index);
                let response = draw_ledger_row(
                    ui,
                    trade,
                    *meta,
                    (self.chart, self.chart),
                    is_selected,
                    true,
                    self.tz,
                );
                if response.navigate {
                    presses.navigate =
                        Some(LedgerAction::Navigate(trade.opened_ms, trade.closed_ms));
                } else if response.clicked {
                    presses.select = Some((!is_selected).then_some(*trade_index));
                }
                if let Some(row) = response.response {
                    self.trade_menu(&row, *meta, presses, || TradePick::Live(*trade_index));
                }
            }
            LedgerRow::Earlier(symbol, trade, meta) => {
                let pick = meta.session.map(|key| (key.clone(), meta.trade_ref));
                let is_selected = pick.is_some() && pick.as_ref() == self.selected_saved;
                let on_chart = (*symbol, self.chart);
                let response =
                    draw_ledger_row(ui, trade, *meta, on_chart, is_selected, false, self.tz);
                if response.clicked {
                    presses.select_saved = Some(if is_selected { None } else { pick.clone() });
                }
                if let Some(row) = response.response {
                    self.trade_menu(&row, *meta, presses, || TradePick::Saved(pick.clone()));
                }
            }
            LedgerRow::More(remaining) => {
                presses.reveal_more |= draw_more_row(ui, *remaining);
            }
        }
    }

    /// A trade's context menu: tag it. Rename and delete belong to the
    /// session row, never to one of its trades.
    fn trade_menu(
        &self,
        row: &egui::Response,
        meta: TradeMeta<'_>,
        presses: &mut RowPresses,
        pick: impl Fn() -> TradePick,
    ) {
        if meta.session.is_none() {
            return;
        }
        row.context_menu(|ui| {
            if ui.button("Tag…").clicked() {
                match pick() {
                    TradePick::Live(index) => presses.select = Some(Some(index)),
                    TradePick::Saved(saved) => presses.select_saved = Some(saved),
                }
                presses.tag = true;
                ui.close_menu();
            }
        });
    }
}

/// Which trade a menu's Tag… selects.
enum TradePick {
    Live(usize),
    Saved(Option<(SessionKey, TradeRef)>),
}

/// The strip under the list: trades and win rate over everything in scope
/// in the footer plate's ink, and the net points.
pub(super) fn draw_totals_strip(
    ui: &mut egui::Ui,
    net: (String, rust_decimal::Decimal),
    totals: &LedgerTotals,
    page: LedgerPage,
    rows_listed: usize,
) {
    let palette = theme::get(ui.ctx());
    let summary = totals.summary(page.remaining > 0, rows_listed);
    ui.label(egui::RichText::new(summary).monospace());
    ui.label(financial_text(
        &palette,
        &net.0,
        palette.mono_font(12.0),
        points_color(&palette, net.1),
    ));
}

/// What the Trades ledger asked of its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerAction {
    /// Center the chart on this round trip (`opened_ms`, `closed_ms`).
    Navigate(i64, i64),
    /// Switch the dock to the Trading tab — the empty state's call to
    /// action.
    OpenTicket,
    /// Rename, tag or delete a session. The host applies it for every tab,
    /// the same way the `session.journal.set` capability does.
    Edit(JournalEdit),
}

#[cfg(test)]
impl ReportState {
    /// Whether the ledger holds a read of the folder.
    pub(crate) fn ledger_loaded(&self) -> bool {
        self.history_cache.is_some()
    }
}

#[cfg(test)]
mod layout_cache_tests {
    use super::*;
    use rust_decimal::Decimal;

    pub(super) fn record(ui: &mut egui::Ui) {
        ui.data_mut(|data| {
            let id = egui::Id::new("ledger_rows_visited");
            data.insert_temp(id, data.get_temp::<usize>(id).unwrap_or_default() + 1);
        });
    }

    #[test]
    fn measured_ledger_warm_frames_only_visit_the_viewport() {
        for count in [100, 1000] {
            let ctx = egui::Context::default();
            let rows: Vec<_> = (0..count)
                .map(|i| {
                    if i % 2 == 0 {
                        LedgerRow::Header("Session", i)
                    } else {
                        LedgerRow::Day(
                            CivilDate::from_ymd(2026, 10, 21),
                            i,
                            (
                                Decimal::from(i),
                                format!(
                                    "+{i} BRL and a complete long calculation with neutral units"
                                ),
                            ),
                            false,
                        )
                    }
                })
                .collect();
            let counter = egui::Id::new("ledger_rows_visited");
            for (frame, width, scale, revision) in [
                (0, 320.0, 1.0, 0),
                (1, 320.0, 1.0, 0),
                (2, 400.0, 1.0, 0),
                (3, 400.0, 1.0, 0),
                (4, 400.0, 1.5, 0),
                (5, 400.0, 1.5, 0),
                (6, 400.0, 1.5, 1),
                (7, 400.0, 1.5, 1),
            ] {
                ctx.data_mut(|data| data.insert_temp(counter, 0_usize));
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 300.0),
                    )),
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(scale);
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        RowList {
                            layout_key: egui::Id::new(revision),
                            rows: &rows,
                            selected: None,
                            selected_saved: None,
                            chart: "",
                            tz: TzOffset::new(0),
                            deletes: false,
                            edit: &mut None,
                            focus_edit: &mut false,
                        }
                        .show(ui, 220.0);
                    });
                });
                let visited = ctx.data(|data| data.get_temp::<usize>(counter).unwrap());
                if frame % 2 == 0 {
                    assert_eq!(visited, count, "content/layout change remeasures");
                } else {
                    assert!(visited < 20, "warm viewport visited {visited} of 1000 rows");
                }
            }
        }
    }
}
