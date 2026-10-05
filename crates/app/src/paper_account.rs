//! The paper account's host: what the headless account cannot hold.
//!
//! The account itself — orders, fills, settlement, risk sizing, the journal
//! and the report numbers — is `quantick_paper::PaperAccount`, in a crate
//! below this one so the backtest and a bot drive the same money path the
//! chart does. What stays here is what that crate may not do: it reads no
//! environment, opens no dialog, runs no thread, reads no clock and holds no
//! window state. So this host owns
//!
//! - the two `QUANTICK_PAPER_*` launch hooks, read here and *told* to the
//!   account;
//! - the performance report and the trades ledger's window state, re-read
//!   the moment the account says a close reached the journal;
//! - the import folder picker and the export writer, which need a thread,
//!   and the export's file stamp, which needs the wall clock.
//!
//! The cmd-trading settings and the armed chart click are the desk's
//! (`quantick_paper::desk`), beside the ticket they belong to.
//!
//! It dereferences to the account, so every caller — the ticket, the dock,
//! the control plane — keeps asking `account().place_intent(..)` and the
//! rest of the money path by the names it already used. Only `on_trade` and
//! `handle_events` re-read the report at once; a close any other core call
//! journals waits for `settle`, which the frame runs before the report window
//! paints. So the painted report never lags the journal it reads.

use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use quantick_engine::Trade;
use quantick_paper::PaperAccount as Core;
use quantick_sim::VenueEvent;

mod export;
mod report;

pub(crate) use quantick_paper::account::{AccountEnv, Leg, elide_path, side_word};

/// `=1` runs a fixed sequence of ordinary sim commands driven by print
/// count, so a screenshot or demo run shows every trading surface without
/// a click. The trades are as real as any simulated trade (journaled,
/// listed, painted); point `QUANTICK_TRADES_DIR` somewhere scratch to keep
/// a demo out of your journal.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_DEMO_ENV: &str = "QUANTICK_PAPER_DEMO";

/// The risk per trade standing up on the first frame, so the derived size,
/// the line under it and the refusal the lock produces are all reachable
/// without a hand. Declared here because the risk is the account's.
#[cfg(any(feature = "scenario-harness", test))]
const PAPER_RISK_ENV: &str = "QUANTICK_PAPER_RISK";

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_PAPER_DEMO", "QUANTICK_PAPER_RISK"];

/// The paper account, hosted: the headless account and the window-side state
/// that travels with it.
pub(crate) struct PaperAccount {
    /// The money path. Reached through `Deref`, so its methods read as this
    /// host's own.
    core: Core,
    /// The performance report and the trades ledger - the reading half of
    /// paper trading, which owns its own state (`paper_report`). Its numbers
    /// are cut in `quantick_paper::report`; this is the window around them.
    pub(crate) report: crate::paper_report::ReportState,
    /// The in-flight export, if any; resolved by [`Self::settle`]'s poll.
    export_rx: Option<std::sync::mpsc::Receiver<Result<(PathBuf, usize), String>>>,
    /// The in-flight history-folder import, if any; resolved by
    /// [`Self::settle`]'s poll. Imports copy — the picked folder keeps its
    /// files.
    import_rx: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
}

impl Deref for PaperAccount {
    type Target = Core;

    fn deref(&self) -> &Core {
        &self.core
    }
}

impl DerefMut for PaperAccount {
    fn deref_mut(&mut self) -> &mut Core {
        &mut self.core
    }
}

impl PaperAccount {
    /// An account journaling to `dir`, already resolved from config and
    /// environment, with this run's launch hooks applied.
    #[must_use]
    pub(crate) fn with_trades_dir(dir: PathBuf) -> Self {
        let core = Core::with_trades_dir(dir);
        #[cfg(any(feature = "scenario-harness", test))]
        let core = apply_launch_hooks(core);
        Self {
            core,
            report: crate::paper_report::ReportState::default(),
            export_rx: None,
            import_rx: None,
        }
    }
}

/// The account's capture hooks — the risk standing up and the scripted demo.
/// Compiled only with the scenario harness (or under test).
#[cfg(any(feature = "scenario-harness", test))]
fn apply_launch_hooks(mut core: Core) -> Core {
    // A spec that does not parse is reported and ignored, never
    // defaulted: a capture run that silently got a different risk than
    // it asked for photographs the wrong thing and says nothing about
    // it. The rule `QUANTICK_PAPER_ORDERS` already follows.
    let hook = crate::hooks::captured::var(PAPER_RISK_ENV).and_then(|value| {
        crate::risk_sizing::parse_hook(&value).or_else(|| {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_RISK_HOOK_REJECTED",
                value = %value,
                action = "risk_left_off",
                "QUANTICK_PAPER_RISK wants `<amount>` or `<percent>%@<capital>`, optionally \
                 `:<point value>:<size step>:<currency>` and `:unlocked`"
            );
            None
        })
    });
    if let Some(hook) = hook {
        core = core.with_risk_hook(hook);
    }
    if crate::hooks::captured::var(PAPER_DEMO_ENV).is_some_and(|value| value == "1") {
        core = core.with_demo();
    }
    core
}

impl PaperAccount {
    /// The journal folder for this run: the environment override wins (an
    /// env var is an explicit request for one run, like every autostart
    /// hook), then `stored` — the folder the user last picked with the
    /// panel's button — then `configured`, the `[paper] trades_dir` key
    /// when the config carries one, and finally the documents home
    /// ([`crate::paper_home::default_trades_dir`]).
    #[must_use]
    pub fn resolve_trades_dir(configured: Option<&str>, stored: Option<&str>) -> PathBuf {
        crate::paper_home::resolve(configured, stored)
    }

    /// Drain whatever the export and import threads finished, once a frame
    /// before the report window paints, and re-read the report if a close
    /// reached the journal through a core call this host does not shadow —
    /// `place_intent` or a strategy's own command, say.
    pub(crate) fn settle(&mut self) {
        self.poll_export();
        self.poll_import();
        self.follow_journal();
    }

    /// Feed one live print through the simulator and act on what it did.
    pub(crate) fn on_trade(&mut self, trade: &Trade) {
        self.core.on_trade(trade);
        self.follow_journal();
    }

    /// Everything the simulator reported, through the account's one funnel
    /// (journal, acknowledgements, the bot buffer), and then the report.
    pub(crate) fn handle_events(&mut self, events: Vec<VenueEvent>) {
        self.core.handle_events(events);
        self.follow_journal();
    }

    /// Re-read the open report after a journaled close.
    ///
    /// The report reads from disk and the close just wrote to disk; re-read
    /// now or the window shows yesterday until the manual refresh - the "my
    /// trade is missing" report. Guarded rather than always gathered: this
    /// is the per-trade path, and building a `ReportEnv` for a window nobody
    /// has open is work a dense tape pays on every single close.
    fn follow_journal(&mut self) {
        if self.core.take_journal_changed() && self.report.is_open() {
            let (report, env) = self.report_parts();
            report.journal_changed(&env);
        }
    }

    /// Point the account at an instrument. `None` when nothing changed,
    /// `Some(arriving)` when it did — see `quantick_paper`'s own
    /// `set_symbol` for why the two are told apart.
    pub(crate) fn set_symbol(&mut self, symbol: &str) -> Option<bool> {
        let arriving = self.core.set_symbol(symbol)?;
        self.report.symbol_changed();
        // The revealed page is deliberately left alone: resetting it here
        // retired the `QUANTICK_LEDGER_PAGES` hook before the first row was
        // painted, and would retire a trader's scroll-back as silently.
        // `LedgerPage::of` already clamps a page the new history cannot back.
        Some(arriving)
    }

    /// Point the journal somewhere new, in-session — the panel's folder
    /// picker. Files already written stay exactly where they are; the next
    /// close opens a new session file under the new folder, and the ledger
    /// and report re-read from the new home.
    pub(crate) fn set_trades_dir(&mut self, dir: PathBuf) {
        if self.core.set_trades_dir(dir) {
            let (report, env) = self.report_parts();
            report.trades_dir_changed(&env);
        }
    }

    /// Apply a raw simulator command through the normal event funnel
    /// (tests only) — for arranging sim state the UI would need gestures
    /// to reach.
    #[cfg(test)]
    pub(crate) fn apply_sim_command_for_tests(&mut self, command: quantick_sim::Command) {
        let events = self.core.dispatch(command);
        self.handle_events(events);
    }

    /// Index (into the session's closed trades) of the ledger's selected
    /// trade, for the chart to emphasize; `None` while nothing is selected.
    #[must_use]
    pub(crate) fn selected_trade_index(&self) -> Option<usize> {
        self.report
            .selected_trade()
            .filter(|index| *index < self.core.session_trades().len())
    }
}
