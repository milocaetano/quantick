//! Which days the Get data calendar picked, and what downloading them means.
//!
//! The calendar of a replay download is a set, not one day: "run me the whole
//! week" is the question a trader preparing a rehearsal actually asks. This
//! module owns every decision behind that, with no window and no process: a
//! click toggles a day the provider offered, a Shift+click adds every offered
//! day between it and the last one clicked, and a download of several days is
//! a queue — oldest first, one single-day request at a time, skipping the days
//! the replay folder already holds — whose rows say how each day went.
//!
//! [`DownloadDays`] is the tab's whole state for that: the picks, the cached
//! days on disk, and the several-day download running over them. The panel in
//! `app` paints it and launches each row's request, built from a
//! [`DayTemplate`] frozen when Download was clicked, so a field edited
//! mid-queue can never reach a later day.
//!
//! Days are the provider's own spelling, `YYYY-MM-DD`, which sorts in date
//! order as a string, so a `BTreeSet<String>` iterates oldest first and that
//! is the order the queue fetches in. Ordered collections only, as everywhere
//! a set can reach what is drawn.
//!
//! # A day on disk is a whole day
//!
//! The exporter writes each file beside its destination (`.csv.part`) and
//! moves it into place only once whole, and the library never lists a `.part`
//! file. So a tape in the folder is never a truncated one. What a refused or
//! cancelled day *can* leave is a whole tape without the run-up written after
//! it. So a day is already here only when the scanned folder holds both its
//! tape and its run-up file — read from disk on every scan, never remembered —
//! and a several-day download fetches any other day again.

use std::collections::BTreeSet;
use std::ops::Bound;
use std::path::PathBuf;

use crate::Library;
use crate::deals::grouped;
pub use crate::format::days_in_month;
use crate::format::{CivilTime, UtcOffset};

/// Roughly how many bytes one tick costs in the replay format, measured on a
/// real B3 export: 19,139,000 bytes for 389,622 prints. Used only to size the
/// estimate shown before a download starts, and labelled as approximate.
pub const BYTES_PER_TICK: u64 = 49;

/// Rough prints per session for the estimate, when nothing better is known.
///
/// A busy WIN day is over a million; this is deliberately a round,
/// order-of-magnitude figure, and the interface says "about".
pub const TYPICAL_TICKS_PER_SESSION: u64 = 1_000_000;

/// The days picked on the calendar.
///
/// The anchor is the last day clicked, picked or not: a Shift+click ranges
/// from there, as it does in every list a trader has used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaySelection {
    days: BTreeSet<String>,
    anchor: Option<String>,
}

impl DaySelection {
    /// A click on `day`; `extend` is Shift held.
    ///
    /// A plain click toggles the day. A Shift+click adds every day `offered`
    /// holds between the anchor and `day`, both included, and never takes one
    /// out. A day the provider does not hold is never picked: it has no tape.
    pub fn click(&mut self, day: &str, offered: &BTreeSet<String>, extend: bool) {
        if !offered.contains(day) {
            return;
        }
        match self.anchor.as_deref().filter(|_| extend) {
            Some(anchor) => {
                let (from, to) = if anchor <= day {
                    (anchor, day)
                } else {
                    (day, anchor)
                };
                self.days.extend(
                    offered
                        .range::<str, _>((Bound::Included(from), Bound::Included(to)))
                        .cloned(),
                );
            }
            None => {
                if !self.days.remove(day) {
                    self.days.insert(day.to_owned());
                }
            }
        }
        self.anchor = Some(day.to_owned());
    }

    /// Forget every pick and the anchor — what a new look-up means, because
    /// another contract's days are not this one's.
    pub fn clear(&mut self) {
        self.days.clear();
        self.anchor = None;
    }

    /// Whether `day` is picked.
    #[must_use]
    pub fn contains(&self, day: &str) -> bool {
        self.days.contains(day)
    }

    /// How many days are picked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.days.len()
    }

    /// Whether nothing is picked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }

    /// The day picked, when exactly one is: the single-day download, with
    /// the day before it and the tape opened at the end, is for that case.
    #[must_use]
    pub fn only(&self) -> Option<&str> {
        if self.days.len() == 1 {
            self.days.first().map(String::as_str)
        } else {
            None
        }
    }

    /// The picked days, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.days.iter().map(String::as_str)
    }
}

/// The days of `symbol` the scanned folder holds whole — tape and run-up
/// both — `YYYY-MM-DD`.
///
/// Another contract's tape of the same date is not this one's, so the symbol
/// is matched exactly.
#[must_use]
pub fn held_days(library: &Library, symbol: &str) -> BTreeSet<String> {
    days_of(library, symbol, true)
}

fn days_of(library: &Library, symbol: &str, whole: bool) -> BTreeSet<String> {
    library
        .sessions
        .iter()
        .filter(|held| held.symbol == symbol && (held.has_context || !whole))
        .filter_map(|held| held.date.map(|d| d.label()))
        .collect()
}

/// The days on disk for the typed symbol, read once per folder scan and
/// symbol rather than once per frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeldDays {
    /// The symbol these were read for; `None` until read, and after a rescan.
    symbol: Option<String>,
    /// Days whose tape and run-up are both in the folder.
    whole: BTreeSet<String>,
    /// Days whose tape is in the folder, run-up or not.
    tapes: BTreeSet<String>,
}

impl HeldDays {
    /// The folder was scanned again: read it afresh on the next refresh.
    pub fn rescanned(&mut self) {
        self.symbol = None;
    }

    /// Read `library` for `symbol`, unless that is what is already held.
    pub fn refresh(&mut self, library: Option<&Library>, symbol: &str) {
        if self.symbol.as_deref() == Some(symbol) {
            return;
        }
        let read = |whole| library.map_or_else(BTreeSet::new, |l| days_of(l, symbol, whole));
        self.whole = read(true);
        self.tapes = read(false);
        self.symbol = Some(symbol.to_owned());
    }

    /// Whether the folder holds the whole of `day`, tape and run-up: the
    /// only day a several-day download skips as already here.
    #[must_use]
    pub fn contains(&self, day: &str) -> bool {
        self.whole.contains(day)
    }

    /// Whether the folder holds `day`'s tape, run-up or not — all the day
    /// before a picked day needs, since it joins the chart as prints only.
    #[must_use]
    pub fn has_tape(&self, day: &str) -> bool {
        self.tapes.contains(day)
    }
}

/// What every day of one several-day download shares, frozen when Download
/// was clicked: the symbol, the run-up, the stated clock and the folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayTemplate {
    /// The instrument.
    pub symbol: String,
    /// Sessions of run-up candles in front of each day.
    pub context_sessions: u32,
    /// The broker clock the trader stated, if they had to.
    pub utc_offset_s: Option<i32>,
    /// The folder every tape goes into.
    pub out_dir: PathBuf,
}

/// Where one day of a several-day download stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DayStatus {
    /// Not started yet.
    Waiting,
    /// Downloading, with prints written so far.
    Running {
        /// Prints written so far.
        ticks: u64,
        /// Sessions of run-up written so far.
        run_up: u32,
    },
    /// On disk.
    Fetched {
        /// Prints it holds.
        ticks: u64,
        /// Sessions of run-up the broker served, which may be fewer than
        /// asked for — or none.
        run_up: u32,
    },
    /// Already in the folder, so skipped: re-exporting it would cost minutes
    /// and overwrite a recording the trader may have corrected by hand.
    AlreadyHere,
    /// The provider refused it.
    Refused {
        /// What the provider said.
        headline: String,
        /// What to do about it, when the provider said.
        fix: Option<String>,
    },
    /// Never asked for: an earlier day was refused for a reason every day
    /// would be refused for.
    NotTried,
    /// The trader cancelled before it landed.
    Cancelled,
}

/// One day of a several-day download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRow {
    /// The day, `YYYY-MM-DD`.
    pub day: String,
    /// Where it stands.
    pub status: DayStatus,
}

impl BatchRow {
    /// The row as the panel prints it, with a refusal's fix on its own line;
    /// `asked` is the run-up every day was asked for.
    #[must_use]
    pub fn line(&self, asked: u32) -> String {
        let day = &self.day;
        match &self.status {
            DayStatus::Waiting => format!("{day} — waiting"),
            DayStatus::Running { ticks, .. } => {
                format!("{day} — downloading, {} prints so far", prints(*ticks))
            }
            DayStatus::Fetched { ticks, run_up } if *run_up < asked => format!(
                "{day} — {} prints downloaded, run-up short: {run_up} of {asked} sessions — \
                 the broker held fewer sessions than asked for",
                prints(*ticks)
            ),
            DayStatus::Fetched { ticks, .. } => {
                format!("{day} — {} prints downloaded", prints(*ticks))
            }
            DayStatus::AlreadyHere => format!("{day} — already here, skipped"),
            DayStatus::Refused { headline, fix } => match fix {
                Some(fix) => format!("{day} — refused: {headline}\n{fix}"),
                None => format!("{day} — refused: {headline}"),
            },
            DayStatus::NotTried => {
                format!("{day} — not tried: every day would be refused the same way")
            }
            DayStatus::Cancelled => format!("{day} — cancelled"),
        }
    }
}

/// How a several-day download ended, or stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchSummary {
    /// Days that landed.
    pub fetched: usize,
    /// Of those, days whose run-up came short of what was asked for.
    pub short_run_up: usize,
    /// Days skipped because the folder held them.
    pub already_here: usize,
    /// Days the provider refused.
    pub refused: usize,
    /// Days never asked for after a refusal every day would repeat.
    pub not_tried: usize,
    /// Days the trader cancelled.
    pub cancelled: usize,
    /// Prints across every day that landed.
    pub ticks: u64,
}

impl BatchSummary {
    /// The summary as the panel prints it. Nothing is opened on the chart
    /// after several days, so it says where they went.
    #[must_use]
    pub fn line(&self) -> String {
        let short = match self.short_run_up {
            0 => String::new(),
            n => format!(", {n} with a short run-up"),
        };
        let mut line = format!(
            "{} fetched ({} prints{short}) · {} already here · {} refused",
            self.fetched,
            prints(self.ticks),
            self.already_here,
            self.refused
        );
        if self.not_tried > 0 {
            line.push_str(&format!(" · {} not tried", self.not_tried));
        }
        if self.cancelled > 0 {
            line.push_str(&format!(" · {} cancelled", self.cancelled));
        }
        line.push_str(" — they are listed under My sessions");
        line
    }
}

/// A several-day download: one row per picked day, oldest first, and the
/// request every one of them is asked with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayBatch {
    rows: Vec<BatchRow>,
    template: DayTemplate,
}

impl DayBatch {
    /// The queue for `selection`, oldest first, with held days skipped.
    #[must_use]
    pub fn plan(
        selection: &DaySelection,
        held: impl Fn(&str) -> bool,
        template: DayTemplate,
    ) -> Self {
        let rows = selection
            .iter()
            .map(|day| BatchRow {
                day: day.to_owned(),
                status: if held(day) {
                    DayStatus::AlreadyHere
                } else {
                    DayStatus::Waiting
                },
            })
            .collect();
        Self { rows, template }
    }

    /// Every row, oldest first.
    #[must_use]
    pub fn rows(&self) -> &[BatchRow] {
        &self.rows
    }

    /// What every day is asked with.
    #[must_use]
    pub fn template(&self) -> &DayTemplate {
        &self.template
    }

    /// How many days still have to be downloaded.
    #[must_use]
    pub fn to_fetch(&self) -> usize {
        self.count(|status| matches!(status, DayStatus::Waiting))
    }

    /// Start the oldest waiting day and name it, unless one is already
    /// running: the provider is asked for one day at a time.
    pub fn begin_next(&mut self) -> Option<String> {
        if self.running().is_some() {
            return None;
        }
        let row = self
            .rows
            .iter_mut()
            .find(|row| row.status == DayStatus::Waiting)?;
        row.status = DayStatus::Running {
            ticks: 0,
            run_up: 0,
        };
        Some(row.day.clone())
    }

    /// The day downloading now.
    #[must_use]
    pub fn running(&self) -> Option<&str> {
        self.rows
            .iter()
            .find(|row| matches!(row.status, DayStatus::Running { .. }))
            .map(|row| row.day.as_str())
    }

    /// Prints written so far for the running day.
    pub fn progress(&mut self, ticks: u64) {
        self.settle(|_, run_up| DayStatus::Running { ticks, run_up });
    }

    /// The running day's run-up landed, `sessions` of it.
    pub fn run_up(&mut self, sessions: u32) {
        self.settle(|ticks, _| DayStatus::Running {
            ticks,
            run_up: sessions,
        });
    }

    /// The running day landed, with whatever run-up it said it wrote.
    pub fn finish(&mut self) {
        self.settle(|ticks, run_up| DayStatus::Fetched { ticks, run_up });
    }

    /// The running day was refused. When `every_day` — no interpreter, no
    /// terminal, an unknown clock, an instrument the broker does not serve —
    /// the days still waiting would only be refused the same way, so they
    /// are not asked for.
    pub fn refuse(&mut self, headline: String, fix: Option<String>, every_day: bool) {
        self.settle(|_, _| DayStatus::Refused { headline, fix });
        if every_day {
            self.replace_waiting(&DayStatus::NotTried);
        }
    }

    /// Stop: the running day and every waiting one are cancelled.
    pub fn cancel(&mut self) {
        self.settle(|_, _| DayStatus::Cancelled);
        self.replace_waiting(&DayStatus::Cancelled);
    }

    /// Whether nothing is running or waiting.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.running().is_none() && self.to_fetch() == 0
    }

    /// The counts so far.
    #[must_use]
    pub fn summary(&self) -> BatchSummary {
        let mut summary = BatchSummary::default();
        for row in &self.rows {
            match row.status {
                DayStatus::Fetched { ticks, run_up } => {
                    summary.fetched += 1;
                    summary.short_run_up += usize::from(run_up < self.template.context_sessions);
                    summary.ticks += ticks;
                }
                DayStatus::AlreadyHere => summary.already_here += 1,
                DayStatus::Refused { .. } => summary.refused += 1,
                DayStatus::NotTried => summary.not_tried += 1,
                DayStatus::Cancelled => summary.cancelled += 1,
                DayStatus::Waiting | DayStatus::Running { .. } => {}
            }
        }
        summary
    }

    fn count(&self, matches: impl Fn(&DayStatus) -> bool) -> usize {
        self.rows.iter().filter(|row| matches(&row.status)).count()
    }

    fn replace_waiting(&mut self, with: &DayStatus) {
        for row in &mut self.rows {
            if row.status == DayStatus::Waiting {
                row.status = with.clone();
            }
        }
    }

    /// Move the running row on, handing the new status its prints and
    /// run-up so far.
    fn settle(&mut self, next: impl FnOnce(u64, u32) -> DayStatus) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| matches!(row.status, DayStatus::Running { .. }))
            && let DayStatus::Running { ticks, run_up } = row.status
        {
            row.status = next(ticks, run_up);
        }
    }
}

/// The day the provider holds immediately before `day`, when it holds one.
///
/// The broker's own list of days is the calendar: a Monday reaches Friday and
/// a holiday is skipped without anyone owning a table of market closures it
/// would have to keep right for every venue. `None` on the oldest day held.
#[must_use]
pub fn day_before<'a>(offered: &'a BTreeSet<String>, day: &str) -> Option<&'a str> {
    // Bounds rather than `..day`: a `RangeTo<&str>` cannot address a set of
    // `String` without an allocation, and this is read every frame.
    offered
        .range::<str, _>((Bound::Unbounded, Bound::Excluded(day)))
        .next_back()
        .map(String::as_str)
}

/// The day a single-day download would fetch behind the chosen one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayBeforeTarget {
    /// The day itself, `YYYY-MM-DD`.
    pub day: String,
    /// Whether the replay folder already holds it.
    ///
    /// It still joins the chosen day on the chart — that is what makes the
    /// setting worth having on a folder the trader has been filling for weeks
    /// — but nothing is downloaded for it. Re-exporting a tape they already
    /// have costs tens of megabytes and minutes, and overwrites a recording
    /// they may have imported or hand-corrected.
    pub on_disk: bool,
}

impl DayBeforeTarget {
    /// The day to *download*: this one, unless the folder already holds it.
    #[must_use]
    pub fn to_fetch(&self) -> Option<String> {
        (!self.on_disk).then(|| self.day.clone())
    }
}

/// The Get data tab's days: what is picked, what the folder holds, and the
/// several-day download running over them, with the bookkeeping that keeps
/// the three agreeing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadDays {
    /// The days picked on the calendar.
    pub selection: DaySelection,
    /// The days on disk for the typed symbol.
    pub held: HeldDays,
    batch: Option<DayBatch>,
}

impl DownloadDays {
    /// The several-day download asked for last.
    #[must_use]
    pub fn batch(&self) -> Option<&DayBatch> {
        self.batch.as_ref()
    }

    /// Forget the last several-day download, as any new request does.
    pub fn forget_batch(&mut self) {
        self.batch = None;
    }

    /// The day of a several-day download being fetched now.
    #[must_use]
    pub fn running(&self) -> Option<&str> {
        self.batch.as_ref()?.running()
    }

    /// Plan a several-day download of the picks, held days skipped.
    pub fn begin(&mut self, template: DayTemplate) {
        let held = &self.held;
        self.batch = Some(DayBatch::plan(
            &self.selection,
            |d| held.contains(d),
            template,
        ));
    }

    /// Start the next waiting day: its date and the frozen request to ask
    /// it with.
    pub fn begin_next(&mut self) -> Option<(String, DayTemplate)> {
        let batch = self.batch.as_mut()?;
        let day = batch.begin_next()?;
        Some((day, batch.template.clone()))
    }

    /// Prints written so far for the running day.
    pub fn progress(&mut self, ticks: u64) {
        self.batch_mut(|batch| batch.progress(ticks));
    }

    /// The running day's run-up landed, `sessions` of it.
    pub fn run_up(&mut self, sessions: u32) {
        self.batch_mut(|batch| batch.run_up(sessions));
    }

    /// The running day landed.
    pub fn finish(&mut self) {
        self.batch_mut(DayBatch::finish);
    }

    /// The running day was refused; see [`DayBatch::refuse`].
    pub fn refuse(&mut self, headline: String, fix: Option<String>, every_day: bool) {
        self.batch_mut(|batch| batch.refuse(headline, fix, every_day));
    }

    /// Cancel the running day and every waiting one.
    pub fn cancel(&mut self) {
        self.batch_mut(DayBatch::cancel);
    }

    /// Whether several days are picked and every one is already here.
    #[must_use]
    pub fn nothing_to_fetch(&self) -> bool {
        self.selection.len() > 1 && self.selection.iter().all(|d| self.held.contains(d))
    }

    /// The sentence above the Download button for several picks: every
    /// picked day counted, sized by the ones that will actually be fetched.
    /// Counted straight from the picks, since it is drawn every frame.
    #[must_use]
    pub fn estimate(&self, context_sessions: u32) -> String {
        let picked = self.selection.len();
        let held = self
            .selection
            .iter()
            .filter(|d| self.held.contains(d))
            .count();
        let fetch = picked - held;
        if fetch == 0 {
            return format!("{picked} days, all already here — nothing to download");
        }
        format!(
            "{fetch} of {picked} days to fetch ({held} already here) · about {} · \
             run-up {context_sessions} session(s) each · nothing opens on the chart",
            size_label(tape_bytes(fetch as u64))
        )
    }

    /// The day a single-day download would fetch behind the one picked:
    /// `None` when the tick is off, unless exactly one day is picked, before
    /// the look-up has answered, or when the broker holds nothing earlier.
    #[must_use]
    pub fn day_before_target(
        &self,
        ticked: bool,
        offered: Option<&BTreeSet<String>>,
    ) -> Option<DayBeforeTarget> {
        if !ticked {
            return None;
        }
        let earlier = day_before(offered?, self.selection.only()?)?;
        Some(DayBeforeTarget {
            day: earlier.to_owned(),
            on_disk: self.held.has_tape(earlier),
        })
    }

    /// The sentence under the *day before* tick: which day the download
    /// would reach for, or why it would reach for none. The tick decides
    /// which sentence is true — never the picked day, which only decides how
    /// precise it can be — and before the look-up has answered nothing is
    /// claimed about what the broker holds.
    #[must_use]
    pub fn day_before_note(
        &self,
        ticked: bool,
        looked_up: bool,
        target: Option<&DayBeforeTarget>,
    ) -> String {
        if self.selection.len() > 1 {
            return "Off while several days are picked: each downloads on its own, and none \
                    opens on the chart."
                .to_string();
        }
        if !ticked {
            return "Only the day you pick is downloaded; the run-up above stays broker candles."
                .to_string();
        }
        if !looked_up {
            return "The session before the day you pick comes with it, once the terminal has \
                    said which days it holds."
                .to_string();
        }
        if self.selection.is_empty() {
            return "The session before the day you pick comes with it.".to_string();
        }
        match target {
            Some(target) if target.on_disk => format!(
                "{} is already in this folder, and replays in front of the day you pick.",
                target.day
            ),
            Some(target) => format!(
                "{} comes too, so the open you rehearse has real order flow behind it.",
                target.day
            ),
            None => "The broker holds no earlier day for this contract.".to_string(),
        }
    }

    fn batch_mut(&mut self, change: impl FnOnce(&mut DayBatch)) {
        if let Some(batch) = self.batch.as_mut() {
            change(batch);
        }
    }
}

/// About how many bytes `tapes` days of prints take on disk.
#[must_use]
pub fn tape_bytes(tapes: u64) -> u64 {
    tapes * TYPICAL_TICKS_PER_SESSION * BYTES_PER_TICK
}

/// A count of prints as the Get data tab has always shown it: `1,234,567`.
#[must_use]
pub fn prints(ticks: u64) -> String {
    grouped(ticks, ',')
}

/// `2026-08-12` → `(2026, 8, 12)`.
#[must_use]
pub fn parse_day(iso: &str) -> Option<(i64, u32, u32)> {
    let mut parts = iso.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

/// The month before `(year, month)`.
#[must_use]
pub fn previous_month(year: i64, month: u32) -> (i64, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

/// The month after `(year, month)`.
#[must_use]
pub fn next_month(year: i64, month: u32) -> (i64, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

/// `8` → `August`.
#[must_use]
pub fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    NAMES.get((month as usize).wrapping_sub(1)).unwrap_or(&"—")
}

/// Which column a date falls in, with Monday at 0.
///
/// Derived from this crate's own civil-date arithmetic rather than a second
/// calendar implementation: a date means one thing in this app.
#[must_use]
pub fn weekday_monday_first(year: i64, month: u32, day: u32) -> usize {
    let ms = CivilTime {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        milli: 0,
    }
    .epoch_millis(UtcOffset::UTC);
    let days = ms.div_euclid(86_400_000);
    // 1970-01-01 was a Thursday, which is column 3 when Monday is 0.
    usize::try_from((days + 3).rem_euclid(7)).unwrap_or(0)
}

/// A size a person reads at a glance.
#[must_use]
pub fn size_label(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{} kB", bytes / 1024)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{SessionDate, SessionEntry, UtcOffset};

    /// Two weeks across a month end, with the weekend between them missing,
    /// as a broker's day list reads.
    fn offered() -> BTreeSet<String> {
        [
            "2026-07-29",
            "2026-07-30",
            "2026-07-31",
            "2026-08-03",
            "2026-08-04",
            "2026-08-05",
        ]
        .iter()
        .map(|d| (*d).to_string())
        .collect()
    }

    fn picked(selection: &DaySelection) -> Vec<&str> {
        selection.iter().collect()
    }

    fn set(days: &[&str]) -> BTreeSet<String> {
        days.iter().map(|d| (*d).to_string()).collect()
    }

    fn selection_of(days: &[&str]) -> DaySelection {
        let mut selection = DaySelection::default();
        for day in days {
            selection.click(day, &offered(), false);
        }
        selection
    }

    #[test]
    fn a_click_toggles_an_offered_day() {
        let mut selection = DaySelection::default();
        selection.click("2026-07-30", &offered(), false);
        assert_eq!(picked(&selection), ["2026-07-30"]);
        selection.click("2026-08-04", &offered(), false);
        assert_eq!(picked(&selection), ["2026-07-30", "2026-08-04"]);
        selection.click("2026-07-30", &offered(), false);
        assert_eq!(
            picked(&selection),
            ["2026-08-04"],
            "a second click takes it out"
        );
    }

    #[test]
    fn a_day_the_provider_does_not_hold_is_never_picked() {
        let mut selection = DaySelection::default();
        selection.click("2026-08-01", &offered(), false);
        assert!(selection.is_empty(), "a Saturday has no tape");
        selection.click("2026-07-30", &offered(), false);
        selection.click("2026-08-02", &offered(), true);
        assert_eq!(
            picked(&selection),
            ["2026-07-30"],
            "nor does a Shift+click on one reach for a range"
        );
    }

    #[test]
    fn shift_click_adds_every_offered_day_between_across_months_and_gaps() {
        let mut selection = DaySelection::default();
        selection.click("2026-07-30", &offered(), false);
        selection.click("2026-08-04", &offered(), true);
        assert_eq!(
            picked(&selection),
            ["2026-07-30", "2026-07-31", "2026-08-03", "2026-08-04"],
            "inclusive, across the month end, and the weekend stays out"
        );
    }

    #[test]
    fn shift_click_runs_backwards_too() {
        let mut selection = DaySelection::default();
        selection.click("2026-08-04", &offered(), false);
        selection.click("2026-07-31", &offered(), true);
        assert_eq!(
            picked(&selection),
            ["2026-07-31", "2026-08-03", "2026-08-04"]
        );
    }

    #[test]
    fn shift_click_adds_to_what_was_already_picked() {
        let mut selection = selection_of(&["2026-07-29", "2026-08-05"]);
        // The last click was the 5th, so the range runs from there.
        selection.click("2026-08-03", &offered(), true);
        assert_eq!(
            picked(&selection),
            ["2026-07-29", "2026-08-03", "2026-08-04", "2026-08-05"]
        );
    }

    #[test]
    fn shift_click_with_nothing_clicked_yet_is_a_plain_click() {
        let mut selection = DaySelection::default();
        selection.click("2026-08-03", &offered(), true);
        assert_eq!(picked(&selection), ["2026-08-03"]);
    }

    #[test]
    fn clearing_forgets_the_anchor_too() {
        let mut selection = selection_of(&["2026-07-29"]);
        selection.clear();
        assert!(selection.is_empty());
        selection.click("2026-08-04", &offered(), true);
        assert_eq!(
            picked(&selection),
            ["2026-08-04"],
            "a range from a day of another look-up is no range"
        );
    }

    #[test]
    fn only_answers_for_exactly_one_day() {
        assert_eq!(DaySelection::default().only(), None);
        assert_eq!(selection_of(&["2026-07-30"]).only(), Some("2026-07-30"));
        let two = selection_of(&["2026-07-30", "2026-07-31"]);
        assert_eq!(two.only(), None);
        assert_eq!(two.len(), 2);
        assert!(two.contains("2026-07-31"));
    }

    /// A scanned folder holding these `(symbol, day)` tapes.
    fn library_of(tapes: &[(&str, &str)]) -> Library {
        let entry = |(symbol, day): &(&str, &str)| SessionEntry {
            path: PathBuf::from(format!("replay/{symbol}/{day}.csv")),
            symbol: (*symbol).to_string(),
            date: SessionDate::parse(day),
            size_bytes: 0,
            timezone: UtcOffset::UTC,
            timezone_declared: true,
            side_source: None,
            has_quotes: false,
            notes: Vec::new(),
            has_context: true,
        };
        Library {
            root: PathBuf::from("replay"),
            sessions: tapes.iter().map(entry).collect(),
            problems: Vec::new(),
        }
    }

    /// A folder holding these tapes with no run-up file beside any of them.
    fn tapes_only(tapes: &[(&str, &str)]) -> Library {
        let mut library = library_of(tapes);
        for entry in &mut library.sessions {
            entry.has_context = false;
        }
        library
    }

    /// A queue for `selection` with `held` already on disk.
    fn plan(selection: &DaySelection, held: &[&str]) -> DayBatch {
        let held = set(held);
        DayBatch::plan(selection, |d| held.contains(d), DayTemplate::default())
    }

    /// The tab's days for WINV26, with `selection` picked over `library`.
    fn days_over(selection: DaySelection, library: &Library) -> DownloadDays {
        let mut days = DownloadDays {
            selection,
            ..DownloadDays::default()
        };
        days.held.refresh(Some(library), "WINV26");
        days
    }

    fn template(symbol: &str) -> DayTemplate {
        DayTemplate {
            symbol: symbol.to_string(),
            context_sessions: 5,
            utc_offset_s: Some(-10_800),
            out_dir: PathBuf::from("replay"),
        }
    }

    #[test]
    fn held_days_are_this_symbol_s_days_in_the_folder() {
        let library = library_of(&[("WINV26", "2026-08-03"), ("WINQ26", "2026-08-04")]);
        assert_eq!(
            held_days(&library, "WINV26"),
            set(&["2026-08-03"]),
            "another contract's tape is not this one's"
        );
    }

    #[test]
    fn the_queue_runs_oldest_first_and_skips_held_days() {
        // Clicked newest first: the queue still starts at the oldest.
        let selection = selection_of(&["2026-08-05", "2026-08-03", "2026-07-30"]);
        let mut batch = plan(&selection, &["2026-08-03"]);

        let rows: Vec<(&str, &DayStatus)> = batch
            .rows()
            .iter()
            .map(|r| (r.day.as_str(), &r.status))
            .collect();
        assert_eq!(
            rows,
            [
                ("2026-07-30", &DayStatus::Waiting),
                ("2026-08-03", &DayStatus::AlreadyHere),
                ("2026-08-05", &DayStatus::Waiting),
            ]
        );
        assert_eq!(batch.to_fetch(), 2);

        assert_eq!(batch.begin_next().as_deref(), Some("2026-07-30"));
        assert_eq!(batch.begin_next(), None, "one day at a time");
        batch.progress(900);
        assert_eq!(batch.running(), Some("2026-07-30"));
        batch.finish();
        assert_eq!(
            batch.rows()[0].status,
            DayStatus::Fetched {
                ticks: 900,
                run_up: 0
            }
        );
        assert_eq!(
            batch.begin_next().as_deref(),
            Some("2026-08-05"),
            "the held day is never asked for"
        );
        batch.finish();
        assert_eq!(batch.begin_next(), None);
        assert!(batch.is_finished());
    }

    #[test]
    fn a_refused_day_does_not_stop_the_rest() {
        let selection = selection_of(&["2026-07-29", "2026-07-30", "2026-07-31"]);
        let mut batch = plan(&selection, &["2026-07-31"]);
        assert_eq!(batch.begin_next().as_deref(), Some("2026-07-29"));
        batch.refuse(
            "the terminal holds no ticks for that day".to_string(),
            Some("Pick another day.".to_string()),
            false,
        );
        assert!(!batch.is_finished());
        assert_eq!(batch.begin_next().as_deref(), Some("2026-07-30"));
        batch.progress(10);
        batch.finish();
        assert!(batch.is_finished());

        let summary = batch.summary();
        assert_eq!(
            summary,
            BatchSummary {
                fetched: 1,
                short_run_up: 0,
                already_here: 1,
                refused: 1,
                not_tried: 0,
                cancelled: 0,
                ticks: 10,
            }
        );
        let line = summary.line();
        assert!(line.contains("1 fetched"), "{line}");
        assert!(line.contains("1 already here"), "{line}");
        assert!(line.contains("1 refused"), "{line}");
        assert!(!line.contains("cancelled"), "nothing was: {line}");
        assert!(
            batch.rows()[0].line(0).contains("no ticks for that day"),
            "the refusal is said on its own row: {}",
            batch.rows()[0].line(0)
        );
    }

    #[test]
    fn cancelling_stops_the_running_day_and_everything_behind_it() {
        let selection = selection_of(&["2026-07-29", "2026-07-30", "2026-07-31"]);
        let mut batch = plan(&selection, &[]);
        batch.begin_next();
        batch.finish();
        batch.begin_next();
        batch.cancel();
        assert!(batch.is_finished());
        assert_eq!(batch.running(), None);
        assert_eq!(batch.begin_next(), None, "nothing starts after a cancel");
        let summary = batch.summary();
        assert_eq!((summary.fetched, summary.cancelled), (1, 2));
        assert!(summary.line().contains("2 cancelled"), "{}", summary.line());
    }

    #[test]
    fn the_estimate_counts_every_selected_day() {
        let selection = selection_of(&["2026-07-29", "2026-07-30", "2026-07-31"]);
        let days = days_over(selection.clone(), &library_of(&[("WINV26", "2026-07-30")]));
        let estimate = days.estimate(5);
        assert!(estimate.contains("2 of 3 days"), "{estimate}");
        assert!(estimate.contains("1 already here"), "{estimate}");
        assert!(estimate.contains("93 MB"), "two tapes' worth: {estimate}");
        assert!(estimate.contains("5 session(s)"), "{estimate}");

        let all = [
            ("WINV26", "2026-07-29"),
            ("WINV26", "2026-07-30"),
            ("WINV26", "2026-07-31"),
        ];
        let all_here = days_over(selection, &library_of(&all));
        assert!(
            all_here.estimate(5).contains("nothing to download"),
            "{}",
            all_here.estimate(5)
        );
    }

    #[test]
    fn a_refusal_every_day_would_repeat_stops_the_rest_untried() {
        let selection = selection_of(&["2026-07-29", "2026-07-30", "2026-07-31"]);
        let mut batch = plan(&selection, &[]);
        batch.begin_next();
        batch.refuse(
            "quantick could not reach the MetaTrader 5 terminal".to_string(),
            Some("Open the terminal and log in.".to_string()),
            true,
        );
        assert!(batch.is_finished(), "nothing is left to ask for");
        assert_eq!(batch.begin_next(), None);
        let summary = batch.summary();
        assert_eq!((summary.refused, summary.not_tried), (1, 2));
        assert!(summary.line().contains("2 not tried"), "{}", summary.line());
        let refused = batch.rows()[0].line(0);
        assert!(
            refused.contains("Open the terminal and log in."),
            "the fix is kept beside the refusal: {refused}"
        );
        assert!(
            batch.rows()[1].line(0).contains("refused the same way"),
            "and the untried days say why: {}",
            batch.rows()[1].line(0)
        );
    }

    #[test]
    fn every_day_is_asked_with_the_template_frozen_at_the_click() {
        let mut days = days_over(
            selection_of(&["2026-07-29", "2026-07-30"]),
            &library_of(&[]),
        );
        days.begin(template("WINV26"));
        let (first, asked) = days.begin_next().expect("a first day");
        assert_eq!(
            (first.as_str(), &asked),
            ("2026-07-29", &template("WINV26"))
        );
        days.finish();
        // Whatever the picks say now, the rest of the queue is this request.
        days.selection.clear();
        let (second, asked) = days.begin_next().expect("a second day");
        assert_eq!((second.as_str(), asked), ("2026-07-30", template("WINV26")));
    }

    #[test]
    fn the_held_days_are_read_once_per_scan_and_symbol() {
        let mut held = HeldDays::default();
        held.refresh(Some(&library_of(&[("WINV26", "2026-07-29")])), "WINV26");
        assert!(held.contains("2026-07-29"));
        // Same symbol, no rescan: the cache answers, not the library.
        held.refresh(Some(&library_of(&[])), "WINV26");
        assert!(held.contains("2026-07-29"), "not re-read every frame");
        held.rescanned();
        held.refresh(Some(&library_of(&[])), "WINV26");
        assert!(!held.contains("2026-07-29"), "a rescan is read afresh");
        held.refresh(Some(&library_of(&[("WINQ26", "2026-07-30")])), "WINQ26");
        assert!(held.contains("2026-07-30"), "so is another symbol");
    }

    /// A refused or cancelled day can leave its whole tape without the run-up
    /// behind it. Held is read from the folder: a tape alone is not already
    /// here, a tape with its run-up is, and a fresh scan answers again with
    /// nothing remembered from the last one.
    #[test]
    fn only_a_tape_with_its_run_up_beside_it_is_already_here() {
        let mut held = HeldDays::default();
        held.refresh(Some(&tapes_only(&[("WINV26", "2026-07-29")])), "WINV26");
        assert!(!held.contains("2026-07-29"), "a tape alone is not held");
        assert!(
            held.has_tape("2026-07-29"),
            "though it can join as a day before"
        );

        held.rescanned();
        held.refresh(Some(&library_of(&[("WINV26", "2026-07-29")])), "WINV26");
        assert!(held.contains("2026-07-29"), "with its run-up it is");

        held.rescanned();
        held.refresh(Some(&tapes_only(&[("WINV26", "2026-07-29")])), "WINV26");
        assert!(!held.contains("2026-07-29"), "and each scan answers afresh");

        let selection = selection_of(&["2026-07-29", "2026-07-30"]);
        let mut days = days_over(selection, &tapes_only(&[("WINV26", "2026-07-29")]));
        days.begin(template("WINV26"));
        assert_eq!(
            days.batch().map(DayBatch::to_fetch),
            Some(2),
            "fetched again"
        );
    }

    /// A run-up the broker served short is said on the row and counted, in
    /// the single-day path's words.
    #[test]
    fn a_short_run_up_is_labelled_on_its_row_and_in_the_summary() {
        let mut days = days_over(
            selection_of(&["2026-07-29", "2026-07-30", "2026-07-31"]),
            &library_of(&[]),
        );
        days.begin(template("WINV26"));
        days.begin_next();
        days.progress(300);
        days.run_up(2);
        days.finish();
        days.begin_next();
        // No run-up written at all: the broker had none.
        days.finish();
        days.begin_next();
        days.run_up(5);
        days.finish();

        let batch = days.batch().expect("a batch");
        let line = batch.rows()[0].line(5);
        assert!(line.contains("run-up short: 2 of 5 sessions"), "{line}");
        assert!(line.contains("fewer sessions than asked for"), "{line}");
        assert!(
            batch.rows()[1].line(5).contains("0 of 5"),
            "{}",
            batch.rows()[1].line(5)
        );
        assert!(
            !batch.rows()[2].line(5).contains("short"),
            "{}",
            batch.rows()[2].line(5)
        );
        let summary = batch.summary();
        assert_eq!(summary.short_run_up, 2);
        assert!(
            summary.line().contains("2 with a short run-up"),
            "{}",
            summary.line()
        );
    }

    #[test]
    fn nothing_is_left_to_fetch_when_every_pick_is_here() {
        let library = library_of(&[("WINV26", "2026-07-29"), ("WINV26", "2026-07-30")]);
        let all = days_over(selection_of(&["2026-07-29", "2026-07-30"]), &library);
        assert!(all.nothing_to_fetch());
        let one_more = days_over(selection_of(&["2026-07-29", "2026-07-31"]), &library);
        assert!(!one_more.nothing_to_fetch());
    }

    #[test]
    fn the_day_before_is_the_broker_s_previous_day_and_only_for_one_pick() {
        let library = library_of(&[("WINV26", "2026-07-31")]);
        let monday = days_over(selection_of(&["2026-08-03"]), &library);
        assert_eq!(day_before(&offered(), "2026-08-03"), Some("2026-07-31"));
        assert_eq!(
            day_before(&offered(), "2026-07-29"),
            None,
            "the oldest held"
        );
        let target = monday.day_before_target(true, Some(&offered()));
        assert_eq!(
            target,
            Some(DayBeforeTarget {
                day: "2026-07-31".to_string(),
                on_disk: true
            })
        );
        assert_eq!(
            target.and_then(|t| t.to_fetch()),
            None,
            "held: joined, not fetched"
        );
        assert_eq!(monday.day_before_target(false, Some(&offered())), None);
        assert_eq!(monday.day_before_target(true, None), None, "no look-up yet");
        let two = days_over(selection_of(&["2026-08-03", "2026-08-04"]), &library);
        assert_eq!(two.day_before_target(true, Some(&offered())), None);
        assert!(
            two.day_before_note(true, true, None)
                .starts_with("Off while several")
        );
    }

    #[test]
    fn the_calendar_lines_up_with_a_known_week() {
        // 2026-08-12 was a Wednesday — the day WINQ26 expired.
        assert_eq!(weekday_monday_first(2026, 8, 12), 2);
        assert_eq!(weekday_monday_first(2026, 8, 10), 0, "Monday is column 0");
        assert_eq!(weekday_monday_first(2026, 8, 16), 6, "Sunday is column 6");
        // 1970-01-01 was a Thursday.
        assert_eq!(weekday_monday_first(1970, 1, 1), 3);
    }

    #[test]
    fn months_have_the_right_number_of_days() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29, "2024 is a leap year");
        assert_eq!(days_in_month(2000, 2), 29, "so is 2000");
        assert_eq!(days_in_month(1900, 2), 28, "but 1900 is not");
        assert_eq!(days_in_month(2026, 8), 31);
    }

    #[test]
    fn month_navigation_crosses_the_year() {
        assert_eq!(previous_month(2026, 1), (2025, 12));
        assert_eq!(next_month(2026, 12), (2027, 1));
        assert_eq!(next_month(2026, 8), (2026, 9));
    }

    #[test]
    fn counts_and_sizes_read_at_a_glance() {
        // The comma the Get data tab has always grouped prints with.
        assert_eq!(prints(389_622), "389,622");
        assert_eq!(prints(1_000_000), "1,000,000");
        assert_eq!(prints(7), "7");
        assert_eq!(size_label(49_000_000), "46 MB");
    }

    #[test]
    fn a_day_is_parsed_out_of_the_provider_s_own_spelling() {
        assert_eq!(parse_day("2026-08-12"), Some((2026, 8, 12)));
        assert_eq!(parse_day("nonsense"), None);
    }
}
