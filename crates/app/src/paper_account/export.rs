//! Getting trades in and out of the journal folder: the host's half.
//!
//! The import and export dialogs run off the UI thread and answer through a
//! channel, and an export file is stamped from the wall clock — the three
//! things the headless account may not do. What they carry is the account's:
//! the rows it closed this session, the CSV it spells them in, and the
//! journal home it copies into.

use quantick_paper::account::{export_csv, utc_compact};

use super::{PaperAccount, elide_path};
use crate::paper_report::HistoryRow;

impl PaperAccount {
    // ------------------------------------------------------------------
    // Import
    // ------------------------------------------------------------------

    /// Ask for a folder whose trades should be copied into the journal
    /// home, off the UI thread — the manual way in for legacy folders the
    /// startup consolidation cannot reach (an old working directory, a
    /// backup). One dialog at a time.
    pub(crate) fn start_import(&mut self) {
        if self.import_rx.is_some() {
            self.set_toast("SIM: an import is already running.".to_owned());
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let start = self.trades_dir().to_path_buf();
        std::thread::Builder::new()
            .name("quantick-trades-import-picker".into())
            .spawn(move || {
                let mut dialog =
                    rfd::FileDialog::new().set_title("Import trades from a folder (copies)");
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_folder());
            })
            .expect("spawn trades-import picker thread");
        self.import_rx = Some(receiver);
    }

    /// Land the picked folder: copy its history into the journal home and
    /// re-read, so the report answers with the merged truth.
    pub(crate) fn poll_import(&mut self) {
        let Some(receiver) = &self.import_rx else {
            return;
        };
        let Ok(choice) = receiver.try_recv() else {
            return;
        };
        self.import_rx = None;
        let Some(source) = choice else { return };
        let summary = crate::paper_home::consolidate_into(self.trades_dir(), &[source]);
        self.set_toast(crate::paper_home::import_toast(&summary));
        let (report, env) = self.report_parts();
        report.history_imported(&env);
    }

    // ------------------------------------------------------------------
    // Export
    // ------------------------------------------------------------------

    /// Write everything the ledger lists (this session plus the saved
    /// history, in the ledger's scope) to one CSV, off the UI thread. The
    /// toast answers with the path or the failure.
    pub(crate) fn start_export(&mut self) {
        if self.export_rx.is_some() {
            self.set_toast("SIM: an export is already running.".to_owned());
            return;
        }
        // The saved half of the export, loaded if the ledger has not been
        // drawn yet: an export that silently skipped it because nobody had
        // opened the tab would write a history that is missing most of
        // itself. Cloned out before the session's own rows are appended,
        // since gathering those borrows this host again.
        let mut rows: Vec<HistoryRow> = {
            let (report, env) = self.report_parts();
            report.saved_rows(&env).to_vec()
        };
        rows.extend(self.session_history_rows());
        rows.sort_by_key(|row| (row.trade.closed_ms, row.trade.opened_ms));
        if rows.is_empty() {
            self.set_toast("SIM: nothing to export yet - close a trade first.".to_owned());
            return;
        }
        let text = export_csv(&rows);
        let count = rows.len();
        // The one wall-clock read on this path, and it names a file rather
        // than deciding anything: no venue time says when the trader pressed
        // the button, and the headless account may not ask a clock.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(0))
            .unwrap_or(0);
        let dir = self.trades_dir().to_path_buf();
        let path = dir.join(format!("export-{}.csv", utc_compact(stamp)));
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::write(&path, text))
                .map(|()| (path, count))
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.export_rx = Some(receiver);
    }

    /// Land the export's result, if it arrived.
    pub(crate) fn poll_export(&mut self) {
        let Some(receiver) = &self.export_rx else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.export_rx = None;
        match result {
            Ok((path, count)) => {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PAPER_TRADES_EXPORTED",
                    path = %path.display(),
                    trades = count,
                    "exported the simulated trade history"
                );
                self.set_toast(format!("Exported {count} trades to {}", elide_path(&path)));
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PAPER_TRADES_EXPORT_FAILED",
                    %error,
                    action = "export_not_saved",
                    "could not write the trade export"
                );
                self.set_toast(
                    "SIM: could not write the export - see the log for the path.".to_owned(),
                );
            }
        }
    }
}
