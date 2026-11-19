//! One history view over two journals, with origin-qualified session identities.
use std::path::{Path, PathBuf};

use quantick_sim::history::SessionSource;
use quantick_trading::TradeMode;

use super::{LoadedHistory, list_symbol_folders, load_history};

pub const BROKER_TRADES_FOLDER: &str = "real-trades";

pub fn broker_dir(paper: &Path) -> PathBuf {
    paper
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(
            || paper.join(BROKER_TRADES_FOLDER),
            |parent| parent.join(BROKER_TRADES_FOLDER),
        )
}

pub fn symbols(dir: &Path) -> Vec<String> {
    let mut symbols = list_symbol_folders(dir);
    symbols.extend(list_symbol_folders(&broker_dir(dir)));
    symbols.sort();
    symbols.dedup();
    symbols
}

/// Only simulator live files are excluded: their in-memory rows already appear.
/// Broker rows always come from their journal, including the current session.
pub fn load(dir: &Path, symbol: Option<&str>, exclude: &[PathBuf]) -> LoadedHistory {
    let mut history = load_history(dir, symbol, exclude);
    let broker = load_history(&broker_dir(dir), symbol, &[]);
    history.files += broker.files;
    history.unreadable_files += broker.unreadable_files;
    history.problem_rows += broker.problem_rows;
    history.rows.extend(broker.rows.into_iter().map(|mut row| {
        row.session = row.session.map(|key| key.for_broker());
        row.source = Some(match row.source {
            Some(source @ SessionSource::Broker(_)) => source,
            _ => SessionSource::Broker(TradeMode::Unknown),
        });
        row
    }));
    history
        .sessions
        .extend(broker.sessions.into_values().map(|mut info| {
            info.key = info.key.for_broker();
            info.source = Some(SessionSource::Broker(TradeMode::Unknown));
            (info.key.clone(), info)
        }));
    history
        .rows
        .sort_by_key(|row| (row.trade.closed_ms, row.trade.opened_ms));
    history
}
