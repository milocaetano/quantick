//! Calendar statistics, selection and date labels shared by report surfaces.
use quantick_civil::{CivilDate, DateRange, TzOffset, parse_iso_date};
use quantick_sim::ClosedTrade;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
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

/// The calendar's click state machine. A first click picks a day and
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

/// Everything the calendar widget remembers between frames: which month is
/// on screen, what is picked, and whether the panel is expanded at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct CalendarState {
    /// Whether the month grid is expanded. Collapsed by default — the
    /// report's existing layout must not shrink for a trader who never
    /// asked for a calendar.
    pub open: bool,
    /// First day of the month on screen; `None` until the first draw picks
    /// the newest day that holds a trade.
    pub month: Option<CivilDate>,
    pub selection: DaySelection,
}

/// Read a harness hook's calendar spec: `1` opens the grid with nothing
/// picked, `YYYY-MM-DD` opens it on one day, and `YYYY-MM-DD..YYYY-MM-DD`
/// on a span. A spec that is none of those is refused rather than guessed
/// — a typo must reach no calendar, never the wrong month.
pub fn parse_selection(spec: &str) -> Option<DaySelection> {
    let spec = spec.trim();
    if spec == "1" {
        return Some(DaySelection::None);
    }
    match spec.split_once("..") {
        Some((start, end)) => Some(DaySelection::Range(DateRange::new(
            parse_iso_date(start)?,
            parse_iso_date(end)?,
        ))),
        None => Some(DaySelection::Anchor(parse_iso_date(spec)?)),
    }
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

    #[test]
    fn a_hook_spec_reaches_a_day_a_span_or_nothing_at_all() {
        assert_eq!(parse_selection("1"), Some(DaySelection::None));
        assert_eq!(
            parse_selection("2026-08-12"),
            Some(DaySelection::Anchor(CivilDate::from_ymd(2026, 8, 12)))
        );
        assert_eq!(
            parse_selection(" 2026-08-12..2026-08-17 "),
            Some(DaySelection::Range(DateRange::new(
                CivilDate::from_ymd(2026, 8, 12),
                CivilDate::from_ymd(2026, 8, 17),
            )))
        );
        // Written backwards it still names the same span.
        assert_eq!(
            parse_selection("2026-08-17..2026-08-12"),
            parse_selection("2026-08-12..2026-08-17")
        );
        for refused in [
            "",
            "0",
            "2026-08",
            "2026-08-12-01",
            "2026-13-01",
            "2026-02-30",
            "2026/08/12",
            "yesterday",
            "2026-08-12..",
            "..2026-08-12",
        ] {
            assert_eq!(
                parse_selection(refused),
                None,
                "{refused:?} must be refused"
            );
        }
    }
}
