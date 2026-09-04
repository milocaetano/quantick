//! Deal recording — the venue's session deal counter, kept and written down.
//!
//! MetaTrader folds several exchange deals into one tick and keeps no count
//! per tick; the session's running total is the only deal count it has, and
//! it exists only while the session is on. So a chart that cuts bars every
//! N deals (the `trades` kind) can be rebuilt for a day only from what
//! *someone wrote down while it happened*. This module is that someone:
//! it holds the readings a feed delivers, appends them to one file per
//! symbol and day, and reads such a file back so the day reopens as the
//! same chart — after a restart, or a week later.
//!
//! Recording belongs to the **asset**, never to the pane. Switching the pane
//! from `trades` to `tick` changes what is drawn and nothing else; the
//! recorder keeps writing, and the readings keep being retained by every
//! pane on the tab, so switching back rebuilds from the whole series.
//!
//! Three states a trader has to be able to tell apart, and this module
//! names them so every surface says the same word:
//!
//! - [`RecState::Recording`] — the market is being written down now.
//! - [`RecState::Recorded`] — what is on screen came from a file; nothing is
//!   being written.
//! - no deal count — prints the rule cannot place, before the first reading
//!   or on a day nobody recorded. Reported as a number, never as a bar.
//!
//! # The file
//!
//! `<dir>/<SYMBOL>/<YYYY-MM-DD>.deals`, text, append-only:
//!
//! ```text
//! # quantick-deals v1 symbol=WINV26 day=2026-09-03 tz_minutes=-180
//! 1788436967023 1990
//! +20 +13
//! ```
//!
//! The first data line is absolute, every later one a delta from the line
//! before — a poll every 20 ms all day is a million lines, and deltas keep
//! that at a few megabytes rather than forty. The day is the one the first
//! reading fell on in the display timezone; a reading that falls on the next
//! day rotates to the next file. The directory is `QUANTICK_DEALS_DIR`, then
//! the `[deals] dir` config key, then `deals/` in the cockpit home
//! (`Documents/Quantick`).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use quantick_engine::DealSample;

/// One-run override of the recording directory.
pub const DEALS_DIR_ENV: &str = "QUANTICK_DEALS_DIR";
/// The directory under the cockpit home when nothing overrides it.
pub const DEALS_DIR: &str = "deals";
/// Scripted REC state: `QUANTICK_DEAL_RECORDING=on|off|menu`. `on`/`off`
/// override the default the tab would otherwise open with; `menu` opens the
/// REC popover on the first frame, for a capture.
pub const RECORDING_HOOK_ENV: &str = "QUANTICK_DEAL_RECORDING";
/// How long the counter may stand still while prints keep coming before
/// REC calls it stale: three of the terminal's readings, which refresh about
/// every 31 seconds (measured over a whole B3 session). Distinct from the
/// engine's `READING_MAX_AGE_MS`, which is how long a reading still counts
/// prints: the interface warns long before the builder gives up.
pub const STALE_AFTER_MS: i64 = 90_000;
/// How often the file buffer reaches the disk while recording.
pub const FLUSH_EVERY_MS: i64 = 1_000;
/// A day whose first reading is below this counted from the open: the
/// counter had barely started when the recording did.
pub const FROM_OPEN_MAX_DEALS: u64 = 1_000;
const HEADER: &str = "# quantick-deals v1";
const EXTENSION: &str = "deals";

/// Whether the file's first line ends in a newline. A header the crash cut
/// short does not, and is nothing to resume from rather than a header to
/// refuse.
fn first_line_terminated(path: &Path) -> io::Result<bool> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.ends_with('\n'))
}

/// Where recordings go this run.
///
/// The one-run override first, then the config, then the cockpit home the
/// other stores live in, then the cwd-relative name for a run with no home.
#[must_use]
pub fn resolve_dir(configured: Option<&str>) -> PathBuf {
    if cfg!(test) {
        // Never the trader's documents from a test, like every other store.
        return crate::store_home::test_path(DEALS_DIR);
    }
    if let Some(explicit) = std::env::var_os(DEALS_DIR_ENV) {
        return PathBuf::from(explicit);
    }
    if let Some(dir) = configured.map(str::trim).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    crate::store_home::home().map_or_else(|| PathBuf::from(DEALS_DIR), |home| home.join(DEALS_DIR))
}

/// The scripted REC state, if the launch asked for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingHook {
    /// Start recording as soon as the feed can count, whatever the default.
    On,
    /// Never start on its own, whatever the default.
    Off,
    /// Open the REC popover on the first frame it can be drawn.
    Menu,
}

impl RecordingHook {
    /// The three words the hook accepts.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Option<Self> {
        match value? {
            "on" | "1" => Some(Self::On),
            "off" | "0" => Some(Self::Off),
            "menu" => Some(Self::Menu),
            _ => None,
        }
    }

    /// The default this hook imposes, when it imposes one.
    #[must_use]
    pub fn default_override(self) -> Option<bool> {
        match self {
            Self::On => Some(true),
            Self::Off => Some(false),
            Self::Menu => None,
        }
    }
}

/// What the REC control asks the tab to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealRecordingAction {
    /// Start counting and writing, from the next reading on.
    Start,
    /// Stop writing. What was written stays; the day reopens as partial.
    Stop,
    /// Switch the focused pane to `trades` bars.
    ShowAsTrades,
    /// Reveal the recording folder in the file manager.
    OpenFolder,
    /// Load the readings of a recorded day into the tab's panes, by index
    /// into [`RecordingView::days`].
    LoadDay(usize),
}

/// The word every surface uses for the recorder's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecState {
    /// The feed has no deal counter: no REC at all here.
    Unsupported,
    /// Counting is possible and nothing is being written.
    Off,
    /// Readings arrive and are being written down.
    Recording,
    /// Recording, but the counter has not moved while the tape has.
    Stale,
    /// Not recording; readings on screen came from a file.
    Recorded,
}

/// One recorded day, as the directory scan found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedDay {
    /// `YYYY-MM-DD`, from the file name.
    pub day: String,
    /// The first and last readings the file holds.
    pub first: DealSample,
    pub last: DealSample,
    /// How many readings the file holds.
    pub samples: u64,
    pub path: PathBuf,
}

impl RecordedDay {
    /// Whether the recording started with the counter barely begun — the
    /// session's open, as near as a counter can say.
    #[must_use]
    pub fn started_at_open(&self) -> bool {
        self.first.session_deals < FROM_OPEN_MAX_DEALS
    }

    /// `09:00:00 – 18:25:03 · 5 821 205 deals`, in the display timezone.
    #[must_use]
    pub fn coverage(&self, tz_minutes: i32) -> String {
        format!(
            "{} – {} · {} deals",
            fmt_hms(self.first.time_ms, tz_minutes),
            fmt_hms(self.last.time_ms, tz_minutes),
            fmt_count(self.last.session_deals)
        )
    }

    /// `complete` when the recording ran from the open; `from HH:MM`
    /// otherwise. A recording cannot know the close, so it never claims one.
    #[must_use]
    pub fn label(&self, tz_minutes: i32) -> String {
        if self.started_at_open() {
            "from open".to_owned()
        } else {
            format!("from {}", fmt_hms(self.first.time_ms, tz_minutes))
        }
    }
}

/// The file being written, and the little state the delta encoding needs.
#[derive(Debug)]
struct Recording {
    day: String,
    path: PathBuf,
    writer: BufWriter<File>,
    /// The last line written, which the next delta is against.
    last: Option<DealSample>,
    written: u64,
    /// The file's first reading, resumed or written: with `last` and the
    /// counts, what the day list shows, without reading the file back.
    first: Option<DealSample>,
    /// Readings the file held when it was opened.
    held: u64,
    /// When the first unflushed line was written.
    dirty_since_ms: Option<i64>,
}

impl Recording {
    /// Open (or create) the day's file for appending, reading back what it
    /// already holds so a restart resumes rather than starts over.
    fn open(
        dir: &Path,
        symbol: &str,
        day: &str,
        tz_minutes: i32,
    ) -> io::Result<(Self, Vec<DealSample>)> {
        let folder = dir.join(symbol);
        fs::create_dir_all(&folder)?;
        let path = folder.join(format!("{day}.{EXTENSION}"));
        // A file that exists but is empty is a header write that failed —
        // disk full, a lock — and is fresh again, not a day that refuses to
        // open until someone deletes it by hand.
        let len = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        // A header the crash cut short — no newline after it yet — is a torn
        // line with nothing before it: the file starts over, as an empty one
        // does, rather than reading as a bad header for the rest of the day.
        let torn_header = len > 0 && !first_line_terminated(&path)?;
        let (existing, complete_bytes) = if torn_header {
            (Vec::new(), Some(0))
        } else if len > 0 {
            let file = read_file(&path)?;
            (file.samples, Some(file.complete_bytes))
        } else {
            (Vec::new(), None)
        };
        if let Some(complete_bytes) = complete_bytes {
            // A torn last line is cut away before anything is appended after
            // it; a file that read whole is untouched by this.
            let file = OpenOptions::new().write(true).open(&path)?;
            if file.metadata()?.len() != complete_bytes {
                file.set_len(complete_bytes)?;
            }
        }
        // Fresh means no header yet: a header-only file is a day that has
        // not printed, and must not gain a second header — while a header
        // torn mid-line was just cut away, and needs one.
        let fresh = complete_bytes.unwrap_or(0) == 0;
        let mut writer = BufWriter::new(OpenOptions::new().create(true).append(true).open(&path)?);
        if fresh {
            writeln!(
                writer,
                "{HEADER} symbol={symbol} day={day} tz_minutes={tz_minutes}"
            )?;
            writer.flush()?;
        }
        let last = existing.last().copied();
        Ok((
            Self {
                day: day.to_owned(),
                path,
                writer,
                last,
                written: 0,
                first: existing.first().copied(),
                held: existing.len() as u64,
                dirty_since_ms: None,
            },
            existing,
        ))
    }

    fn append(&mut self, sample: DealSample, now_ms: i64) -> io::Result<()> {
        match self.last {
            // The very reading the file ends on (a resumed day re-delivering
            // where it stopped) is already covered. A reading older than the
            // last line is not skipped: a bridge restarted with another clock
            // offset stamps behind the file for a while, the chart keeps
            // those readings, and the file must hold what the chart cut from
            // — the delta goes negative, and the reader accepts it.
            Some(last) if sample == last => return Ok(()),
            Some(last) => writeln!(
                self.writer,
                "+{} {}{}",
                sample.time_ms.saturating_sub(last.time_ms),
                if sample.session_deals >= last.session_deals {
                    "+"
                } else {
                    "-"
                },
                sample.session_deals.abs_diff(last.session_deals)
            )?,
            None => writeln!(self.writer, "{} {}", sample.time_ms, sample.session_deals)?,
        }
        self.last = Some(sample);
        self.first.get_or_insert(sample);
        self.written += 1;
        self.dirty_since_ms.get_or_insert(now_ms);
        Ok(())
    }

    fn flush_if_due(&mut self, now_ms: i64) -> io::Result<()> {
        if self
            .dirty_since_ms
            .is_some_and(|since| now_ms - since >= FLUSH_EVERY_MS)
        {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        self.dirty_since_ms = None;
        Ok(())
    }
}

/// A `.deals` file, read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DealFile {
    pub symbol: String,
    pub day: String,
    pub samples: Vec<DealSample>,
    /// Bytes up to the end of the last complete line. A file cut mid-line
    /// by a crash reads back to here, and a writer resumes from here rather
    /// than after the torn line.
    pub complete_bytes: u64,
}

/// Read one recording. A line the format does not describe is an error
/// naming the line, never a sample guessed around it — except a torn last
/// line, which is what a crash mid-write leaves and carries no sample: the
/// file reads back to the line before it, and [`DealFile::complete_bytes`]
/// says where a writer resumes.
pub fn read_file(path: &Path) -> io::Result<DealFile> {
    let mut reader = BufReader::new(File::open(path)?);
    let bad = |line_no: usize, what: &str| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: line {line_no}: {what}", path.display()),
        )
    };
    let mut buf = String::new();
    let mut header_bytes = reader.read_line(&mut buf)?;
    if header_bytes == 0 {
        return Err(bad(1, "empty file"));
    }
    let header = buf.trim_end_matches(['\r', '\n']).to_owned();
    let Some(fields) = header.strip_prefix(HEADER) else {
        return Err(bad(1, "not a quantick-deals v1 file"));
    };
    let mut symbol = None;
    let mut day = None;
    for field in fields.split_whitespace() {
        match field.split_once('=') {
            Some(("symbol", value)) => symbol = Some(value.to_owned()),
            Some(("day", value)) => day = Some(value.to_owned()),
            _ => {}
        }
    }
    let (Some(symbol), Some(day)) = (symbol, day) else {
        return Err(bad(1, "header names no symbol or day"));
    };
    if !buf.ends_with('\n') {
        header_bytes = 0;
    }
    let mut complete_bytes = header_bytes as u64;
    let mut samples: Vec<DealSample> = Vec::new();
    let mut line_no = 1;
    let mut torn: Option<io::Error> = None;
    loop {
        buf.clear();
        let read = reader.read_line(&mut buf)?;
        if read == 0 {
            break;
        }
        line_no += 1;
        if let Some(error) = torn.take() {
            // A bad line followed by another line is corruption, not a torn
            // tail.
            return Err(error);
        }
        let line = buf.trim();
        let terminated = buf.ends_with('\n');
        if line.is_empty() || line.starts_with('#') {
            if terminated {
                complete_bytes += read as u64;
            }
            continue;
        }
        let parsed = parse_sample_line(line, samples.last()).map_err(|what| bad(line_no, what));
        match parsed {
            Ok(sample) if terminated => {
                samples.push(sample);
                complete_bytes += read as u64;
            }
            // An unterminated last line is a torn write even when it parses:
            // its digits may be half of the number. A terminated line that
            // does not parse was written whole, and is corruption.
            Ok(_) => torn = Some(bad(line_no, "unterminated line")),
            Err(error) if !terminated => torn = Some(error),
            Err(error) => return Err(error),
        }
    }
    Ok(DealFile {
        symbol,
        day,
        samples,
        complete_bytes,
    })
}

/// One data line, absolute or a delta against `previous`.
fn parse_sample_line(
    line: &str,
    previous: Option<&DealSample>,
) -> Result<DealSample, &'static str> {
    let Some((time, deals)) = line.split_once(' ') else {
        return Err("expected two fields");
    };
    if let Some(delta_t) = time.strip_prefix('+') {
        let Some(last) = previous else {
            return Err("a delta line before any absolute line");
        };
        let dt: i64 = delta_t.parse().map_err(|_| "bad time delta")?;
        let (sign, magnitude) = match deals.split_at_checked(1) {
            Some(("+", rest)) => (1_i64, rest),
            Some(("-", rest)) => (-1_i64, rest),
            _ => return Err("bad deal delta"),
        };
        let dd: i64 = magnitude.parse().map_err(|_| "bad deal delta")?;
        let deals = i64::try_from(last.session_deals)
            .ok()
            .and_then(|d| d.checked_add(sign * dd))
            .and_then(|d| u64::try_from(d).ok())
            .ok_or("deal delta out of range")?;
        Ok(DealSample {
            time_ms: last.time_ms.saturating_add(dt),
            session_deals: deals,
        })
    } else {
        Ok(DealSample {
            time_ms: time.parse().map_err(|_| "bad time")?,
            session_deals: deals.parse().map_err(|_| "bad deal count")?,
        })
    }
}

/// What a scan remembers per file, so a stop re-reads only the file it
/// closed: a day file is a million delta lines, and a folder holds a
/// month of them.
pub type DayCache = BTreeMap<PathBuf, (u64, Option<SystemTime>, RecordedDay)>;

/// Every recorded day under `dir/symbol`, oldest first. Unreadable files are
/// left out rather than shown as days with numbers nobody can trust. A file
/// whose size and modification time the cache already knows is not parsed
/// again.
#[must_use]
pub fn scan_days(dir: &Path, symbol: &str, cache: &mut DayCache) -> Rc<[RecordedDay]> {
    let Ok(entries) = fs::read_dir(dir.join(symbol)) else {
        return Rc::from(Vec::new());
    };
    let mut days = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
            continue;
        }
        let stamp = entry
            .metadata()
            .map(|meta| (meta.len(), meta.modified().ok()))
            .unwrap_or((0, None));
        let day = match cache.get(&path) {
            Some((len, modified, day)) if (*len, *modified) == stamp => day.clone(),
            _ => {
                let Ok(file) = read_file(&path) else { continue };
                let (Some(first), Some(last)) = (file.samples.first(), file.samples.last()) else {
                    continue;
                };
                let day = RecordedDay {
                    day: file.day,
                    first: *first,
                    last: *last,
                    samples: file.samples.len() as u64,
                    path: path.clone(),
                };
                cache.insert(path, (stamp.0, stamp.1, day.clone()));
                day
            }
        };
        days.insert(day.day.clone(), day);
    }
    Rc::from(days.into_values().collect::<Vec<_>>())
}

/// The recorder for one asset: what it is doing, and what it wrote.
#[derive(Debug)]
pub struct DealRecorder {
    /// The feed this recorder was built for; see [`Self::is_for`].
    feed_id: String,
    symbol: String,
    dir: PathBuf,
    tz_minutes: i32,
    /// The display offset the open file's day was named in, kept for as
    /// long as that file is open: a display timezone changed mid-session
    /// must not rotate a running recording into a second file.
    file_tz_minutes: i32,
    /// The feed says its prints come with a deal counter.
    available: bool,
    /// Start on the first frame the feed can count, unless already decided.
    default_on: bool,
    /// The default has been applied (or declined by a hand); never twice.
    auto_started: bool,
    enabled: bool,
    /// The first reading of this run, written or not.
    first_reading: Option<DealSample>,
    /// The newest reading of this run.
    latest: Option<DealSample>,
    /// Where the open file starts: its first line, resumed or written. What
    /// the button and the chip call "since".
    recording_since_ms: Option<i64>,
    recording: Option<Recording>,
    error: Option<String>,
    /// Shared with every view handed out this frame: a clone is a refcount.
    days: Rc<[RecordedDay]>,
    /// What the last scan parsed, keyed by file, so the next parses less.
    day_cache: DayCache,
    loaded_days: Vec<String>,
    /// Built by the app for this tab's market, as opposed to the placeholder
    /// a tab is constructed with. See [`Self::is_for`].
    configured: bool,
    /// The live market's readings, put aside while a replay holds the
    /// panes, to go back when the market does. See [`Self::stash`].
    stash: Vec<DealSample>,
}

impl DealRecorder {
    /// A recorder for `symbol`, writing under `dir`, opening on `default_on`.
    #[must_use]
    pub fn new(symbol: impl Into<String>, dir: PathBuf, default_on: bool) -> Self {
        Self::with_cache(symbol, dir, default_on, DayCache::new())
    }

    /// [`Self::new`] carrying the scan cache of the recorder it replaces, so
    /// a market switch does not re-parse a month of day files.
    #[must_use]
    pub fn with_cache(
        symbol: impl Into<String>,
        dir: PathBuf,
        default_on: bool,
        mut day_cache: DayCache,
    ) -> Self {
        let symbol = symbol.into();
        let days = scan_days(&dir, &symbol, &mut day_cache);
        Self {
            feed_id: String::new(),
            symbol,
            dir,
            tz_minutes: 0,
            file_tz_minutes: 0,
            available: false,
            default_on,
            auto_started: false,
            enabled: false,
            first_reading: None,
            latest: None,
            recording_since_ms: None,
            recording: None,
            error: None,
            days,
            day_cache,
            loaded_days: Vec::new(),
            configured: true,
            stash: Vec::new(),
        }
    }

    /// Put the live market's readings aside: a replay is another day's
    /// prints and must not join to them, and the reset that clears the panes
    /// for it would otherwise lose a morning counted with REC off.
    pub fn stash(&mut self, readings: Vec<DealSample>) {
        // A replay opened over a replay hands in the first one's empty
        // panes; the live readings already put aside are the ones to keep.
        if !readings.is_empty() || self.stash.is_empty() {
            self.stash = readings;
        }
    }

    /// The readings put aside, for the panes now that the market is back.
    pub fn take_stash(&mut self) -> Vec<DealSample> {
        std::mem::take(&mut self.stash)
    }

    /// The scan cache, handed to the recorder that replaces this one.
    pub fn take_day_cache(&mut self) -> DayCache {
        std::mem::take(&mut self.day_cache)
    }

    /// The recorder a tab is constructed with, before the app knows which
    /// market it streams: records nothing, scans nothing, and answers
    /// [`Self::is_for`] with `false` so the app replaces it.
    #[must_use]
    pub fn placeholder(symbol: impl Into<String>) -> Self {
        let mut recorder = Self::new(symbol, PathBuf::new(), false);
        recorder.configured = false;
        recorder
    }

    /// Whether this recorder was built for `symbol` — the check the app
    /// runs every frame so a restored tab, a market switch and a fresh tab
    /// all end up with a recorder for the market they stream.
    #[must_use]
    pub fn is_for(&self, feed_id: &str, symbol: &str) -> bool {
        self.configured && self.feed_id == feed_id && self.symbol == symbol
    }

    /// Name the feed this recorder serves. A tab that switches feed under
    /// the same symbol gets a new recorder: the default and the counter are
    /// the feed's, and two feeds must not append to one file.
    #[must_use]
    pub fn for_feed(mut self, feed_id: impl Into<String>) -> Self {
        self.feed_id = feed_id.into();
        self
    }

    /// The display timezone the day names and the readouts use.
    pub fn set_timezone(&mut self, tz_minutes: i32) {
        self.tz_minutes = tz_minutes;
    }

    /// What the feed said about its counter, re-read every frame.
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    /// Change the default; a recorder that already decided is left alone.
    pub fn set_default(&mut self, default_on: bool) {
        self.default_on = default_on;
    }

    /// Whether the default asks this frame to start recording.
    #[must_use]
    pub fn auto_start_due(&self) -> bool {
        self.available && self.default_on && !self.auto_started && !self.enabled
    }

    /// Start recording. Returns the readings today's file already held, for
    /// the caller to hand to its panes — a restart resumes the day.
    pub fn start(&mut self, now_ms: i64) -> Vec<DealSample> {
        if !self.configured {
            // A placeholder has no folder: writing would land beside the
            // working directory. Said, and refused, until the app hands the
            // tab its real recorder.
            self.error = Some("the recorder is not configured for this market yet".to_owned());
            return Vec::new();
        }
        if self.enabled {
            return Vec::new();
        }
        if !self.available {
            // Reachable from the control plane, where a recorded day on disk
            // keeps the capability answering; the button never offers it.
            self.error = Some("this source declares no deal counter; nothing to record".to_owned());
            return Vec::new();
        }
        // Decided only now: a start refused above — before the hello, on a
        // placeholder — must leave the config's default to fire later.
        self.auto_started = true;
        self.enabled = true;
        self.error = None;
        // The tape's day while the reading in hand is current, the clock's
        // otherwise: a file is named for the day its readings belong to, and
        // a reading left over from last night names yesterday.
        let current = self
            .latest
            .filter(|s| now_ms.saturating_sub(s.time_ms) <= STALE_AFTER_MS);
        let day = day_of(current.map_or(now_ms, |s| s.time_ms), self.tz_minutes);
        let resumed = self.open_day(&day);
        self.recording_since_ms = resumed.first().map(|s| s.time_ms);
        if self.recording.is_some()
            && let Some(latest) = current
        {
            // The reading already in hand — drained in the same frame as
            // the hello, before the default fired — is the file's first
            // line, or a restart resumes from the next one and the first
            // window is uncounted.
            self.append(latest, now_ms);
        }
        if self.recording.is_none() {
            // The open failed and said why. Not recording, then — the
            // surfaces say Off with the reason in the popover, and REC
            // pressed again retries, instead of a red button over readings
            // being dropped.
            self.enabled = false;
        }
        resumed
    }

    /// Stop recording and flush. The file stays; the day is partial.
    pub fn stop(&mut self) {
        self.auto_started = true;
        self.enabled = false;
        self.recording_since_ms = None;
        self.close_recording();
    }

    fn open_day(&mut self, day: &str) -> Vec<DealSample> {
        self.close_recording();
        match Recording::open(&self.dir, &self.symbol, day, self.tz_minutes) {
            Ok((recording, existing)) => {
                // A resumed day is the recording, not a loaded one: its
                // readings reach the panes, and a later Stop leaves the chart
                // counting live rather than "from a recording".
                self.recording = Some(recording);
                self.file_tz_minutes = self.tz_minutes;
                existing
            }
            Err(error) => {
                self.error = Some(format!("cannot write the recording: {error}"));
                Vec::new()
            }
        }
    }

    fn close_recording(&mut self) {
        if let Some(mut recording) = self.recording.take() {
            if let Err(error) = recording.flush() {
                self.error = Some(format!("cannot write the recording: {error}"));
            }
            // What the scan would read the whole file back for, this
            // recorder already knows: the entry goes into the cache under
            // the flushed file's stamp, and the scan finds it there instead
            // of parsing a day of lines on the interface's thread.
            if let (Some(first), Some(last)) = (recording.first, recording.last) {
                let stamp = fs::metadata(&recording.path)
                    .map(|meta| (meta.len(), meta.modified().ok()))
                    .unwrap_or((0, None));
                let day = RecordedDay {
                    day: recording.day.clone(),
                    first,
                    last,
                    samples: recording.held + recording.written,
                    path: recording.path.clone(),
                };
                self.day_cache
                    .insert(recording.path.clone(), (stamp.0, stamp.1, day));
            }
        }
        self.days = scan_days(&self.dir, &self.symbol, &mut self.day_cache);
    }

    /// One reading from the feed, at wall-clock `now_ms`. Returns the
    /// readings a day file already held when this reading rotated into it
    /// — a resumed day, like [`Self::start`] — for the caller's panes.
    pub fn observe(&mut self, sample: DealSample, now_ms: i64) -> Vec<DealSample> {
        self.first_reading.get_or_insert(sample);
        self.latest = Some(sample);
        if !self.enabled {
            return Vec::new();
        }
        // Judged in the open file's own offset while one is open, so a
        // display timezone changed mid-session names the *next* day's file
        // and never rotates this one.
        let tz_minutes = if self.recording.is_some() {
            self.file_tz_minutes
        } else {
            self.tz_minutes
        };
        let day = day_of(sample.time_ms, tz_minutes);
        if self.recording.as_ref().is_none_or(|r| r.day != day) {
            // Midnight in the display timezone: the day's own file, opened
            // now. An open that fails turns recording off with its reason,
            // so no reading retries it fifty times a second; REC, pressed
            // again, retries once.
            let resumed = self.open_day(&day);
            self.recording_since_ms = resumed.first().map(|s| s.time_ms);
            if self.recording.is_none() {
                self.enabled = false;
                return resumed;
            }
            self.append(sample, now_ms);
            return resumed;
        }
        self.append(sample, now_ms);
        Vec::new()
    }

    fn append(&mut self, sample: DealSample, now_ms: i64) {
        if let Some(recording) = self.recording.as_mut() {
            match recording.append(sample, now_ms) {
                Ok(()) => {
                    self.recording_since_ms.get_or_insert(sample.time_ms);
                }
                Err(error) => {
                    self.error = Some(format!("cannot write the recording: {error}"));
                }
            }
        }
    }

    /// Reach the disk once a second while recording.
    pub fn flush_if_due(&mut self, now_ms: i64) {
        if let Some(recording) = self.recording.as_mut()
            && let Err(error) = recording.flush_if_due(now_ms)
        {
            self.error = Some(format!("cannot write the recording: {error}"));
        }
    }

    /// Read a recorded day's readings for the caller's panes, by index into
    /// [`Self::days`].
    pub fn load_day(&mut self, index: usize) -> Vec<DealSample> {
        let Some(day) = self.days.get(index).cloned() else {
            return Vec::new();
        };
        if self.recording.as_ref().is_some_and(|r| r.day == day.day) {
            // The popover hides this row; the control plane does not. The
            // day being recorded is on the chart already, and marking it
            // loaded would read "recorded" after a Stop while live readings
            // keep cutting.
            self.error = Some(format!(
                "{} is the day being recorded; its readings are already on the chart",
                day.day
            ));
            return Vec::new();
        }
        match read_file(&day.path) {
            Ok(file) => {
                if !self.loaded_days.contains(&file.day) {
                    self.loaded_days.push(file.day);
                }
                self.error = None;
                file.samples
            }
            Err(error) => {
                self.error = Some(format!("cannot read the recording: {error}"));
                Vec::new()
            }
        }
    }

    /// The state, judged on the tape's own clock: `latest_trade_ms` is the
    /// newest print the tab holds. A counter is stale when prints keep
    /// arriving [`STALE_AFTER_MS`] past the newest reading — no wall clock,
    /// so a quiet tape is never mistaken for a stopped counter.
    #[must_use]
    pub fn state(&self, latest_trade_ms: Option<i64>) -> RecState {
        // Writing first: a feed reload resets the capabilities until the
        // next hello, and a recorder still appending must not read as
        // "unsupported" for the seconds in between.
        if !self.enabled {
            return match (self.loaded_days.is_empty(), self.available) {
                (false, _) => RecState::Recorded,
                (true, false) => RecState::Unsupported,
                (true, true) => RecState::Off,
            };
        }
        if self.counter_stale(latest_trade_ms) {
            RecState::Stale
        } else {
            RecState::Recording
        }
    }

    /// The counter stands still while prints keep coming: judged once, for
    /// the button, the chip, the cell and the wire alike, REC on or off.
    fn counter_stale(&self, latest_trade_ms: Option<i64>) -> bool {
        self.counter_age_ms(latest_trade_ms)
            .is_some_and(|age| age >= STALE_AFTER_MS)
    }

    /// How far the tape has moved past the newest reading, on the tape's
    /// clock; none before the first reading or the first print.
    fn counter_age_ms(&self, latest_trade_ms: Option<i64>) -> Option<i64> {
        let newest = self.latest?.time_ms;
        latest_trade_ms.map(|trade| trade.saturating_sub(newest).max(0))
    }

    /// Everything a surface draws, in one value.
    #[must_use]
    pub fn view(&self, latest_trade_ms: Option<i64>) -> RecordingView {
        RecordingView {
            symbol: self.symbol.clone(),
            state: self.state(latest_trade_ms),
            reading: self.latest.map(|s| s.session_deals),
            since_ms: self.recording_since_ms,
            first_reading_ms: self.first_reading.map(|s| s.time_ms),
            counter_age_ms: self.counter_age_ms(latest_trade_ms),
            counter_stale: self.counter_stale(latest_trade_ms),
            default_on: self.default_on,
            written: self.recording.as_ref().map_or(0, |r| r.written),
            path: self.recording.as_ref().map(|r| r.path.clone()),
            dir: self.dir.clone(),
            error: self.error.clone(),
            days: self.days.clone(),
            loaded_days: self.loaded_days.clone(),
            tz_minutes: self.tz_minutes,
        }
    }
}

/// What every REC surface reads: the toolbar button and its popover, the
/// chart-corner chip, the status cell and the control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingView {
    pub symbol: String,
    pub state: RecState,
    /// The newest reading, if any arrived this session.
    pub reading: Option<u64>,
    /// Where the open file starts, on the tape's clock — the recording's
    /// own "since", resumed or written this run.
    pub since_ms: Option<i64>,
    /// When the first reading of this run arrived, written or not.
    pub first_reading_ms: Option<i64>,
    /// How far the tape has moved past the newest reading.
    pub counter_age_ms: Option<i64>,
    /// Lines written to the open file this run.
    pub written: u64,
    /// The file being written, while recording.
    pub path: Option<PathBuf>,
    pub dir: PathBuf,
    pub error: Option<String>,
    /// The recorded days, oldest first — shared, not copied, per view.
    pub days: Rc<[RecordedDay]>,
    /// Days whose readings were loaded into the panes this session.
    pub loaded_days: Vec<String>,
    pub tz_minutes: i32,
    /// The standing choice this recorder opens on: record by default.
    pub default_on: bool,
    /// The counter has not moved for [`STALE_AFTER_MS`] of tape while
    /// prints kept coming — REC on or off. With `reading == Some(0)` it
    /// never moved at all.
    pub counter_stale: bool,
}

impl RecordingView {
    /// Whether the feed offers a REC control at all.
    #[must_use]
    pub fn supported(&self) -> bool {
        // A recorded day on disk is reachable with no bridge connected —
        // a weekend, the pre-open — so the control that lists it is drawn.
        self.state != RecState::Unsupported || !self.days.is_empty()
    }

    /// Whether a `trades` pane has anything to cut on right now.
    #[must_use]
    pub fn deal_count_available(&self) -> bool {
        // A resumed file's readings are on the chart before a live reading.
        self.reading.is_some() || self.since_ms.is_some() || !self.loaded_days.is_empty()
    }

    /// The button's own text: `REC`, `REC 2 301 455 · 09:00:00`,
    /// `REC · counter stale 4 s`, `RECORDED · 2026-09-03`.
    #[must_use]
    pub fn button_label(&self) -> String {
        match self.state {
            RecState::Unsupported | RecState::Off => "REC".to_owned(),
            RecState::Recording => match (self.reading, self.since_ms) {
                (Some(reading), Some(since)) => format!(
                    "REC {} · {}",
                    fmt_count(reading),
                    fmt_hms(since, self.tz_minutes)
                ),
                _ => "REC · waiting for the counter".to_owned(),
            },
            RecState::Stale if self.reading == Some(0) => "REC · counter stuck at 0".to_owned(),
            RecState::Stale => format!(
                "REC · counter stale {} s",
                self.counter_age_ms.unwrap_or(0) / 1000
            ),
            RecState::Recorded => format!(
                "RECORDED · {}",
                self.loaded_days.last().map_or("", String::as_str)
            ),
        }
    }

    /// The status bar's cell, or none where there is no REC.
    #[must_use]
    pub fn status_cell(&self) -> Option<String> {
        match self.state {
            RecState::Unsupported => None,
            RecState::Off => Some("REC off".to_owned()),
            RecState::Recording | RecState::Stale => Some(format!(
                "{}{}",
                self.button_label(),
                self.path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|name| format!(" · {}", name.to_string_lossy()))
                    .unwrap_or_default()
            )),
            RecState::Recorded => Some(self.button_label()),
        }
    }

    /// The sentence the popover opens with.
    #[must_use]
    pub fn headline(&self) -> String {
        match self.state {
            RecState::Unsupported => "This source has no deal counter".to_owned(),
            RecState::Off => format!("Not recording {} deals", self.symbol),
            RecState::Recording => format!("Recording {} deals", self.symbol),
            RecState::Stale if self.reading == Some(0) => format!(
                "Recording {} deals — the counter never moved; this broker may not report it",
                self.symbol
            ),
            RecState::Stale => format!("Recording {} deals — the counter stopped", self.symbol),
            RecState::Recorded => format!("{} deals from a recording", self.symbol),
        }
    }
}

/// `YYYY-MM-DD` of `time_ms` in the display timezone.
#[must_use]
pub fn day_of(time_ms: i64, tz_minutes: i32) -> String {
    // The paper journal's own day: one rule for which day a print belongs to.
    crate::paper_calendar::CivilDate::from_ms(time_ms, crate::timezone::TzOffset::new(tz_minutes))
        .iso()
}

/// `HH:MM:SS` of `time_ms` in the display timezone.
#[must_use]
pub fn fmt_hms(time_ms: i64, tz_minutes: i32) -> String {
    // The time axis's own formatter: one clock, one spelling.
    crate::plot_area::fmt_time(time_ms, crate::timezone::TzOffset::new(tz_minutes))
}

/// `2301455` as `2 301 455` — the grouping the mock uses, readable at a
/// glance where a bare seven-digit number is not.
#[must_use]
pub fn fmt_count(n: u64) -> String {
    // The replay browser's grouping, so a count reads the same everywhere.
    crate::replay_view::thousands(usize::try_from(n).unwrap_or(usize::MAX))
}

crate::hooks::declare_hooks!["QUANTICK_DEALS_DIR", "QUANTICK_DEAL_RECORDING"];

#[cfg(test)]
mod tests {
    use super::*;

    use crate::scratch::ScratchDir;

    /// A recording folder of the test's own, gone with the value.
    fn scratch(name: &str) -> ScratchDir {
        ScratchDir::new(&format!("deals-{name}"))
    }

    fn sample(time_ms: i64, session_deals: u64) -> DealSample {
        DealSample {
            time_ms,
            session_deals,
        }
    }

    /// 2026-09-04 02:00:00 UTC: 23:00:00 on the 3rd in UTC-3.
    const LATE_EVENING_BRT_MS: i64 = 1_788_487_200_000;

    /// Two feeds can list one symbol; a tab that switches feed under it gets
    /// a new recorder rather than appending the second feed's counter to the
    /// first one's file.
    /// A recorder still writing says so across a feed reload, which resets
    /// the capabilities until the next hello.
    #[test]
    fn a_reload_that_forgets_the_counter_for_a_moment_keeps_recording() {
        let dir = scratch("reload-state");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_available(true);
        rec.start(LATE_EVENING_BRT_MS);
        rec.observe(sample(LATE_EVENING_BRT_MS, 10), LATE_EVENING_BRT_MS);
        rec.set_available(false);
        assert_eq!(rec.state(None), RecState::Recording);
        rec.stop();
        assert_eq!(rec.state(None), RecState::Unsupported);
    }

    /// A source that declares no counter has nothing to record: a start
    /// reached through the control plane is refused with its reason.
    #[test]
    fn a_start_on_a_source_with_no_counter_is_refused() {
        let dir = scratch("no-counter-start");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_available(false);
        assert!(rec.start(LATE_EVENING_BRT_MS).is_empty());
        assert_eq!(rec.state(None), RecState::Unsupported);
        assert!(rec.view(None).error.is_some());
        assert!(rec.view(None).path.is_none(), "no file was opened");
    }

    /// A reading left over from last night does not name today's file.
    #[test]
    fn a_stale_reading_does_not_name_the_days_file() {
        let dir = scratch("stale-day");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        rec.set_available(true);
        rec.observe(sample(LATE_EVENING_BRT_MS, 10), LATE_EVENING_BRT_MS);
        // The next morning, 08:50 in UTC-3, REC is pressed by hand.
        let morning = LATE_EVENING_BRT_MS + 9 * 3_600_000 + 50 * 60_000;
        rec.start(morning);
        let view = rec.view(None);
        let name = view.path.as_ref().and_then(|p| p.file_name()).unwrap();
        assert_eq!(name.to_string_lossy(), "2026-09-04.deals");
    }

    /// Closing a day lists it from what the recorder wrote, and the list
    /// says exactly what a scan that read the file back would.
    #[test]
    fn a_closed_day_is_listed_without_reading_it_back() {
        let dir = scratch("close-cache");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        rec.set_available(true);
        rec.start(LATE_EVENING_BRT_MS);
        for i in 0..5 {
            rec.observe(sample(LATE_EVENING_BRT_MS + i * 1_000, 10 + i as u64), 0);
        }
        rec.stop();
        let listed = rec.view(None).days.to_vec();
        let mut cold = DayCache::new();
        let scanned = scan_days(dir.path(), "WINV26", &mut cold).to_vec();
        assert_eq!(listed, scanned);
        assert_eq!(listed[0].samples, 5);
    }

    /// The readings a replay puts aside come back whole.
    #[test]
    fn the_stash_round_trips_and_an_empty_one_keeps_what_was_put_aside() {
        let mut rec = DealRecorder::placeholder("WINV26");
        rec.stash(vec![sample(1, 1), sample(2, 2)]);
        // A second replay opened over the first hands in empty panes.
        rec.stash(Vec::new());
        assert_eq!(rec.take_stash(), vec![sample(1, 1), sample(2, 2)]);
        assert!(rec.take_stash().is_empty());
    }

    /// A start refused before the hello does not spend the config's
    /// default: the counter arrives, and the default fires as configured.
    #[test]
    fn a_refused_start_does_not_spend_the_default() {
        let dir = scratch("refused-default");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), true);
        rec.set_available(false);
        assert!(rec.start(LATE_EVENING_BRT_MS).is_empty());
        rec.set_available(true);
        assert!(rec.auto_start_due(), "the default is still to be applied");
    }

    /// A reading that rotates the recording into a day whose file exists
    /// hands that file's readings back, as a start does: the panes hold
    /// what the file holds.
    #[test]
    fn a_rotation_into_an_existing_day_hands_its_readings_back() {
        let dir = scratch("rotate-existing");
        let (mut earlier, _) = Recording::open(dir.path(), "WINV26", "2026-09-04", -180).unwrap();
        earlier
            .append(sample(LATE_EVENING_BRT_MS + 4 * 3_600_000, 7), 0)
            .unwrap();
        earlier.flush().unwrap();
        drop(earlier);

        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        rec.set_available(true);
        rec.start(LATE_EVENING_BRT_MS);
        assert!(rec.observe(sample(LATE_EVENING_BRT_MS, 10), 0).is_empty());
        // 01:00 on the 4th in UTC-3: the next day's file, which exists.
        let resumed = rec.observe(sample(LATE_EVENING_BRT_MS + 2 * 3_600_000, 12), 0);
        assert_eq!(
            resumed,
            vec![sample(LATE_EVENING_BRT_MS + 4 * 3_600_000, 7)]
        );
    }

    /// The day being recorded cannot be loaded as a recorded day: its
    /// readings are on the chart, and "recorded" after a Stop would lie.
    #[test]
    fn the_day_being_recorded_is_not_loadable() {
        let dir = scratch("load-recording-day");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        rec.set_available(true);
        rec.start(LATE_EVENING_BRT_MS);
        rec.observe(sample(LATE_EVENING_BRT_MS, 10), 0);
        rec.stop();
        assert_eq!(rec.view(None).days.len(), 1);
        rec.start(LATE_EVENING_BRT_MS + 1_000);
        assert!(rec.load_day(0).is_empty());
        assert!(rec.view(None).loaded_days.is_empty());
        assert!(rec.view(None).error.is_some());
        assert_eq!(rec.state(None), RecState::Recording);
    }

    /// A reading drained before the default fired — the same frame as the
    /// hello — is the file's first line, not the one a restart starts after.
    #[test]
    fn a_start_writes_the_reading_already_in_hand() {
        let dir = scratch("start-writes-latest");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), true);
        rec.set_timezone(-180);
        rec.set_available(true);
        rec.observe(sample(LATE_EVENING_BRT_MS, 10), LATE_EVENING_BRT_MS);
        assert!(rec.auto_start_due());
        rec.start(LATE_EVENING_BRT_MS + 20);
        rec.observe(
            sample(LATE_EVENING_BRT_MS + 31_000, 12),
            LATE_EVENING_BRT_MS + 31_000,
        );
        rec.stop();
        let path = dir.path().join("WINV26").join("2026-09-03.deals");
        let file = read_file(&path).unwrap();
        assert_eq!(
            file.samples,
            vec![
                sample(LATE_EVENING_BRT_MS, 10),
                sample(LATE_EVENING_BRT_MS + 31_000, 12)
            ]
        );
    }

    /// A reading stamped behind the file's last line — a bridge restarted
    /// with another clock offset — is written, not skipped: the chart cuts
    /// from it, and the file holds what the chart holds.
    #[test]
    fn a_reading_behind_the_last_line_is_written() {
        let dir = scratch("behind-last");
        let (mut recording, _) = Recording::open(dir.path(), "WINV26", "2026-09-03", -180).unwrap();
        recording.append(sample(1_000_000, 10), 0).unwrap();
        recording.append(sample(100_000, 12), 0).unwrap();
        recording.flush().unwrap();
        let file = read_file(&dir.path().join("WINV26").join("2026-09-03.deals")).unwrap();
        assert_eq!(
            file.samples,
            vec![sample(1_000_000, 10), sample(100_000, 12)]
        );
    }

    #[test]
    fn a_recorder_is_for_one_feed_and_one_symbol() {
        let dir = scratch("feed-key");
        let rec =
            DealRecorder::new("WINV26", dir.path().to_path_buf(), false).for_feed("metatrader-b3");
        assert!(rec.is_for("metatrader-b3", "WINV26"));
        assert!(!rec.is_for("metatrader-tickmill", "WINV26"));
        assert!(!rec.is_for("metatrader-b3", "WINQ26"));
        assert!(!DealRecorder::placeholder("WINV26").is_for("", "WINV26"));
    }

    /// A restart resumes today's file; Stop afterwards is Off — the chart
    /// keeps counting live — never "recorded", which says nothing is.
    #[test]
    fn a_stop_after_a_resume_is_off_not_recorded() {
        let dir = scratch("resume-stop");
        let day = day_of(LATE_EVENING_BRT_MS, -180);
        let (mut earlier, _) = Recording::open(dir.path(), "WINV26", &day, -180).unwrap();
        earlier.append(sample(LATE_EVENING_BRT_MS, 10), 0).unwrap();
        earlier.flush().unwrap();
        drop(earlier);

        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        rec.set_available(true);
        let resumed = rec.start(LATE_EVENING_BRT_MS + 60_000);
        assert_eq!(resumed, vec![sample(LATE_EVENING_BRT_MS, 10)]);
        assert!(
            rec.view(None).loaded_days.is_empty(),
            "a resumed day is not a loaded one"
        );
        assert!(
            rec.view(None).deal_count_available(),
            "its readings are on the chart"
        );
        rec.stop();
        assert_eq!(rec.state(None), RecState::Off);
    }

    /// The display timezone names the day's file when it opens; changing it
    /// while the file is open does not rotate the recording into a second
    /// file for the same session.
    #[test]
    fn a_timezone_change_mid_session_keeps_the_days_file() {
        let dir = scratch("tz-change");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        rec.set_available(true);
        rec.start(LATE_EVENING_BRT_MS);
        rec.observe(sample(LATE_EVENING_BRT_MS, 10), LATE_EVENING_BRT_MS);
        // The trader switches the view to UTC, where it is already the 4th.
        rec.set_timezone(0);
        rec.observe(
            sample(LATE_EVENING_BRT_MS + 1_000, 12),
            LATE_EVENING_BRT_MS + 1_000,
        );
        let view = rec.view(None);
        let name = view.path.as_ref().and_then(|p| p.file_name()).unwrap();
        assert_eq!(name.to_string_lossy(), "2026-09-03.deals");
        assert_eq!(view.written, 2, "both readings went to the one file");
    }

    /// With no bridge connected — a weekend, the pre-open — a day recorded
    /// earlier is still listed and still opens.
    #[test]
    fn a_recorded_day_opens_with_no_counter_declared() {
        let dir = scratch("offline-day");
        let (mut earlier, _) = Recording::open(dir.path(), "WINV26", "2026-09-03", -180).unwrap();
        earlier.append(sample(1_788_436_800_000, 10), 0).unwrap();
        earlier.flush().unwrap();
        drop(earlier);

        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), true);
        rec.set_available(false);
        let view = rec.view(None);
        assert_eq!(view.state, RecState::Unsupported);
        assert!(view.supported(), "the day on disk keeps the control drawn");
        assert_eq!(view.days.len(), 1);
        assert!(!rec.auto_start_due(), "nothing to record without a counter");

        let loaded = rec.load_day(0);
        assert_eq!(loaded, vec![sample(1_788_436_800_000, 10)]);
        assert_eq!(rec.state(None), RecState::Recorded);
    }

    #[test]
    fn counts_and_clocks_read_as_the_mock_writes_them() {
        assert_eq!(fmt_count(0), "0");
        assert_eq!(fmt_count(999), "999");
        assert_eq!(fmt_count(2_301_455), "2 301 455");
        // 2026-09-03 12:00:00 UTC is 09:00:00 in UTC-3.
        assert_eq!(fmt_hms(1_788_436_800_000, -180), "09:00:00");
        assert_eq!(day_of(1_788_436_800_000, -180), "2026-09-03");
        // Two hours after midnight UTC is still the day before in UTC-3.
        assert_eq!(day_of(1_788_487_200_000, -180), "2026-09-03");
        assert_eq!(day_of(1_788_487_200_000, 0), "2026-09-04");
    }

    #[test]
    fn a_file_round_trips_through_the_delta_encoding() {
        let dir = scratch("roundtrip");
        let (mut recording, existing) =
            Recording::open(&dir, "WINV26", "2026-09-03", -180).unwrap();
        assert!(existing.is_empty());
        let written = [
            sample(1_788_436_967_023, 1_990),
            sample(1_788_436_967_043, 2_003),
            sample(1_788_436_967_100, 2_010),
            // A rollover: the counter went down.
            sample(1_788_436_967_200, 7),
        ];
        for s in written {
            recording.append(s, 0).unwrap();
        }
        recording.flush().unwrap();
        let read = read_file(&recording.path).unwrap();
        assert_eq!(read.symbol, "WINV26");
        assert_eq!(read.day, "2026-09-03");
        assert_eq!(read.samples, written);
        let text = fs::read_to_string(&recording.path).unwrap();
        assert!(
            text.starts_with("# quantick-deals v1 symbol=WINV26 day=2026-09-03 tz_minutes=-180\n")
        );
        assert!(
            text.contains("\n+20 +13\n"),
            "deltas, not absolutes: {text}"
        );
        assert!(
            text.contains("\n+100 -2003\n"),
            "a drop is written as one: {text}"
        );
    }

    #[test]
    fn a_restart_resumes_the_days_file_and_writes_no_line_twice() {
        let dir = scratch("resume");
        let now = 1_788_436_967_023; // 2026-09-03, 09:02:47 UTC-3
        let mut first = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        first.set_timezone(-180);
        first.set_available(true);
        assert!(first.start(now).is_empty(), "nothing recorded yet");
        first.observe(sample(now, 1_990), now);
        first.observe(sample(now + 20, 2_003), now + 20);
        first.stop();

        let mut second = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        second.set_timezone(-180);
        second.set_available(true);
        let resumed = second.start(now + 60_000);
        assert_eq!(resumed, vec![sample(now, 1_990), sample(now + 20, 2_003)]);
        // The feed re-delivers the reading the first run stopped at, then
        // moves on: only the new one reaches the file.
        second.observe(sample(now + 20, 2_003), now + 60_000);
        second.observe(sample(now + 40, 2_011), now + 60_000);
        second.stop();
        let read = read_file(&dir.join("WINV26").join("2026-09-03.deals")).unwrap();
        assert_eq!(
            read.samples,
            vec![
                sample(now, 1_990),
                sample(now + 20, 2_003),
                sample(now + 40, 2_011)
            ]
        );
    }

    #[test]
    fn the_scan_lists_days_with_their_coverage() {
        let dir = scratch("scan");
        let day1 = 1_788_436_800_000; // 2026-09-03 09:00:00 UTC-3
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        rec.set_available(true);
        rec.start(day1);
        rec.observe(sample(day1, 12), day1);
        rec.observe(sample(day1 + 33_903_000, 5_821_205), day1);
        // Midnight passes in the display timezone: a new file.
        let day2 = day1 + 86_400_000 + 3_600_000;
        rec.observe(sample(day2, 400_000), day2);
        rec.observe(sample(day2 + 1_000, 400_100), day2);
        rec.stop();

        let view = rec.view(None);
        let days = &view.days;
        assert_eq!(days.len(), 2, "{days:?}");
        assert_eq!(days[0].day, "2026-09-03");
        assert!(days[0].started_at_open());
        assert_eq!(
            days[0].coverage(-180),
            "09:00:00 – 18:25:03 · 5 821 205 deals"
        );
        assert_eq!(days[0].label(-180), "from open");
        assert_eq!(days[1].day, "2026-09-04");
        assert_eq!(days[1].label(-180), "from 10:00:00");

        let loaded = rec.load_day(0);
        assert_eq!(loaded.len(), 2);
        let view = rec.view(None);
        assert_eq!(view.state, RecState::Recorded);
        assert_eq!(view.button_label(), "RECORDED · 2026-09-03");
    }

    #[test]
    fn the_default_starts_once_and_a_hand_that_stopped_it_is_respected() {
        let dir = scratch("default");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), true);
        assert!(!rec.auto_start_due(), "not before the feed can count");
        rec.set_available(true);
        assert!(rec.auto_start_due());
        rec.start(0);
        assert_eq!(rec.state(None), RecState::Recording);
        assert!(!rec.auto_start_due());
        rec.stop();
        assert!(
            !rec.auto_start_due(),
            "stopped by a hand: the default does not restart it"
        );
    }

    #[test]
    fn the_states_say_what_every_surface_says() {
        let dir = scratch("states");
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_timezone(-180);
        assert_eq!(rec.state(None), RecState::Unsupported);
        assert_eq!(rec.view(None).status_cell(), None);
        rec.set_available(true);
        assert_eq!(rec.state(None), RecState::Off);
        assert_eq!(rec.view(None).button_label(), "REC");
        // Readings arrive before a hand presses REC: counted, not written.
        let open = 1_788_436_800_000; // 09:00:00 in UTC-3
        rec.observe(sample(open, 2_000_000), 1_000);
        assert_eq!(rec.view(Some(open)).state, RecState::Off);
        assert!(rec.view(None).first_reading_ms == Some(open));
        assert!(rec.view(None).since_ms.is_none(), "nothing written yet");
        rec.start(open + 12_960_000); // 12:36
        assert_eq!(
            rec.view(None).button_label(),
            "REC · waiting for the counter",
            "started, but no reading has been written yet"
        );
        let pressed = open + 12_960_000;
        rec.observe(sample(pressed, 2_301_455), 2_000);
        let view = rec.view(Some(pressed + 100));
        assert_eq!(view.state, RecState::Recording);
        assert_eq!(
            view.button_label(),
            "REC 2 301 455 · 12:36:00",
            "since is where the file starts, not the first reading of the run"
        );
        assert_eq!(view.first_reading_ms, Some(open));
        assert!(view.status_cell().unwrap().ends_with("2026-09-03.deals"));
        // The tape kept printing four seconds past the newest reading.
        let view = rec.view(Some(pressed + STALE_AFTER_MS));
        assert_eq!(view.state, RecState::Stale);
        assert_eq!(view.button_label(), "REC · counter stale 90 s");
        // A quiet tape is not a stale counter.
        assert_eq!(rec.state(Some(pressed + 900)), RecState::Recording);
        assert_eq!(rec.state(None), RecState::Recording);
        rec.stop();
        assert_eq!(rec.state(Some(pressed + STALE_AFTER_MS)), RecState::Off);
    }

    /// A header write that failed leaves an empty file; a day that never
    /// printed leaves a header-only file. The first is fresh again, the
    /// second keeps its one header.
    #[test]
    fn an_empty_file_is_fresh_and_a_header_only_file_keeps_one_header() {
        let dir = scratch("empty");
        fs::create_dir_all(dir.join("WINV26")).unwrap();
        let path = dir.join("WINV26").join("2026-09-03.deals");
        fs::write(&path, "").unwrap();
        let (recording, existing) = Recording::open(&dir, "WINV26", "2026-09-03", -180).unwrap();
        assert!(existing.is_empty());
        drop(recording);
        let (recording, _) = Recording::open(&dir, "WINV26", "2026-09-03", -180).unwrap();
        drop(recording);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches(HEADER).count(), 1, "{text}");
    }

    /// A crash in the middle of the header leaves an unterminated first
    /// line: the reader keeps nothing, the writer cuts it away and starts
    /// the day with one whole header, never none.
    /// A header cut before its fields — `# quantick-de` — is not a bad
    /// header the day refuses to open on; it is nothing, and the day starts
    /// over with one whole header.
    #[test]
    fn a_header_torn_before_its_fields_starts_the_day_over() {
        let dir = scratch("torn-prefix");
        fs::create_dir_all(dir.join("WINV26")).unwrap();
        let path = dir.join("WINV26").join("2026-09-03.deals");
        fs::write(&path, "# quantick-de").unwrap();
        let (mut recording, existing) =
            Recording::open(&dir, "WINV26", "2026-09-03", -180).unwrap();
        assert!(existing.is_empty());
        recording.append(sample(100, 10), 0).unwrap();
        recording.flush().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches(HEADER).count(), 1, "{text}");
        assert!(text.starts_with(HEADER), "{text}");
        assert_eq!(read_file(&path).unwrap().samples, vec![sample(100, 10)]);
    }

    #[test]
    fn a_torn_header_resumes_with_one_whole_header() {
        let dir = scratch("torn-header");
        fs::create_dir_all(dir.join("WINV26")).unwrap();
        let path = dir.join("WINV26").join("2026-09-03.deals");
        fs::write(&path, "# quantick-deals v1 symbol=WINV26 day=2026-09-0").unwrap();
        let (mut recording, existing) =
            Recording::open(&dir, "WINV26", "2026-09-03", -180).unwrap();
        assert!(existing.is_empty());
        recording.append(sample(100, 10), 0).unwrap();
        recording.flush().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches(HEADER).count(), 1, "{text}");
        assert!(
            text.ends_with(
                "
100 10
"
            ),
            "{text}"
        );
        assert_eq!(read_file(&path).unwrap().samples, vec![sample(100, 10)]);
    }

    #[test]
    fn the_hook_reads_three_words_and_nothing_else() {
        assert_eq!(RecordingHook::parse(Some("on")), Some(RecordingHook::On));
        assert_eq!(RecordingHook::parse(Some("off")), Some(RecordingHook::Off));
        assert_eq!(
            RecordingHook::parse(Some("menu")),
            Some(RecordingHook::Menu)
        );
        assert_eq!(RecordingHook::parse(Some("yes")), None);
        assert_eq!(RecordingHook::parse(None), None);
        assert_eq!(RecordingHook::Menu.default_override(), None);
        assert_eq!(RecordingHook::Off.default_override(), Some(false));
    }

    #[test]
    fn a_broken_file_is_refused_with_its_line_named() {
        let dir = scratch("broken");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.deals");
        fs::write(
            &path,
            "# quantick-deals v1 symbol=X day=2026-01-01\n+5 +1\n",
        )
        .unwrap();
        let error = read_file(&path).unwrap_err().to_string();
        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("before any absolute line"), "{error}");
    }

    /// A crash mid-write leaves a torn last line. The file reads back to
    /// the line before it, and a writer that resumes cuts the tail away so
    /// the next line lands after a complete one.
    #[test]
    fn a_torn_last_line_is_cut_away_rather_than_refusing_the_day() {
        let dir = scratch("torn");
        fs::create_dir_all(dir.join("WINV26")).unwrap();
        let path = dir.join("WINV26").join("2026-09-03.deals");
        fs::write(
            &path,
            "# quantick-deals v1 symbol=WINV26 day=2026-09-03 tz_minutes=-180\n100 10\n+20 +5\n+3",
        )
        .unwrap();
        let file = read_file(&path).unwrap();
        assert_eq!(file.samples, vec![sample(100, 10), sample(120, 15)]);
        let (mut recording, existing) =
            Recording::open(&dir, "WINV26", "2026-09-03", -180).unwrap();
        assert_eq!(existing.len(), 2);
        recording.append(sample(140, 21), 0).unwrap();
        recording.flush().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("+20 +5\n+20 +6\n"), "{text}");
    }

    /// Ticks share milliseconds across poll rounds, so two readings can
    /// carry one time. The live chart joined prints to both; the file keeps
    /// both, and the exact reading a resumed day re-delivers is kept once.
    #[test]
    fn two_readings_at_one_millisecond_are_both_written() {
        let dir = scratch("same-ms");
        let (mut recording, _) = Recording::open(&dir, "WINV26", "2026-09-03", -180).unwrap();
        recording.append(sample(100, 10), 0).unwrap();
        recording.append(sample(100, 10), 0).unwrap();
        recording.append(sample(100, 13), 0).unwrap();
        recording.append(sample(90, 20), 0).unwrap();
        recording.flush().unwrap();
        let read = read_file(&recording.path).unwrap();
        // The older reading is written too: the chart holds it, and the file
        // holds what the chart cut from, in arrival order.
        assert_eq!(
            read.samples,
            vec![sample(100, 10), sample(100, 13), sample(90, 20)]
        );
    }

    /// A file that cannot be opened is reported once, not retried on every
    /// reading — fifty rescans a second on the UI thread is a frozen chart.
    #[test]
    fn a_failed_open_is_not_retried_per_reading() {
        let dir = scratch("locked");
        // The symbol's folder is a file, so the folder cannot be created.
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("WINV26"), "not a folder").unwrap();
        let mut rec = DealRecorder::new("WINV26", dir.path().to_path_buf(), false);
        rec.set_available(true);
        rec.start(1_788_436_800_000);
        let first = rec
            .view(None)
            .error
            .clone()
            .expect("the open failed and said so");
        assert_eq!(rec.state(None), RecState::Off, "not recording, and says so");
        rec.observe(sample(1_788_436_800_000, 1), 0);
        rec.observe(sample(1_788_436_800_020, 2), 0);
        assert_eq!(rec.view(None).error, Some(first));
        assert!(rec.view(None).path.is_none());
        assert_eq!(
            rec.state(Some(1_788_436_800_020 + STALE_AFTER_MS)),
            RecState::Off
        );
    }
}
