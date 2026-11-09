//! Time-axis labels the way TradingView writes them: round instants a steady
//! distance apart, each written as little as it takes.
//!
//! A step is chosen from a ladder of round durations — the smallest whose
//! labels stand at least [`TIME_MARK_SPACING_PX`] apart at the chart's zoom —
//! and a bar is labelled where its open crosses into a new step. What it
//! crossed decides the text: a new year writes the year, a new month its
//! name, a new day the day number alone, and anything else the clock as
//! `HH:MM` (`HH:MM:SS` only when the step itself is under a minute). Labels
//! that would touch give way to the more important one, so a day change is
//! never hidden by the hour beside it.
//!
//! Pure: bar positions and open times in, labels out. Bars cut by ticks,
//! volume or dollars open at irregular instants and work the same way; the
//! step comes from the average pace across the view.

use quantick_civil::{CivilDate, DAY_MS, TzOffset, month_abbr};

/// Least distance between two neighbouring time labels' centres, in pixels.
/// TradingView's light chart, sampled at 15-minute bars, steps three hours at
/// 117 px and refuses two hours at 78 px: its floor sits between the two.
pub const TIME_MARK_SPACING_PX: f32 = 90.0;

/// Least air between two time labels' text, in pixels.
pub const TIME_MARK_GAP_PX: f32 = 12.0;

const SECOND_MS: i64 = 1_000;
const MINUTE_MS: i64 = 60 * SECOND_MS;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// A round duration labels may step by, smallest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeStep {
    /// A whole divisor of a day, in milliseconds, aligned on local midnight.
    Clock(i64),
    /// Whole days.
    Days(i64),
    /// Whole months.
    Months(i64),
    /// Whole years.
    Years(i64),
}

impl TimeStep {
    /// The ladder, smallest first. Every clock step divides a day, so its
    /// boundaries fall on the same instants every day.
    pub const LADDER: [Self; 25] = [
        Self::Clock(SECOND_MS),
        Self::Clock(5 * SECOND_MS),
        Self::Clock(15 * SECOND_MS),
        Self::Clock(30 * SECOND_MS),
        Self::Clock(MINUTE_MS),
        Self::Clock(2 * MINUTE_MS),
        Self::Clock(5 * MINUTE_MS),
        Self::Clock(10 * MINUTE_MS),
        Self::Clock(15 * MINUTE_MS),
        Self::Clock(30 * MINUTE_MS),
        Self::Clock(HOUR_MS),
        Self::Clock(2 * HOUR_MS),
        Self::Clock(3 * HOUR_MS),
        Self::Clock(4 * HOUR_MS),
        Self::Clock(6 * HOUR_MS),
        Self::Clock(12 * HOUR_MS),
        Self::Days(1),
        Self::Days(2),
        Self::Days(7),
        Self::Days(14),
        Self::Months(1),
        Self::Months(3),
        Self::Months(6),
        Self::Years(1),
        Self::Years(5),
    ];

    /// Roughly how long one step lasts, to compare against the view's pace.
    #[must_use]
    pub fn approx_ms(self) -> f64 {
        match self {
            Self::Clock(ms) => ms as f64,
            Self::Days(days) => (days * DAY_MS) as f64,
            Self::Months(months) => months as f64 * 30.44 * DAY_MS as f64,
            Self::Years(years) => years as f64 * 365.25 * DAY_MS as f64,
        }
    }

    /// The smallest step lasting at least `least_ms`, or the largest one.
    #[must_use]
    pub fn at_least(least_ms: f64) -> Self {
        Self::LADDER
            .into_iter()
            .find(|step| step.approx_ms() >= least_ms)
            .unwrap_or(Self::Years(5))
    }

    /// Which step an instant falls in, in the display zone.
    fn bucket(self, ms: i64, tz: TzOffset) -> i64 {
        let date = CivilDate::from_ms(ms, tz);
        let (year, month, _) = date.ymd();
        match self {
            Self::Clock(step) => ms.saturating_add(tz.offset_ms()).div_euclid(step),
            Self::Days(days) => date.day_number().div_euclid(days),
            Self::Months(months) => (year * 12 + month - 1).div_euclid(months),
            Self::Years(years) => year.div_euclid(years),
        }
    }
}

/// How much a label says: a later variant always wins a collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkWeight {
    Clock,
    Day,
    Month,
    Year,
}

/// One label on the time axis.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeMark {
    /// The bar slot it labels.
    pub slot: usize,
    /// Its centre on screen.
    pub x: f32,
    pub text: String,
    pub weight: MarkWeight,
}

/// One visible bar: its slot, its centre on screen and when it opened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarAt {
    pub slot: usize,
    pub x: f32,
    pub open_ms: i64,
}

/// The labels for `bars` (left to right). `seed_ms` is when the bar before
/// the first one opened, so a boundary the first bar crosses is labelled;
/// `width_of` measures a label's text, and labels closer than their half
/// widths plus `gap_px` give way to the weightier one.
#[must_use]
pub fn time_marks(
    bars: &[BarAt],
    seed_ms: Option<i64>,
    tz: TzOffset,
    gap_px: f32,
    width_of: impl Fn(&str) -> f32,
) -> Vec<TimeMark> {
    let Some(step) = view_step(bars) else {
        return Vec::new();
    };
    let mut marks: Vec<(TimeMark, f32)> = Vec::new();
    let mut previous = seed_ms;
    for bar in bars {
        let before = previous.replace(bar.open_ms);
        let Some(before) = before else { continue };
        let (text, weight) = text_at(before, bar.open_ms, tz, step);
        // A step boundary, or a month or a year turning between two of a
        // multi-day step's boundaries: TradingView names those too.
        let crossed = step.bucket(before, tz) != step.bucket(bar.open_ms, tz);
        if !crossed && weight < MarkWeight::Month {
            continue;
        }
        let width = width_of(&text);
        let mark = TimeMark {
            slot: bar.slot,
            x: bar.x,
            text,
            weight,
        };
        // Labels too close to touch: the weightier stays. A winner is checked
        // again against the one before the label it displaced.
        let mut keep = true;
        while let Some((last, last_width)) = marks.last() {
            if mark.x - last.x >= (last_width + width) / 2.0 + gap_px {
                break;
            }
            if mark.weight > last.weight {
                marks.pop();
            } else {
                keep = false;
                break;
            }
        }
        if keep {
            marks.push((mark, width));
        }
    }
    marks.into_iter().map(|(mark, _)| mark).collect()
}

/// The step for this view: its pace in time per pixel times the spacing.
fn view_step(bars: &[BarAt]) -> Option<TimeStep> {
    let (first, last) = (bars.first()?, bars.last()?);
    let span_px = last.x - first.x;
    let span_ms = last.open_ms.saturating_sub(first.open_ms);
    if !(span_px.is_finite() && span_px > 0.0) || span_ms <= 0 {
        return None;
    }
    let ms_per_px = span_ms as f64 / f64::from(span_px);
    Some(TimeStep::at_least(
        ms_per_px * f64::from(TIME_MARK_SPACING_PX),
    ))
}

/// What a label crossing from `before` into `at` writes.
fn text_at(before: i64, at: i64, tz: TzOffset, step: TimeStep) -> (String, MarkWeight) {
    let (old, new) = (CivilDate::from_ms(before, tz), CivilDate::from_ms(at, tz));
    let ((year, month, day), (old_year, old_month, _)) = (new.ymd(), old.ymd());
    if year != old_year {
        (format!("{year:04}"), MarkWeight::Year)
    } else if month != old_month {
        (month_abbr(month).to_owned(), MarkWeight::Month)
    } else if new != old {
        (day.to_string(), MarkWeight::Day)
    } else {
        let local = at.saturating_add(tz.offset_ms());
        let secs = local.div_euclid(SECOND_MS).rem_euclid(DAY_MS / SECOND_MS);
        let (hours, minutes, seconds) = (secs / 3600, (secs % 3600) / 60, secs % 60);
        let text = match step {
            TimeStep::Clock(ms) if ms < MINUTE_MS => {
                format!("{hours:02}:{minutes:02}:{seconds:02}")
            }
            _ => format!("{hours:02}:{minutes:02}"),
        };
        (text, MarkWeight::Clock)
    }
}

/// The sample a calendar chart's time labels are measured by, or `None`
/// below a day, where the clock time is the label.
///
/// A daily, weekly or monthly bar opens at 00:00 UTC, so a clock time says
/// nothing about it: every label would read the same. The date does.
#[must_use]
pub fn calendar_label_sample(interval_ms: i64) -> Option<&'static str> {
    if interval_ms < quantick_engine::time_bucket::DAY_MS {
        return None;
    }
    Some(
        match quantick_engine::time_bucket::calendar_months(interval_ms) {
            Some(1) => "Mmm 0000",
            Some(_) => "0000",
            None => "00 Mmm",
        },
    )
}

/// The time label of a bar opening at `open_ms` on a calendar chart: the
/// UTC bucket it was cut in — a day or week as `07 Oct`, written as its year
/// where the year changes from the bar labelled before it (`previous_ms`), a
/// month as `Oct 2026`, and several months as the year. In UTC rather than
/// the display zone because the buckets are UTC days: the label names the
/// bucket, and a zone west of UTC would otherwise name the day before it.
/// `None` below a day.
#[must_use]
pub fn fmt_calendar_label(
    open_ms: i64,
    interval_ms: i64,
    previous_ms: Option<i64>,
) -> Option<String> {
    calendar_label_sample(interval_ms)?;
    let date = |ms: i64| {
        quantick_civil::CivilDate::from_ms(
            quantick_engine::time_bucket::time_bucket_start(ms, interval_ms),
            TzOffset::new(0),
        )
    };
    let day = date(open_ms);
    let (year, month, _) = day.ymd();
    Some(
        match quantick_engine::time_bucket::calendar_months(interval_ms) {
            Some(1) => format!("{} {year:04}", quantick_civil::month_abbr(month)),
            Some(_) => format!("{year:04}"),
            None if previous_ms.is_some_and(|previous| date(previous).ymd().0 != year) => {
                format!("{year:04}")
            }
            // `07 Oct`: the day, then the month's abbreviation.
            None => day.short(),
        },
    )
}

/// A calendar chart's labels: every `stride`-th of `bars`, written as the
/// UTC bucket each opened in ([`fmt_calendar_label`]). `seed_ms` is the bar
/// before the first, so a year turning on the first label is written.
#[must_use]
pub fn calendar_marks(
    bars: &[BarAt],
    interval_ms: i64,
    seed_ms: Option<i64>,
    stride: usize,
) -> Vec<TimeMark> {
    let mut previous = seed_ms;
    bars.iter()
        .step_by(stride.max(1))
        .filter_map(|bar| {
            let before = previous.replace(bar.open_ms);
            Some(TimeMark {
                slot: bar.slot,
                x: bar.x,
                text: fmt_calendar_label(bar.open_ms, interval_ms, before)?,
                weight: MarkWeight::Day,
            })
        })
        .collect()
}

/// The time strip's labels for `bars` on a strip spanning `left..right`,
/// each `width_of` its text wide: a chart cut at a day or longer
/// (`interval_ms`) writes its UTC buckets' dates every few bars, as many as
/// `px_per_bar` leaves room for; an intraday chart writes TradingView's
/// round instants ([`time_marks`]). Only labels whose whole text fits are
/// kept: one centred a few pixels from the end drew its other half over the
/// gutter.
#[must_use]
pub fn strip_marks(
    bars: &[BarAt],
    seed_ms: Option<i64>,
    interval_ms: Option<i64>,
    tz: TzOffset,
    px_per_bar: f32,
    (left, right): (f32, f32),
    width_of: impl Fn(&str) -> f32,
) -> Vec<TimeMark> {
    let marks = match interval_ms.zip(interval_ms.and_then(calendar_label_sample)) {
        Some((interval, sample)) => {
            let stride = crate::geometry::time_label_stride(px_per_bar, width_of(sample));
            calendar_marks(bars, interval, seed_ms, stride)
        }
        None => time_marks(bars, seed_ms, tz, TIME_MARK_GAP_PX, &width_of),
    };
    marks
        .into_iter()
        .filter(|mark| crate::geometry::label_fits(mark.x, width_of(&mark.text), left, right))
        .collect()
}

#[cfg(test)]
#[path = "time_marks_tests.rs"]
mod tests;
