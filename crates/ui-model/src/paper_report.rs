//! Per-account report and ledger presentation, composed by the Paper surface.
//! IO returns moved terminal receipts; projections borrow the real account facts.
use std::collections::BTreeSet;

use quantick_civil::{CivilDate, TzOffset};
use quantick_paper::pnl::{PnlKey, PnlPick, PnlPrimary, PnlReader};
use quantick_paper::rates::{Currency, RateStore, StoredRates, USD};
use quantick_paper::report::calendar::{
    CalendarState, DayIndex, DayPick, DaySelection, DaySet, MonthPicker,
};
use quantick_paper::report::ledger_rows::LedgerScope;
use quantick_paper::report::ledger_state::{
    LedgerCut, LedgerEdit, LedgerState, LedgerViewChange, LiveCloses, SessionPress,
};
use quantick_paper::report::readout::LedgerTotals;
use quantick_paper::report::scope::{PreparedReport, ReportReceipt, ReportRequest};
use quantick_paper::report::session_edit::JournalEdit;
use quantick_paper::report::sessions::{SessionKey, TradeRef};
use quantick_paper::report::snapshot::ReportSnapshot;
use quantick_paper::report::{
    LoadedHistory, ReportPeriod, ReportView, SourceFilter, SourceSelection, parse_period,
};

/// History generation, sources, timezone and whether the grid reads the scope's journal rows.
type ReportDaysKey = (u64, SourceSelection, TzOffset, bool);
pub mod io;

struct ReportFilter {
    days: DayPick,
    custom_edited: bool,
    custom_text: String,
    period: ReportPeriod,
    source: SourceSelection,
    scope: Option<ReportReceipt>,
}

enum ReportSymbol {
    FirstOpen,
    /// The symbols chosen; empty reads every symbol.
    Selected(BTreeSet<String>),
}

/// Cached presentation can be read by a host, but changes enter as intents.
///
/// Filter writes cannot bypass the revision that protects projection receipts.
/// ```compile_fail,E0616
/// use quantick_ui_model::paper_report::State;
/// use quantick_paper::report::SourceFilter;
/// let mut state = State::default();
/// state.filter.source = SourceFilter::Replay;
/// ```
/// Projection caches cannot be installed by a caller.
/// ```compile_fail,E0616
/// use quantick_ui_model::paper_report::State;
/// let mut state = State::default();
/// state.report_view = None;
/// ```
/// Borrowed domain state is also read only.
/// ```compile_fail,E0594
/// use quantick_ui_model::paper_report::State;
/// let state = State::default();
/// state.sessions().cut = None;
/// ```
pub struct State {
    filter: ReportFilter,
    revision: u64,
    toast: Option<String>,
    report_open: bool,
    report_symbol: ReportSymbol,
    report_symbols: Vec<String>,
    report: Option<LoadedHistory>,
    /// The rates store read with the history, for a merge.
    report_rates: RateStore,
    /// Merge every curve into this currency; `None` reads per asset.
    merge: Option<Currency>,
    report_generation: u64,
    report_view: Option<ReportView>,
    report_layout_revision: u64,
    calendar: MonthPicker,
    report_days: DayIndex,
    report_days_key: Option<ReportDaysKey>,
    report_list_open: bool,
    ledger_scope: LedgerScope,
    ledger_symbols: Vec<String>,
    collapsed_days: BTreeSet<i64>,
    ledger_tz: TzOffset,
    ledger_pages: usize,
    history_cache: Option<LoadedHistory>,
    saved_totals: LedgerTotals,
    selected_trade: Option<usize>,
    sessions: LedgerState,
    ledger_calendar: CalendarState,
    ledger_days: Option<DayIndex>,
    ledger_layout_revision: u64,
    ledger_pnl: Option<PnlKey>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            filter: ReportFilter {
                days: DayPick::default(),
                custom_edited: false,
                custom_text: String::new(),
                period: ReportPeriod::All,
                source: SourceFilter::Real.into(),
                scope: None,
            },
            revision: 0,
            toast: None,
            report_open: false,
            report_symbol: ReportSymbol::FirstOpen,
            report_symbols: Vec::new(),
            report: None,
            report_rates: RateStore::default(),
            merge: None,
            report_generation: 0,
            report_view: None,
            report_layout_revision: 0,
            calendar: MonthPicker::default(),
            report_days: DayIndex::default(),
            report_days_key: None,
            report_list_open: true,
            ledger_scope: LedgerScope::Chart,
            ledger_symbols: Vec::new(),
            collapsed_days: BTreeSet::new(),
            ledger_tz: TzOffset::new(0),
            ledger_pages: 1,
            history_cache: None,
            saved_totals: LedgerTotals::default(),
            selected_trade: None,
            sessions: LedgerState::default(),
            ledger_calendar: CalendarState::default(),
            ledger_days: None,
            ledger_layout_revision: 0,
            ledger_pnl: None,
        }
    }
}

/// The existing ledger host actions, also used by the native ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerAction {
    Navigate(i64, i64),
    OpenTicket,
    Edit(JournalEdit),
}
#[derive(Debug, PartialEq, Eq)]
pub enum Load {
    Scoped(ReportRequest, TzOffset),
    /// The chosen symbols' history and the rates store; empty reads all.
    Report(BTreeSet<String>),
    Ledger(LedgerScope),
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Effects {
    pub load: Option<Load>,
    pub start_import: bool,
    pub toast: Option<String>,
    pub ledger: Option<LedgerAction>,
    /// A P&L choice the trader made on the report — the figure on its
    /// switch, the currency it merges in; the host applies it app-wide and
    /// persists it, as the ticket's switch does.
    pub pnl_pick: Option<PnlPick>,
}
impl Effects {
    /// Collect outputs after the local handler has completed synchronous loads.
    pub fn append_delivered(&mut self, effects: Self) {
        self.start_import |= effects.start_import;
        if effects.toast.is_some() {
            self.toast = effects.toast;
        }
        if effects.ledger.is_some() {
            self.ledger = effects.ledger;
        }
        if effects.pnl_pick.is_some() {
            self.pnl_pick = effects.pnl_pick;
        }
    }
}
/// The host moves a completed load back before dependent painting resumes.
pub struct LoadedReport {
    pub history: LoadedHistory,
    pub symbols: Vec<String>,
    pub rates: RateStore,
}
pub struct LoadedLedger {
    history: LoadedHistory,
    symbols: Vec<String>,
    totals: LedgerTotals,
    pnl: PnlKey,
}
impl LoadedLedger {
    pub fn new(history: LoadedHistory, symbols: Vec<String>, pnl: PnlReader<'_>) -> Self {
        // This model owns freshness. Do not embed another cloned book key in
        // LedgerTotals: its per-frame plus() would clone that book again.
        let totals = LedgerTotals::of(
            history
                .rows
                .iter()
                .map(|row| (pnl.of(&row.symbol), &row.trade)),
        );
        Self {
            history,
            symbols,
            totals,
            pnl: pnl.key(),
        }
    }
}

pub enum Intent {
    Scoped(Box<PreparedReport>),
    Open(String),
    Reveal(String),
    Close,
    ReloadReport,
    DirectoryChanged,
    SymbolChanged,
    JournalChanged,
    HistoryImported,
    JournalEdited,
    LoadedReport(LoadedReport),
    LoadedLedger(LoadedLedger),
    InstallReport(ReportProjection),
    InstallLedger(LedgerProjection),
    /// Choose the symbols outright; empty reads every symbol.
    Symbols(BTreeSet<String>),
    /// Tick or untick one symbol of the multi-select.
    ToggleSymbol(String),
    /// Per asset (`None`) or Merge all in a currency, as the switch and a
    /// call set it.
    Merge(Option<Currency>),
    /// The currency combo's pick: merge in it, or per asset where it is
    /// the one asset's own; the host persists it as the next default.
    MergeCurrency(Currency),
    Source(SourceSelection),
    Period(ReportPeriod),
    CustomText(String),
    SubmitCustom {
        enter: bool,
    },
    /// Replace the picked days outright: a preset, a call, a clear.
    Days(DaySet),
    /// A grid click: toggle `day`, or with `extend` (Shift) add every day
    /// from the last clicked one through it.
    PickDay {
        day: CivilDate,
        extend: bool,
    },
    /// Open or close the Dates popover; opening pages to the newest picked day.
    DatesOpen(bool),
    ShowMonth(Option<CivilDate>),
    ListOpen(bool),
    ClearFilters(String),
    Import,
    PnlPrimary(PnlPrimary),
    Toast(String),
    DrainToast,
    Ledger(LedgerIntent),
}
pub enum LedgerIntent {
    EnsureLoaded,
    Reload,
    Rescope,
    Scope(LedgerScope),
    Timezone(TzOffset),
    Pages(usize),
    ClearSelection,
    FoldDay(CivilDate, bool),
    FoldDays(DayFolds),
    View(LedgerViewChange),
    Calendar(CalendarState),
    PrepareCalendar(TzOffset),
    Session(SessionPress),
    Rows(Box<RowInput>),
    TagTarget(Option<(SessionKey, TradeRef, Vec<String>)>),
    TagText(String),
    SubmitTags,
    OpenTicket,
}

/// A single native list transaction, preserving row, session, key precedence.
#[derive(Default)]
pub struct RowInput {
    /// A session row's selection changed: `Some(index)` selects, `None`
    /// clears.
    pub select: Option<Option<usize>>,
    /// A saved row's selection changed.
    pub select_saved: Option<Option<(SessionKey, TradeRef)>>,
    /// Center the chart on a round trip.
    pub navigate: Option<LedgerAction>,
    /// Reveal one more page of saved history.
    pub reveal_more: bool,
    /// Fold (`true`) or unfold one day.
    pub fold: Option<(CivilDate, bool)>,
    /// A session row, its menu or an inline row asked for something.
    pub session: Option<SessionPress>,
    pub keyed: Option<SessionPress>,
    pub edit: Option<LedgerEdit>,
    pub focus_edit: bool,
    pub inline_placed: bool,
}

/// Opaque: only the borrowed projection helper can create this receipt.
pub struct ReportProjection {
    revision: u64,
    view: Option<Box<ReportView>>,
    days: Option<(DayIndex, Option<ReportDaysKey>)>,
}
pub struct LedgerProjection {
    revision: u64,
    cut: Option<LedgerCut>,
    pnl: Option<PnlKey>,
    totals: Option<LedgerTotals>,
}
pub struct DayFolds {
    revision: u64,
    days: BTreeSet<i64>,
}
impl State {
    /// The opening receipt applies only while its requested day is still
    /// selected. A whole-session report applies only while no dates are
    /// picked: picked dates ask about days, and one session file cut to
    /// other days is empty by construction.
    pub fn report_scope(&self) -> Option<&ReportReceipt> {
        self.filter
            .scope
            .as_ref()
            .filter(|scope| match scope.request.day() {
                Some(day) => *self.picked_days() == DaySet::single(day),
                None => {
                    !matches!(scope.request, ReportRequest::Session { .. })
                        || self.picked_days().is_empty()
                }
            })
    }
    pub fn report_scope_label(&self) -> Option<&str> {
        self.report_scope()
            .map(|scope| scope.title.as_str())
            .or_else(|| {
                self.filter
                    .scope
                    .as_ref()
                    .map(|scope| scope.request.source().label())
            })
    }

    fn clear_filters(&mut self, symbol: String) {
        self.filter.scope = None;
        self.report_days_key = None;
        self.report_symbol = ReportSymbol::Selected(chart_only(symbol));
        self.set_period(ReportPeriod::All);
        self.filter.source = SourceFilter::Real.into();
    }

    fn install_scoped(&mut self, prepared: PreparedReport) {
        let request = &prepared.receipt.request;
        self.report_open = true;
        self.report_symbol = ReportSymbol::Selected(BTreeSet::new());
        self.filter.period = ReportPeriod::All;
        self.filter.source = request.source().into();
        self.filter.custom_text.clear();
        self.filter.custom_edited = false;
        self.calendar = MonthPicker::default();
        self.set_days(DayPick::of(
            request.day().map_or_else(DaySet::default, DaySet::single),
        ));
        if let Some(day) = request.day() {
            self.calendar.month = Some(day.month_start());
        }
        self.filter.scope = Some(prepared.receipt);
        self.report = Some(prepared.history);
        self.report_symbols = prepared.symbols;
        self.report_generation = self.report_generation.wrapping_add(1);
        self.report_view = None;
    }
    /// The picked days in force; empty while the period pills are.
    pub fn picked_days(&self) -> &DaySet {
        &self.filter.days.days
    }
    fn set_days(&mut self, pick: DayPick) {
        if pick.days != self.filter.days.days {
            self.report_view = None;
        }
        self.filter.days = pick;
    }
    fn set_period(&mut self, period: ReportPeriod) {
        // A period asks about days, as picked dates do: a session or day
        // report gives way to its origin, which the source pills already
        // hold. A tag report stays, cut to the period.
        if self
            .filter
            .scope
            .as_ref()
            .is_some_and(|scope| !matches!(scope.request, ReportRequest::Tag { .. }))
        {
            self.filter.scope = None;
            self.report_days_key = None;
        }
        self.set_days(DayPick::default());
        self.filter.period = period;
        self.filter.custom_edited = false;
        self.filter.custom_text = match period {
            ReportPeriod::Custom(ms) => quantick_paper::report::fmt_period_ms(ms),
            _ => String::new(),
        };
        self.report_view = None;
    }
    pub fn report_open(&self) -> bool {
        self.report_open
    }
    /// The symbols chosen; empty reads every symbol.
    pub fn report_symbols_chosen(&self) -> &BTreeSet<String> {
        static NONE: BTreeSet<String> = BTreeSet::new();
        match &self.report_symbol {
            ReportSymbol::FirstOpen => &NONE,
            ReportSymbol::Selected(symbols) => symbols,
        }
    }
    /// The one symbol chosen, when exactly one is.
    pub fn report_symbol(&self) -> Option<&str> {
        let chosen = self.report_symbols_chosen();
        (chosen.len() == 1)
            .then(|| chosen.first().map(String::as_str))
            .flatten()
    }
    /// The multi-select's head: `All 4 symbols`, `2 of 4 symbols`.
    pub fn symbols_summary(&self) -> String {
        let (chosen, all) = (
            self.report_symbols_chosen().len(),
            self.report_symbols.len(),
        );
        if chosen == 0 {
            format!("All {all} symbols")
        } else {
            format!("{chosen} of {} symbols", all.max(chosen))
        }
    }
    /// Each symbol on offer, whether it is ticked and its label with the
    /// currency its money is in.
    pub fn symbol_choices(&self, pnl: PnlReader<'_>) -> Vec<(String, bool, String)> {
        let chosen = self.report_symbols_chosen();
        self.report_symbols
            .iter()
            .map(|symbol| {
                let currency = pnl
                    .book
                    .money(symbol)
                    .map_or("pt value unset", |money| money.currency.code());
                let label = format!("{symbol} · {currency}");
                (symbol.clone(), chosen.contains(symbol), label)
            })
            .collect()
    }
    /// The merge asked for; `None` reads per asset.
    pub fn report_merge(&self) -> Option<&Currency> {
        self.merge.as_ref()
    }
    /// What the merge controls show for the current view: the switch when
    /// the selection walks more than one curve, the currency combo in
    /// money, and what Merge all reads in when it is switched on — the
    /// last currency picked, else the chart symbol's, else USD.
    pub fn merge_controls(&self, pnl: PnlReader<'_>, chart_symbol: &str) -> MergeControls {
        let rules = pnl.settings.catalog.currencies();
        let usd = Currency::new(USD).expect("USD is a currency code");
        let Some(view) = &self.report_view else {
            return MergeControls {
                switch: None,
                currency: None,
                default: usd,
            };
        };
        let offered = &view.merge.currencies;
        let chart =
            (pnl.book.money(chart_symbol)).map(|money| rules.canonical(&money.currency).clone());
        let default = [pnl.settings.merge_currency.clone(), chart]
            .into_iter()
            .flatten()
            .find(|currency| offered.contains(currency))
            .unwrap_or(usd);
        let current = self.merge.clone().or_else(|| view.merge.own.clone());
        MergeControls {
            switch: (view.merge.per_asset_walks > 1).then_some(self.merge.is_some()),
            currency: current
                .filter(|_| view.merge.reads_money)
                .map(|current| (current, offered.clone())),
            default,
        }
    }
    pub fn report_period(&self) -> ReportPeriod {
        self.filter.period
    }
    pub fn report_source(&self) -> SourceSelection {
        self.filter.source
    }
    pub fn report_custom_text(&self) -> &str {
        &self.filter.custom_text
    }
    pub fn report_symbols(&self) -> &[String] {
        &self.report_symbols
    }
    pub fn report(&self) -> &Option<LoadedHistory> {
        &self.report
    }
    pub fn report_view(&self) -> &Option<ReportView> {
        &self.report_view
    }
    /// Why an empty report lists nothing, and the way back: the picked
    /// dates are the trader's own and are named; trades held back by the
    /// source or a report scope are counted; else the period over the symbol.
    #[must_use]
    pub fn empty_hint(&self) -> String {
        if self.filter.source == SourceSelection::default() {
            return "Select one or more sources to see their saved trades.".to_owned();
        }
        let chosen = self.report_symbols_chosen();
        let scope = if chosen.is_empty() {
            "any symbol".to_owned()
        } else {
            chosen.iter().cloned().collect::<Vec<_>>().join(", ")
        };
        let hidden = (self.report_view.as_ref()).map_or(0, |view| view.hidden_by_source_or_scope);
        if !self.picked_days().is_empty() {
            format!(
                "{scope} closed no trades on {} - pick other days, or clear \
                 the dates to go back to the period pills.",
                self.picked_days().label(),
            )
        } else if hidden > 0 {
            format!("{scope} has {hidden} trade(s) outside the selected source or report scope.")
        } else {
            format!(
                "{scope} has no trades in {}. Try \"All\", or another symbol.",
                self.filter.period.phrase(),
            )
        }
    }
    /// Whether a filter narrows the report past `symbol`, all time and no
    /// dates — whether "Clear filters" has anything to clear.
    #[must_use]
    pub fn filters_narrowed(&self, symbol: &str) -> bool {
        self.filter.source != SourceFilter::Real.into()
            || self.filter.period != ReportPeriod::All
            || *self.report_symbols_chosen() != chart_only(symbol.to_owned())
            || !self.picked_days().is_empty()
    }
    pub fn report_layout_revision(&self) -> u64 {
        self.report_layout_revision
    }
    pub fn calendar(&self) -> MonthPicker {
        self.calendar
    }
    pub fn report_days(&self) -> &DayIndex {
        &self.report_days
    }
    pub fn report_list_open(&self) -> bool {
        self.report_list_open
    }
    pub fn ledger_scope(&self) -> &LedgerScope {
        &self.ledger_scope
    }
    pub fn ledger_symbols(&self) -> &[String] {
        &self.ledger_symbols
    }
    pub fn collapsed_days(&self) -> &BTreeSet<i64> {
        &self.collapsed_days
    }
    pub fn ledger_pages(&self) -> usize {
        self.ledger_pages
    }
    pub fn history_cache(&self) -> &Option<LoadedHistory> {
        &self.history_cache
    }
    pub fn saved_totals(&self) -> &LedgerTotals {
        &self.saved_totals
    }
    pub fn selected_trade(&self) -> Option<usize> {
        self.selected_trade
    }
    pub fn sessions(&self) -> &LedgerState {
        &self.sessions
    }
    pub fn ledger_calendar(&self) -> CalendarState {
        self.ledger_calendar
    }
    pub fn ledger_days(&self) -> &Option<DayIndex> {
        &self.ledger_days
    }
    pub fn ledger_layout_revision(&self) -> u64 {
        self.ledger_layout_revision
    }

    pub fn snapshot(&self) -> Option<ReportSnapshot<'_>> {
        Some(ReportSnapshot::new(
            self.report_symbol(),
            self.report_view.as_ref()?,
        ))
    }

    /// No history or account clone on a warm frame; PnlReader compares borrowed data.
    pub fn project_report(&self, tz: TzOffset, pnl: PnlReader<'_>) -> Option<ReportProjection> {
        let days = self.picked_days();
        if self.report_view.as_ref().is_some_and(|view| {
            view.period == self.filter.period
                && view.source == self.filter.source
                && view.days == *days
                && view.tz == tz
                && view.merge.request == self.merge
                && view.worded_by(pnl)
        }) {
            return None;
        }
        let Some(history) = &self.report else {
            return Some(ReportProjection {
                revision: self.revision,
                view: None,
                days: Some((DayIndex::default(), None)),
            });
        };
        // Out of force, a scope keeps only its origin: the source pills
        // were set to it, and the day and tag reports never read one file.
        let in_force = self.report_scope().is_some();
        let admits = |by_journal: bool, row: &_| {
            self.filter.scope.as_ref().is_none_or(|scope| {
                if by_journal {
                    scope.request.admits_journal_row(row)
                } else {
                    scope.request.admits_origin(row)
                }
            })
        };
        // A whole-session report's grid is its origin's even in force: those
        // are the days a pick reports, so the grid must not hide them.
        let grid_by_journal = in_force
            && !self.filter.scope.as_ref().is_some_and(|scope| {
                matches!(scope.request, ReportRequest::Session { day: None, .. })
            });
        let key = (
            self.report_generation,
            self.filter.source,
            tz,
            grid_by_journal,
        );
        let index = (self.report_days_key != Some(key)).then(|| {
            (
                DayIndex::build(
                    history
                        .rows
                        .iter()
                        .filter(|row| self.filter.source.admits(row.source))
                        .filter(|row| admits(grid_by_journal, row))
                        .map(|row| &row.trade),
                    tz,
                ),
                Some(key),
            )
        });
        let rates = StoredRates {
            rules: pnl.settings.catalog.currencies(),
            store: &self.report_rates,
        };
        Some(ReportProjection {
            revision: self.revision,
            view: Some(Box::new(
                ReportView::cut_matching(
                    history,
                    self.filter.period,
                    self.filter.source,
                    days,
                    tz,
                    |row| admits(in_force, row),
                )
                .with_pnl(pnl)
                .merged(self.merge.clone(), pnl, &rates),
            )),
            days: index,
        })
    }

    pub fn project_ledger(
        &self,
        live: LiveCloses<'_>,
        tz: TzOffset,
        pnl: PnlReader<'_>,
    ) -> Option<LedgerProjection> {
        let cut = self
            .sessions
            .project_cut(self.history_cache.as_ref(), live, tz);
        let key = self
            .ledger_pnl
            .as_ref()
            .is_none_or(|key| !pnl.wrote(key))
            .then(|| pnl.key());
        let totals = key.as_ref().map(|_| {
            self.history_cache
                .as_ref()
                .map_or_else(LedgerTotals::default, |history| {
                    LedgerTotals::of(
                        history
                            .rows
                            .iter()
                            .map(|row| (pnl.of(&row.symbol), &row.trade)),
                    )
                })
        });
        (cut.is_some() || key.is_some()).then_some(LedgerProjection {
            revision: self.revision,
            cut,
            pnl: key,
            totals,
        })
    }

    fn foldable_ledger_days(&self, live: LiveCloses<'_>) -> BTreeSet<i64> {
        self.history_cache
            .iter()
            .flat_map(|cache| cache.rows.iter().map(|row| &row.trade))
            .chain(live.trades.iter())
            .map(|trade| CivilDate::from_ms(trade.closed_ms, self.ledger_tz).day_number())
            .collect()
    }
    pub fn all_days_collapsed(&self, live: LiveCloses<'_>) -> bool {
        let days = self.foldable_ledger_days(live);
        !days.is_empty() && days.is_subset(&self.collapsed_days)
    }
    pub fn project_day_folds(&self, expand: bool, live: LiveCloses<'_>) -> DayFolds {
        DayFolds {
            revision: self.revision,
            days: if expand {
                BTreeSet::new()
            } else {
                self.foldable_ledger_days(live)
            },
        }
    }
    fn reveal(&mut self, symbol: String) -> Effects {
        self.report_open = true;
        if matches!(self.report_symbol, ReportSymbol::FirstOpen) {
            self.report_symbol = ReportSymbol::Selected(chart_only(symbol));
        }
        self.report_load()
    }
    fn report_load(&self) -> Effects {
        Effects {
            load: Some(Load::Report(self.report_symbols_chosen().clone())),
            ..Default::default()
        }
    }
    fn ledger_load(&self) -> Effects {
        Effects {
            load: Some(Load::Ledger(self.ledger_scope.clone())),
            ..Default::default()
        }
    }
}

/// The chart's symbol alone, or every symbol before a market is chosen.
fn chart_only(symbol: String) -> BTreeSet<String> {
    (!symbol.is_empty()).then_some(symbol).into_iter().collect()
}

/// The report's merge controls for one frame; see [`State::merge_controls`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeControls {
    /// `Some(merged)`: the Per asset | Merge all switch, offered only when
    /// the selection walks more than one curve.
    pub switch: Option<bool>,
    /// The currency combo, in money only: the one in force, then the
    /// currencies on offer.
    pub currency: Option<(Currency, Vec<Currency>)>,
    /// What Merge all reads in when switched on.
    pub default: Currency,
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        let revision = state.revision;
        state.revision = state.revision.wrapping_add(1);
        match intent {
            Intent::Scoped(prepared) => state.install_scoped(*prepared),
            Intent::Reveal(symbol) => return state.reveal(symbol),
            Intent::Open(symbol) => {
                if state.filter.scope.is_some() {
                    state.clear_filters(symbol.clone());
                }
                return state.reveal(symbol);
            }
            Intent::Close => {
                state.report_open = false;
                state.calendar.open = false;
            }
            Intent::ReloadReport => return state.report_load(),
            Intent::JournalChanged => {
                state.history_cache = None;
                if state.report_open {
                    return state.report_load();
                }
            }
            Intent::DirectoryChanged | Intent::HistoryImported => {
                let directory = matches!(intent, Intent::DirectoryChanged);
                state.history_cache = None;
                state.ledger_pages = 1;
                if directory {
                    state.report = None;
                    state.report_view = None;
                }
                if state.report_open {
                    return state.report_load();
                }
            }
            Intent::SymbolChanged => {
                state.history_cache = None;
                state.selected_trade = None;
            }
            Intent::JournalEdited => {
                state.history_cache = None;
                if state.report_open {
                    return state.report_load();
                }
            }
            Intent::LoadedReport(loaded) => {
                state.report = Some(loaded.history);
                state.report_symbols = loaded.symbols;
                state.report_rates = loaded.rates;
                state.report_generation = state.report_generation.wrapping_add(1);
                state.report_view = None;
            }
            Intent::LoadedLedger(loaded) => {
                state.sessions.reloaded(&loaded.history);
                state.ledger_days = None;
                state.history_cache = Some(loaded.history);
                state.ledger_symbols = loaded.symbols;
                state.saved_totals = loaded.totals;
                state.ledger_pnl = Some(loaded.pnl);
                state.ledger_layout_revision = state.ledger_layout_revision.wrapping_add(1);
            }
            Intent::InstallReport(receipt) if receipt.revision == revision => {
                state.report_view = receipt.view.map(|view| *view);
                if let Some(snapshot) = state.snapshot() {
                    tracing::info!(target: "quantick::app", schema_version = 1_u8,
                        event_code = "PAPER_REPORT_CUT", symbol = snapshot.symbol.unwrap_or("*"),
                        source = snapshot.source.label(), window = %snapshot.window.label(),
                        trades = snapshot.rows.len(), hidden_outside = snapshot.hidden_outside,
                        hidden_by_source_or_scope = snapshot.hidden_by_source_or_scope,
                        net_points = %snapshot.report.net_points,
                        "the trading performance report was re-cut");
                }
                state.report_layout_revision = state.report_layout_revision.wrapping_add(1);
                if let Some((days, key)) = receipt.days {
                    // No history clears the day index without moving an explicitly shown month.
                    if key.is_some() && state.calendar.month.is_none() {
                        state.calendar.month = days.last().map(CivilDate::month_start);
                    }
                    state.report_days = days;
                    state.report_days_key = key;
                }
            }
            Intent::InstallLedger(receipt) if receipt.revision == revision => {
                if let Some(cut) = receipt.cut {
                    state.sessions.cut = Some(cut);
                }
                if let Some(pnl) = receipt.pnl {
                    state.ledger_pnl = Some(pnl);
                    state.ledger_layout_revision = state.ledger_layout_revision.wrapping_add(1);
                }
                if let Some(totals) = receipt.totals {
                    state.saved_totals = totals;
                }
            }
            Intent::InstallReport(_) | Intent::InstallLedger(_) => {}
            Intent::Symbols(symbols) => {
                state.report_symbol = ReportSymbol::Selected(symbols);
                return state.report_load();
            }
            Intent::ToggleSymbol(symbol) => {
                let mut symbols = state.report_symbols_chosen().clone();
                if !symbols.remove(&symbol) {
                    symbols.insert(symbol);
                }
                state.report_symbol = ReportSymbol::Selected(symbols);
                return state.report_load();
            }
            Intent::Merge(currency) => {
                state.merge = currency;
                state.report_view = None;
                // Read again: the feed side may have stored rates since.
                return state.report_load();
            }
            Intent::MergeCurrency(currency) => {
                // The one asset's own currency reads per asset, not a merge of one.
                let own = (state.report_view.as_ref()).and_then(|view| view.merge.own.clone());
                state.merge = (own.as_ref() != Some(&currency)).then(|| currency.clone());
                state.report_view = None;
                let mut effects = state.report_load();
                effects.pnl_pick = Some(PnlPick::MergeCurrency(currency));
                return effects;
            }
            Intent::Source(source) => {
                state.filter.source = source;
                state.filter.scope = None;
                state.report_days_key = None;
                state.report_view = None;
            }
            Intent::Period(period) => state.set_period(period),
            Intent::CustomText(text) => {
                state.filter.custom_text = text;
                state.filter.custom_edited = true;
            }
            Intent::SubmitCustom { enter: false } if !state.filter.custom_edited => {}
            Intent::SubmitCustom { enter } => match parse_period(&state.filter.custom_text) {
                Some(ms) => {
                    state.set_period(ReportPeriod::Custom(ms));
                }
                None if enter => {
                    state.toast = Some(format!(
                        "SIM: could not read `{}` as a period - use 45m, 12h, 2d or 1w",
                        state.filter.custom_text.trim()
                    ))
                }
                None => {}
            },
            Intent::Days(days) => state.set_days(DayPick::of(days)),
            Intent::PickDay { day, extend } => {
                let mut pick = state.filter.days.clone();
                pick.click(day, extend);
                state.set_days(pick);
            }
            Intent::DatesOpen(open) => {
                if open
                    && !state.calendar.open
                    && let Some(day) = state.picked_days().last()
                {
                    state.calendar.month = Some(day.month_start());
                }
                state.calendar.open = open;
            }
            Intent::ShowMonth(month) => state.calendar.month = month,
            Intent::ListOpen(open) => state.report_list_open = open,
            Intent::ClearFilters(symbol) => {
                state.clear_filters(symbol);
                state.report_open = true;
                return state.report_load();
            }
            Intent::Import => {
                return Effects {
                    start_import: true,
                    ..Default::default()
                };
            }
            Intent::PnlPrimary(primary) => {
                return Effects {
                    pnl_pick: Some(PnlPick::Primary(primary)),
                    ..Default::default()
                };
            }
            Intent::Toast(message) => state.toast = Some(message),
            Intent::DrainToast => {
                return Effects {
                    toast: state.toast.take(),
                    ..Default::default()
                };
            }
            Intent::Ledger(intent) => return update_ledger(state, intent, revision),
        }
        Effects::default()
    }
}

fn update_ledger(state: &mut State, intent: LedgerIntent, revision: u64) -> Effects {
    let mut effects = Effects::default();
    match intent {
        LedgerIntent::EnsureLoaded if state.history_cache.is_none() => return state.ledger_load(),
        LedgerIntent::EnsureLoaded => {}
        LedgerIntent::Reload => return state.ledger_load(),
        LedgerIntent::Rescope => {
            state.ledger_pages = 1;
            return state.ledger_load();
        }
        LedgerIntent::Scope(scope) => {
            state.ledger_scope = scope;
            state.history_cache = None;
        }
        LedgerIntent::Timezone(tz) => state.ledger_tz = tz,
        LedgerIntent::Pages(pages) => state.ledger_pages = pages.max(1),
        LedgerIntent::ClearSelection => state.selected_trade = None,
        LedgerIntent::FoldDay(day, collapsed) => {
            if collapsed {
                state.collapsed_days.insert(day.day_number());
            } else {
                state.collapsed_days.remove(&day.day_number());
            }
        }
        LedgerIntent::FoldDays(receipt) if receipt.revision == revision => {
            state.collapsed_days = receipt.days
        }
        LedgerIntent::FoldDays(_) => {}
        LedgerIntent::View(change) => {
            if change.clear || change.range.is_some() {
                state.ledger_calendar.selection = DaySelection::None;
            }
            state.sessions.view.apply(&change);
        }
        LedgerIntent::Calendar(calendar) => {
            if state.ledger_calendar.selection != calendar.selection {
                state.sessions.view.filter.range = calendar.selection.range();
            }
            state.ledger_calendar = calendar;
        }
        LedgerIntent::PrepareCalendar(tz) => {
            if state.ledger_days.is_none() {
                state.ledger_days = Some(
                    state
                        .history_cache
                        .as_ref()
                        .map_or_else(DayIndex::default, |history| {
                            DayIndex::build(history.rows.iter().map(|row| &row.trade), tz)
                        }),
                );
            }
        }
        LedgerIntent::Session(SessionPress::Report(request)) => {
            effects.load = Some(Load::Scoped(request, state.ledger_tz));
        }
        LedgerIntent::Session(press) => {
            effects.ledger = state
                .sessions
                .apply(press, state.history_cache.as_ref())
                .map(LedgerAction::Edit);
        }
        LedgerIntent::Rows(mut input) => {
            if let Some(SessionPress::Report(request)) = input.session.as_ref() {
                effects.load = Some(Load::Scoped(request.clone(), state.ledger_tz));
                input.session = None;
            }
            effects.ledger = apply_rows(state, *input);
        }
        LedgerIntent::TagTarget(target) => match target {
            None => state.sessions.tag_text = None,
            Some((session, trade, tags)) => {
                if state
                    .sessions
                    .tag_text
                    .as_ref()
                    .is_none_or(|(key, at, _)| *key != session || *at != trade)
                {
                    state.sessions.tag_text = Some((
                        session,
                        trade,
                        tags.iter()
                            .map(|tag| format!("#{tag}"))
                            .collect::<Vec<_>>()
                            .join(" "),
                    ));
                }
            }
        },
        LedgerIntent::TagText(text) => {
            if let Some((_, _, held)) = &mut state.sessions.tag_text {
                *held = text;
            }
        }
        LedgerIntent::SubmitTags => {
            if let Some((session, trade, text)) = &state.sessions.tag_text {
                effects.ledger = Some(LedgerAction::Edit(JournalEdit::Tag {
                    session: session.clone(),
                    trade: *trade,
                    tags: text.split_whitespace().map(str::to_owned).collect(),
                }));
            }
        }
        LedgerIntent::OpenTicket => effects.ledger = Some(LedgerAction::OpenTicket),
    }
    effects
}

fn apply_rows(state: &mut State, input: RowInput) -> Option<LedgerAction> {
    state.sessions.edit = input.edit;
    state.sessions.focus_edit = input.focus_edit;
    state.sessions.edit_placed(input.inline_placed);
    if let Some(selection) = input.select {
        state.selected_trade = selection;
        if selection.is_some() {
            state.sessions.selected_saved = None;
            state.sessions.trade_selected();
        }
    }
    if let Some(selection) = input.select_saved {
        if selection.is_some() {
            state.selected_trade = None;
            state.sessions.trade_selected();
        }
        state.sessions.selected_saved = selection;
    }
    let row_action = input
        .session
        .and_then(|press| state.sessions.apply(press, state.history_cache.as_ref()))
        .map(LedgerAction::Edit);
    let action = if row_action.is_some() {
        row_action
    } else {
        if input.reveal_more {
            state.ledger_pages = state.ledger_pages.saturating_add(1);
        }
        if let Some((day, collapsed)) = input.fold {
            if collapsed {
                state.collapsed_days.insert(day.day_number());
            } else {
                state.collapsed_days.remove(&day.day_number());
            }
        }
        input.navigate
    };
    // The key transition still runs when a row action wins, as before.
    let keyed = input
        .keyed
        .and_then(|press| state.sessions.apply(press, state.history_cache.as_ref()))
        .map(LedgerAction::Edit);
    action.or(keyed)
}

#[cfg(test)]
mod tests;
