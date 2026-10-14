//! Bounded display averages; captured intervals remain unchanged.

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::grouping::VisualLiquidityRun;
use crate::history::CoverageSegment;

#[cfg(test)]
#[path = "time_average_tests.rs"]
mod tests;

pub(super) struct DepthRun {
    pub run: VisualLiquidityRun,
    pub time_averaged: bool,
}

/// Keep the current constant intervals exact. Older intervals become time
/// averages, including observed zero depth, within one synchronized generation.
/// Never average through a transport gap. The safety cap still reports omissions
/// if even one cell per row/generation cannot fit.
pub(super) fn project(
    runs: Vec<VisualLiquidityRun>,
    coverage: impl Iterator<Item = CoverageSegment>,
    start: i64,
    end: i64,
    observed_end: i64,
    cap: usize,
) -> Vec<DepthRun> {
    if runs.len() <= cap || end <= start || cap == 0 {
        return runs
            .into_iter()
            .map(|run| DepthRun {
                run,
                time_averaged: false,
            })
            .collect();
    }
    let mut exact = Vec::new();
    let mut historical = Vec::new();
    for run in runs {
        if run.end_ms == end {
            exact.push(DepthRun {
                run,
                time_averaged: false,
            });
        } else {
            historical.push(run);
        }
    }
    let mut current: BTreeMap<_, _> = exact
        .iter()
        .map(|depth| {
            (
                (depth.run.generation, depth.run.side, depth.run.price_bucket),
                depth.run.start_ms,
            )
        })
        .collect();
    let exact_rows: std::collections::BTreeSet<_> = current.keys().copied().collect();
    // An absent row's trailing observed zero must stay empty, especially at
    // the newest confirmation. Average internal zeros, never extend a deleted
    // level to the live edge merely because its generation remains open.
    for run in &historical {
        let key = (run.generation, run.side, run.price_bucket);
        if !exact_rows.contains(&key) {
            current
                .entry(key)
                .and_modify(|end| *end = (*end).max(run.end_ms))
                .or_insert(run.end_ms);
        }
    }
    let coverage: BTreeMap<_, _> = coverage
        .map(|segment| (segment.generation, segment))
        .collect();
    let span = observed_end.saturating_sub(start).max(1);
    let rows: std::collections::BTreeSet<_> = historical
        .iter()
        .map(|run| (run.generation, run.side, run.price_bucket))
        .collect();
    let slots = cap.saturating_sub(exact.len()).max(1);
    let mut width = ((span as u128 * rows.len().max(1) as u128).div_ceil(slots as u128))
        .min(span as u128) as i64;
    width = width.max(1);
    loop {
        let mut bins = BTreeMap::new();
        for run in &historical {
            let Some(segment) = coverage.get(&run.generation) else {
                continue;
            };
            let mut from = run.start_ms;
            while from < run.end_ms {
                let bin_start = start
                    .saturating_add((from.saturating_sub(start) / width) * width)
                    .max(segment.start_ms);
                let bin_end = start
                    .saturating_add((from.saturating_sub(start) / width + 1).saturating_mul(width))
                    .min(observed_end)
                    .min(segment.end_ms.unwrap_or(observed_end))
                    .min(
                        current
                            .get(&(run.generation, run.side, run.price_bucket))
                            .copied()
                            .unwrap_or(observed_end),
                    );
                let until = run.end_ms.min(bin_end);
                if until <= from {
                    break;
                }
                let key = (
                    run.generation,
                    run.side,
                    run.price_bucket,
                    bin_start,
                    bin_end,
                );
                *bins.entry(key).or_insert(Decimal::ZERO) +=
                    run.quantity * Decimal::from(until - from);
                from = until;
            }
        }
        if bins.len().saturating_add(exact.len()) <= cap || width >= span {
            exact.extend(bins.into_iter().map(
                |((generation, side, price_bucket, start_ms, end_ms), weighted)| DepthRun {
                    run: VisualLiquidityRun {
                        generation,
                        side,
                        price_bucket,
                        quantity: weighted / Decimal::from(end_ms - start_ms),
                        start_ms,
                        end_ms,
                    },
                    time_averaged: true,
                },
            ));
            return exact;
        }
        width = width.saturating_mul(2).min(span);
    }
}
