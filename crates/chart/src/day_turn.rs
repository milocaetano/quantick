//! Where the display day turns over, and how the time strip and the
//! pointer's tag name a day: the turns the day separator rules across the
//! candles, the labels that write the day number, month or year where it
//! changes, and the pointer's dated time tag.

use quantick_civil::{CivilDate, TzOffset, month_abbr, weekday_abbr};

use crate::geometry::{TimeLabelFormat, time_label};
use crate::viewport::Viewport;

/// A slot whose bar opens a new display day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayTurn {
    pub slot: usize,
    /// The date the new day opens, from `quantick_civil::day_starts`.
    pub date: CivilDate,
}

/// `Mon 28 Sep`: a bar's date as the pointer's tag writes it.
fn pinned_text(date: CivilDate) -> String {
    format!("{} {}", weekday_abbr(date.weekday()), date.short())
}

/// The pointer's time tag below a day, in the display timezone `tz`:
/// `Fri 09 Oct '26 10:57:03` — the [`pinned_text`] date, the two-digit year
/// and the clock. The clock alone reads the same on every day of a tape that
/// spans several; the tag names the day the bar opened on.
#[must_use]
pub fn pointer_time_label(ms: i64, tz: TzOffset) -> String {
    let date = CivilDate::from_ms(ms, tz);
    let (year, _, _) = date.ymd();
    format!(
        "{} '{:02} {}",
        pinned_text(date),
        year.rem_euclid(100),
        time_label(ms, tz, TimeLabelFormat::Full)
    )
}

/// The widest form of the pointer's sub-day tag that `fits`: the full
/// [`pointer_time_label`], then the weekday and day with the clock
/// (`Fri 09 10:57:03`), then the clock (`10:57:03`), then the short clock
/// (`10:57`). Each is true and shorter; a strip too narrow even for the last
/// gets the last, clipped by the caller.
#[must_use]
pub fn pointer_time_fitting(ms: i64, tz: TzOffset, fits: impl Fn(&str) -> bool) -> String {
    let date = CivilDate::from_ms(ms, tz);
    let clock = time_label(ms, tz, TimeLabelFormat::Full);
    let short = time_label(ms, tz, TimeLabelFormat::Short);
    let (_, _, day) = date.ymd();
    let dated = format!("{} {day:02} {clock}", weekday_abbr(date.weekday()));
    [pointer_time_label(ms, tz), dated, clock]
        .into_iter()
        .find(|text| fits(text))
        .unwrap_or(short)
}

/// Writes the time strip's labels left to right, as TradingView does: the
/// first label *painted* in a new local day writes the day (`9`), in a new
/// month the month (`Oct`), in a new year the year (`2026`); every other
/// label is the clock in its format.
///
/// Fed only the labels that are painted, so a label skipped for room, for a
/// chip or for a missing open time never moves the comparison.
///
/// A turn's name is never wider than the format's
/// [`sample`](TimeLabelFormat::sample) — four characters at most against the
/// clock's five — so the stride measured from the sample still holds.
#[derive(Debug, Clone)]
pub struct TickLabels {
    tz: TzOffset,
    format: TimeLabelFormat,
    /// The date of the last label painted, or the seed before the first.
    last: Option<CivilDate>,
}

impl TickLabels {
    /// `seed_ms` is an instant before the first label (the bar before the
    /// first visible one), so a day that turns before the first label is
    /// still named on it; `None` writes the first label as a clock.
    #[must_use]
    pub fn new(tz: TzOffset, format: TimeLabelFormat, seed_ms: Option<i64>) -> Self {
        Self {
            tz,
            format,
            last: seed_ms.map(|ms| CivilDate::from_ms(ms, tz)),
        }
    }

    /// The text of the label about to be painted at `ms`. Call it only for a
    /// label that is painted.
    pub fn label_at(&mut self, ms: i64) -> String {
        let date = CivilDate::from_ms(ms, self.tz);
        let Some(before) = self.last.replace(date).filter(|before| *before != date) else {
            return time_label(ms, self.tz, self.format);
        };
        let ((year, month, day), (before_year, before_month, _)) = (date.ymd(), before.ymd());
        if year != before_year {
            format!("{year:04}")
        } else if month != before_month {
            month_abbr(month).to_owned()
        } else {
            day.to_string()
        }
    }
}

/// Each turn's x on screen, left to right: the left edge of the first bar of
/// the day, between the two days' bars rather than on either. Only the turns
/// inside `left..=right` are kept.
#[must_use]
pub fn turn_xs<'t>(
    turns: &'t [DayTurn],
    viewport: &Viewport,
    (left, right): (f32, f32),
    total: usize,
    candle_width: f32,
) -> Vec<(f32, &'t DayTurn)> {
    turns
        .iter()
        .map(|turn| {
            let x = viewport.x_center(turn.slot, right, total) - candle_width / 2.0;
            (x, turn)
        })
        .filter(|&(x, _)| x >= left && x <= right)
        .collect()
}

#[cfg(test)]
#[path = "day_turn_tests.rs"]
mod day_turn_tests;
