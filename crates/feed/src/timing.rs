//! Provider-neutral display of stamp timing, without reading any clock.
use crate::FeedLatency;

#[must_use]
pub fn tape_arrival_ms(arrival_ms: Option<i64>, latency: Option<FeedLatency>) -> Option<i64> {
    latency.map(|split| split.arrival_lag_ms).or(arrival_ms)
}
pub use quantick_sources::timing::{STAMP_OFFSET_TOOLTIP, stamp_offset_label};

/// A quiet tape reports age; unsplit stamp subtraction reports a signed clock
/// difference. Provider breakdowns may name their own delayed handover hop.
pub fn live_tape_text(
    arrival: Option<i64>,
    age: Option<i64>,
    latency: Option<FeedLatency>,
    stale_ms: i64,
    high_ms: i64,
) -> String {
    if let Some(age) = age.filter(|age| *age > stale_ms) {
        return format!("stale {}", crate::stall::spoken_ms(age));
    }
    let Some(arrival) = tape_arrival_ms(arrival, latency) else {
        return "arrival —".to_owned();
    };
    if !latency
        .is_some_and(|split| split.source_lag_ms.is_some() && split.transport_lag_ms.is_some())
        || arrival < 0
    {
        return stamp_offset_label(arrival);
    }
    match latency
        .filter(|_| arrival > high_ms)
        .and_then(|split| split.hop)
    {
        Some(hop) => format!("{hop} {arrival} ms"),
        None => format!("arrival {arrival} ms"),
    }
}

/// What the tape cell always says on hover, split or no split.
pub const TAPE_TOOLTIP_BASE: &str = "arrival: the provider's stamp timing breakdown; stale: how long the tape has been silent (a socket can \
     stay open and deliver nothing); replay shows speed and progress instead";

/// The hover text under the tape cell.
///
/// Always explains the two measurements the cell can show, because they are
/// easy to conflate. When the provider has cut its own chain the split follows
/// — the numbers behind the one word on the cell, so a trader who wants to know
/// *how much* of the delay is theirs to fix can read it without leaving the
/// chart.
///
/// Every figure says where it was taken. The feed measures at the socket and
/// the chart measures at the frame, so the two totals are not the same number
/// and must never be printed as though they were; their *difference* is what
/// quantick's own queueing and drawing cost, and that is the one hop neither
/// side can see alone.
#[must_use]
pub fn tape_tooltip(
    arrival_ms: Option<i64>,
    latency: Option<FeedLatency>,
    ms: impl Fn(i64) -> String,
) -> String {
    let mut text = format!("{}\n{}", TAPE_TOOLTIP_BASE, STAMP_OFFSET_TOOLTIP);
    let Some(split) = latency else {
        return text;
    };
    // Both halves or neither: they come from the same pair of stamps, and one
    // alone would invite the reader to infer the other by subtracting from a
    // total that has a different sample behind it.
    if let (Some(source), Some(transport)) = (split.source_lag_ms, split.transport_lag_ms) {
        text.push_str(&format!(
            "\n\nthe feed measured {} from the venue's stamp: {} before it was \
             handed over, {} from there to the socket",
            ms(split.arrival_lag_ms),
            ms(source),
            ms(transport),
        ));
        if let Some(hop) = split.hop {
            text.push_str(&format!(" — most of it in {hop}"));
        }
    }
    if let Some(peak) = split.source_lag_peak_ms {
        text.push_str(&format!(
            "\nslowest handover in the last {} prints: {}",
            split.prints,
            ms(peak),
        ));
    }
    // The hop neither side can see alone. Two measurements taken at two
    // instants, so the difference is an estimate — said out loud rather than
    // presented as a third measured figure.
    if let Some(drawn) = arrival_ms {
        let queued = drawn - split.arrival_lag_ms;
        if queued > 0 {
            text.push_str(&format!(
                "\nthe chart drew it {} after the venue's stamp, so roughly {} \
                 of that was the chart's own queue and drawing",
                ms(drawn),
                ms(queued),
            ));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_unsplit_offsets_and_real_provider_breakdown_keep_their_meaning() {
        assert_eq!(
            live_tape_text(Some(-920), None, None, 2000, 500),
            "clock -920 ms"
        );
        assert_eq!(
            live_tape_text(Some(920), None, None, 2000, 500),
            "clock 920 ms"
        );
        let split = FeedLatency {
            arrival_lag_ms: 920,
            source_lag_ms: Some(800),
            source_lag_peak_ms: Some(810),
            transport_lag_ms: Some(120),
            hop: Some("terminal"),
            prints: 1,
        };
        assert_eq!(
            live_tape_text(None, None, Some(split), 2000, 500),
            "terminal 920 ms"
        );
        assert_eq!(
            live_tape_text(Some(-920), Some(3000), None, 2000, 500),
            "stale 3 s"
        );
    }
}
