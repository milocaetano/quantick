//! Synchronous journal loads shared by native and headless report hosts.
use quantick_paper::report::snapshot::ReportEnv;
use quantick_paper::report::{journals, scope};

use super::{Effects, Intent, Load, LoadedLedger, LoadedReport};

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
