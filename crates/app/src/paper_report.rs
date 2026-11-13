//! Native IO and painting for the one report model owned by each paper account.
#[cfg(any(feature = "scenario-harness", test))]
use crate::paper_calendar::DaySelection;
use crate::timezone::TzOffset;
use eframe::egui;
use quantick_paper::pnl::PnlReader;
use quantick_paper::report::layout::*;
use quantick_paper::report::readout::LedgerTotals;
use quantick_ui_model::paper_report::{
    Effects, Intent, LedgerIntent, Load, LoadedLedger, LoadedReport, Model, State,
};
use quantick_ui_model::{Adapter, UpdateSession};

mod curve;
mod ledger;
mod rows;
#[cfg(any(feature = "scenario-harness", test))]
mod scroll_probe;
mod sessions;
mod tables;
mod window;

pub(crate) use quantick_paper::report::ledger_rows::LedgerScope;
#[cfg(test)]
pub(crate) use quantick_paper::report::load_history;
pub(crate) use quantick_paper::report::snapshot::{OpenRow, ReportEnv};
pub(crate) use quantick_paper::report::{HistoryRow, LoadedHistory, ReportView, SourceFilter};
#[cfg(test)]
use quantick_sim::{ClosedTrade, PerformanceReport};
pub(crate) use quantick_ui_model::paper_report::LedgerAction;
/// Aggregate loaded history rows — the ticket tests' shortcut from a journal
/// on disk to a report.
#[cfg(test)]
pub(crate) fn report_from_history(history: &LoadedHistory) -> PerformanceReport {
    let trades: Vec<ClosedTrade> = history.rows.iter().map(|row| row.trade.clone()).collect();
    PerformanceReport::from_trades(&trades)
}

#[derive(Default)]
pub(crate) struct ReportResponse {
    pub start_import: bool,
    pub toast: Option<String>,
}

#[derive(Default)]
struct NativeView {
    report_table_layout: Option<(egui::FontId, f32, f32, Vec<f32>)>,
    report_layout_revision: u64,
    #[cfg(any(feature = "scenario-harness", test))]
    scroll_probe: scroll_probe::Probe,
}

/// Nested adapter, without an account reference or global surface ownership.
pub(crate) struct ReportAdapter {
    adapter: Adapter<Model, NativeView, Effects>,
}
/// Preserve existing account and control wrapper signatures.
pub(crate) type ReportState = ReportAdapter;
impl Default for ReportAdapter {
    fn default() -> Self {
        Self {
            adapter: Adapter::new(NativeView::default(), std::convert::identity),
        }
    }
}
impl std::ops::Deref for ReportAdapter {
    type Target = State;
    fn deref(&self) -> &State {
        &self.adapter.model_state
    }
}
struct Paint<'a, 's> {
    updates: &'a mut UpdateSession<'s, Model>,
    native: &'a mut NativeView,
}
impl std::ops::Deref for Paint<'_, '_> {
    type Target = State;
    fn deref(&self) -> &State {
        self.updates.state()
    }
}
impl Paint<'_, '_> {
    fn update(&mut self, intent: Intent) {
        self.updates.update(intent);
    }
    fn ledger(&mut self, intent: LedgerIntent) {
        self.update(Intent::Ledger(intent));
    }
    fn ensure_report_view(&mut self, tz: TzOffset, pnl: PnlReader<'_>) {
        if let Some(receipt) = self.project_report(tz, pnl) {
            self.update(Intent::InstallReport(receipt));
            if let Some(snapshot) = self.snapshot() {
                tracing::info!(target: "quantick::app", schema_version = 1_u8,
                    event_code = "PAPER_REPORT_CUT", symbol = snapshot.symbol.unwrap_or("*"),
                    source = snapshot.source.label(), window = %snapshot.window.label(),
                    trades = snapshot.rows.len(), hidden_outside = snapshot.hidden_outside,
                    hidden_by_source = snapshot.hidden_by_source, net_points = %snapshot.report.net_points,
                    "the simulated performance report was re-cut");
            }
        }
        if self.native.report_layout_revision != self.report_layout_revision() {
            self.native.report_table_layout = None;
            self.native.report_layout_revision = self.report_layout_revision();
        }
    }
}

/// The existing IO, completed synchronously before UpdateSession returns.
fn local_effects(mut effects: Effects, env: &ReportEnv<'_>) -> (Effects, Option<Intent>) {
    let receipt = effects.load.take().map(|load| match load {
        Load::Report(symbol) => Intent::LoadedReport(LoadedReport {
            history: quantick_paper::report::load_history(env.dir, symbol.as_deref(), &[]),
            symbols: crate::paper_chrome::list_symbol_folders(env.dir),
        }),
        Load::Ledger(scope) => {
            let history = quantick_paper::report::load_history(
                env.dir,
                scope.folder(env.symbol),
                env.session_journal_paths,
            );
            Intent::LoadedLedger(LoadedLedger::new(
                history,
                crate::paper_chrome::list_symbol_folders(env.dir),
                env.pnl,
            ))
        }
    });
    (effects, receipt)
}
impl ReportAdapter {
    fn with<R>(
        &mut self,
        env: &ReportEnv<'_>,
        paint: impl FnOnce(&mut Paint<'_, '_>) -> R,
    ) -> (R, Effects) {
        let mut output = Effects::default();
        let result = self.adapter.with_updates(
            |effects| output.append_delivered(effects),
            |native, session| {
                session.with_local_effects(&mut |effects| local_effects(effects, env), |updates| {
                    paint(&mut Paint { updates, native })
                })
            },
        );
        (result, output)
    }
    pub(crate) fn update(&mut self, intent: Intent) -> Effects {
        self.adapter.update(intent)
    }
    fn event(&mut self, intent: Intent, env: &ReportEnv<'_>) {
        self.with(env, |paint| paint.update(intent));
    }
    pub(crate) fn is_open(&self) -> bool {
        self.report_open()
    }
    pub(crate) fn open(&mut self, env: &ReportEnv<'_>) {
        self.event(Intent::Open(env.symbol.to_owned()), env);
    }
    pub(crate) fn trades_dir_changed(&mut self, env: &ReportEnv<'_>) {
        self.event(Intent::DirectoryChanged, env);
    }
    pub(crate) fn symbol_changed(&mut self) {
        self.update(Intent::SymbolChanged);
    }
    pub(crate) fn journal_changed(&mut self, env: &ReportEnv<'_>) {
        self.event(Intent::JournalChanged, env);
    }
    pub(crate) fn history_imported(&mut self, env: &ReportEnv<'_>) {
        self.event(Intent::HistoryImported, env);
    }
    pub(crate) fn journal_edited(&mut self, env: &ReportEnv<'_>) {
        self.event(Intent::JournalEdited, env);
    }
    #[cfg(test)]
    pub(crate) fn reload_ledger(&mut self, env: &ReportEnv<'_>) {
        self.event(Intent::Ledger(LedgerIntent::Reload), env);
    }
    pub(crate) fn selected_trade(&self) -> Option<usize> {
        self.adapter.model_state.selected_trade()
    }
    pub(crate) fn clear_selected_trade(&mut self) -> bool {
        let selected = self.selected_trade().is_some();
        self.update(Intent::Ledger(LedgerIntent::ClearSelection));
        selected
    }
    pub(crate) fn saved_rows(&mut self, env: &ReportEnv<'_>) -> &[HistoryRow] {
        self.event(Intent::Ledger(LedgerIntent::EnsureLoaded), env);
        self.history_cache()
            .as_ref()
            .map_or(&[], |cache| cache.rows.as_slice())
    }
    pub(crate) fn ledger_view(
        &mut self,
        change: &quantick_paper::report::ledger_state::LedgerViewChange,
    ) -> (quantick_paper::report::ledger_state::LedgerView, bool) {
        let before = self.sessions().view.clone();
        self.update(Intent::Ledger(LedgerIntent::View(change.clone())));
        (self.sessions().view.clone(), self.sessions().view != before)
    }
    pub(crate) fn draw_window(
        &mut self,
        ctx: &egui::Context,
        tz: TzOffset,
        env: &ReportEnv<'_>,
    ) -> ReportResponse {
        let (_, effects) = self.with(env, |paint| paint.draw_window(ctx, tz, env));
        ReportResponse {
            start_import: effects.start_import,
            toast: effects.toast,
        }
    }
    pub(crate) fn draw_trades_tab(
        &mut self,
        ui: &mut egui::Ui,
        tz: TzOffset,
        env: &ReportEnv<'_>,
    ) -> Option<LedgerAction> {
        self.with(env, |paint| paint.draw_trades_tab(ui, tz, env))
            .1
            .ledger
    }
}

#[cfg(any(feature = "scenario-harness", test))]
impl ReportAdapter {
    pub(crate) fn autostart_report(&mut self, env: &ReportEnv<'_>) {
        self.update(Intent::Symbol(None));
        self.open(env);
    }
    pub(crate) fn autostart_calendar(&mut self, selection: DaySelection, env: &ReportEnv<'_>) {
        self.autostart_report(env);
        let mut calendar = self.calendar();
        calendar.open = true;
        calendar.selection = selection;
        if let Some(range) = selection.range() {
            calendar.month = Some(range.start.month_start());
        }
        self.update(Intent::Calendar(calendar));
    }
    pub(crate) fn set_ledger_scope(&mut self, scope: LedgerScope) {
        self.update(Intent::Ledger(LedgerIntent::Scope(scope)));
    }
    pub(crate) fn autostart_folded_days(&mut self, tz: TzOffset, env: &ReportEnv<'_>) {
        self.event(Intent::Ledger(LedgerIntent::EnsureLoaded), env);
        self.update(Intent::Ledger(LedgerIntent::Timezone(tz)));
        let folds = self.project_day_folds(false, env.live_closes());
        self.update(Intent::Ledger(LedgerIntent::FoldDays(folds)));
    }
    pub(crate) fn autostart_ledger_pages(&mut self, pages: usize) {
        self.update(Intent::Ledger(LedgerIntent::Pages(pages)));
    }
    pub(crate) fn set_report_list_open(&mut self, open: bool) {
        self.update(Intent::ListOpen(open));
    }
}
#[cfg(test)]
impl ReportAdapter {
    pub(crate) fn rescope_ledger(&mut self, env: &ReportEnv<'_>) {
        self.event(Intent::Ledger(LedgerIntent::Rescope), env);
    }
    pub(crate) fn set_day_collapsed(
        &mut self,
        day: crate::paper_calendar::CivilDate,
        collapsed: bool,
    ) {
        self.update(Intent::Ledger(LedgerIntent::FoldDay(day, collapsed)));
    }
    fn show_toast(&mut self, message: String) {
        self.update(Intent::Toast(message));
    }
    pub(crate) fn saved_rows_loaded(&self) -> Option<&[HistoryRow]> {
        self.history_cache()
            .as_ref()
            .map(|cache| cache.rows.as_slice())
    }
    pub(crate) fn revealed_pages(&self) -> usize {
        self.ledger_pages()
    }
    pub(crate) fn view_rows(&self) -> Option<&[HistoryRow]> {
        self.report_view().as_ref().map(|view| view.rows.as_slice())
    }
    pub(crate) fn ledger_loaded(&self) -> bool {
        self.history_cache().is_some()
    }
    pub(crate) fn ensure_report_view(&mut self, tz: TzOffset, pnl: PnlReader<'_>) {
        if let Some(receipt) = self.project_report(tz, pnl) {
            self.update(Intent::InstallReport(receipt));
        }
    }
}
#[cfg(test)]
mod tests;
