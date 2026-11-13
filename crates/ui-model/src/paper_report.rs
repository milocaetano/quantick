//! Per-account report and ledger presentation, composed by the Paper surface.
//! IO returns moved terminal receipts; projections borrow the real account facts.
use std::collections::BTreeSet;

use quantick_civil::{CivilDate, TzOffset};
use quantick_paper::pnl::{PnlKey, PnlReader};
use quantick_paper::report::calendar::{CalendarState, DayIndex, DaySelection};
use quantick_paper::report::ledger_rows::LedgerScope;
use quantick_paper::report::ledger_state::{
    LedgerCut, LedgerEdit, LedgerState, LedgerViewChange, LiveCloses, SessionPress,
};
use quantick_paper::report::readout::LedgerTotals;
use quantick_paper::report::session_edit::JournalEdit;
use quantick_paper::report::sessions::{SessionKey, TradeRef};
use quantick_paper::report::snapshot::ReportSnapshot;
use quantick_paper::report::{LoadedHistory, ReportPeriod, ReportView, SourceFilter, parse_period};

type ReportDaysKey = (u64, SourceFilter, TzOffset);

/// Cached presentation can be read by a host, but changes enter as intents.
///
/// Filter writes cannot bypass the revision that protects projection receipts.
/// ```compile_fail,E0616
/// use quantick_ui_model::paper_report::State;
/// use quantick_paper::report::SourceFilter;
/// let mut state = State::default();
/// state.report_source = SourceFilter::Replay;
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
    revision: u64,
    toast: Option<String>,
    report_open: bool,
    report_symbol: Option<String>,
    report_period: ReportPeriod,
    report_source: SourceFilter,
    report_custom_text: String,
    report_symbols: Vec<String>,
    report: Option<LoadedHistory>,
    report_generation: u64,
    report_view: Option<ReportView>,
    report_layout_revision: u64,
    calendar: CalendarState,
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
            revision: 0,
            toast: None,
            report_open: false,
            report_symbol: None,
            report_period: ReportPeriod::All,
            report_source: SourceFilter::Real,
            report_custom_text: String::new(),
            report_symbols: Vec::new(),
            report: None,
            report_generation: 0,
            report_view: None,
            report_layout_revision: 0,
            calendar: CalendarState::default(),
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
    Report(Option<String>),
    Ledger(LedgerScope),
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Effects {
    pub load: Option<Load>,
    pub start_import: bool,
    pub toast: Option<String>,
    pub ledger: Option<LedgerAction>,
}
/// The host moves a completed load back before dependent painting resumes.
pub struct LoadedReport {
    pub history: LoadedHistory,
    pub symbols: Vec<String>,
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
    Open(String),
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
    Symbol(Option<String>),
    Source(SourceFilter),
    Period(ReportPeriod),
    CustomText(String),
    SubmitCustom { enter: bool },
    Dates(DaySelection),
    Calendar(CalendarState),
    ListOpen(bool),
    ClearFilters(String),
    Import,
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
    pub select: Option<Option<usize>>,
    pub select_saved: Option<Option<(SessionKey, TradeRef)>>,
    pub navigate: Option<LedgerAction>,
    pub reveal_more: bool,
    pub fold: Option<(CivilDate, bool)>,
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
    pub fn report_open(&self) -> bool {
        self.report_open
    }
    pub fn report_symbol(&self) -> &Option<String> {
        &self.report_symbol
    }
    pub fn report_period(&self) -> ReportPeriod {
        self.report_period
    }
    pub fn report_source(&self) -> SourceFilter {
        self.report_source
    }
    pub fn report_custom_text(&self) -> &str {
        &self.report_custom_text
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
    pub fn report_layout_revision(&self) -> u64 {
        self.report_layout_revision
    }
    pub fn calendar(&self) -> CalendarState {
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
            self.report_symbol.as_deref(),
            self.report_view.as_ref()?,
        ))
    }

    /// No history or account clone on a warm frame; PnlReader compares borrowed data.
    pub fn project_report(&self, tz: TzOffset, pnl: PnlReader<'_>) -> Option<ReportProjection> {
        let range = self.calendar.selection.range();
        if self.report_view.as_ref().is_some_and(|view| {
            view.period == self.report_period
                && view.source == self.report_source
                && view.range == range
                && view.tz == tz
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
        let key = (self.report_generation, self.report_source, tz);
        let days = (self.report_days_key != Some(key)).then(|| {
            (
                DayIndex::build(
                    history
                        .rows
                        .iter()
                        .filter(|row| self.report_source.admits(row.source))
                        .map(|row| &row.trade),
                    tz,
                ),
                Some(key),
            )
        });
        Some(ReportProjection {
            revision: self.revision,
            view: Some(Box::new(
                ReportView::cut(history, self.report_period, self.report_source, range, tz)
                    .with_pnl(pnl),
            )),
            days,
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
    fn report_load(&self) -> Effects {
        Effects {
            load: Some(Load::Report(self.report_symbol.clone())),
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

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        let revision = state.revision;
        state.revision = state.revision.wrapping_add(1);
        match intent {
            Intent::Open(symbol) => {
                state.report_open = true;
                if state.report.is_none() && !symbol.is_empty() {
                    state.report_symbol = Some(symbol);
                }
                return state.report_load();
            }
            Intent::Close => state.report_open = false,
            Intent::ReloadReport | Intent::JournalChanged => return state.report_load(),
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
                state.report_layout_revision = state.report_layout_revision.wrapping_add(1);
                if let Some((days, key)) = receipt.days {
                    // No history clears the day index without moving an explicitly shown month.
                    if key.is_some() {
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
            Intent::Symbol(symbol) => state.report_symbol = symbol,
            Intent::Source(source) => {
                state.report_source = source;
                state.report_view = None;
            }
            Intent::Period(period) => {
                state.calendar.selection = DaySelection::None;
                state.report_period = period;
                state.report_view = None;
            }
            Intent::CustomText(text) => state.report_custom_text = text,
            Intent::SubmitCustom { enter } => match parse_period(&state.report_custom_text) {
                Some(ms) => {
                    state.report_period = ReportPeriod::Custom(ms);
                    state.report_view = None;
                }
                None if enter => {
                    state.toast = Some(format!(
                        "SIM: could not read `{}` as a period - use 45m, 12h, 2d or 1w",
                        state.report_custom_text.trim()
                    ))
                }
                None => {}
            },
            Intent::Dates(selection) => {
                state.calendar.selection = selection;
                state.report_view = None;
            }
            Intent::Calendar(calendar) => {
                if state.calendar.selection != calendar.selection {
                    state.report_view = None;
                }
                state.calendar = calendar;
            }
            Intent::ListOpen(open) => state.report_list_open = open,
            Intent::ClearFilters(symbol) => {
                state.report_symbol = (!symbol.is_empty()).then_some(symbol);
                state.report_period = ReportPeriod::All;
                state.report_source = SourceFilter::Real;
                state.calendar.selection = DaySelection::None;
                state.report_view = None;
            }
            Intent::Import => {
                return Effects {
                    start_import: true,
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
        LedgerIntent::Session(press) => {
            effects.ledger = state
                .sessions
                .apply(press, state.history_cache.as_ref())
                .map(LedgerAction::Edit);
        }
        LedgerIntent::Rows(input) => effects.ledger = apply_rows(state, *input),
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
