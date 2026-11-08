//! Caller-stamped bar time; owned by child 2 after skeleton integration.
use quantick_indicators::IndicatorBar;
/// Open timestamp in epoch milliseconds, without consulting a clock.
pub(crate) fn open_time(bar: &IndicatorBar) -> f64 {
    bar.open_time as f64
}
/// Close timestamp in epoch milliseconds, without inferring an interval.
pub(crate) fn close_time(bar: &IndicatorBar) -> f64 {
    bar.close_time as f64
}
