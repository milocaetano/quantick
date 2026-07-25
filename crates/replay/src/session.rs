//! A loaded replay session: the trades of one instrument on one day.

use std::path::{Path, PathBuf};

use quantick_engine::Trade;

use crate::format::{self, FileHeader, FormatError, ParseOptions, Quote, UtcOffset};

/// The calendar day a session file covers, taken from its file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionDate {
    /// Four-digit year.
    pub year: i32,
    /// Month, 1–12.
    pub month: u32,
    /// Day of month, 1–31.
    pub day: u32,
}

impl SessionDate {
    /// Parse a `YYYYMMDD` or `YYYY-MM-DD` file stem.
    #[must_use]
    pub fn parse(stem: &str) -> Option<Self> {
        let digits: String = stem.chars().filter(char::is_ascii_digit).collect();
        if digits.len() != 8 || digits.len() != stem.chars().filter(|c| *c != '-').count() {
            return None;
        }
        let year: i32 = digits[0..4].parse().ok()?;
        let month: u32 = digits[4..6].parse().ok()?;
        let day: u32 = digits[6..8].parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// The ISO rendering, e.g. `2026-03-16`.
    #[must_use]
    pub fn label(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Why a session file could not be loaded.
#[derive(Debug)]
pub enum SessionError {
    /// The file could not be read from disk.
    Read {
        /// The file that failed.
        path: PathBuf,
        /// The operating system's message.
        message: String,
    },
    /// The file was read but does not follow the replay format.
    Format {
        /// The file that failed.
        path: PathBuf,
        /// Where and what.
        error: FormatError,
    },
}

impl SessionError {
    /// The file this error is about.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            SessionError::Read { path, .. } | SessionError::Format { path, .. } => path,
        }
    }

    /// What to change to make the file load.
    #[must_use]
    pub fn advice(&self) -> &'static str {
        match self {
            SessionError::Read { .. } => {
                "Check the file still exists and that quantick may read it."
            }
            SessionError::Format { error, .. } => error.advice(),
        }
    }
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Read { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
            SessionError::Format { path, error } => {
                write!(f, "{}: {error}", file_label(path))
            }
        }
    }
}

impl std::error::Error for SessionError {}

/// The file name alone, for messages that already name the folder.
fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

/// One instrument's trades for one session day, ready to play.
#[derive(Debug, Clone)]
pub struct Session {
    /// Where it was loaded from.
    pub path: PathBuf,
    /// Instrument, from the `# symbol=` line or the parent folder.
    pub symbol: String,
    /// Session day, from the file name, when it followed the convention.
    pub date: Option<SessionDate>,
    /// The metadata block and column positions the file declared.
    pub header: FileHeader,
    /// Trades in chronological order — the replay's whole input.
    pub trades: Vec<Trade>,
    /// Quotes parallel to `trades`, when the caller asked to keep them.
    pub quotes: Vec<Quote>,
}

impl Session {
    /// Load and parse a session file.
    ///
    /// # Errors
    ///
    /// [`SessionError::Read`] when the file cannot be read, or
    /// [`SessionError::Format`] with the offending line when it does not follow
    /// the format.
    pub fn load(path: &Path, options: ParseOptions) -> Result<Self, SessionError> {
        let text = std::fs::read_to_string(path).map_err(|e| SessionError::Read {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Self::from_text(path, &text, options)
    }

    /// Parse an already-read session file. Split out so the whole load path is
    /// testable without touching a disk.
    ///
    /// # Errors
    ///
    /// [`SessionError::Format`] with the offending line.
    pub fn from_text(path: &Path, text: &str, options: ParseOptions) -> Result<Self, SessionError> {
        let parsed = format::parse_file(text, options).map_err(|error| SessionError::Format {
            path: path.to_path_buf(),
            error,
        })?;
        let symbol = parsed
            .header
            .symbol
            .clone()
            .or_else(|| symbol_from_path(path))
            .unwrap_or_else(|| "unknown".to_string());
        Ok(Self {
            path: path.to_path_buf(),
            symbol,
            date: date_from_path(path),
            header: parsed.header,
            trades: parsed.trades,
            quotes: parsed.quotes,
        })
    }

    /// Instrument and day, e.g. `WINJ26 · 2026-03-16`.
    #[must_use]
    pub fn label(&self) -> String {
        match self.date {
            Some(date) => format!("{} · {}", self.symbol, date.label()),
            None => format!("{} · {}", self.symbol, file_label(&self.path)),
        }
    }

    /// Timestamp of the first trade, in epoch milliseconds.
    #[must_use]
    pub fn start_ms(&self) -> i64 {
        self.trades.first().map_or(0, |t| t.timestamp_ms)
    }

    /// Timestamp of the last trade, in epoch milliseconds.
    #[must_use]
    pub fn end_ms(&self) -> i64 {
        self.trades.last().map_or(0, |t| t.timestamp_ms)
    }

    /// How much market time the session covers, in milliseconds.
    #[must_use]
    pub fn span_ms(&self) -> i64 {
        self.end_ms().saturating_sub(self.start_ms())
    }

    /// The offset the file's clock readings are expressed in.
    #[must_use]
    pub fn timezone(&self) -> UtcOffset {
        self.header.timezone
    }
}

/// The instrument a path implies: the `SYMBOL` in `SYMBOL/20260316.csv`, or the
/// `SYMBOL` in `SYMBOL-20260316.csv`.
#[must_use]
pub fn symbol_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy();
    if let Some((symbol, tail)) = stem.rsplit_once(['-', '_'])
        && SessionDate::parse(tail).is_some()
        && !symbol.is_empty()
    {
        return Some(symbol.to_string());
    }
    let parent = path.parent()?.file_name()?.to_string_lossy().to_string();
    (!parent.is_empty()).then_some(parent)
}

/// The session day a file name implies, from `20260316.csv` or
/// `SYMBOL-20260316.csv`.
#[must_use]
pub fn date_from_path(path: &Path) -> Option<SessionDate> {
    let stem = path.file_stem()?.to_string_lossy();
    SessionDate::parse(&stem).or_else(|| {
        let (_, tail) = stem.rsplit_once(['-', '_'])?;
        SessionDate::parse(tail)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# quantick-replay 1
# symbol=WINJ26
# timezone=-03:00
Date,Time,Price,Volume,Side
2026-03-16,10:00:00.000,182035,12,B
2026-03-16,10:00:02.500,182040,3,B
";

    #[test]
    fn session_dates_parse_both_spellings() {
        assert_eq!(
            SessionDate::parse("20260316"),
            Some(SessionDate {
                year: 2026,
                month: 3,
                day: 16
            })
        );
        assert_eq!(
            SessionDate::parse("2026-03-16").map(SessionDate::label),
            Some("2026-03-16".to_string())
        );
        assert_eq!(SessionDate::parse("2026031"), None);
        assert_eq!(SessionDate::parse("20261316"), None);
        assert_eq!(SessionDate::parse("session"), None);
    }

    #[test]
    fn symbol_and_date_come_from_the_folder_layout() {
        let path = Path::new("replay/WINJ26/20260316.csv");
        assert_eq!(symbol_from_path(path).as_deref(), Some("WINJ26"));
        assert_eq!(
            date_from_path(path).map(SessionDate::label).as_deref(),
            Some("2026-03-16")
        );
    }

    #[test]
    fn symbol_and_date_come_from_a_flat_file_name() {
        let path = Path::new("replay/WINJ26-20260316.csv");
        assert_eq!(symbol_from_path(path).as_deref(), Some("WINJ26"));
        assert_eq!(
            date_from_path(path).map(SessionDate::label).as_deref(),
            Some("2026-03-16")
        );
        let underscore = Path::new("replay/WDO$-20260317.csv");
        assert_eq!(symbol_from_path(underscore).as_deref(), Some("WDO$"));
    }

    #[test]
    fn a_session_exposes_its_span_and_label() {
        let path = Path::new("replay/WINJ26/20260316.csv");
        let session = Session::from_text(path, SAMPLE, ParseOptions::default()).unwrap();
        assert_eq!(session.symbol, "WINJ26");
        assert_eq!(session.trades.len(), 2);
        assert_eq!(session.span_ms(), 2_500);
        assert_eq!(session.label(), "WINJ26 · 2026-03-16");
        assert_eq!(session.timezone().minutes(), -180);
    }

    #[test]
    fn the_metadata_symbol_wins_over_the_folder_name() {
        let path = Path::new("replay/renamed-folder/20260316.csv");
        let session = Session::from_text(path, SAMPLE, ParseOptions::default()).unwrap();
        assert_eq!(session.symbol, "WINJ26");
    }

    #[test]
    fn a_format_error_keeps_the_file_and_the_line() {
        let path = Path::new("replay/WINJ26/20260316.csv");
        let err =
            Session::from_text(path, "Date,Time,Price\n", ParseOptions::default()).unwrap_err();
        assert_eq!(err.path(), path);
        assert!(err.to_string().contains("20260316.csv"), "{err}");
        assert!(!err.advice().is_empty());
    }
}
