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
//! The panel in `app` paints it and launches each row's request. Days are the
//! provider's own spelling, `YYYY-MM-DD`, which sorts in date order as a
//! string, so a `BTreeSet<String>` iterates oldest first and that is the order
//! the queue fetches in. Ordered collections only, as everywhere a set can
//! reach what is drawn.
//!
//! The calendar's month arithmetic and the numbers printed beside it live here
//! too: they are the same day strings read as dates, and none of it needs the
//! screen it is shown on.

use std::collections::BTreeSet;
use std::ops::Bound;

use crate::Library;
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

/// The days of `symbol` the scanned folder already holds, `YYYY-MM-DD`.
///
/// Another contract's tape of the same date is not this one's, so the symbol
/// is matched exactly — the same test the day before a download makes.
#[must_use]
pub fn held_days(library: &Library, symbol: &str) -> BTreeSet<String> {
    library
        .sessions
        .iter()
        .filter(|held| held.symbol == symbol)
        .filter_map(|held| held.date.map(|d| d.label()))
        .collect()
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
    },
    /// On disk.
    Fetched {
        /// Prints it holds.
        ticks: u64,
    },
    /// Already in the folder, so skipped: re-exporting it would cost minutes
    /// and overwrite a recording the trader may have corrected by hand.
    AlreadyHere,
    /// The provider refused it; the days after it are still asked for.
    Refused {
        /// What the provider said.
        headline: String,
    },
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
    /// The row as the panel prints it.
    #[must_use]
    pub fn line(&self) -> String {
        let day = &self.day;
        match &self.status {
            DayStatus::Waiting => format!("{day} — waiting"),
            DayStatus::Running { ticks } => {
                format!("{day} — downloading, {} prints so far", thousands(*ticks))
            }
            DayStatus::Fetched { ticks } => {
                format!("{day} — {} prints downloaded", thousands(*ticks))
            }
            DayStatus::AlreadyHere => format!("{day} — already here, skipped"),
            DayStatus::Refused { headline } => format!("{day} — refused: {headline}"),
            DayStatus::Cancelled => format!("{day} — cancelled"),
        }
    }
}

/// How a several-day download ended, or stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchSummary {
    /// Days that landed.
    pub fetched: usize,
    /// Days skipped because the folder held them.
    pub already_here: usize,
    /// Days the provider refused.
    pub refused: usize,
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
        let mut line = format!(
            "{} fetched ({} prints) · {} already here · {} refused",
            self.fetched,
            thousands(self.ticks),
            self.already_here,
            self.refused
        );
        if self.cancelled > 0 {
            line.push_str(&format!(" · {} cancelled", self.cancelled));
        }
        line.push_str(" — they are listed under My sessions");
        line
    }
}

/// A several-day download: one row per picked day, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayBatch {
    rows: Vec<BatchRow>,
}

impl DayBatch {
    /// The queue for `selection`, oldest first, with `held` days skipped.
    #[must_use]
    pub fn plan(selection: &DaySelection, held: &BTreeSet<String>) -> Self {
        let rows = selection
            .iter()
            .map(|day| BatchRow {
                day: day.to_owned(),
                status: if held.contains(day) {
                    DayStatus::AlreadyHere
                } else {
                    DayStatus::Waiting
                },
            })
            .collect();
        Self { rows }
    }

    /// Every row, oldest first.
    #[must_use]
    pub fn rows(&self) -> &[BatchRow] {
        &self.rows
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
        row.status = DayStatus::Running { ticks: 0 };
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
        self.settle(|_| DayStatus::Running { ticks });
    }

    /// The running day landed.
    pub fn finish(&mut self) {
        self.settle(|ticks| DayStatus::Fetched { ticks });
    }

    /// The running day was refused.
    pub fn refuse(&mut self, headline: String) {
        self.settle(|_| DayStatus::Refused { headline });
    }

    /// Stop: the running day and every waiting one are cancelled.
    pub fn cancel(&mut self) {
        for row in &mut self.rows {
            if matches!(row.status, DayStatus::Waiting | DayStatus::Running { .. }) {
                row.status = DayStatus::Cancelled;
            }
        }
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
                DayStatus::Fetched { ticks } => {
                    summary.fetched += 1;
                    summary.ticks += ticks;
                }
                DayStatus::AlreadyHere => summary.already_here += 1,
                DayStatus::Refused { .. } => summary.refused += 1,
                DayStatus::Cancelled => summary.cancelled += 1,
                DayStatus::Waiting | DayStatus::Running { .. } => {}
            }
        }
        summary
    }

    /// The sentence above the Download button: every picked day counted,
    /// and sized by the ones that will actually be fetched.
    #[must_use]
    pub fn estimate(&self, context_sessions: u32) -> String {
        let (picked, fetch) = (self.rows.len(), self.to_fetch());
        if fetch == 0 {
            return format!("{picked} days, all already here — nothing to download");
        }
        let held = self.count(|status| *status == DayStatus::AlreadyHere);
        format!(
            "{fetch} of {picked} days to fetch ({held} already here) · about {} · \
             run-up {context_sessions} session(s) each · nothing opens on the chart",
            size_label(tape_bytes(fetch as u64))
        )
    }

    fn count(&self, matches: impl Fn(&DayStatus) -> bool) -> usize {
        self.rows.iter().filter(|row| matches(&row.status)).count()
    }

    /// Move the running row on, handing the new status its prints so far.
    fn settle(&mut self, next: impl FnOnce(u64) -> DayStatus) {
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| matches!(row.status, DayStatus::Running { .. }))
        {
            let DayStatus::Running { ticks } = row.status else {
                return;
            };
            row.status = next(ticks);
        }
    }
}

/// About how many bytes `tapes` days of prints take on disk.
#[must_use]
pub fn tape_bytes(tapes: u64) -> u64 {
    tapes * TYPICAL_TICKS_PER_SESSION * BYTES_PER_TICK
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

/// How many days `month` of `year` has.
#[must_use]
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
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

/// `1234567` → `1,234,567`.
#[must_use]
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
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

    #[test]
    fn held_days_are_this_symbol_s_days_in_the_folder() {
        let entry = |symbol: &str, day: &str| SessionEntry {
            path: PathBuf::from(format!("replay/{symbol}/{day}.csv")),
            symbol: symbol.to_string(),
            date: SessionDate::parse(day),
            size_bytes: 0,
            timezone: UtcOffset::UTC,
            timezone_declared: true,
            side_source: None,
            has_quotes: false,
            notes: Vec::new(),
        };
        let library = Library {
            root: PathBuf::from("replay"),
            sessions: vec![entry("WINV26", "2026-08-03"), entry("WINQ26", "2026-08-04")],
            problems: Vec::new(),
        };
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
        let mut batch = DayBatch::plan(&selection, &set(&["2026-08-03"]));

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
        assert_eq!(batch.rows()[0].status, DayStatus::Fetched { ticks: 900 });
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
        let mut batch = DayBatch::plan(&selection, &set(&["2026-07-31"]));
        assert_eq!(batch.begin_next().as_deref(), Some("2026-07-29"));
        batch.refuse("the terminal holds no ticks for that day".to_string());
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
                already_here: 1,
                refused: 1,
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
            batch.rows()[0].line().contains("no ticks for that day"),
            "the refusal is said on its own row: {}",
            batch.rows()[0].line()
        );
    }

    #[test]
    fn cancelling_stops_the_running_day_and_everything_behind_it() {
        let selection = selection_of(&["2026-07-29", "2026-07-30", "2026-07-31"]);
        let mut batch = DayBatch::plan(&selection, &BTreeSet::new());
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
        let batch = DayBatch::plan(&selection, &set(&["2026-07-30"]));
        let estimate = batch.estimate(5);
        assert!(estimate.contains("2 of 3 days"), "{estimate}");
        assert!(estimate.contains("1 already here"), "{estimate}");
        assert!(estimate.contains("93 MB"), "two tapes' worth: {estimate}");
        assert!(estimate.contains("5 session(s)"), "{estimate}");

        let all_here = DayBatch::plan(
            &selection,
            &set(&["2026-07-29", "2026-07-30", "2026-07-31"]),
        );
        assert!(
            all_here.estimate(5).contains("nothing to download"),
            "{}",
            all_here.estimate(5)
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
        assert_eq!(thousands(389_622), "389,622");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(thousands(7), "7");
        assert_eq!(size_label(49_000_000), "46 MB");
    }

    #[test]
    fn a_day_is_parsed_out_of_the_provider_s_own_spelling() {
        assert_eq!(parse_day("2026-08-12"), Some((2026, 8, 12)));
        assert_eq!(parse_day("nonsense"), None);
    }
}
