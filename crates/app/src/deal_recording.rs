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

use quantick_engine::DealSample;

/// One-run override of the recording directory.
pub const DEALS_DIR_ENV: &str = "QUANTICK_DEALS_DIR";
/// The directory under the cockpit home when nothing overrides it.
pub const DEALS_DIR: &str = "deals";
/// Scripted REC state: `QUANTICK_DEAL_RECORDING=on|off|menu`. `on`/`off`
/// override the default the tab would otherwise open with; `menu` opens the
/// REC popover on the first frame, for a capture.
pub const RECORDING_HOOK_ENV: &str = "QUANTICK_DEAL_RECORDING";
/// The tape flows but the counter has not moved for this long: the venue is
/// not counting, and the bars must wait rather than close by estimate.
pub const STALE_AFTER_MS: i64 = 4_000;
/// How often the file buffer reaches the disk while recording.
pub const FLUSH_EVERY_MS: i64 = 1_000;
/// A day whose first reading is below this counted from the open: the
/// counter had barely started when the recording did.
pub const FROM_OPEN_MAX_DEALS: u64 = 1_000;
const HEADER: &str = "# quantick-deals v1";
const EXTENSION: &str = "deals";

/// Where recordings go this run.
///
/// The one-run override first, then the config, then the cockpit home the
/// other stores live in, then the cwd-relative name for a run with no home.
#[must_use]
pub fn resolve_dir(configured: Option<&str>) -> PathBuf {
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
        let existing = if path.exists() {
            read_file(&path)?.samples
        } else {
            Vec::new()
        };
        let fresh = existing.is_empty();
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
                dirty_since_ms: None,
            },
            existing,
        ))
    }

    fn append(&mut self, sample: DealSample, now_ms: i64) -> io::Result<()> {
        match self.last {
            // A reading not after the last line is one the file already
            // covers — a resumed day re-delivering the reading it stopped at.
            Some(last) if sample.time_ms <= last.time_ms => return Ok(()),
            Some(last) => writeln!(
                self.writer,
                "+{} {}{}",
                sample.time_ms - last.time_ms,
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
}

/// Read one recording. A line the format does not describe is an error
/// naming the line, never a sample guessed around it.
pub fn read_file(path: &Path) -> io::Result<DealFile> {
    let reader = BufReader::new(File::open(path)?);
    let bad = |line_no: usize, what: &str| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: line {line_no}: {what}", path.display()),
        )
    };
    let mut lines = reader.lines().enumerate();
    let Some((_, header)) = lines.next() else {
        return Err(bad(1, "empty file"));
    };
    let header = header?;
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
    let mut samples: Vec<DealSample> = Vec::new();
    for (index, line) in lines {
        let line = line?;
        let line_no = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((time, deals)) = line.split_once(' ') else {
            return Err(bad(line_no, "expected two fields"));
        };
        let sample = if let Some(delta_t) = time.strip_prefix('+') {
            let Some(last) = samples.last() else {
                return Err(bad(line_no, "a delta line before any absolute line"));
            };
            let dt: i64 = delta_t
                .parse()
                .map_err(|_| bad(line_no, "bad time delta"))?;
            let (sign, magnitude) = match deals.split_at_checked(1) {
                Some(("+", rest)) => (1_i64, rest),
                Some(("-", rest)) => (-1_i64, rest),
                _ => return Err(bad(line_no, "bad deal delta")),
            };
            let dd: i64 = magnitude
                .parse()
                .map_err(|_| bad(line_no, "bad deal delta"))?;
            let deals = i64::try_from(last.session_deals)
                .ok()
                .and_then(|d| d.checked_add(sign * dd))
                .and_then(|d| u64::try_from(d).ok())
                .ok_or_else(|| bad(line_no, "deal delta out of range"))?;
            DealSample {
                time_ms: last.time_ms.saturating_add(dt),
                session_deals: deals,
            }
        } else {
            DealSample {
                time_ms: time.parse().map_err(|_| bad(line_no, "bad time"))?,
                session_deals: deals.parse().map_err(|_| bad(line_no, "bad deal count"))?,
            }
        };
        samples.push(sample);
    }
    Ok(DealFile {
        symbol,
        day,
        samples,
    })
}

/// Every recorded day under `dir/symbol`, oldest first. Unreadable files are
/// left out rather than shown as days with numbers nobody can trust.
#[must_use]
pub fn scan_days(dir: &Path, symbol: &str) -> Vec<RecordedDay> {
    let Ok(entries) = fs::read_dir(dir.join(symbol)) else {
        return Vec::new();
    };
    let mut days = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
            continue;
        }
        let Ok(file) = read_file(&path) else { continue };
        let (Some(first), Some(last)) = (file.samples.first(), file.samples.last()) else {
            continue;
        };
        days.insert(
            file.day.clone(),
            RecordedDay {
                day: file.day,
                first: *first,
                last: *last,
                samples: file.samples.len() as u64,
                path,
            },
        );
    }
    days.into_values().collect()
}

/// The recorder for one asset: what it is doing, and what it wrote.
#[derive(Debug)]
pub struct DealRecorder {
    symbol: String,
    dir: PathBuf,
    tz_minutes: i32,
    /// The feed says its prints come with a deal counter.
    available: bool,
    /// Start on the first frame the feed can count, unless already decided.
    default_on: bool,
    /// The default has been applied (or declined by a hand); never twice.
    auto_started: bool,
    enabled: bool,
    since: Option<DealSample>,
    latest: Option<DealSample>,
    latest_arrival_ms: Option<i64>,
    recording: Option<Recording>,
    error: Option<String>,
    days: Vec<RecordedDay>,
    loaded_days: Vec<String>,
    /// Built by the app for this tab's market, as opposed to the placeholder
    /// a tab is constructed with. See [`Self::is_for`].
    configured: bool,
}

impl DealRecorder {
    /// A recorder for `symbol`, writing under `dir`, opening on `default_on`.
    #[must_use]
    pub fn new(symbol: impl Into<String>, dir: PathBuf, default_on: bool) -> Self {
        let symbol = symbol.into();
        let days = scan_days(&dir, &symbol);
        Self {
            symbol,
            dir,
            tz_minutes: 0,
            available: false,
            default_on,
            auto_started: false,
            enabled: false,
            since: None,
            latest: None,
            latest_arrival_ms: None,
            recording: None,
            error: None,
            days,
            loaded_days: Vec::new(),
            configured: true,
        }
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
    pub fn is_for(&self, symbol: &str) -> bool {
        self.configured && self.symbol == symbol
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
        self.auto_started = true;
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
        self.enabled = true;
        self.error = None;
        let day = day_of(now_ms, self.tz_minutes);
        self.open_day(&day)
    }

    /// Stop recording and flush. The file stays; the day is partial.
    pub fn stop(&mut self) {
        self.auto_started = true;
        self.enabled = false;
        self.close_recording();
    }

    fn open_day(&mut self, day: &str) -> Vec<DealSample> {
        self.close_recording();
        match Recording::open(&self.dir, &self.symbol, day, self.tz_minutes) {
            Ok((recording, existing)) => {
                self.recording = Some(recording);
                if !existing.is_empty() && !self.loaded_days.iter().any(|d| d == day) {
                    self.loaded_days.push(day.to_owned());
                }
                existing
            }
            Err(error) => {
                self.error = Some(format!("cannot write the recording: {error}"));
                Vec::new()
            }
        }
    }

    fn close_recording(&mut self) {
        if let Some(mut recording) = self.recording.take()
            && let Err(error) = recording.flush()
        {
            self.error = Some(format!("cannot write the recording: {error}"));
        }
        self.days = scan_days(&self.dir, &self.symbol);
    }

    /// One reading from the feed, at wall-clock `now_ms`.
    pub fn observe(&mut self, sample: DealSample, now_ms: i64) {
        self.since.get_or_insert(sample);
        self.latest = Some(sample);
        self.latest_arrival_ms = Some(now_ms);
        if !self.enabled {
            return;
        }
        let day = day_of(sample.time_ms, self.tz_minutes);
        if self.recording.as_ref().is_none_or(|r| r.day != day) {
            // Midnight in the display timezone, or the first reading after a
            // failed open: the day's own file, opened now.
            let _ = self.open_day(&day);
        }
        if let Some(recording) = self.recording.as_mut()
            && let Err(error) = recording.append(sample, now_ms)
        {
            self.error = Some(format!("cannot write the recording: {error}"));
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
        let Some(day) = self.days.get(index) else {
            return Vec::new();
        };
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

    /// The state, judged at `now_ms` against when the tape last printed.
    #[must_use]
    pub fn state(&self, now_ms: i64, tape_arrival_ms: Option<i64>) -> RecState {
        if !self.available {
            return if self.loaded_days.is_empty() {
                RecState::Unsupported
            } else {
                RecState::Recorded
            };
        }
        if !self.enabled {
            return if self.loaded_days.is_empty() {
                RecState::Off
            } else {
                RecState::Recorded
            };
        }
        let counter_at = self.latest_arrival_ms;
        let stale = match (counter_at, tape_arrival_ms) {
            (Some(counter), Some(tape)) => {
                tape > counter && now_ms.saturating_sub(counter) >= STALE_AFTER_MS
            }
            _ => false,
        };
        if stale {
            RecState::Stale
        } else {
            RecState::Recording
        }
    }

    /// Everything a surface draws, in one value.
    #[must_use]
    pub fn view(&self, now_ms: i64, tape_arrival_ms: Option<i64>) -> RecordingView {
        RecordingView {
            symbol: self.symbol.clone(),
            state: self.state(now_ms, tape_arrival_ms),
            reading: self.latest.map(|s| s.session_deals),
            since_ms: self.since.map(|s| s.time_ms),
            counter_age_ms: self.latest_arrival_ms.map(|at| now_ms.saturating_sub(at)),
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
    /// When the first reading of this run was taken, on the tape's clock.
    pub since_ms: Option<i64>,
    /// How long ago the newest reading arrived.
    pub counter_age_ms: Option<i64>,
    /// Lines written to the open file this run.
    pub written: u64,
    /// The file being written, while recording.
    pub path: Option<PathBuf>,
    pub dir: PathBuf,
    pub error: Option<String>,
    pub days: Vec<RecordedDay>,
    /// Days whose readings were loaded into the panes this session.
    pub loaded_days: Vec<String>,
    pub tz_minutes: i32,
}

impl RecordingView {
    /// Whether the feed offers a REC control at all.
    #[must_use]
    pub fn supported(&self) -> bool {
        self.state != RecState::Unsupported
    }

    /// Whether a `trades` pane has anything to cut on right now.
    #[must_use]
    pub fn deal_count_available(&self) -> bool {
        self.reading.is_some() || !self.loaded_days.is_empty()
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
            RecState::Stale => format!("Recording {} deals — the counter stopped", self.symbol),
            RecState::Recorded => format!("{} deals from a recording", self.symbol),
        }
    }
}

/// `YYYY-MM-DD` of `time_ms` in the display timezone.
#[must_use]
pub fn day_of(time_ms: i64, tz_minutes: i32) -> String {
    let (year, month, day, ..) = crate::paper_calendar::civil_utc(shifted(time_ms, tz_minutes));
    format!("{year:04}-{month:02}-{day:02}")
}

/// `HH:MM:SS` of `time_ms` in the display timezone.
#[must_use]
pub fn fmt_hms(time_ms: i64, tz_minutes: i32) -> String {
    let (_, _, _, h, m, s) = crate::paper_calendar::civil_utc(shifted(time_ms, tz_minutes));
    format!("{h:02}:{m:02}:{s:02}")
}

fn shifted(time_ms: i64, tz_minutes: i32) -> i64 {
    time_ms.saturating_add(i64::from(tz_minutes).saturating_mul(60_000))
}

/// `2301455` as `2 301 455` — the grouping the mock uses, readable at a
/// glance where a bare seven-digit number is not.
#[must_use]
pub fn fmt_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quantick-deals-{name}-{}-{}",
            std::process::id(),
            crate::metrics::wall_clock_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sample(time_ms: i64, session_deals: u64) -> DealSample {
        DealSample {
            time_ms,
            session_deals,
        }
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
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restart_resumes_the_days_file_and_writes_no_line_twice() {
        let dir = scratch("resume");
        let now = 1_788_436_967_023; // 2026-09-03, 09:02:47 UTC-3
        let mut first = DealRecorder::new("WINV26", dir.clone(), false);
        first.set_timezone(-180);
        first.set_available(true);
        assert!(first.start(now).is_empty(), "nothing recorded yet");
        first.observe(sample(now, 1_990), now);
        first.observe(sample(now + 20, 2_003), now + 20);
        first.stop();

        let mut second = DealRecorder::new("WINV26", dir.clone(), false);
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
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_scan_lists_days_with_their_coverage() {
        let dir = scratch("scan");
        let day1 = 1_788_436_800_000; // 2026-09-03 09:00:00 UTC-3
        let mut rec = DealRecorder::new("WINV26", dir.clone(), false);
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

        let view = rec.view(day2, None);
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
        let view = rec.view(day2, None);
        assert_eq!(view.state, RecState::Recorded);
        assert_eq!(view.button_label(), "RECORDED · 2026-09-03");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_starts_once_and_a_hand_that_stopped_it_is_respected() {
        let dir = scratch("default");
        let mut rec = DealRecorder::new("WINV26", dir.clone(), true);
        assert!(!rec.auto_start_due(), "not before the feed can count");
        rec.set_available(true);
        assert!(rec.auto_start_due());
        rec.start(0);
        assert_eq!(rec.state(0, None), RecState::Recording);
        assert!(!rec.auto_start_due());
        rec.stop();
        assert!(
            !rec.auto_start_due(),
            "stopped by a hand: the default does not restart it"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_states_say_what_every_surface_says() {
        let dir = scratch("states");
        let mut rec = DealRecorder::new("WINV26", dir.clone(), false);
        assert_eq!(rec.state(0, None), RecState::Unsupported);
        assert_eq!(rec.view(0, None).status_cell(), None);
        rec.set_available(true);
        assert_eq!(rec.state(0, None), RecState::Off);
        assert_eq!(rec.view(0, None).button_label(), "REC");
        rec.start(0);
        assert_eq!(
            rec.view(0, None).button_label(),
            "REC · waiting for the counter"
        );
        rec.observe(sample(1_788_436_800_000, 2_301_455), 1_000);
        rec.set_timezone(-180);
        let view = rec.view(1_500, Some(1_200));
        assert_eq!(view.state, RecState::Recording);
        assert_eq!(view.button_label(), "REC 2 301 455 · 09:00:00");
        assert!(view.status_cell().unwrap().ends_with("2026-09-03.deals"));
        // The tape kept printing for four seconds and the counter did not.
        let view = rec.view(1_000 + STALE_AFTER_MS, Some(1_000 + STALE_AFTER_MS - 100));
        assert_eq!(view.state, RecState::Stale);
        assert_eq!(view.button_label(), "REC · counter stale 4 s");
        // A quiet tape is not a stale counter.
        assert_eq!(
            rec.state(1_000 + STALE_AFTER_MS, Some(900)),
            RecState::Recording
        );
        rec.stop();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_placeholder_records_nothing_and_says_so() {
        let mut rec = DealRecorder::placeholder("WINV26");
        rec.set_available(true);
        assert!(!rec.is_for("WINV26"), "unconfigured, whatever the symbol");
        assert!(rec.start(0).is_empty());
        let view = rec.view(0, None);
        assert_eq!(view.state, RecState::Off, "nothing was started");
        assert!(view.path.is_none(), "and nothing was opened");
        assert!(view.error.is_some());
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
        let _ = fs::remove_dir_all(&dir);
    }
}
