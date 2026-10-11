//! Where the display day turns over, as the time strip writes it: a tick at
//! the boundary with the time the old day's last bar closed left of it, and
//! the date the new day opens with its first bar's open time right of it
//! (`18:24 | Tue 29 09:00`), plus the date of the leftmost bar pinned to the
//! strip's left edge.
//!
//! Text and placement only: the window measures one monospace character and
//! paints what [`plan_strip`] places. Every label is monospace ASCII, so
//! [`label_width`] bounds its width from its length without laying it out,
//! and no label is laid out that is not drawn.

use quantick_civil::{CivilDate, TzOffset, month_abbr, weekday_abbr};

use crate::geometry::{TimeLabelFormat, time_label};
use crate::viewport::Viewport;

/// Space between a tick and the labels either side of it, and between two
/// labels.
pub const DAY_LABEL_GAP_PX: f32 = 3.0;

/// A slot whose bar opens a new display day, and when the two days ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayTurn {
    pub slot: usize,
    /// The date the new day opens, from `quantick_civil::day_starts`.
    pub date: CivilDate,
    /// The last print of the bar before the turn: when the old day ended.
    pub ended_ms: Option<i64>,
    /// When the first bar of the new day opened.
    pub opened_ms: Option<i64>,
}

impl DayTurn {
    /// The turn at `slot`, given the bar before it as `(open_ms, close_ms)`
    /// and the open of the bar that starts the new day. The bar before is
    /// the old day's end only when it closed on the day it opened. A bar
    /// spanning the day change holds both days' prints: its close is in the
    /// new day, and the new day's first prints are in it rather than in the
    /// bar at `slot`, so neither time is known and the date stands alone.
    #[must_use]
    pub fn new(
        slot: usize,
        date: CivilDate,
        before: Option<(i64, i64)>,
        opened_ms: Option<i64>,
        tz: TzOffset,
    ) -> Self {
        let spans_the_turn = before.is_some_and(|(open, close)| {
            CivilDate::from_ms(open, tz) != CivilDate::from_ms(close, tz)
        });
        if spans_the_turn {
            return Self {
                slot,
                date,
                ended_ms: None,
                opened_ms: None,
            };
        }
        Self {
            slot,
            date,
            ended_ms: before.map(|(_, close)| close),
            opened_ms,
        }
    }
}

/// `Tue 29`; the month joins it on the first of the month, where the month
/// is what changed.
#[must_use]
pub fn day_text(date: CivilDate) -> String {
    let (_, _, day) = date.ymd();
    if day == 1 {
        format!("{} {}", weekday_abbr(date.weekday()), date.short())
    } else {
        format!("{} {day}", weekday_abbr(date.weekday()))
    }
}

/// `Mon 28 Sep`: the leftmost bar's date, pinned to the strip's left edge.
#[must_use]
pub fn pinned_text(date: CivilDate) -> String {
    format!("{} {}", weekday_abbr(date.weekday()), date.short())
}

/// `18:24`: when the old day's last bar closed, left of the tick.
#[must_use]
pub fn ended_text(ms: i64, tz: TzOffset) -> String {
    time_label(ms, tz, TimeLabelFormat::Short)
}

/// ` 09:00`: when the new day's first bar opened, as it follows the date.
#[must_use]
pub fn opened_suffix(ms: i64, tz: TzOffset) -> String {
    format!(" {}", time_label(ms, tz, TimeLabelFormat::Short))
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

/// A date the day separator wrote on the strip, and whether its text named
/// the month: the pinned date always does, a turn's only on the 1st. Neither
/// names the year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrittenDate {
    pub date: CivilDate,
    pub month: bool,
}

impl WrittenDate {
    /// The leftmost bar's date, as [`pinned_text`] writes it.
    #[must_use]
    pub fn pinned(date: CivilDate) -> Self {
        Self { date, month: true }
    }

    /// A turn's date, as [`day_text`] writes it.
    #[must_use]
    pub fn turn(date: CivilDate) -> Self {
        let (_, _, day) = date.ymd();
        Self {
            date,
            month: day == 1,
        }
    }
}

/// Writes the time strip's labels left to right, as TradingView does: the
/// first label *painted* in a new local day writes the day (`9`), in a new
/// month the month (`Oct`), in a new year the year (`2026`); every other
/// label is the clock in its format.
///
/// Fed only the labels that are painted, so a label skipped for room, for a
/// chip or for a missing open time never moves the comparison. A turn the
/// day separator already wrote keeps its label a clock, but only when its
/// text carried what changed: any of its dates covers a day, only one that
/// named the month covers a month, and none covers a year.
///
/// A turn's name is never wider than the format's
/// [`sample`](TimeLabelFormat::sample) — four characters at most against the
/// clock's five — so the stride measured from the sample still holds.
#[derive(Debug, Clone)]
pub struct TickLabels<'a> {
    tz: TzOffset,
    format: TimeLabelFormat,
    /// The date of the last label painted, or the seed before the first.
    last: Option<CivilDate>,
    /// Dates the day separator wrote on the strip this frame.
    named: &'a [WrittenDate],
}

impl<'a> TickLabels<'a> {
    /// `seed_ms` is an instant before the first label (the bar before the
    /// first visible one), so a day that turns before the first label is
    /// still named on it; `None` writes the first label as a clock.
    #[must_use]
    pub fn new(
        tz: TzOffset,
        format: TimeLabelFormat,
        seed_ms: Option<i64>,
        named: &'a [WrittenDate],
    ) -> Self {
        Self {
            tz,
            format,
            last: seed_ms.map(|ms| CivilDate::from_ms(ms, tz)),
            named,
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
        let written = self.named.iter().find(|written| written.date == date);
        if year != before_year {
            format!("{year:04}")
        } else if month != before_month && !written.is_some_and(|written| written.month) {
            month_abbr(month).to_owned()
        } else if written.is_some() {
            time_label(ms, self.tz, self.format)
        } else {
            day.to_string()
        }
    }
}

/// At least as wide as `chars` characters of a monospace font whose
/// characters measure `char_width`, as the window lays them out: the painter
/// rounds glyph positions and the galley's size, so each character is
/// rounded up to a whole point and a point is added for the galley's edge.
/// A label placed on this width never overruns its limit once drawn.
fn chars_width(chars: usize, char_width: f32) -> f32 {
    chars as f32 * char_width.ceil() + 1.0
}

/// [`chars_width`] of `text`: every label goes through this, the pinned date
/// included, so they are measured on one rule.
#[must_use]
pub fn label_width(text: &str, char_width: f32) -> f32 {
    chars_width(text.chars().count(), char_width)
}

/// How wide each label of one turn is; a time that is unknown is `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnWidths {
    /// The old day's end time, left of the tick.
    pub ended: Option<f32>,
    /// The date with the new day's start time, right of the tick.
    pub dated_opened: Option<f32>,
    /// The date alone.
    pub dated: f32,
}

impl TurnWidths {
    /// The widths `turn`'s labels take at most in a monospace font.
    #[must_use]
    pub fn of(turn: &DayTurn, char_width: f32) -> Self {
        let time = TimeLabelFormat::Short.sample().chars().count();
        let dated = day_text(turn.date).chars().count();
        Self {
            ended: turn.ended_ms.map(|_| chars_width(time, char_width)),
            // The date, a space and the time, as one galley.
            dated_opened: turn
                .opened_ms
                .map(|_| chars_width(dated + 1 + time, char_width)),
            dated: chars_width(dated, char_width),
        }
    }
}

/// Which of a turn's labels are written: the end time starts at `ended_at`,
/// the date always right of the tick, with its start time or without.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnPlacement {
    pub ended_at: Option<f32>,
    pub with_opened: bool,
}

/// Where a turn's labels go beside the tick at `x`, inside `floor..limit`
/// and where `free` allows. Short of space the end time is dropped first,
/// then the start time; the date is never dropped while it fits, and when it
/// does not, nothing is written.
#[must_use]
pub fn place_turn(
    x: f32,
    widths: TurnWidths,
    floor: f32,
    limit: f32,
    free: impl Fn(f32, f32) -> bool,
) -> Option<TurnPlacement> {
    let right_at = x + DAY_LABEL_GAP_PX;
    let right_fits = |width: Option<f32>| {
        width.is_some_and(|width| {
            right_at >= floor && right_at + width <= limit && free(right_at, width)
        })
    };
    let ended_at = widths
        .ended
        .map(|width| (x - DAY_LABEL_GAP_PX - width, width))
        .filter(|&(at, width)| at >= floor && free(at, width))
        .map(|(at, _)| at);
    if right_fits(widths.dated_opened) {
        return Some(TurnPlacement {
            ended_at,
            with_opened: true,
        });
    }
    right_fits(Some(widths.dated)).then_some(TurnPlacement {
        ended_at: None,
        with_opened: false,
    })
}

/// One day tick on the strip: whether it is drawn, and its labels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickPlan {
    /// False under the pointer's chip, which wins tick and labels alike.
    pub drawn: bool,
    pub labels: Option<TurnPlacement>,
}

/// Everything the strip writes at the day turns, left to right.
#[derive(Debug, Clone, PartialEq)]
pub struct StripPlan {
    /// Where the pinned date starts, when it is written.
    pub pinned_at: Option<f32>,
    /// One per tick, in the order given.
    pub ticks: Vec<TickPlan>,
}

/// Places the pinned date of width `pinned` and every tick's labels on a
/// strip spanning `left..right`. `ticks` are the ticks' x, left to right,
/// with their labels' widths; `free` says whether a label's span is clear of
/// the pointer's chip and `tick_free` whether a tick is. A label is never
/// crossed by the tick after it, and never overlaps a label before it.
#[must_use]
pub fn plan_strip(
    left: f32,
    right: f32,
    pinned: Option<f32>,
    ticks: &[(f32, TurnWidths)],
    free: impl Fn(f32, f32) -> bool,
    tick_free: impl Fn(f32) -> bool,
) -> StripPlan {
    // Where the next tick stands, or the strip's end.
    let limit_before = |index: usize| {
        ticks
            .get(index)
            .map_or(right, |&(x, _)| x - DAY_LABEL_GAP_PX)
    };
    let mut written_right = f32::NEG_INFINITY;
    // Pinned first, so a boundary label close to the left edge wins by
    // pushing it out rather than being pushed itself.
    let pinned_at = pinned.and_then(|width| {
        let start = left + DAY_LABEL_GAP_PX;
        let fits = start + width <= limit_before(0) && free(start, width);
        fits.then(|| {
            written_right = start + width;
            start
        })
    });
    let mut plans = Vec::with_capacity(ticks.len());
    let mut tick_before = f32::NEG_INFINITY;
    for (index, &(x, widths)) in ticks.iter().enumerate() {
        if !tick_free(x) {
            plans.push(TickPlan {
                drawn: false,
                labels: None,
            });
            continue;
        }
        // Clear of the last label and of the tick before, which a label
        // crosses when the turn before it wrote nothing.
        let floor = (written_right.max(tick_before) + DAY_LABEL_GAP_PX).max(left);
        tick_before = x;
        let labels = place_turn(x, widths, floor, limit_before(index + 1), &free);
        if let Some(placed) = labels {
            let width = match (placed.with_opened, widths.dated_opened) {
                (true, Some(width)) => width,
                _ => widths.dated,
            };
            written_right = x + DAY_LABEL_GAP_PX + width;
        }
        plans.push(TickPlan {
            drawn: true,
            labels,
        });
    }
    StripPlan {
        pinned_at,
        ticks: plans,
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

/// Whether a time label spanning `from..to` would touch a day label's span.
#[must_use]
pub fn reserved_by(from: f32, to: f32, reserved: &[(f32, f32)]) -> bool {
    reserved
        .iter()
        .any(|&(start, end)| from < end + DAY_LABEL_GAP_PX && to > start - DAY_LABEL_GAP_PX)
}

#[cfg(test)]
#[path = "day_turn_tests.rs"]
mod day_turn_tests;
