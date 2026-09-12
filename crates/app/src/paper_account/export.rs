//! Getting trades in and out of the journal folder.
//!
//! The import and export dialogs run off the UI thread and answer through a
//! channel, so both halves are here beside the CSV they write and the
//! session file the journal appends to. `handle_events` stays with the
//! account itself: it is the fill path, and only its last step lands here.

use std::path::{Path, PathBuf};

use quantick_engine::Side;
use quantick_sim::{ClosedTrade, history};
use rust_decimal::Decimal;

use super::{PaperAccount, account_env, utc_compact};
use crate::paper_calendar::civil_utc;
use crate::paper_chrome::{fmt_decimal, sanitize_symbol};
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
        let start = self.dir.clone();
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
        let summary = crate::paper_home::consolidate_into(&self.dir, &[source]);
        self.set_toast(crate::paper_home::import_toast(&summary));
        let env = account_env!(self);
        self.report.history_imported(&env);
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
            let env = account_env!(self);
            self.report.saved_rows(&env).to_vec()
        };
        rows.extend(
            self.venue
                .closed_trades()
                .iter()
                .enumerate()
                .map(|(index, trade)| HistoryRow {
                    symbol: self.symbol.clone(),
                    // The source the trade actually closed under — the
                    // session may have flipped live/replay since.
                    source: Some(
                        self.session_trade_sources
                            .get(index)
                            .copied()
                            .unwrap_or(self.session_source),
                    ),
                    trade: trade.clone(),
                }),
        );
        rows.sort_by_key(|row| (row.trade.closed_ms, row.trade.opened_ms));
        if rows.is_empty() {
            self.set_toast("SIM: nothing to export yet - close a trade first.".to_owned());
            return;
        }
        let text = export_csv(&rows);
        let count = rows.len();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(0))
            .unwrap_or(0);
        let dir = self.dir.clone();
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

    /// Append one closed trade to the session's history file, creating the
    /// file (with its header) on the first close. Also records the source
    /// the trade closed under, for the export. Returns whether the write
    /// landed — a failed write warns once and never crashes a trading
    /// session, but its caller must not paint a healthy toast over the
    /// warning.
    pub(crate) fn journal(&mut self, trade: &ClosedTrade) -> bool {
        self.session_trade_sources.push(self.session_source);
        if self.symbol.is_empty() {
            return false;
        }
        let folder = self.dir.join(sanitize_symbol(&self.symbol));
        let path = self
            .journal_path
            .get_or_insert_with(|| free_session_path(&folder, &utc_compact(trade.closed_ms)));
        if self.session_journal_paths.last() != Some(path) {
            self.session_journal_paths.push(path.clone());
        }
        let mut text = String::new();
        if !path.exists() {
            text.push_str(&history::write_header(&self.symbol, self.session_source));
        }
        text.push_str(&history::write_trade(trade));
        let written = std::fs::create_dir_all(&folder).and_then(|()| {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&*path)
                .and_then(|mut file| file.write_all(text.as_bytes()))
        });
        if let Err(error) = &written
            && !self.journal_warned
        {
            self.journal_warned = true;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_TRADE_JOURNAL_FAILED",
                path = %path.display(),
                %error,
                action = "trade_not_saved",
                "could not append to the paper-trading history"
            );
            self.set_toast(
                "SIM: could not save the trade history - see the log for the path.".to_owned(),
            );
        }
        written.is_ok()
    }
}
/// A toast-sized path: printed whole when short, elided to `…/file.csv`
/// past the limit — the file name always stays whole.
pub(crate) fn elide_path(path: &Path) -> String {
    let text = path.display().to_string();
    if text.chars().count() <= EXPORT_PATH_ELIDE_CHARS {
        return text;
    }
    path.file_name().map_or(text, |name| {
        format!("…{}{}", std::path::MAIN_SEPARATOR, name.to_string_lossy())
    })
}

/// The export CSV: one merged, Excel-facing artifact — the journal stays
/// the machine-readable source of truth. Human-readable UTC stamps ride
/// beside the venue epoch, decimals always use `.`, and the running
/// equity is a column so a spreadsheet shows it without a formula.
pub(crate) fn export_csv(rows: &[HistoryRow]) -> String {
    let mut text = String::from(
        "symbol,side,quantity,opened_ms,opened_utc,entry_price,closed_ms,closed_utc,\
         exit_price,pnl_points,cum_pnl_points,duration_ms,exit_reason,entry_agg_id,\
         exit_agg_id,mae_points,mfe_points,source\n",
    );
    let mut cumulative = Decimal::ZERO;
    let opt_u64 = |value: Option<u64>| value.map(|value| value.to_string()).unwrap_or_default();
    let opt_points = |value: Option<Decimal>| value.map(fmt_decimal).unwrap_or_default();
    for row in rows {
        let trade = &row.trade;
        cumulative = cumulative.saturating_add(trade.pnl_points);
        let side = match trade.side {
            Side::Buy => "long",
            Side::Sell => "short",
        };
        text.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            row.symbol.replace(',', "_"),
            side,
            fmt_decimal(trade.quantity),
            trade.opened_ms,
            fmt_utc_iso(trade.opened_ms),
            fmt_decimal(trade.entry_price),
            trade.closed_ms,
            fmt_utc_iso(trade.closed_ms),
            fmt_decimal(trade.exit_price),
            fmt_decimal(trade.pnl_points),
            fmt_decimal(cumulative),
            trade.closed_ms.saturating_sub(trade.opened_ms).max(0),
            trade.exit_reason.as_str(),
            opt_u64(trade.entry_agg_id),
            opt_u64(trade.exit_agg_id),
            opt_points(trade.mae_points),
            opt_points(trade.mfe_points),
            // Empty when the file never recorded one — unknown, not live.
            row.source.map(history::SessionSource::as_str).unwrap_or(""),
        ));
    }
    text
}

/// The session file for `stamp` under `folder`: the plain name when free,
/// else `stamp.rerun-N` — file names derive from venue time, so replaying
/// the same recording twice reproduces the same stamp, and the second run
/// must land beside the first instead of appending duplicate trades into
/// it.
pub(crate) fn free_session_path(folder: &Path, stamp: &str) -> PathBuf {
    let plain = folder.join(format!("{stamp}.{}", history::FILE_EXTENSION));
    if !plain.exists() {
        return plain;
    }
    for rerun in 1..=MAX_SESSION_RERUNS {
        let candidate = folder.join(format!("{stamp}.rerun-{rerun}.{}", history::FILE_EXTENSION));
        if !candidate.exists() {
            return candidate;
        }
    }
    // A folder with a thousand same-stamp sessions is not a real journal;
    // appending to the plain file keeps the trades at the cost of
    // duplicates, which beats losing them.
    plain
}

/// `YYYY-MM-DDTHH:MM:SSZ` in UTC — the export's human-readable stamp.
fn fmt_utc_iso(timestamp_ms: i64) -> String {
    let (year, month, day, hour, minute, second) = civil_utc(timestamp_ms);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// How many `.rerun-N` session names one venue-time stamp may try — far
/// beyond any real journal, a backstop against a pathological folder.
const MAX_SESSION_RERUNS: usize = 999;

/// Longest export path a toast prints whole; past it the folder elides
/// and the file name stays.
const EXPORT_PATH_ELIDE_CHARS: usize = 64;
