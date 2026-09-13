//! Getting trades into the journal folder, and back out as one CSV.
//!
//! The journal is the account's own record: one append per close, into a
//! session file named from venue time. The export is the Excel-facing copy
//! of everything the ledger lists. Both are here beside the rows they write;
//! what is *not* here is the part that needs a host — the folder picker, the
//! background writer and the wall-clock stamp an export file is named with.
//! `handle_events` stays with the account itself: it is the fill path, and
//! only its last step lands here.

use std::path::{Path, PathBuf};

use quantick_engine::Side;
use quantick_sim::{ClosedTrade, history};
use rust_decimal::Decimal;

use super::{PaperAccount, utc_compact};
use crate::civil::civil_utc;
use crate::format::{fmt_decimal, sanitize_symbol};
use crate::report::HistoryRow;

impl PaperAccount {
    /// This session's closed round trips as history rows, each stamped with
    /// the source it actually closed under — the session may have flipped
    /// live/replay since, and an export must not restamp a pre-switch trade
    /// with the current source. The host merges these with the saved
    /// history it loaded and hands the lot to [`export_csv`].
    #[must_use]
    pub fn session_history_rows(&self) -> Vec<HistoryRow> {
        self.venue
            .closed_trades()
            .iter()
            .enumerate()
            .map(|(index, trade)| HistoryRow {
                symbol: self.symbol.clone(),
                source: Some(
                    self.session_trade_sources
                        .get(index)
                        .copied()
                        .unwrap_or(self.session_source),
                ),
                trade: trade.clone(),
            })
            .collect()
    }

    /// Append one closed trade to the session's history file, creating the
    /// file (with its header) on the first close. Also records the source
    /// the trade closed under, for the export. Returns whether the write
    /// landed — a failed write warns once and never crashes a trading
    /// session, but its caller must not paint a healthy toast over the
    /// warning.
    pub(super) fn journal(&mut self, trade: &ClosedTrade) -> bool {
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
pub fn elide_path(path: &Path) -> String {
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
pub fn export_csv(rows: &[HistoryRow]) -> String {
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
fn free_session_path(folder: &Path, stamp: &str) -> PathBuf {
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
