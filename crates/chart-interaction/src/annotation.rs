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
        if series.time_at_position(bar) != input.time_ms {
            return Err(Refusal::AnchorMismatch);
        }
        if slot < series.slots() && input.time_ms.is_none() {
            return Err(Refusal::AnchorMismatch);
        }
        return Ok(ResolvedAnchor {
            point: Anchor {
                bar,
                price: input.price,
                time_ms: input.time_ms,
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

    struct Fixture;
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
            self.open_time((bar - 0.5).ceil() as usize)
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
            let result = resolve(&Fixture, &[anchor], Some(reference()), 1).unwrap();
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
            resolve(&Fixture, &[ignored_bar], None, 1).unwrap()[0].slot,
            Some(2)
        );
        assert_eq!(
            resolve(&Fixture, &[ignored_bar], Some(reference()), 1),
            Err(Refusal::InvalidBar)
        );
        assert_eq!(
            resolve(&Fixture, &[input()], None, 1).unwrap()[0].slot,
            Some(2)
        );
        assert_eq!(
            resolve(&Fixture, &[input()], Some(reference()), 1).unwrap()[0].slot,
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
                resolve(&Fixture, &[input()], Some(reference), 1),
                Err(Refusal::StaleReference)
            );
        }
        assert_eq!(
            resolve(
                &Fixture,
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

    #[test]
    fn valid_future_coordinates_remain_future_and_last_invalid_anchor_refuses_all() {
        let future = InputAnchor {
            bar: Some(8.25),
            time_ms: None,
            ..input()
        };
        let result = resolve(&Fixture, &[input(), future], Some(reference()), 2).unwrap();
        assert_eq!(result[1].point.bar, 8.25);
        assert_eq!(result[1].point.time_ms, None);
        assert_eq!(result[1].slot, None);
        assert_eq!(
            resolve(
                &Fixture,
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
