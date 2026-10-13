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
use crate::format::{fmt_exact, sanitize_symbol};
use crate::pnl::{PnlBook, TradePnl, csv_cells};
use crate::report::HistoryRow;
use crate::report::sessions::{JournaledTrade, SessionKey};
use quantick_civil::civil_utc;

impl PaperAccount {
    /// This session's closed round trips as history rows, each stamped with
    /// the source it actually closed under — the session may have flipped
    /// live/replay since, and an export must not restamp a pre-switch trade
    /// with the current source. The host merges these with the saved
    /// history it loaded and hands the lot to [`export_csv`].
    #[must_use]
    pub fn session_history_rows(&self) -> Vec<HistoryRow> {
        let at = |index: usize| self.session_trade_at.get(index).and_then(Option::as_ref);
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
                session: at(index).map(|at| at.session.clone()),
                trade_ref: at(index)
                    .map_or_else(|| history::TradeRefs::default().next(trade), |at| at.trade),
                // Read from the file by the ledger; the export carries none.
                tags: Vec::new(),
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
            self.session_trade_at.push(None);
            return false;
        }
        let folder = self.dir.join(sanitize_symbol(&self.symbol));
        let opened = self.journal_path.is_none();
        let path = self
            .journal_path
            .get_or_insert_with(|| free_session_path(&folder, &utc_compact(trade.closed_ms)));
        if self.session_journal_paths.last() != Some(path) {
            self.session_journal_paths.push(path.clone());
        }
        // Which trade of its file this close is, counted the way a reader of
        // the file counts: from the rows the file already parses to when the
        // account opens it — an append into an existing file, or rows the
        // account did not write — then one per row that lands.
        if opened || self.journal_refs.as_ref().is_none_or(|(at, _)| at != path) {
            self.journal_refs = Some((path.clone(), file_trade_refs(path)));
        }
        let session = SessionKey::of(&self.dir, path);
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
        // A row that never reached the file is no trade of it to tag, and
        // takes no place in its count.
        let at = match (session, &written, self.journal_refs.as_mut()) {
            (Some(session), Ok(()), Some((_, refs))) => Some(JournaledTrade {
                session,
                trade: refs.next(trade),
            }),
            _ => None,
        };
        self.session_trade_at.push(at);
        written.is_ok()
    }
}

/// The trade counter of the session file at `path` as a reader counts it:
/// one per row that parses, in file order. A missing file counts nothing.
fn file_trade_refs(path: &Path) -> history::TradeRefs {
    let parsed = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| history::parse(&text).ok());
    history::TradeRefs::after(parsed.iter().flat_map(|parsed| &parsed.trades))
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
///
/// The last three columns are what the report reads the trade as: the move
/// per contract in the instrument's unit, and the money it made with its
/// currency - empty where no point value is known, never a guessed zero.
pub fn export_csv(rows: &[HistoryRow], book: &PnlBook<'_>) -> String {
    let mut text = String::from(
        "symbol,side,quantity,opened_ms,opened_utc,entry_price,closed_ms,closed_utc,\
         exit_price,pnl_points,cum_pnl_points,duration_ms,exit_reason,entry_agg_id,\
         exit_agg_id,mae_points,mfe_points,source,pnl_per_unit,pnl_money,currency\n",
    );
    let mut cumulative = Decimal::ZERO;
    let opt_u64 = |value: Option<u64>| value.map(|value| value.to_string()).unwrap_or_default();
    let opt_points = |value: Option<Decimal>| value.map(fmt_exact).unwrap_or_default();
    for row in rows {
        let trade = &row.trade;
        cumulative = cumulative.saturating_add(trade.pnl_points);
        let side = match trade.side {
            Side::Buy => "long",
            Side::Sell => "short",
        };
        let [per_unit, money, currency] = csv_cells(&TradePnl::of(
            trade.pnl_points,
            trade.quantity,
            &book.units(&row.symbol),
        ));
        text.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            row.symbol.replace(',', "_"),
            side,
            fmt_exact(trade.quantity),
            trade.opened_ms,
            fmt_utc_iso(trade.opened_ms),
            fmt_exact(trade.entry_price),
            trade.closed_ms,
            fmt_utc_iso(trade.closed_ms),
            fmt_exact(trade.exit_price),
            fmt_exact(trade.pnl_points),
            fmt_exact(cumulative),
            trade.closed_ms.saturating_sub(trade.opened_ms).max(0),
            trade.exit_reason.as_str(),
            opt_u64(trade.entry_agg_id),
            opt_u64(trade.exit_agg_id),
            opt_points(trade.mae_points),
            opt_points(trade.mfe_points),
            // Empty when the file never recorded one — unknown, not live.
            row.source.map(history::SessionSource::as_str).unwrap_or(""),
            per_unit,
            money,
            currency,
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
