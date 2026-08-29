//! A loaded replay session: the trades of one instrument on one day.

use std::path::{Path, PathBuf};

use quantick_engine::Trade;

use crate::context::{self, ContextSeries};
use crate::format::{self, CivilTime, FileHeader, FormatError, ParseOptions, Quote, UtcOffset};

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

    /// Days since 1970-01-01, for measuring the gap between two session days.
    ///
    /// Read at UTC on purpose: only differences are ever taken from this, and
    /// both days go through the same conversion, so the broker's clock cancels
    /// out instead of having to be known here.
    #[must_use]
    pub fn epoch_day(self) -> i64 {
        CivilTime {
            year: i64::from(self.year),
            month: self.month,
            day: self.day,
            hour: 0,
            minute: 0,
            second: 0,
            milli: 0,
        }
        .epoch_millis(UtcOffset::UTC)
        .div_euclid(86_400_000)
    }
}

/// How far back a neighbouring tape may sit and still be the session before
/// this one.
///
/// Carnival, Easter and a national holiday landing on a Monday can put five
/// calendar days between two B3 sessions, so a week covers every real gap in a
/// trading calendar with room to spare. Past it the newest file in the folder
/// is not yesterday, it is an old recording that happens to be the closest one
/// there — and joining that would put a three-month hole in the middle of the
/// chart and present it as order flow.
pub const MAX_DAY_BEFORE_GAP_DAYS: i64 = 7;

/// The session day joined in front of a recording, when one was.
#[derive(Debug, Clone)]
pub struct JoinedDay {
    /// The tape its prints came from.
    pub path: PathBuf,
    /// Its session day, when the file name followed the convention.
    pub date: Option<SessionDate>,
    /// How many prints at the head of [`Session::trades`] are its.
    ///
    /// A count, and named for what the interface calls them, because
    /// `trades` next to [`Session::trades`] would be a `Vec` to every reader
    /// who met it second.
    pub prints: usize,
}

/// Why the day before was not joined, when there was a file to join.
///
/// Carried rather than raised, exactly as a broken run-up is: refusing to
/// replay a good day because the tape beside it is malformed would cost the
/// trader the session over its context.
#[derive(Debug, Clone)]
pub struct DayBeforeProblem {
    /// What went wrong, naming the file it is about.
    pub detail: String,
    /// The one thing to do about it.
    pub advice: &'static str,
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
    /// Broker candles for the sessions before this one, from the sibling
    /// context file when there is one. `None` means the recording has no
    /// context — an honest absence, and the chart simply opens at the first
    /// print.
    pub context: Option<ContextSeries>,
    /// Why the sibling context file was not used, when one exists but could
    /// not be read.
    ///
    /// A broken context never fails the load: the tape is the recording, and
    /// refusing to replay a good day because the candles beside it are
    /// malformed would cost the trader the session over the decoration. The
    /// problem is carried so the interface can say so rather than showing
    /// blank space and letting it pass for "no context was downloaded".
    pub context_problem: Option<FormatError>,
    /// The session day joined in front of this one, when one was asked for and
    /// found. Its prints are the first [`JoinedDay::trades`] of
    /// [`trades`](Self::trades).
    pub day_before: Option<JoinedDay>,
    /// Why the day before is not there, when a file for it was found and could
    /// not be used. See [`DayBeforeProblem`].
    pub day_before_problem: Option<DayBeforeProblem>,
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
        let mut session = Self::load_tape(path, options)?;
        session.attach_context(&context::context_path(path));
        Ok(session)
    }

    /// The tape alone, with no sibling file read.
    ///
    /// What [`load`](Self::load) is before it picks up the run-up beside the
    /// recording, and all a joined day needs: the day before contributes its
    /// prints, never its own run-up, because the candles in front of the
    /// chosen day already cover the week behind it.
    fn load_tape(path: &Path, options: ParseOptions) -> Result<Self, SessionError> {
        let text = std::fs::read_to_string(path).map_err(|e| SessionError::Read {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Self::from_text(path, &text, options)
    }

    /// Load `path` with the session day before it joined in front of its
    /// prints, when one sits beside it in the folder.
    ///
    /// One recording, two days. A trader rehearsing an open reads the previous
    /// session's order flow, which the broker candles of the run-up cannot
    /// carry. The session stays *about* `path` - its day, its header, its
    /// run-up - and says what was joined in [`day_before`](Self::day_before).
    ///
    /// A neighbour that is absent, too far back, malformed or out of order is
    /// not an error: the day the trader asked for loads either way, and the
    /// reason reaches [`day_before_problem`](Self::day_before_problem) so the
    /// interface can say it rather than showing an unexplained single day.
    ///
    /// Rare path - once per session opened - and it costs a second parse and a
    /// second tape held in memory, which is the whole price of the feature.
    ///
    /// # Errors
    ///
    /// The same as [`load`](Self::load), and only ever about `path` itself.
    pub fn load_with_day_before(path: &Path, options: ParseOptions) -> Result<Self, SessionError> {
        let mut session = Self::load(path, options)?;
        let Some(earlier) = day_before_path(path) else {
            return Ok(session);
        };
        match Self::load_tape(&earlier, options) {
            Ok(before) => session.join_day_before(&earlier, before),
            Err(error) => {
                session.day_before_problem = Some(DayBeforeProblem {
                    detail: error.to_string(),
                    advice: error.advice(),
                });
            }
        }
        Ok(session)
    }

    /// Put `before`'s prints in front of this session's own.
    ///
    /// The joined stream is numbered once through. Two tapes carry two
    /// independent id spaces - an export with no `Id` column numbers every
    /// file from one - and a consumer keyed by print id must never be handed
    /// the same id twice.
    ///
    /// Refused, with a reason, when the neighbour holds nothing or ends after
    /// this session opens: the format requires prints that never step
    /// backwards, and two days that overlap are not the day before and the
    /// day, whatever their file names say.
    fn join_day_before(&mut self, path: &Path, before: Self) {
        // Checked here as well as when the file was picked: the symbol in the
        // metadata block outranks the one in the file name, so a file named
        // for this contract can still hold another's prints. Two prices two
        // orders of magnitude apart drawn as one series is the worst thing
        // this feature could do.
        if before.symbol != self.symbol {
            self.day_before_problem = Some(DayBeforeProblem {
                detail: format!(
                    "{} holds {} prints, not {}",
                    file_label(path),
                    before.symbol,
                    self.symbol
                ),
                advice: "Keep one folder per instrument, or name the files for the contract they hold.",
            });
            return;
        }
        let Some(last) = before.trades.last() else {
            self.day_before_problem = Some(DayBeforeProblem {
                detail: format!("{} holds no prints", file_label(path)),
                advice: "Download that day again; an empty tape has nothing to replay.",
            });
            return;
        };
        if last.timestamp_ms > self.start_ms() {
            self.day_before_problem = Some(DayBeforeProblem {
                detail: format!("{} ends after this session opens", file_label(path)),
                advice: "Check both files are the days their names claim, and that both were exported against the same broker clock.",
            });
            return;
        }
        let joined = before.trades.len();
        // Quotes are parallel to trades or they are nothing: a half-aligned
        // column would put yesterday's bid beside today's print. Kept only
        // when both days carry one for every print they hold.
        let quotes_align = before.quotes.len() == joined && self.quotes.len() == self.trades.len();
        let date = before.date;

        let mut trades = before.trades;
        trades.append(&mut self.trades);
        for (index, trade) in trades.iter_mut().enumerate() {
            trade.agg_id = index as u64 + 1;
        }
        self.trades = trades;

        let mut own_quotes = std::mem::take(&mut self.quotes);
        self.quotes = if quotes_align {
            let mut quotes = before.quotes;
            quotes.append(&mut own_quotes);
            quotes
        } else {
            Vec::new()
        };

        self.day_before = Some(JoinedDay {
            path: path.to_path_buf(),
            date,
            prints: joined,
        });
    }

    /// Read the sibling context file into this session, if it is there.
    ///
    /// Absent is not a problem: most recordings have no context, and the chart
    /// opens at the first print. Present-but-unreadable *is* a problem, and it
    /// is recorded in [`context_problem`](Self::context_problem) rather than
    /// failing the load — see that field.
    fn attach_context(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        match context::parse_context(&text) {
            Ok(series) => self.context = Some(series),
            Err(error) => self.context_problem = Some(error),
        }
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
            context: None,
            context_problem: None,
            day_before: None,
            day_before_problem: None,
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

    /// Timestamp of the first trade, in epoch milliseconds - of the joined
    /// day when there is one, because that is where the recording's prints
    /// begin and where anything drawn in front of them has to stop.
    #[must_use]
    pub fn start_ms(&self) -> i64 {
        self.trades.first().map_or(0, |t| t.timestamp_ms)
    }

    /// How many prints at the head of [`trades`](Self::trades) came from the
    /// day joined in front of this session. Zero when none was.
    #[must_use]
    pub fn day_before_prints(&self) -> usize {
        self.day_before.as_ref().map_or(0, |joined| joined.prints)
    }

    /// Timestamp of the first print of the session's *own* day: where playback
    /// opens, and where a restart returns to. The same as
    /// [`start_ms`](Self::start_ms) unless a day was joined in front of it.
    #[must_use]
    pub fn day_start_ms(&self) -> i64 {
        self.trades
            .get(self.day_before_prints())
            .or_else(|| self.trades.last())
            .map_or(0, |t| t.timestamp_ms)
    }

    /// The joined day as a person reads it - `2026-03-13`, or the file name
    /// when it does not follow the convention. `None` when nothing was joined.
    #[must_use]
    pub fn day_before_label(&self) -> Option<String> {
        let joined = self.day_before.as_ref()?;
        Some(
            joined
                .date
                .map_or_else(|| file_label(&joined.path), SessionDate::label),
        )
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

/// The tape of the session day before `path`, when one sits beside it.
///
/// The newest file in the same folder whose day is strictly earlier - so a
/// Monday reaches Friday without anything here knowing what a weekend is. The
/// folder's own contents are the calendar, which is the only calendar that can
/// be right for every venue.
///
/// Only the same instrument. The flat layout the library also accepts —
/// `SYMBOL-YYYYMMDD.csv`, all contracts in one folder — puts WDO's Friday
/// beside WIN's Monday, and joining that would open the chart on another
/// contract's order flow with a badge asserting it belongs. The instrument is
/// read from the file the same way the session's own is, so the two answers
/// cannot disagree.
///
/// `None` when `path` has no day in its name, when nothing earlier is there
/// for this instrument, or when the nearest one is further back than
/// [`MAX_DAY_BEFORE_GAP_DAYS`]. A folder that cannot be listed answers `None`
/// too: the day the trader asked for still plays, which is the policy the
/// run-up beside it already follows.
///
/// Rare path: one directory listing per session opened.
#[must_use]
pub fn day_before_path(path: &Path) -> Option<PathBuf> {
    let day = date_from_path(path)?;
    let symbol = symbol_from_path(path);
    let folder = path.parent()?;
    let mut best: Option<(SessionDate, PathBuf)> = None;
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let candidate = entry.path();
        if !candidate
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
        {
            continue;
        }
        // A `.context.csv` beside a tape carries no day in its stem, so it
        // falls out here rather than needing a rule of its own.
        let Some(date) = date_from_path(&candidate) else {
            continue;
        };
        // Strictly earlier, which also drops `path` itself and any second
        // spelling of the same day sitting in the folder.
        if date >= day {
            continue;
        }
        if symbol_from_path(&candidate) != symbol {
            continue;
        }
        // Newer wins; a tie is broken by path so the answer is the same on
        // every machine. `read_dir` yields in filesystem order, and a folder
        // holding both spellings of one day would otherwise join whichever
        // copy happened to be listed first — iteration order reaching the
        // output, which this crate does not do.
        let better = match best.as_ref() {
            None => true,
            Some((newest, path)) => date > *newest || (date == *newest && candidate < *path),
        };
        if better {
            best = Some((date, candidate));
        }
    }
    let (date, tape) = best?;
    (day.epoch_day().saturating_sub(date.epoch_day()) <= MAX_DAY_BEFORE_GAP_DAYS).then_some(tape)
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

    use crate::scratch::Scratch;

    const CONTEXT: &str = "\
# quantick-context 1
# symbol=WINJ26
# timezone=-03:00
# interval_ms=60000
Date,Time,Open,High,Low,Close,Volume
2026-03-13,17:00:00.000,181900,182000,181850,181950,4200
";

    #[test]
    fn a_context_file_beside_the_tape_is_loaded_with_it() {
        let scratch = Scratch::new("session-context");
        let tape = scratch.write("WINJ26/20260316.csv", SAMPLE);
        scratch.write("WINJ26/20260316.context.csv", CONTEXT);

        let session = Session::load(&tape, ParseOptions::default()).unwrap();
        let context = session
            .context
            .as_ref()
            .expect("the sibling file is picked up");
        assert_eq!(context.bars.len(), 1);
        assert_eq!(context.interval_ms, 60_000);
        assert!(session.context_problem.is_none());
        // The context sits before the tape, which is the whole point of it.
        assert!(context.end_ms().unwrap() < session.start_ms());
    }

    #[test]
    fn a_session_with_no_context_file_loads_unchanged() {
        let scratch = Scratch::new("session-no-context");
        let tape = scratch.write("WINJ26/20260316.csv", SAMPLE);

        let session = Session::load(&tape, ParseOptions::default()).unwrap();
        assert!(session.context.is_none(), "absent is not a problem");
        assert!(session.context_problem.is_none());
        assert_eq!(session.trades.len(), 2);
    }

    #[test]
    fn a_broken_context_never_costs_the_trader_the_session() {
        let scratch = Scratch::new("session-bad-context");
        let tape = scratch.write("WINJ26/20260316.csv", SAMPLE);
        // No `# interval_ms=`, so the candles cannot be folded to anything.
        scratch.write(
            "WINJ26/20260316.context.csv",
            "Date,Time,Open,High,Low,Close,Volume\n",
        );

        let session = Session::load(&tape, ParseOptions::default()).expect("the tape still plays");
        assert_eq!(session.trades.len(), 2);
        assert!(session.context.is_none());
        let problem = session
            .context_problem
            .expect("and the interface can say why the space is blank");
        assert!(!problem.advice().is_empty());
    }

    /// A tape carrying the `Bid`/`Ask` columns, for the quote-alignment rule.
    const QUOTED: &str = "\
# quantick-replay 1
# symbol=WINJ26
# timezone=-03:00
Date,Time,Price,Bid,Ask,Volume,Side
2026-03-16,10:00:00.000,182035,182030,182040,12,B
2026-03-16,10:00:02.500,182040,182035,182045,3,B
";

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

    /// A two-print tape for `date`, an ISO day, on a B3 clock.
    fn tape(date: &str, price: u32) -> String {
        named_tape("WINJ26", date, price)
    }

    /// The same, for a stated instrument.
    fn named_tape(symbol: &str, date: &str, price: u32) -> String {
        let mut text = format!("# quantick-replay 1\n# symbol={symbol}\n# timezone=-03:00\n");
        text.push_str("Date,Time,Price,Volume,Side\n");
        text.push_str(&format!("{date},09:00:00.000,{price},1,B\n"));
        text.push_str(&format!("{date},17:00:00.000,{price},2,S\n"));
        text
    }

    #[test]
    fn the_day_before_is_the_newest_tape_that_sits_before_this_one() {
        let scratch = Scratch::new("day-before-pick");
        scratch.write("WINJ26/20260313.csv", &tape("2026-03-13", 181_000));
        scratch.write("WINJ26/20260312.csv", &tape("2026-03-12", 180_000));
        let monday = scratch.write("WINJ26/20260316.csv", &tape("2026-03-16", 182_000));
        // Not a tape: the run-up beside Monday must never be taken for one.
        scratch.write("WINJ26/20260316.context.csv", CONTEXT);

        let picked = day_before_path(&monday).expect("Friday is the session before Monday");
        assert_eq!(picked.file_name().unwrap(), "20260313.csv");
    }

    /// The flat layout the library also accepts puts every contract in one
    /// folder. Joining the newest earlier file there would open a WIN chart on
    /// WDO's order flow — two prices two orders of magnitude apart drawn as one
    /// series, with a badge saying it belongs.
    #[test]
    fn only_the_same_instrument_is_the_day_before() {
        let scratch = Scratch::new("day-before-symbol");
        scratch.write(
            "WDOV26-20260313.csv",
            &named_tape("WDOV26", "2026-03-13", 5_700),
        );
        scratch.write(
            "WINV26-20260312.csv",
            &named_tape("WINV26", "2026-03-12", 181_000),
        );
        let monday = scratch.write(
            "WINV26-20260316.csv",
            &named_tape("WINV26", "2026-03-16", 182_000),
        );

        let picked = day_before_path(&monday).expect("WIN's own Thursday");
        assert_eq!(
            picked.file_name().unwrap(),
            "WINV26-20260312.csv",
            "the nearer file is another contract's"
        );
    }

    /// The metadata block outranks the file name, so a file named for this
    /// contract can still hold another's prints. The join is the last place
    /// that can catch it.
    #[test]
    fn a_tape_that_holds_another_contract_is_refused_at_the_join() {
        let scratch = Scratch::new("day-before-symbol-lies");
        scratch.write(
            "WINV26/20260313.csv",
            &named_tape("WDOV26", "2026-03-13", 5_700),
        );
        let monday = scratch.write(
            "WINV26/20260316.csv",
            &named_tape("WINV26", "2026-03-16", 182_000),
        );

        let session = Session::load_with_day_before(&monday, ParseOptions::default()).unwrap();
        assert!(session.day_before.is_none(), "not this instrument's day");
        assert_eq!(session.trades.len(), 2, "and Monday still plays");
        let problem = session.day_before_problem.expect("and it says so");
        assert!(problem.detail.contains("WDOV26"), "{problem:?}");
    }

    /// `read_dir` yields in filesystem order. A folder holding both spellings
    /// of one day must still join the same file on every machine.
    #[test]
    fn a_tie_between_two_spellings_of_a_day_is_broken_the_same_way_everywhere() {
        let scratch = Scratch::new("day-before-tie");
        scratch.write(
            "WINJ26/20260313.csv",
            &named_tape("WINJ26", "2026-03-13", 181_000),
        );
        scratch.write(
            "WINJ26/WINJ26-20260313.csv",
            &named_tape("WINJ26", "2026-03-13", 181_000),
        );
        let monday = scratch.write(
            "WINJ26/20260316.csv",
            &named_tape("WINJ26", "2026-03-16", 182_000),
        );

        let first = day_before_path(&monday).expect("one of the two");
        for _ in 0..5 {
            assert_eq!(
                day_before_path(&monday).as_ref(),
                Some(&first),
                "the same file every time, whatever order the folder lists in"
            );
        }
    }

    /// Quotes are parallel to trades or they are nothing. Documented at the
    /// join, and pinned here so it stays a decision rather than an accident.
    #[test]
    fn a_joined_day_without_quotes_leaves_the_session_without_them() {
        let scratch = Scratch::new("day-before-quotes");
        scratch.write(
            "WINJ26/20260313.csv",
            &named_tape("WINJ26", "2026-03-13", 181_000),
        );
        let monday = scratch.write("WINJ26/20260316.csv", QUOTED);

        let options = ParseOptions { keep_quotes: true };
        let alone = Session::load(&monday, options).unwrap();
        assert_eq!(alone.quotes.len(), alone.trades.len(), "the day has quotes");

        let joined = Session::load_with_day_before(&monday, options).unwrap();
        assert_eq!(joined.day_before_prints(), 2, "the day before joined");
        assert!(
            joined.quotes.is_empty(),
            "a half-aligned column would put yesterday's bid beside today's print"
        );
    }

    #[test]
    fn a_tape_further_back_than_a_long_weekend_is_not_the_day_before() {
        let scratch = Scratch::new("day-before-gap");
        scratch.write("WINJ26/20251201.csv", &tape("2025-12-01", 170_000));
        let day = scratch.write("WINJ26/20260316.csv", &tape("2026-03-16", 182_000));

        assert!(
            day_before_path(&day).is_none(),
            "an old recording is not the session before this one"
        );
    }

    #[test]
    fn joining_puts_yesterdays_prints_in_front_and_says_how_many() {
        let scratch = Scratch::new("day-before-join");
        scratch.write("WINJ26/20260313.csv", &tape("2026-03-13", 181_000));
        let monday = scratch.write("WINJ26/20260316.csv", &tape("2026-03-16", 182_000));

        let session = Session::load_with_day_before(&monday, ParseOptions::default()).unwrap();
        let joined = session.day_before.as_ref().expect("Friday was joined");
        assert_eq!(joined.prints, 2);
        assert_eq!(joined.date.unwrap().label(), "2026-03-13");
        assert_eq!(session.trades.len(), 4);
        assert_eq!(session.day_before_prints(), 2);
        // The session is still about Monday: its path, its day, its own open.
        assert_eq!(session.date.unwrap().label(), "2026-03-16");
        assert_eq!(session.path, monday);
        assert_eq!(
            session.trades[session.day_before_prints()].timestamp_ms,
            session.day_start_ms()
        );
        // The stream runs strictly forward, numbered once through, so nothing
        // keyed by print id ever sees the same one twice.
        assert!(
            session
                .trades
                .windows(2)
                .all(|w| w[0].timestamp_ms <= w[1].timestamp_ms),
            "the joined stream moves forward"
        );
        let ids: Vec<u64> = session.trades.iter().map(|t| t.agg_id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4]);
    }

    #[test]
    fn a_session_with_nothing_to_join_is_the_session_it_always_was() {
        let scratch = Scratch::new("day-before-alone");
        let tape = scratch.write("WINJ26/20260316.csv", SAMPLE);

        let joined = Session::load_with_day_before(&tape, ParseOptions::default()).unwrap();
        let plain = Session::load(&tape, ParseOptions::default()).unwrap();
        assert!(joined.day_before.is_none());
        assert!(joined.day_before_problem.is_none());
        assert_eq!(joined.day_before_prints(), 0);
        assert_eq!(joined.trades, plain.trades);
        assert_eq!(joined.day_start_ms(), plain.start_ms());
    }

    #[test]
    fn a_broken_day_before_never_costs_the_trader_the_session() {
        let scratch = Scratch::new("day-before-broken");
        scratch.write("WINJ26/20260313.csv", "Date,Time,Price\n");
        let monday = scratch.write("WINJ26/20260316.csv", &tape("2026-03-16", 182_000));

        let session = Session::load_with_day_before(&monday, ParseOptions::default())
            .expect("Monday still plays");
        assert_eq!(session.trades.len(), 2, "Monday, whole");
        assert!(session.day_before.is_none());
        let problem = session
            .day_before_problem
            .as_ref()
            .expect("and the interface can say why yesterday is missing");
        assert!(problem.detail.contains("20260313.csv"), "{problem:?}");
        assert!(!problem.advice.is_empty());
    }

    #[test]
    fn the_run_up_beside_the_chosen_day_is_still_the_one_that_is_read() {
        let scratch = Scratch::new("day-before-context");
        scratch.write("WINJ26/20260313.csv", &tape("2026-03-13", 181_000));
        let monday = scratch.write("WINJ26/20260316.csv", &tape("2026-03-16", 182_000));
        scratch.write("WINJ26/20260316.context.csv", CONTEXT);

        let session = Session::load_with_day_before(&monday, ParseOptions::default()).unwrap();
        assert!(session.context.is_some(), "Monday's run-up, not Friday's");
        // The whole stream now opens on Friday, which is where the candles in
        // front of it have to stop.
        assert_eq!(session.start_ms(), session.trades[0].timestamp_ms);
        assert!(session.start_ms() < session.day_start_ms());
    }
}
