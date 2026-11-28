//! Annotation coordinates are resolved completely before a caller mutates a store.
//! A supplied reference opts into exact slot identity; legacy time-only requests
//! retain their original lookup rule. The caller implements a read-only series port.

use crate::quick_range::{Anchor, RangeContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChartReference {
    pub pane_id: u64,
    pub revision: u64,
    pub layout_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputAnchor {
    pub bar: Option<f32>,
    pub price: f64,
    pub time_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedAnchor {
    pub point: Anchor,
    pub slot: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    AnchorCount { expected: usize, actual: usize },
    DraftInProgress,
    InvalidPrice,
    InvalidBar,
    MissingCoordinate,
    NoBars,
    NoCoveringBar,
    StaleReference,
    AnchorMismatch,
}

impl Refusal {
    /// The session, not the request, is at fault: the same call may succeed later.
    #[must_use]
    pub fn is_unavailable(self) -> bool {
        matches!(
            self,
            Self::DraftInProgress | Self::StaleReference | Self::NoBars
        )
    }

    /// The English an operator reads; every wire version answers one mistake with it.
    #[must_use]
    pub fn message(self) -> String {
        match self {
            Self::AnchorCount { expected, actual } => {
                format!("annotation takes exactly {expected} anchor(s), not {actual}")
            }
            Self::DraftInProgress => "the trader is drawing on that pane right now; an annotation would land in their unfinished object".into(),
            Self::InvalidPrice => "an anchor price is not a finite decimal".into(),
            Self::InvalidBar => "an anchor bar_position is not a non-negative chart slot".into(),
            Self::MissingCoordinate => "an anchor names neither a market time nor a bar_position".into(),
            Self::NoBars => "that chart has no bars yet, so an anchor has nothing to land on".into(),
            Self::NoCoveringBar => "no bar on that chart covers the anchor time".into(),
            Self::StaleReference => "the range belongs to a different pane, layout or series revision; draw the range again".into(),
            Self::AnchorMismatch => "an anchor's bar_position and time_unix_ms name different bars on that chart".into(),
        }
    }

    /// What the operator does next, where the message alone does not say.
    #[must_use]
    pub fn next_step(self) -> Option<&'static str> {
        match self {
            Self::InvalidBar => Some(
                "Read a bar_position from chart.window.read (a bar's centre is its slot + 0.5), or drop chart_reference and anchor by time_unix_ms.",
            ),
            Self::MissingCoordinate => Some(
                "Give each anchor a bar's open_time_unix_ms from chart.window.read, or a bar_position for space past the newest bar.",
            ),
            Self::NoCoveringBar => Some(
                "Read a bar's open_time_unix_ms from chart.window.read or the cursor, and anchor to that.",
            ),
            Self::AnchorMismatch => Some(
                "Read the bar again from chart.window.read and send its bar_position with its own open_time_unix_ms; a bar_position past the newest bar carries no time.",
            ),
            _ => None,
        }
    }
}

pub trait Series {
    fn context(&self) -> RangeContext;
    fn slots(&self) -> usize;
    fn slot_at_time(&self, time_ms: i64) -> Option<usize>;
    fn open_time(&self, slot: usize) -> Option<i64>;
    fn time_at_position(&self, bar: f32) -> Option<i64>;
    fn draft_in_progress(&self) -> bool;
}

/// Canonical chart slot: the interval closes at `slot + 0.5`, so both a
/// candle centre and a stored right-edge anchor name the same bar. Future
/// slots remain valid coordinates; callers decide whether data exists there.
#[must_use]
pub fn slot_at_position(bar: f32) -> Option<usize> {
    if !bar.is_finite() {
        return None;
    }
    let slot = (bar - 0.5).ceil();
    (slot >= 0.0).then_some(slot as usize)
}

/// One anchor per horizontal level of a two-anchor range, each at the
/// earlier bar so the lines start where the range does.
pub fn horizontal_level_anchors(
    anchors: [ResolvedAnchor; 2],
) -> impl Iterator<Item = ResolvedAnchor> {
    let left = anchors[usize::from(anchors[1].point.bar < anchors[0].point.bar)];
    let levels = crate::quick_range::horizontal_levels(anchors.map(|anchor| anchor.point));
    levels.into_prices().map(move |price| ResolvedAnchor {
        point: Anchor {
            price,
            ..left.point
        },
        ..left
    })
}

/// Rare operation: at most three anchors. No drawing-store callback is accepted,
/// so a failure at the last anchor cannot leave a partially installed drawing.
pub fn resolve(
    series: &impl Series,
    input: &[InputAnchor],
    reference: Option<ChartReference>,
    required: usize,
) -> Result<Vec<ResolvedAnchor>, Refusal> {
    if input.len() != required || !(1..=3).contains(&required) {
        return Err(Refusal::AnchorCount {
            expected: required,
            actual: input.len(),
        });
    }
    if series.draft_in_progress() {
        return Err(Refusal::DraftInProgress);
    }
    if let Some(reference) = reference {
        let context = series.context();
        if reference.pane_id != context.owner.pane
            || reference.revision != context.revision
            || reference.layout_id != context.owner.layout
        {
            return Err(Refusal::StaleReference);
        }
    }
    input
        .iter()
        .map(|anchor| resolve_anchor(series, *anchor, reference.is_some()))
        .collect()
}

/// Whether `time_ms` is an estimate for future space: past the newest bar
/// of a series with no clock there (tick, volume, renko), any time later
/// than the newest bar's open is a pace projected by whoever sent it. The
/// bar decides where the anchor lands and the time is dropped, so the pane
/// names its own projection; a time chart's exact clock still has to match.
fn projected_past_end(
    series: &impl Series,
    slot: usize,
    exact: Option<i64>,
    time: Option<i64>,
) -> bool {
    let newest = series.slots().checked_sub(1);
    let newest_open = newest.and_then(|newest| series.open_time(newest));
    slot >= series.slots()
        && exact.is_none()
        && time.is_some_and(|time| newest_open.is_none_or(|open| time > open))
}

fn resolve_anchor(
    series: &impl Series,
    input: InputAnchor,
    exact: bool,
) -> Result<ResolvedAnchor, Refusal> {
    if !input.price.is_finite() {
        return Err(Refusal::InvalidPrice);
    }
    if (exact || input.time_ms.is_none())
        && input
            .bar
            .is_some_and(|bar| !bar.is_finite() || bar < 0.0 || bar as f64 >= usize::MAX as f64)
    {
        return Err(Refusal::InvalidBar);
    }
    if exact {
        let bar = input.bar.ok_or(Refusal::MissingCoordinate)?;
        let slot = slot_at_position(bar).ok_or(Refusal::InvalidBar)?;
        let exact_time = series.time_at_position(bar);
        let projected = projected_past_end(series, slot, exact_time, input.time_ms);
        if exact_time != input.time_ms && !projected {
            return Err(Refusal::AnchorMismatch);
        }
        if slot < series.slots() && input.time_ms.is_none() {
            return Err(Refusal::AnchorMismatch);
        }
        return Ok(ResolvedAnchor {
            point: Anchor {
                bar,
                price: input.price,
                time_ms: input.time_ms.filter(|_| !projected),
            },
            slot: (slot < series.slots()).then_some(slot),
        });
    }
    if let Some(time_ms) = input.time_ms {
        if series.slots() == 0 {
            return Err(Refusal::NoBars);
        }
        let slot = series.slot_at_time(time_ms).ok_or(Refusal::NoCoveringBar)?;
        let resolved_time = series.open_time(slot).unwrap_or(time_ms);
        Ok(ResolvedAnchor {
            point: Anchor {
                bar: slot as f32 + 0.5,
                price: input.price,
                time_ms: Some(resolved_time),
            },
            slot: Some(slot),
        })
    } else {
        Ok(ResolvedAnchor {
            point: Anchor {
                bar: input.bar.ok_or(Refusal::MissingCoordinate)?,
                price: input.price,
                time_ms: None,
            },
            slot: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quick_range::Owner;

    /// Three bars opened at 1000; `clock`: past them a time chart's clock
    /// names every slot, otherwise (tick, volume, renko) none does.
    struct Fixture {
        clock: bool,
    }
    const TICKS: Fixture = Fixture { clock: false };
    impl Series for Fixture {
        fn context(&self) -> RangeContext {
            RangeContext {
                owner: Owner {
                    tab: 1,
                    pane: 2,
                    layout: Some(3),
                },
                revision: 4,
            }
        }
        fn slots(&self) -> usize {
            3
        }
        fn slot_at_time(&self, _: i64) -> Option<usize> {
            Some(2)
        }
        fn open_time(&self, slot: usize) -> Option<i64> {
            (slot < 3).then_some(1000)
        }
        fn time_at_position(&self, bar: f32) -> Option<i64> {
            let slot = (bar - 0.5).ceil() as usize;
            let clock = self.clock.then(|| 1000 + 60 * slot as i64);
            self.open_time(slot).or(clock)
        }
        fn draft_in_progress(&self) -> bool {
            false
        }
    }

    fn reference() -> ChartReference {
        ChartReference {
            pane_id: 2,
            revision: 4,
            layout_id: Some(3),
        }
    }
    fn input() -> InputAnchor {
        InputAnchor {
            bar: Some(0.5),
            price: 100.0,
            time_ms: Some(1000),
        }
    }

    #[test]
    fn every_refusal_reads_as_english_and_coordinate_mistakes_name_a_next_step() {
        use Refusal::*;
        let every = [
            AnchorCount {
                expected: 2,
                actual: 1,
            },
            DraftInProgress,
            InvalidPrice,
            InvalidBar,
            MissingCoordinate,
            NoBars,
            NoCoveringBar,
            StaleReference,
            AnchorMismatch,
        ];
        for refusal in every {
            let name = format!("{refusal:?}");
            let name = name.split([' ', '{']).next().unwrap();
            assert!(
                !refusal.message().contains(name),
                "{name} leaks its Rust name"
            );
        }
        for refusal in [InvalidBar, MissingCoordinate, NoCoveringBar, AnchorMismatch] {
            assert!(refusal.next_step().is_some(), "{refusal:?} names no remedy");
        }
    }

    #[test]
    fn exact_fractional_position_uses_the_series_slot_including_future_space() {
        for (bar, time_ms, expected_slot) in [
            (0.5, Some(1000), Some(0)),
            (0.75, Some(1000), Some(1)),
            (1.75, Some(1000), Some(2)),
            (2.5, Some(1000), Some(2)),
            (2.75, None, None),
        ] {
            let anchor = InputAnchor {
                bar: Some(bar),
                time_ms,
                ..input()
            };
            let result = resolve(&TICKS, &[anchor], Some(reference()), 1).unwrap();
            assert_eq!(result[0].slot, expected_slot, "position {bar}");
            assert_eq!(result[0].point.bar, bar);
            assert_eq!(result[0].point.time_ms, time_ms);
        }
    }

    #[test]
    fn exact_slot_and_legacy_timestamp_are_distinct_declared_modes() {
        let ignored_bar = InputAnchor {
            bar: Some(-1.0),
            ..input()
        };
        assert_eq!(
            resolve(&TICKS, &[ignored_bar], None, 1).unwrap()[0].slot,
            Some(2)
        );
        assert_eq!(
            resolve(&TICKS, &[ignored_bar], Some(reference()), 1),
            Err(Refusal::InvalidBar)
        );
        assert_eq!(
            resolve(&TICKS, &[input()], None, 1).unwrap()[0].slot,
            Some(2)
        );
        assert_eq!(
            resolve(&TICKS, &[input()], Some(reference()), 1).unwrap()[0].slot,
            Some(0)
        );
    }

    #[test]
    fn every_reference_component_and_timestamp_is_checked() {
        for reference in [
            ChartReference {
                pane_id: 99,
                ..reference()
            },
            ChartReference {
                revision: 99,
                ..reference()
            },
            ChartReference {
                layout_id: Some(99),
                ..reference()
            },
        ] {
            assert_eq!(
                resolve(&TICKS, &[input()], Some(reference), 1),
                Err(Refusal::StaleReference)
            );
        }
        assert_eq!(
            resolve(
                &TICKS,
                &[InputAnchor {
                    time_ms: Some(999),
                    ..input()
                }],
                Some(reference()),
                1
            ),
            Err(Refusal::AnchorMismatch)
        );
    }

    /// The trader's report: a range dragged past the newest bar of a tick
    /// chart carries the time the pane projected there, and placed nothing.
    #[test]
    fn a_future_bar_keeps_its_slot_whatever_time_a_chart_without_a_clock_was_sent() {
        for (row, series, time_ms, expected) in [
            ("projected time, tick chart", TICKS, Some(5000), Ok(None)),
            (
                "time inside the newest bar, tick chart",
                TICKS,
                Some(1000),
                Err(Refusal::AnchorMismatch),
            ),
            (
                "the clock's time, time chart",
                Fixture { clock: true },
                Some(1480),
                Ok(Some(1480)),
            ),
            (
                "another time, time chart",
                Fixture { clock: true },
                Some(5000),
                Err(Refusal::AnchorMismatch),
            ),
        ] {
            let future = InputAnchor {
                bar: Some(8.25),
                time_ms,
                ..input()
            };
            let result = resolve(&series, &[input(), future], Some(reference()), 2);
            let placed = result.map(|anchors| {
                assert_eq!(anchors[1].point.bar, 8.25, "{row}: the released slot");
                assert_eq!(anchors[1].slot, None, "{row}: no bar holds it yet");
                anchors[1].point.time_ms
            });
            assert_eq!(placed, expected, "{row}");
        }
    }

    #[test]
    fn valid_future_coordinates_remain_future_and_last_invalid_anchor_refuses_all() {
        let future = InputAnchor {
            bar: Some(8.25),
            time_ms: None,
            ..input()
        };
        let result = resolve(&TICKS, &[input(), future], Some(reference()), 2).unwrap();
        assert_eq!(result[1].point.bar, 8.25);
        assert_eq!(result[1].point.time_ms, None);
        assert_eq!(result[1].slot, None);
        assert_eq!(
            resolve(
                &TICKS,
                &[
                    input(),
                    InputAnchor {
                        price: f64::NAN,
                        ..future
                    }
                ],
                Some(reference()),
                2
            ),
            Err(Refusal::InvalidPrice)
        );
    }
}
