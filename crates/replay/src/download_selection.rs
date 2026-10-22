//! Which days the Get data calendar picked, and what downloading them means.
//!
//! Stub: the tests below are written first; the implementation follows.

use std::collections::BTreeSet;

use crate::Library;

/// The days picked on the calendar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaySelection {
    days: BTreeSet<String>,
    anchor: Option<String>,
}

impl DaySelection {
    /// A click on `day`; `extend` is Shift held.
    pub fn click(&mut self, _day: &str, _offered: &BTreeSet<String>, _extend: bool) {
        todo!()
    }

    /// Forget every pick and the anchor.
    pub fn clear(&mut self) {
        todo!()
    }

    /// Whether `day` is picked.
    #[must_use]
    pub fn contains(&self, _day: &str) -> bool {
        todo!()
    }

    /// How many days are picked.
    #[must_use]
    pub fn len(&self) -> usize {
        todo!()
    }

    /// Whether nothing is picked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        todo!()
    }

    /// The day picked, when exactly one is.
    #[must_use]
    pub fn only(&self) -> Option<&str> {
        todo!()
    }

    /// The picked days, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.days.iter().map(String::as_str)
    }
}

/// The days of `symbol` the scanned folder already holds.
#[must_use]
pub fn held_days(_library: &Library, _symbol: &str) -> BTreeSet<String> {
    todo!()
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
    /// Already in the folder, so skipped.
    AlreadyHere,
    /// The provider refused it.
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
        todo!()
    }
}

/// A several-day download.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayBatch {
    rows: Vec<BatchRow>,
}

/// How a several-day download ended.
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
    /// The summary as the panel prints it.
    #[must_use]
    pub fn line(&self) -> String {
        todo!()
    }
}

impl DayBatch {
    /// The queue for `selection`, oldest first, with `held` days skipped.
    #[must_use]
    pub fn plan(_selection: &DaySelection, _held: &BTreeSet<String>) -> Self {
        todo!()
    }

    /// Every row, oldest first.
    #[must_use]
    pub fn rows(&self) -> &[BatchRow] {
        &self.rows
    }

    /// How many days still have to be downloaded.
    #[must_use]
    pub fn to_fetch(&self) -> usize {
        todo!()
    }

    /// Start the oldest waiting day, unless one is already running.
    pub fn begin_next(&mut self) -> Option<String> {
        todo!()
    }

    /// The day downloading now.
    #[must_use]
    pub fn running(&self) -> Option<&str> {
        todo!()
    }

    /// Prints written so far for the running day.
    pub fn progress(&mut self, _ticks: u64) {
        todo!()
    }

    /// The running day landed.
    pub fn finish(&mut self) {
        todo!()
    }

    /// The running day was refused.
    pub fn refuse(&mut self, _headline: String) {
        todo!()
    }

    /// Stop: the running day and every waiting one are cancelled.
    pub fn cancel(&mut self) {
        todo!()
    }

    /// Whether nothing is running or waiting.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        todo!()
    }

    /// The counts so far.
    #[must_use]
    pub fn summary(&self) -> BatchSummary {
        todo!()
    }

    /// The sentence above the Download button.
    #[must_use]
    pub fn estimate(&self, _context_sessions: u32) -> String {
        todo!()
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
}
