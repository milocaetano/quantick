//! Honest leading absence and clipped, ordered coverage gaps.
use super::model::{BEFORE_CAPTURE, GapPrimitive};
use crate::history::LiquidityHistory;
use crate::timeline::BarTimeline;

pub(super) fn project_gaps(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    time_start: i64,
    time_end: i64,
    depth_enabled: bool,
) -> Vec<GapPrimitive> {
    let config = history.config();
    // Coverage primitives describe the depth layer. With L2 capture off there
    // is no map whose absence needs explaining, so a bubbles-only frame emits
    // no gap marks at all.
    let mut gaps: Vec<GapPrimitive> = if depth_enabled && config.show_gaps {
        history
            .coverage_gaps()
            .filter_map(|gap| {
                let gap_end = gap.end_ms.unwrap_or(time_end);
                if gap_end <= time_start || gap.start_ms >= time_end {
                    return None;
                }
                let x0 = timeline.locate_clamped(gap.start_ms.max(time_start))?;
                let x1 = timeline.locate_clamped(gap_end.min(time_end))?;
                (x1.normalized > x0.normalized).then(|| GapPrimitive {
                    from_generation: gap.from_generation,
                    to_generation: gap.to_generation,
                    x0: x0.normalized,
                    x1: x1.normalized,
                    reason: gap.reason.clone(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    // Historical trades can precede the first locally captured L2 snapshot.
    // Make that absence an explicit primitive instead of a transparent region
    // that could be mistaken for zero resting liquidity. The gap switch covers
    // this leading span too: it is one legend entry, and half-hiding it would
    // leave the legend describing marks the viewer cannot see.
    if depth_enabled && config.show_gaps {
        match history.coverage_segments().next() {
            Some(first_coverage) if first_coverage.start_ms > time_start => {
                let unavailable_end = first_coverage.start_ms.min(time_end);
                if let (Some(x0), Some(x1)) = (
                    timeline.locate_clamped(time_start),
                    timeline.locate_clamped(unavailable_end),
                ) && x1.normalized > x0.normalized
                {
                    gaps.push(GapPrimitive {
                        from_generation: None,
                        to_generation: Some(first_coverage.generation),
                        x0: x0.normalized,
                        x1: x1.normalized,
                        reason: BEFORE_CAPTURE.to_owned(),
                    });
                }
            }
            None => gaps.push(GapPrimitive {
                from_generation: None,
                to_generation: None,
                x0: 0.0,
                x1: 1.0,
                reason: "book_unavailable_before_capture".to_owned(),
            }),
            Some(_) => {}
        }
    }
    gaps.sort_by(|a, b| a.x0.total_cmp(&b.x0).then_with(|| a.x1.total_cmp(&b.x1)));
    gaps
}

#[cfg(test)]
mod tests;
