//! How one candle-history request is cut into the slices a provider answers
//! with — the newest part of the span first, so a chart fills in from the
//! right instead of staying empty until the whole quarter has landed.
//!
//! This is the whole progressive-loading policy, and it is a pure function of
//! three numbers. Nothing here reads a clock, opens a socket or knows which
//! venue is being asked: `now_ms` is handed in, and what comes back is a list
//! of windows for the caller to fetch in order. That is what lets the same
//! plan drive Binance's REST paging and Hyperliquid's websocket paging without
//! either of them growing a copy of the rule, and what lets the rule itself be
//! tested against fixed numbers.
//!
//! # Why the newest window is fetched first
//!
//! A span of one-minute candles is a run of sequential venue pages — a week
//! is roughly eleven on Binance, and the reach a trader pages back to over a
//! session is that many times over. Fetched as one job the chart shows no
//! venue prefix at all for the whole run and then gains the lot in a single
//! frame. Fetched newest-window-first the trader sees the most recent part
//! within a second or two, reads it while the rest arrives behind, and every
//! later slice lands *left* of what they are
//! already looking at — which is the one direction new bars can appear without
//! moving anything under the cursor (see `ChartPane::install_history_prefix`,
//! which shifts the viewport and every bar-anchored drawing to match).
//!
//! # Why the slice count is capped
//!
//! Each arriving slice costs a refold of the whole accumulated base and an
//! indicator rebuild per pane. That is fine a dozen times and not fine a
//! thousand, so [`MAX_SLICES`] bounds it: a span that would cut into more
//! pieces than that gets proportionally wider slices instead of more of them.
//! The cap is on the *count*, never on the coverage — every plan covers the
//! full span it was asked for, whatever the arithmetic.

/// One window of the span, to fetch as a unit.
///
/// Half-open in spirit, closed in the numbers: `to_ms` is the last millisecond
/// this window owns, so the next (older) window ends at `from_ms - 1` and no
/// candle bucket can be claimed by two windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OhlcvWindow {
    /// Oldest millisecond this window covers.
    pub from_ms: i64,
    /// Newest millisecond this window covers, inclusive.
    pub to_ms: i64,
    /// Whether this is the oldest window in the plan — the one whose reply
    /// ends the request.
    pub last: bool,
}

/// The most slices one request is ever cut into. See the module docs: this
/// bounds the per-slice refold work, not the span covered.
pub const MAX_SLICES: usize = 16;

/// `ceil(a / b)` for the strictly positive values this module deals in, and
/// never zero: a width or count of zero would be a loop that never advances.
///
/// Hand-rolled because `i64::div_ceil` is still unstable, and the alternative
/// — floating point — would make the plan depend on rounding.
///
/// Saturating rather than wrapping: `a + b - 1` overflows for a span near
/// `i64::MAX`, and this is a public entry point's arithmetic. A saturated sum
/// yields one enormous window, which is the same answer the caller would get
/// from a span that large anyway — where a panic would be a crash on input
/// the type permits.
fn div_ceil(a: i64, b: i64) -> i64 {
    debug_assert!(a > 0 && b > 0, "callers guard the degenerate cases");
    (a.saturating_add(b - 1) / b).max(1)
}

/// Cut `span_ms` ending at `now_ms` into windows, newest first.
///
/// `slice_ms` is the requested slice width: `None` — or a width that is not
/// smaller than the span — yields exactly one window covering everything,
/// which is precisely the single all-at-once reply every provider made before
/// progressive loading existed. A width at or below zero is meaningless and
/// read as `None` rather than refused, because the alternative is a provider
/// with no plan to fetch at all.
///
/// The result is never empty: a request always deserves an answer, and a
/// degenerate span (zero, negative) still produces the one window whose reply
/// tells the UI that the wait is over.
#[must_use]
pub fn plan(now_ms: i64, span_ms: i64, slice_ms: Option<i64>) -> Vec<OhlcvWindow> {
    let span = span_ms.max(0);
    let from_ms = now_ms.saturating_sub(span);
    let whole = OhlcvWindow {
        from_ms,
        to_ms: now_ms,
        last: true,
    };

    let Some(requested) = slice_ms.filter(|width| *width > 0) else {
        return vec![whole];
    };
    if requested >= span || span == 0 {
        return vec![whole];
    }

    // Widen rather than multiply when the arithmetic asks for more slices than
    // the cap allows. Rounding *up* on both counts keeps the last (oldest)
    // window from being a sliver: the remainder is spread across the windows
    // instead of left over, and a plan that ends in a one-minute window would
    // spend a whole round trip on it.
    let wanted = div_ceil(span, requested);
    let count = wanted.min(MAX_SLICES as i64);
    let width = div_ceil(span, count);

    let mut windows = Vec::new();
    let mut to = now_ms;
    while to > from_ms {
        let next = to.saturating_sub(width).max(from_ms);
        windows.push(OhlcvWindow {
            from_ms: next,
            // Each older window stops one millisecond short of the newer one's
            // first, so the two never claim the same candle.
            to_ms: to,
            last: false,
        });
        if next == from_ms {
            break;
        }
        to = next.saturating_sub(1);
    }
    if windows.is_empty() {
        return vec![whole];
    }
    if let Some(oldest) = windows.last_mut() {
        oldest.last = true;
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000;
    const DAY: i64 = 24 * 60 * MINUTE;

    #[test]
    fn no_slice_width_asks_for_the_whole_span_at_once() {
        let plan = plan(1_000_000, 90 * DAY, None);
        assert_eq!(
            plan,
            vec![OhlcvWindow {
                from_ms: 1_000_000 - 90 * DAY,
                to_ms: 1_000_000,
                last: true,
            }]
        );
    }

    #[test]
    fn a_slice_at_least_as_wide_as_the_span_is_one_window() {
        let one = plan(0, 7 * DAY, Some(7 * DAY));
        assert_eq!(one.len(), 1);
        assert!(one[0].last);
        let wider = plan(0, 7 * DAY, Some(30 * DAY));
        assert_eq!(wider, one);
    }

    #[test]
    fn a_nonsense_slice_width_falls_back_to_one_window() {
        for width in [0, -1, i64::MIN] {
            let plan = plan(0, 90 * DAY, Some(width));
            assert_eq!(plan.len(), 1, "width {width} should not slice");
            assert!(plan[0].last);
        }
    }

    #[test]
    fn slices_run_newest_first_and_only_the_oldest_is_last() {
        let now = 100 * DAY;
        let plan = plan(now, 90 * DAY, Some(7 * DAY));
        assert!(plan.len() > 1);
        assert_eq!(plan[0].to_ms, now, "the first window ends at now");
        for pair in plan.windows(2) {
            assert!(
                pair[0].from_ms > pair[1].to_ms,
                "windows must walk backwards in time"
            );
        }
        assert!(!plan[0].last);
        assert!(plan.last().expect("non-empty").last);
        assert_eq!(
            plan.iter().filter(|window| window.last).count(),
            1,
            "exactly one reply ends the request"
        );
    }
    /// Reaching further back is the same plan with a different right-hand
    /// edge — which is the whole reason `FetchOhlcv::before_ms` needed no new
    /// planning code. A *load older* passes the millisecond before the oldest
    /// bucket it holds, so the two runs must meet exactly: the newest window
    /// of the older plan ends there, and nothing it covers was already drawn.
    #[test]
    fn a_plan_anchored_before_held_history_meets_it_without_overlapping() {
        let held_oldest = 1_000_000_000_i64;
        let span = 7 * 24 * 60 * 60 * 1_000;
        let older = plan(held_oldest - 1, span, Some(24 * 60 * 60 * 1_000));

        assert_eq!(
            older.first().expect("a plan is never empty").to_ms,
            held_oldest - 1,
            "the newest window stops one millisecond short of what is held"
        );
        assert_eq!(
            older.last().expect("a plan is never empty").from_ms,
            held_oldest - 1 - span,
            "and the oldest reaches a full span further back"
        );
        assert!(
            older.iter().all(|window| window.to_ms < held_oldest),
            "no window claims a bucket already on screen"
        );
    }

    #[test]
    fn the_windows_cover_the_whole_span_without_overlapping() {
        let now = 100 * DAY;
        let span = 90 * DAY;
        let plan = plan(now, span, Some(7 * DAY));
        assert_eq!(plan[0].to_ms, now);
        assert_eq!(plan.last().expect("non-empty").from_ms, now - span);
        for pair in plan.windows(2) {
            // Newer window's first millisecond sits immediately after the
            // older window's last: contiguous, and no shared millisecond.
            assert_eq!(pair[0].from_ms, pair[1].to_ms + 1);
        }
        for window in &plan {
            assert!(window.from_ms <= window.to_ms);
        }
    }

    #[test]
    fn the_slice_count_is_capped_and_the_span_still_covered() {
        let now = 1_000 * DAY;
        let span = 90 * DAY;
        // One minute per slice would ask for 129 600 replies.
        let plan = plan(now, span, Some(MINUTE));
        assert!(
            plan.len() <= MAX_SLICES,
            "{} slices exceeds the cap",
            plan.len()
        );
        assert_eq!(plan[0].to_ms, now);
        assert_eq!(plan.last().expect("non-empty").from_ms, now - span);
        for pair in plan.windows(2) {
            assert_eq!(pair[0].from_ms, pair[1].to_ms + 1);
        }
    }

    /// The cap is what bounds the per-slice refold work, so it holds for
    /// every input the type permits — including the ones that overflow a
    /// naive `ceil`, which must saturate rather than panic.
    #[test]
    fn no_input_the_type_permits_exceeds_the_cap_or_panics() {
        let cases = [
            (0_i64, 90 * DAY, 1_i64),
            (i64::MAX, i64::MAX, 1),
            (i64::MAX, i64::MAX, MINUTE),
            (i64::MIN, i64::MAX, DAY),
            (0, i64::MAX, i64::MAX - 1),
            (0, 90 * DAY, MINUTE),
        ];
        for (now, span, slice) in cases {
            let plan = plan(now, span, Some(slice));
            assert!(
                !plan.is_empty() && plan.len() <= MAX_SLICES,
                "now={now} span={span} slice={slice} gave {} windows",
                plan.len()
            );
            assert_eq!(
                plan.iter().filter(|window| window.last).count(),
                1,
                "exactly one reply must close the request"
            );
            for window in &plan {
                assert!(window.from_ms <= window.to_ms);
            }
        }
    }

    #[test]
    fn a_degenerate_span_still_produces_one_answerable_window() {
        for span in [0, -1, i64::MIN] {
            let plan = plan(1_000, span, Some(DAY));
            assert_eq!(plan.len(), 1, "span {span} should answer once");
            assert!(plan[0].last);
            assert_eq!(plan[0].to_ms, 1_000);
        }
    }

    #[test]
    fn the_oldest_window_absorbs_the_remainder_instead_of_being_a_sliver() {
        // 10 days in 3-day slices: 4 windows, and the last must not be the
        // one-day leftover a naive walk would leave.
        let plan = plan(100 * DAY, 10 * DAY, Some(3 * DAY));
        let widths: Vec<i64> = plan
            .iter()
            .map(|window| window.to_ms - window.from_ms)
            .collect();
        let smallest = widths.iter().copied().min().expect("non-empty");
        let largest = widths.iter().copied().max().expect("non-empty");
        assert!(smallest * 2 >= largest, "windows are lopsided: {widths:?}");
    }
}
