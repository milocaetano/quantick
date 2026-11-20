//! Synchronous journal loads shared by native and headless report hosts.
use quantick_paper::report::snapshot::ReportEnv;
use quantick_paper::report::{journals, scope};

use super::{Effects, Intent, Load, LoadedLedger, LoadedReport};

pub fn sources(
    updates: &mut crate::UpdateSession<'_, super::Model>,
    sources: Option<quantick_paper::report::SourceSelection>,
    tz: quantick_civil::TzOffset,
    env: &ReportEnv<'_>,
) -> quantick_paper::report::snapshot::ReportReadout {
    if let Some(sources) = sources {
        updates.update(Intent::Source(sources));
        updates.update(Intent::Open(env.symbol.to_owned()));
    }
    if let Some(receipt) = updates.state().project_report(tz, env.pnl) {
        updates.update(Intent::InstallReport(receipt));
    }
    let state = updates.state();
    let snapshot = state.snapshot();
    quantick_paper::report::snapshot::ReportReadout {
        open: state.report_open(),
        loaded: state.report().is_some(),
        source: state.report_source(),
        symbol: state.report_symbol().clone(),
        window: snapshot.as_ref().map_or_else(
            || state.report_period().phrase(),
            |snapshot| snapshot.window.label(),
        ),
        scope: state.report_scope().map(|scope| scope.title.clone()),
        trade_count: snapshot.as_ref().map_or(0, |snapshot| snapshot.rows.len()),
        unreadable_files: state
            .report()
            .as_ref()
            .map_or(0, |history| history.unreadable_files),
        skipped_rows: state
            .report()
            .as_ref()
            .map_or(0, |history| history.problem_rows),
        net_points: snapshot.map_or_else(Default::default, |snapshot| snapshot.report.net_points),
    }
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
