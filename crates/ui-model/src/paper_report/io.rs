//! Synchronous journal loads shared by native and headless report hosts.
use quantick_paper::report::snapshot::{ReportChange, ReportEnv, ReportReadout};
use quantick_paper::report::{journals, scope};

use super::{Effects, Intent, Load, LoadedLedger, LoadedReport};

pub fn report(
    updates: &mut crate::UpdateSession<'_, super::Model>,
    change: Option<ReportChange>,
    tz: quantick_civil::TzOffset,
    env: &ReportEnv<'_>,
) -> Result<ReportReadout, &'static str> {
    if let Some(change) = change {
        let reload = matches!(change, ReportChange::Sources(_));
        let intent = match change {
            ReportChange::Sources(sources) => Intent::Source(sources),
            ReportChange::Draft(draft) => Intent::Draft(draft),
            ReportChange::Generate => {
                updates.state().date_draft().range()?;
                Intent::GenerateDates
            }
            ReportChange::ClearDates => Intent::Dates(super::DaySelection::None),
            ReportChange::Period(period) => Intent::Period(period),
        };
        updates.update(intent);
        if reload || !updates.state().report_open() || updates.state().report().is_none() {
            updates.update(Intent::Reveal(env.symbol.to_owned()));
        }
    }
    let state = updates.state();
    // An inspection immediately after a filter change may need a fresh cut.
    // Borrow it for this answer without installing it or touching the window.
    let projection = state.project_report(tz, env.pnl);
    let view = projection
        .as_ref()
        .and_then(|receipt| receipt.view.as_deref())
        .or(state.report_view().as_ref());
    Ok(ReportReadout {
        draft: state.date_draft().clone(),
        applied: state.applied_dates(),
        open: state.report_open(),
        loaded: state.report().is_some(),
        source: state.report_source(),
        symbol: state.report_symbol().clone(),
        window: state
            .applied_dates()
            .map_or_else(|| state.report_period().phrase(), |range| range.label()),
        scope: state.report_scope_label().map(str::to_owned),
        trade_count: view.map_or(0, |view| view.rows.len()),
        unreadable_files: state
            .report()
            .as_ref()
            .map_or(0, |history| history.unreadable_files),
        skipped_rows: state
            .report()
            .as_ref()
            .map_or(0, |history| history.problem_rows),
        net_points: view.map_or_else(Default::default, |view| view.report.net_points),
    })
}

pub fn local_effects(mut effects: Effects, env: &ReportEnv<'_>) -> (Effects, Option<Intent>) {
    let receipt = effects.load.take().map(|load| match load {
        Load::Scoped(request, tz) => match scope::prepare(env.dir, request, tz) {
            Ok(prepared) => Intent::Scoped(Box::new(prepared)),
            Err(error) => Intent::Toast(format!("Report: {error}")),
        },
        Load::Report(symbol) => Intent::LoadedReport(LoadedReport {
            history: journals::load(env.dir, symbol.as_deref(), &[]),
            symbols: journals::symbols(env.dir),
        }),
        Load::Ledger(scope) => Intent::LoadedLedger(LoadedLedger::new(
            journals::load(env.dir, scope.folder(env.symbol), env.session_journal_paths),
            journals::symbols(env.dir),
            env.pnl,
        )),
    });
    (effects, receipt)
}
