//! The forming bar's progress as the last-price tag prints it.
//!
//! One reading per closing rule, straight from the builder that owns the rule:
//! a count rule reads `done/target`, a time rule the time left until its own
//! bucket ends on the market clock the host brings, an adaptive imbalance rule
//! the signed accumulation it has run up. The chart never re-derives where a
//! bar closes, and nothing here reads a clock.
use quantick_engine::display::display_decimal;
use rust_decimal::{Decimal, RoundingStrategy};

use quantick_engine::bar_registry::BarConfiguration;
use quantick_engine::{BarBuilder, BarProgress};

/// The unit a time reading reports in, on the wire.
pub const TIME_UNIT: &str = "ms";

/// A target from which a count is printed in thousands, millions or billions:
/// below it the figures stay whole, so a 2000-tick bar reads `522/2000`. Low
/// enough that a five-figure target fits the price gutter (`12.3K/20K`).
const COMPACT_FROM: i64 = 10_000;

/// The longest a countdown waits for its next frame, in wall milliseconds.
/// Its second is market time, which a replay above 1x runs faster than the
/// wall: waiting the full market second would leave the text seconds behind.
const REPAINT_AT_MOST_MS: i64 = 250;

const MS_PER_SECOND: i64 = 1_000;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3_600;

/// How far the forming bar is from closing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarProgressReading {
    /// A rule counting toward a fixed threshold: tick, volume, dollar, deals,
    /// Renko.
    Count {
        done: Decimal,
        target: Decimal,
        unit: &'static str,
    },
    /// A time rule: the bucket's length and what the market clock leaves of it.
    Remaining { length_ms: i64, remaining_ms: i64 },
    /// An adaptive imbalance rule: no fixed target, only the running total.
    Imbalance {
        running: Decimal,
        unit: &'static str,
    },
}

impl BarProgressReading {
    /// The second line of the last-price tag.
    #[must_use]
    pub fn tag_text(&self) -> String {
        match *self {
            Self::Count { done, target, .. } => count_text(done, target),
            Self::Remaining { remaining_ms, .. } => countdown_text(remaining_ms),
            Self::Imbalance { running, .. } => imbalance_text(running),
        }
    }

    /// What has accumulated: the count, the elapsed milliseconds, or the
    /// running imbalance.
    #[must_use]
    pub fn done(&self) -> Decimal {
        match *self {
            Self::Count { done, .. } => done,
            Self::Remaining {
                length_ms,
                remaining_ms,
            } => Decimal::from(length_ms - remaining_ms),
            Self::Imbalance { running, .. } => running,
        }
    }

    /// Where the bar closes; `None` for a rule without a fixed target.
    #[must_use]
    pub fn target(&self) -> Option<Decimal> {
        match *self {
            Self::Count { target, .. } => Some(target),
            Self::Remaining { length_ms, .. } => Some(Decimal::from(length_ms)),
            Self::Imbalance { .. } => None,
        }
    }

    /// The reading as a fixed-threshold progress, for a consumer that asks
    /// what share of the bar is formed: `None` for an imbalance rule, which
    /// has no target to take a share of.
    #[must_use]
    pub fn fixed_progress(self) -> Option<BarProgress> {
        Some(BarProgress {
            done: self.done(),
            target: self.target()?,
        })
    }

    /// How long until the tag should be painted again with no print: the
    /// next whole second of a countdown, at most [`REPAINT_AT_MOST_MS`] so a
    /// fast replay keeps up; `None` for a reading only a print moves.
    #[must_use]
    pub fn repaint_after_ms(&self) -> Option<i64> {
        match *self {
            Self::Remaining { remaining_ms, .. } if remaining_ms > 0 => {
                Some((remaining_ms % MS_PER_SECOND + 1).min(REPAINT_AT_MOST_MS))
            }
            _ => None,
        }
    }

    /// The unit [`done`](Self::done) and [`target`](Self::target) count in.
    #[must_use]
    pub fn unit(&self) -> &'static str {
        match *self {
            Self::Count { unit, .. } | Self::Imbalance { unit, .. } => unit,
            Self::Remaining { .. } => TIME_UNIT,
        }
    }
}

/// What the host tells a pane about its last-price tag's second line: the
/// market's setting and the market clock to read a time rule on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TagProgress {
    pub on: bool,
    pub clock_ms: Option<i64>,
}

impl TagProgress {
    /// The reading the tag prints, `read` at the host's clock; `None` while
    /// the market has the line off. Asked twice a frame: by the axis claim
    /// for the chip's second row, and by the chip's painter.
    #[must_use]
    pub fn reading(
        self,
        read: impl FnOnce(Option<i64>) -> Option<BarProgressReading>,
    ) -> Option<BarProgressReading> {
        if self.on { read(self.clock_ms) } else { None }
    }
}

/// The forming bar's progress with the market clock at `clock_ms`.
///
/// A time rule counts down from its own bucket end; with no clock it falls
/// back on the time of the trades seen, which freezes in a quiet stretch
/// rather than inventing time. A clock past the bucket end with no print to
/// open the next bar rolls over into the bucket the clock stands in, as
/// TradingView's bar clock does, instead of resting on `00:00`. `None` while no bar is forming on a time rule,
/// and for a rule that reports nothing.
#[must_use]
pub fn read(
    builder: &dyn BarBuilder,
    spec: &BarConfiguration,
    clock_ms: Option<i64>,
) -> Option<BarProgressReading> {
    if let Some(running) = builder.running_imbalance() {
        let unit = spec.choice().unwrap_or_else(|| spec.kind().progress_unit());
        return Some(BarProgressReading::Imbalance { running, unit });
    }
    let progress = builder.progress()?;
    if let Some(closes_at) = builder.closes_at_ms() {
        let length_ms = whole_ms(progress.target)?;
        let seen = whole_ms(progress.done)?;
        let remaining_ms = clock_ms
            .map_or(length_ms - seen, |now| left_at(now, closes_at, length_ms))
            .clamp(0, length_ms);
        return Some(BarProgressReading::Remaining {
            length_ms,
            remaining_ms,
        });
    }
    Some(BarProgressReading::Count {
        done: progress.done,
        target: progress.target,
        unit: spec.kind().progress_unit(),
    })
}

/// What is left of the bucket the clock stands in: of the forming one before
/// `closes_at`, of a later one the same length after it.
fn left_at(now: i64, closes_at: i64, length_ms: i64) -> i64 {
    if now < closes_at || length_ms <= 0 {
        return closes_at.saturating_sub(now);
    }
    length_ms - now.saturating_sub(closes_at) % length_ms
}

fn whole_ms(value: Decimal) -> Option<i64> {
    i64::try_from(value.trunc()).ok()
}

/// `522/2000`, `312/500`, `1.2M/2M`: both figures in the target's unit, the
/// count at one place and the target, usually round, without a dead `.0`.
#[must_use]
pub fn count_text(done: Decimal, target: Decimal) -> String {
    match compact_scale(target.abs()) {
        Some((divisor, suffix)) => format!(
            "{}{suffix}/{}{suffix}",
            one_place(done / divisor),
            display_decimal(cut(target / divisor, 1))
        ),
        None => format!(
            "{}/{}",
            display_decimal(cut(done, 2)),
            display_decimal(target)
        ),
    }
}

/// `00:52` below an hour, `1:04:12` above, as TradingView prints a bar clock.
#[must_use]
pub fn countdown_text(remaining_ms: i64) -> String {
    let seconds = remaining_ms.max(0) / MS_PER_SECOND;
    let (hours, rest) = (seconds / SECONDS_PER_HOUR, seconds % SECONDS_PER_HOUR);
    let (minutes, seconds) = (rest / SECONDS_PER_MINUTE, rest % SECONDS_PER_MINUTE);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// `+340`, `-120`, `0`; compact from ten thousand like a count. The sign is
/// the printed figure's: a total cut to zero prints `0`, never `+0`.
#[must_use]
pub fn imbalance_text(running: Decimal) -> String {
    let (figure, suffix) = match compact_scale(running.abs()) {
        Some((divisor, suffix)) => (cut(running / divisor, 1), suffix),
        None => (cut(running, 2), ""),
    };
    if figure.is_zero() {
        return "0".to_owned();
    }
    let sign = if figure > Decimal::ZERO { "+" } else { "" };
    let figure = if suffix.is_empty() {
        display_decimal(figure)
    } else {
        format!("{figure:.1}")
    };
    format!("{sign}{figure}{suffix}")
}

fn compact_scale(magnitude: Decimal) -> Option<(Decimal, &'static str)> {
    const SCALES: [(i64, &str); 3] = [(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "K")];
    if magnitude < Decimal::from(COMPACT_FROM) {
        return None;
    }
    SCALES
        .iter()
        .find(|(divisor, _)| magnitude >= Decimal::from(*divisor))
        .map(|&(divisor, suffix)| (Decimal::from(divisor), suffix))
}

/// One decimal place, cut toward zero: a bar at 1.99M has not reached 2.0M.
fn one_place(value: Decimal) -> String {
    format!("{:.1}", cut(value, 1))
}

/// `value` cut toward zero at `places`: a figure never claims more than came.
fn cut(value: Decimal, places: u32) -> Decimal {
    value.round_dp_with_strategy(places, RoundingStrategy::ToZero)
}

#[cfg(test)]
#[path = "bar_progress_tests.rs"]
mod bar_progress_tests;
