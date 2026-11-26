//! Calendar statistics, selection and date labels shared by report surfaces.
use quantick_civil::{CivilDate, DateRange, TzOffset, parse_iso_date};
use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};
/// `January`…`December`, for the calendar's own header.
pub fn month_name(month: i64) -> &'static str {
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
    usize::try_from(month - 1)
        .ok()
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or("???")
}

// ----------------------------------------------------------------------
// The day index
// ----------------------------------------------------------------------

/// What one civil day holds — the calendar cell's whole story.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DayStat {
    pub trades: usize,
    pub wins: usize,
    pub net: Decimal,
}

/// What a day cell says under its number, and how a picked day is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayFace {
    /// The ledger's Dates picker: the day's trade count, picked days tinted.
    Trades,
    /// The report's popover: the day's compact net, picked days filled solid.
    Net,
}

impl DayFace {
    /// The figure printed under the day number.
    pub fn figure(self, stat: &DayStat) -> String {
        match self {
            Self::Trades => stat.trades.to_string(),
            Self::Net => crate::format::fmt_compact_signed_points(stat.net),
        }
    }

    /// Whether a picked day is filled solid rather than tinted.
    pub fn fills_pick(self) -> bool {
        self == Self::Net
    }
}

/// Which civil days hold trades, and what each one did. Built once per
/// history load and per timezone — never per frame: a calendar repainting
/// at 60 fps must not walk months of history to decide a cell's colour.
#[derive(Debug, Clone, Default)]
pub struct DayIndex {
    days: BTreeMap<i64, DayStat>,
}

impl DayIndex {
    /// Index every trade by the civil day it *closed* on. Closing time is
    /// the one the ledger, the equity curve and the period filter already
    /// agree on; indexing by open time would put a trade on a day whose
    /// P&L it did not produce.
    /// The index does not keep `tz`: which timezone it was cut with is the
    /// caller's cache key (`report_days_key`), because the caller is what
    /// decides when to rebuild.
    pub fn build<'a>(trades: impl Iterator<Item = &'a ClosedTrade>, tz: TzOffset) -> Self {
        let mut days: BTreeMap<i64, DayStat> = BTreeMap::new();
        for trade in trades {
            let day = CivilDate::from_ms(trade.closed_ms, tz).day_number();
            let stat = days.entry(day).or_default();
            stat.trades += 1;
            if trade.pnl_points > Decimal::ZERO {
                stat.wins += 1;
            }
            stat.net = stat.net.saturating_add(trade.pnl_points);
        }
        Self { days }
    }

    /// What that day holds, or `None` when it holds nothing.
    pub fn stat(&self, date: CivilDate) -> Option<DayStat> {
        self.days.get(&date.day_number()).copied()
    }

    /// Oldest day holding a trade.
    pub fn first(&self) -> Option<CivilDate> {
        self.days
            .keys()
            .next()
            .map(|day| CivilDate::from_day_number(*day))
    }

    /// Newest day holding a trade — where the calendar opens.
    pub fn last(&self) -> Option<CivilDate> {
        self.days
            .keys()
            .next_back()
            .map(|day| CivilDate::from_day_number(*day))
    }

    pub fn span_label(&self) -> String {
        match (self.first(), self.last()) {
            (Some(first), Some(last)) => format!(
                "{} day(s) with trades · {} to {}",
                self.len(),
                first.iso(),
                last.iso()
            ),
            _ => "no days on record in this scope".to_owned(),
        }
    }

    /// How many days hold at least one trade.
    pub fn len(&self) -> usize {
        self.days.len()
    }

    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }
}

// ----------------------------------------------------------------------
// The selection
// ----------------------------------------------------------------------

/// The ledger calendar's click state machine. A first click picks a day and
/// filters to it immediately — a trader asking "what happened on the 12th"
/// should not have to click twice. A second click turns the pick into a
/// span; clicking the picked day again clears, so the calendar is its own
/// undo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DaySelection {
    /// Nothing picked — the anchor-relative period pills are in charge.
    #[default]
    None,
    /// One day picked, and filtering to it; a second click extends.
    Anchor(CivilDate),
    /// A closed span.
    Range(DateRange),
}

impl DaySelection {
    /// Apply a click on `day`.
    pub fn click(self, day: CivilDate) -> Self {
        match self {
            // Clicking the one picked day again is the deselect gesture.
            Self::Anchor(anchor) if anchor == day => Self::None,
            Self::Anchor(anchor) => Self::Range(DateRange::new(anchor, day)),
            // A closed span restarts rather than growing: extending an
            // existing range on click would make the two ends impossible
            // to tell apart, and "start over" is what a second thought is.
            Self::None | Self::Range(_) => Self::Anchor(day),
        }
    }

    /// The span this selection filters on; `None` means "not filtering".
    pub fn range(self) -> Option<DateRange> {
        match self {
            Self::None => None,
            Self::Anchor(day) => Some(DateRange {
                start: day,
                end: day,
            }),
            Self::Range(range) => Some(range),
        }
    }

    /// Whether `day` sits inside the selection.
    pub fn contains(self, day: CivilDate) -> bool {
        self.range()
            .is_some_and(|range| day >= range.start && day <= range.end)
    }
}

/// The ledger calendar's widget state between frames: which month is on
/// screen, what is picked, and whether the panel is expanded at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct CalendarState {
    pub open: bool,
    /// First day of the month on screen; `None` until the first draw picks
    /// the newest day that holds a trade.
    pub month: Option<CivilDate>,
    pub selection: DaySelection,
}

// ----------------------------------------------------------------------
// The report's picked days
// ----------------------------------------------------------------------

/// How many runs a label names before it says "…": a chip has one line.
const LABEL_RUNS: usize = 3;

/// Most days one call may name, about a quarter; a longer window is what
/// the period pills are for, and the readback stays bounded.
pub const MAX_LISTED_DAYS: usize = 92;

/// The report's date filter: any set of civil days in the display timezone,
/// contiguous or not. Empty means "not filtering" — the period pills are in
/// charge. A trade belongs when the civil day it *closed* on is in the set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct DaySet {
    days: BTreeSet<i64>,
}

impl DaySet {
    /// Every day from `a` through `b`, in either order.
    #[must_use]
    pub fn span(a: CivilDate, b: CivilDate) -> Self {
        let mut set = Self::default();
        set.add_span(a, b);
        set
    }

    #[must_use]
    pub fn single(day: CivilDate) -> Self {
        Self::span(day, day)
    }

    /// Picked days in order. A click is a toggle: picking a picked day again
    /// takes it out, so the grid is its own undo.
    pub fn toggle(&mut self, day: CivilDate) {
        if !self.days.remove(&day.day_number()) {
            self.days.insert(day.day_number());
        }
    }

    /// Add every day from `a` through `b`, in either order; never removes.
    pub fn add_span(&mut self, a: CivilDate, b: CivilDate) {
        let range = DateRange::new(a, b);
        self.days
            .extend(range.start.day_number()..=range.end.day_number());
    }

    pub fn contains(&self, day: CivilDate) -> bool {
        self.days.contains(&day.day_number())
    }

    /// Whether a trade closing at `timestamp_ms` closed on a picked day.
    pub fn contains_ms(&self, timestamp_ms: i64, tz: TzOffset) -> bool {
        self.contains(CivilDate::from_ms(timestamp_ms, tz))
    }

    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }

    pub fn len(&self) -> usize {
        self.days.len()
    }

    pub fn first(&self) -> Option<CivilDate> {
        self.days.first().copied().map(CivilDate::from_day_number)
    }

    pub fn last(&self) -> Option<CivilDate> {
        self.days.last().copied().map(CivilDate::from_day_number)
    }

    /// Picked days, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = CivilDate> + '_ {
        self.days.iter().copied().map(CivilDate::from_day_number)
    }

    /// Consecutive picked days as closed spans, oldest first.
    pub fn runs(&self) -> Vec<DateRange> {
        let mut runs: Vec<DateRange> = Vec::new();
        for day in self.iter() {
            match runs.last_mut() {
                Some(run) if run.end.offset_days(1) == day => run.end = day,
                _ => runs.push(DateRange {
                    start: day,
                    end: day,
                }),
            }
        }
        runs
    }

    /// `Nov 24 to Nov 25 · 2 days` for one span; `Nov 12, Nov 24, Nov 25 ·
    /// 3 days` for a sparse set, with longer spans inside it joined by `to`
    /// (the proportional font has no arrow glyph). The year is spelled only
    /// when the set does not sit in one year.
    pub fn label(&self) -> String {
        let (Some(first), Some(last)) = (self.first(), self.last()) else {
            return "no days".to_owned();
        };
        let with_year = first.ymd().0 != last.ymd().0;
        let name = |day: CivilDate| {
            let (year, month, date) = day.ymd();
            if with_year {
                format!("{} {date} {year}", quantick_civil::month_abbr(month))
            } else {
                format!("{} {date}", quantick_civil::month_abbr(month))
            }
        };
        let runs = self.runs();
        let mut parts: Vec<String> = Vec::new();
        for run in runs.iter().take(LABEL_RUNS) {
            match run.days() {
                1 => parts.push(name(run.start)),
                2 => parts.extend([name(run.start), name(run.end)]),
                _ => parts.push(format!("{} to {}", name(run.start), name(run.end))),
            }
        }
        if runs.len() > LABEL_RUNS {
            parts.push("…".to_owned());
        }
        let separator = if runs.len() == 1 && runs[0].days() == 2 {
            " to "
        } else {
            ", "
        };
        let count = self.len();
        format!(
            "{} · {count} day{}",
            parts.join(separator),
            if count == 1 { "" } else { "s" }
        )
    }

    /// Read a list of `YYYY-MM-DD` days. A day that is not a real date in
    /// exactly that spelling refuses the whole list — a typo must never
    /// select the wrong day — and so do more than [`MAX_LISTED_DAYS`]
    /// distinct days, refused as soon as the set passes the cap, so the
    /// work is bounded whatever the list's length. Duplicates collapse;
    /// order does not matter.
    pub fn parse_list<S: AsRef<str>>(days: &[S]) -> Result<Self, DayListError> {
        let mut set = Self::default();
        for text in days {
            let text = text.as_ref();
            if text.len() > ISO_DATE_LEN {
                return Err(DayListError::TooLong(text.len()));
            }
            let day = parse_iso_date(text)
                .filter(|day| day.iso() == text)
                .ok_or_else(|| DayListError::NotADate(text.to_owned()))?;
            set.days.insert(day.day_number());
            if set.len() > MAX_LISTED_DAYS {
                return Err(DayListError::TooMany);
            }
        }
        Ok(set)
    }
}

/// `YYYY-MM-DD`: the longest entry a day list reads, and so the longest
/// one a refusal ever echoes.
pub const ISO_DATE_LEN: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DayListError {
    NotADate(String),
    /// An entry longer than [`ISO_DATE_LEN`] bytes, by its length alone.
    TooLong(usize),
    /// More than [`MAX_LISTED_DAYS`] distinct days.
    TooMany,
}

impl std::fmt::Display for DayListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotADate(text) => write!(f, "`{text}` is not a YYYY-MM-DD date"),
            Self::TooLong(bytes) => write!(
                f,
                "an entry of {bytes} bytes is not a YYYY-MM-DD date (at most {ISO_DATE_LEN})"
            ),
            Self::TooMany => write!(
                f,
                "more than {MAX_LISTED_DAYS} days named - use a period for a longer window"
            ),
        }
    }
}

/// The popover's one-click windows, each ending on the display clock's
/// today, which the caller supplies: this crate never reads a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayPreset {
    Today,
    Yesterday,
    /// Monday of today's week through today.
    ThisWeek,
    /// The first of today's month through today.
    ThisMonth,
}

impl DayPreset {
    pub const ALL: [Self; 4] = [
        Self::Today,
        Self::Yesterday,
        Self::ThisWeek,
        Self::ThisMonth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::ThisWeek => "This week",
            Self::ThisMonth => "This month",
        }
    }

    #[must_use]
    pub fn days(self, today: CivilDate) -> DaySet {
        match self {
            Self::Today => DaySet::single(today),
            Self::Yesterday => DaySet::single(today.offset_days(-1)),
            Self::ThisWeek => DaySet::span(today.offset_days(-today.weekday()), today),
            Self::ThisMonth => DaySet::span(today.month_start(), today),
        }
    }
}

/// The report grid's gesture state: the picked days, and the day the last
/// click landed on, where a Shift+click range starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayPick {
    pub days: DaySet,
    anchor: Option<CivilDate>,
}

impl DayPick {
    /// Replace the whole set (a preset, a call, a clear). Its newest day
    /// becomes the anchor, so a Shift+click right after "This week" grows
    /// the week rather than starting from nowhere.
    #[must_use]
    pub fn of(days: DaySet) -> Self {
        let anchor = days.last();
        Self { days, anchor }
    }

    /// A click toggles `day`; with `extend` (Shift) it adds every day from
    /// the last clicked day through this one, in either direction. A Shift
    /// click with nothing clicked yet adds just that day.
    pub fn click(&mut self, day: CivilDate, extend: bool) {
        match (extend, self.anchor) {
            (true, Some(anchor)) => self.days.add_span(anchor, day),
            (true, None) => self.days.add_span(day, day),
            (false, _) => self.days.toggle(day),
        }
        self.anchor = Some(day);
    }
}

/// The report's month grid between frames: whether its popover is open and
/// which month is on screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MonthPicker {
    pub open: bool,
    /// First day of the month on screen; `None` until the first draw picks
    /// the newest day that holds a trade.
    pub month: Option<CivilDate>,
}

/// Read a harness hook's report-calendar spec: `1` opens the popover with
/// nothing picked; otherwise a comma list of `YYYY-MM-DD` days and
/// `YYYY-MM-DD..YYYY-MM-DD` spans (either order) opens it with those picked.
/// A spec that is none of those is refused rather than guessed — a typo
/// must reach no calendar, never the wrong month.
pub fn parse_days(spec: &str) -> Option<DaySet> {
    let spec = spec.trim();
    if spec == "1" {
        return Some(DaySet::default());
    }
    let day = |text: &str| parse_iso_date(text.trim());
    let mut set = DaySet::default();
    for part in spec.split(',') {
        match part.split_once("..") {
            Some((start, end)) => set.add_span(day(start)?, day(end)?),
            None => {
                let day = day(part)?;
                set.add_span(day, day);
            }
        }
    }
    Some(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() -> TzOffset {
        TzOffset::new(0)
    }

    /// The user's own timezone — every off-by-one this module could have
    /// shows up here first.
    fn sao_paulo() -> TzOffset {
        TzOffset::new(-180)
    }

    fn trade(closed_ms: i64, pnl: i64) -> ClosedTrade {
        ClosedTrade {
            side: quantick_engine::Side::Buy,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100) + Decimal::from(pnl),
            opened_ms: closed_ms - 1_000,
            closed_ms,
            pnl_points: Decimal::from(pnl),
            exit_reason: quantick_sim::ExitReason::Manual,
            entry_agg_id: None,
            exit_agg_id: None,
            mae_points: None,
            mfe_points: None,
        }
    }

    #[test]
    fn the_day_index_counts_by_the_closing_day_in_the_display_timezone() {
        let day = CivilDate::from_ymd(2026, 8, 17);
        let trades = [
            trade(day.start_ms(sao_paulo()) + 3_600_000, 10),
            trade(day.end_ms(sao_paulo()) - 1, -4),
            // 00:30 local on the next day — a different cell.
            trade(day.end_ms(sao_paulo()) + 1_800_000, 7),
        ];
        let index = DayIndex::build(trades.iter(), sao_paulo());
        assert_eq!(index.len(), 2);
        let stat = index.stat(day).expect("the 17th holds trades");
        assert_eq!(stat.trades, 2);
        assert_eq!(stat.wins, 1);
        assert_eq!(stat.net, Decimal::from(6));
        assert_eq!(index.first(), Some(day));
        assert_eq!(index.last(), Some(day.offset_days(1)));
        assert_eq!(
            index.stat(day.offset_days(-1)),
            None,
            "a quiet day is empty"
        );
    }

    #[test]
    fn the_same_trades_land_on_different_days_under_a_different_timezone() {
        let boundary = CivilDate::from_ymd(2026, 8, 18).start_ms(utc()) + 60 * 60_000;
        let trades = [trade(boundary, 5)];
        assert_eq!(
            DayIndex::build(trades.iter(), utc())
                .first()
                .map(CivilDate::iso),
            Some("2026-08-18".to_owned())
        );
        assert_eq!(
            DayIndex::build(trades.iter(), sao_paulo())
                .first()
                .map(CivilDate::iso),
            Some("2026-08-17".to_owned()),
            "the index must be rebuilt when the display timezone moves"
        );
    }

    #[test]
    fn one_click_picks_a_day_and_filters_to_it() {
        let day = CivilDate::from_ymd(2026, 8, 12);
        let selection = DaySelection::None.click(day);
        assert_eq!(selection, DaySelection::Anchor(day));
        let range = selection.range().expect("one day is still a filter");
        assert_eq!(range.days(), 1);
        assert_eq!(range.label(), "2026-08-12");
    }

    #[test]
    fn a_second_click_makes_a_range_in_either_direction() {
        let earlier = CivilDate::from_ymd(2026, 8, 12);
        let later = CivilDate::from_ymd(2026, 8, 17);
        let forwards = DaySelection::None.click(earlier).click(later);
        let backwards = DaySelection::None.click(later).click(earlier);
        assert_eq!(forwards, backwards, "click order must not change the span");
        let range = forwards.range().expect("a closed span");
        assert_eq!(range.label(), "2026-08-12 to 2026-08-17");
        assert_eq!(range.days(), 6, "both ends counted");
    }

    #[test]
    fn clicking_the_picked_day_again_clears_the_filter() {
        let day = CivilDate::from_ymd(2026, 8, 12);
        assert_eq!(DaySelection::None.click(day).click(day), DaySelection::None);
        assert!(DaySelection::None.click(day).click(day).range().is_none());
    }

    #[test]
    fn clicking_inside_a_closed_range_starts_a_new_pick() {
        let start = CivilDate::from_ymd(2026, 8, 12);
        let end = CivilDate::from_ymd(2026, 8, 17);
        let middle = CivilDate::from_ymd(2026, 8, 14);
        let restarted = DaySelection::None.click(start).click(end).click(middle);
        assert_eq!(restarted, DaySelection::Anchor(middle));
    }

    #[test]
    fn selection_membership_matches_the_range_it_reports() {
        let selection = DaySelection::None
            .click(CivilDate::from_ymd(2026, 8, 12))
            .click(CivilDate::from_ymd(2026, 8, 17));
        assert!(selection.contains(CivilDate::from_ymd(2026, 8, 12)));
        assert!(selection.contains(CivilDate::from_ymd(2026, 8, 14)));
        assert!(selection.contains(CivilDate::from_ymd(2026, 8, 17)));
        assert!(!selection.contains(CivilDate::from_ymd(2026, 8, 11)));
        assert!(!selection.contains(CivilDate::from_ymd(2026, 8, 18)));
    }

    fn nov(day: i64) -> CivilDate {
        CivilDate::from_ymd(2026, 11, day)
    }

    fn picked(pick: &DayPick) -> Vec<i64> {
        pick.days.iter().map(|day| day.ymd().2).collect()
    }

    #[test]
    fn a_click_toggles_one_day_and_a_second_click_takes_it_out() {
        let mut pick = DayPick::default();
        pick.click(nov(24), false);
        pick.click(nov(12), false);
        assert_eq!(
            picked(&pick),
            [12, 24],
            "kept in day order, not click order"
        );
        pick.click(nov(24), false);
        assert_eq!(picked(&pick), [12]);
        pick.click(nov(12), false);
        assert!(pick.days.is_empty(), "the grid is its own undo");
    }

    #[test]
    fn shift_click_adds_every_day_from_the_last_click_in_either_direction() {
        let mut forwards = DayPick::default();
        forwards.click(nov(20), false);
        forwards.click(nov(24), true);
        assert_eq!(picked(&forwards), [20, 21, 22, 23, 24]);
        let mut backwards = DayPick::default();
        backwards.click(nov(24), false);
        backwards.click(nov(20), true);
        assert_eq!(backwards.days, forwards.days, "direction does not matter");
        // The anchor moves with every click: the next range starts at the 20th.
        backwards.click(nov(18), true);
        assert_eq!(picked(&backwards), [18, 19, 20, 21, 22, 23, 24]);
    }

    #[test]
    fn shift_click_never_removes_and_starts_alone_without_an_anchor() {
        let mut pick = DayPick::default();
        pick.click(nov(25), true);
        assert_eq!(picked(&pick), [25], "nothing clicked yet: just that day");
        pick.click(nov(22), false);
        pick.click(nov(22), false);
        assert_eq!(
            picked(&pick),
            [25],
            "the 22nd toggled out, still the anchor"
        );
        pick.click(nov(24), true);
        assert_eq!(picked(&pick), [22, 23, 24, 25], "a range only ever adds");
    }

    #[test]
    fn a_preset_or_call_replaces_the_set_and_anchors_on_its_newest_day() {
        let mut pick = DayPick::of(DayPreset::ThisWeek.days(nov(25)));
        assert_eq!(picked(&pick), [23, 24, 25], "Mon 23 through Wed 25");
        pick.click(nov(20), true);
        assert_eq!(picked(&pick), (20..=25).collect::<Vec<_>>());
    }

    #[test]
    fn presets_end_on_the_today_they_are_told() {
        let today = nov(25);
        let days = |preset: DayPreset| {
            preset
                .days(today)
                .iter()
                .map(CivilDate::iso)
                .collect::<Vec<_>>()
        };
        assert_eq!(days(DayPreset::Today), ["2026-11-25"]);
        assert_eq!(days(DayPreset::Yesterday), ["2026-11-24"]);
        assert_eq!(
            DayPreset::ThisMonth.days(today),
            DaySet::span(nov(1), today)
        );
        // A Monday's week is that one day; a Sunday's is the whole week.
        assert_eq!(DayPreset::ThisWeek.days(nov(23)).len(), 1);
        assert_eq!(
            DayPreset::ThisWeek.days(nov(29)),
            DaySet::span(nov(23), nov(29))
        );
        assert_eq!(
            DayPreset::ThisMonth
                .days(CivilDate::from_ymd(2027, 1, 1))
                .len(),
            1,
            "the 1st of a month is a one-day month so far"
        );
    }

    #[test]
    fn the_label_joins_a_span_with_to_and_lists_a_sparse_set() {
        assert_eq!(
            DaySet::span(nov(24), nov(25)).label(),
            "Nov 24 to Nov 25 · 2 days"
        );
        assert_eq!(
            DaySet::span(nov(20), nov(25)).label(),
            "Nov 20 to Nov 25 · 6 days"
        );
        assert_eq!(DaySet::single(nov(24)).label(), "Nov 24 · 1 day");
        let mut sparse = DaySet::span(nov(24), nov(25));
        sparse.toggle(nov(12));
        assert_eq!(sparse.label(), "Nov 12, Nov 24, Nov 25 · 3 days");
        let mut long = DaySet::span(nov(2), nov(6));
        for day in [10, 12, 14, 16] {
            long.toggle(nov(day));
        }
        assert_eq!(long.label(), "Nov 2 to Nov 6, Nov 10, Nov 12, … · 9 days");
        let across = DaySet::span(
            CivilDate::from_ymd(2026, 12, 31),
            CivilDate::from_ymd(2027, 1, 1),
        );
        assert_eq!(across.label(), "Dec 31 2026 to Jan 1 2027 · 2 days");
    }

    #[test]
    fn membership_breaks_where_the_display_timezone_sees_midnight() {
        let set = DaySet::span(nov(24), nov(25));
        let tz = sao_paulo();
        assert!(set.contains_ms(nov(24).start_ms(tz), tz));
        assert!(
            set.contains_ms(nov(25).end_ms(tz) - 1, tz),
            "23:59:59.999 of today"
        );
        assert!(!set.contains_ms(nov(24).start_ms(tz) - 1, tz));
        assert!(!set.contains_ms(nov(25).end_ms(tz), tz));
        // 01:30 UTC on the 26th is still the 25th in São Paulo, not in UTC.
        let late = nov(26).start_ms(utc()) + 90 * 60_000;
        assert!(set.contains_ms(late, tz));
        assert!(!set.contains_ms(late, utc()));
    }

    #[test]
    fn a_day_list_is_read_strictly_and_bounded() {
        let set = DaySet::parse_list(&["2026-11-25", "2026-11-24", "2026-11-25"]).unwrap();
        assert_eq!(set, DaySet::span(nov(24), nov(25)), "duplicates collapse");
        assert_eq!(DaySet::parse_list::<&str>(&[]).unwrap(), DaySet::default());
        for refused in ["2026-11-31", "2026-11-5", "2026/11/25", "", "today"] {
            assert_eq!(
                DaySet::parse_list(&["2026-11-24", refused]),
                Err(DayListError::NotADate(refused.to_owned())),
                "{refused:?} refuses the whole list"
            );
        }
        let many: Vec<String> = (0..=MAX_LISTED_DAYS as i64)
            .map(|i| nov(1).offset_days(i).iso())
            .collect();
        assert_eq!(DaySet::parse_list(&many), Err(DayListError::TooMany));
        assert!(DaySet::parse_list(&many[..MAX_LISTED_DAYS]).is_ok());
        let mut past_the_cap = many.clone();
        past_the_cap.push("not a day".to_owned());
        assert_eq!(
            DaySet::parse_list(&past_the_cap),
            Err(DayListError::TooMany),
            "the read stops at the cap, before the entries after it"
        );
    }

    /// The cap counts days, not entries: a retry that names a day twice
    /// is still the same quarter.
    #[test]
    fn the_day_cap_counts_distinct_days() {
        let mut repeated: Vec<String> = (0..MAX_LISTED_DAYS as i64)
            .map(|i| nov(1).offset_days(i).iso())
            .collect();
        repeated.extend(repeated.clone());
        let set = DaySet::parse_list(&repeated).unwrap();
        assert_eq!(set.len(), MAX_LISTED_DAYS);
    }

    /// An over-long entry is refused by its length and never echoed whole.
    #[test]
    fn an_over_long_entry_is_refused_without_its_text() {
        let long = format!("2026-11-24{}", "x".repeat(10_000));
        let error = DaySet::parse_list(&["2026-11-25", long.as_str()]).unwrap_err();
        assert_eq!(error, DayListError::TooLong(long.len()));
        let message = error.to_string();
        assert!(!message.contains("xxxx"), "{message}");
        assert!(message.len() < 80, "{message}");
        assert_eq!(
            DaySet::parse_list(&["2026-11-24 "]),
            Err(DayListError::TooLong(11))
        );
    }

    #[test]
    fn a_hook_spec_reaches_days_spans_or_an_empty_open_grid() {
        assert_eq!(parse_days("1"), Some(DaySet::default()));
        assert_eq!(parse_days("2026-11-24"), Some(DaySet::single(nov(24))));
        assert_eq!(
            parse_days(" 2026-11-25..2026-11-24 "),
            Some(DaySet::span(nov(24), nov(25)))
        );
        let mut sparse = DaySet::span(nov(24), nov(25));
        sparse.toggle(nov(12));
        assert_eq!(
            parse_days("2026-11-12,2026-11-24..2026-11-25,2026-11-24"),
            Some(sparse)
        );
        for refused in [
            "",
            "0",
            "2026-11",
            "2026-13-01",
            "2026-02-30",
            "yesterday",
            "2026-11-12..",
            "2026-11-12,,2026-11-13",
        ] {
            assert_eq!(parse_days(refused), None, "{refused:?} must be refused");
        }
    }

    #[test]
    fn the_ledger_face_counts_trades_and_the_report_face_fills_a_compact_net() {
        let stat = DayStat {
            trades: 3,
            wins: 2,
            net: Decimal::new(123_456, 1),
        };
        assert_eq!(DayFace::Trades.figure(&stat), "3");
        assert_eq!(DayFace::Net.figure(&stat), "+12k");
        assert!(!DayFace::Trades.fills_pick());
        assert!(DayFace::Net.fills_pick());
    }
}
