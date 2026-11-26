//! The equity curve's shape in data space — trade index against equity —
//! so the window only maps points to pixels.

use super::readout::EquityWalk;

/// One filled area between a walk and zero, as `(trade index, equity)`
/// corners: a gain's area or a loss's, never both.
#[derive(Debug, Clone, PartialEq)]
pub struct FillPiece {
    pub corners: Vec<(f32, f32)>,
    pub gain: bool,
}

impl EquityWalk {
    /// Trades walked; the walk holds one point more.
    #[must_use]
    pub fn trades(&self) -> usize {
        self.plot.len().saturating_sub(1)
    }

    /// The step that keeps a curve under `max_points` points; at least 1.
    #[must_use]
    pub fn stride(&self, max_points: usize) -> usize {
        self.trades().div_ceil(max_points.max(1)).max(1)
    }

    /// The indices a curve draws at `stride`: the start, every
    /// `stride`-th trade, and always the last.
    #[must_use]
    pub fn drawn(&self, stride: usize) -> Vec<usize> {
        let n = self.trades();
        let mut drawn = vec![0];
        while let Some(&last) = drawn.last().filter(|&&last| last < n) {
            drawn.push((last + stride.max(1)).min(n));
        }
        drawn
    }

    /// The diverging fill to zero under the drawn line: each step one
    /// piece, or two split at the exact zero crossing.
    #[must_use]
    pub fn fill_pieces(&self, stride: usize) -> Vec<FillPiece> {
        self.drawn(stride)
            .windows(2)
            .flat_map(|pair| {
                let (x0, x1) = (pair[0] as f32, pair[1] as f32);
                step(x0, x1, self.plot[pair[0]], self.plot[pair[1]])
            })
            .collect()
    }
}

/// The fill under one step, from `a` at `x0` to `b` at `x1`.
fn step(x0: f32, x1: f32, a: f32, b: f32) -> Vec<FillPiece> {
    if a == 0.0 && b == 0.0 {
        return Vec::new();
    }
    if (a >= 0.0) == (b >= 0.0) {
        let gain = if a == 0.0 { b } else { a } >= 0.0;
        return vec![FillPiece {
            corners: vec![(x0, a), (x1, b), (x1, 0.0), (x0, 0.0)],
            gain,
        }];
    }
    let crossing = x0 + (x1 - x0) * (a / (a - b));
    vec![
        FillPiece {
            corners: vec![(x0, a), (crossing, 0.0), (x0, 0.0)],
            gain: a >= 0.0,
        },
        FillPiece {
            corners: vec![(crossing, 0.0), (x1, b), (x1, 0.0)],
            gain: b >= 0.0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pnl::PnlPrimary;
    use rust_decimal::Decimal;

    fn walk(values: &[i64]) -> EquityWalk {
        EquityWalk::walk(
            values.iter().map(|value| Decimal::from(*value)),
            PnlPrimary::PointsTimesQty,
            "pts".to_owned(),
        )
    }

    #[test]
    fn a_step_across_zero_splits_at_the_exact_crossing() {
        // 0 -> +30 -> -10: a gain, then a step crossing zero at 3/4.
        let pieces = walk(&[30, -40]).fill_pieces(1);
        assert_eq!(
            pieces,
            vec![
                FillPiece {
                    corners: vec![(0.0, 0.0), (1.0, 30.0), (1.0, 0.0), (0.0, 0.0)],
                    gain: true,
                },
                FillPiece {
                    corners: vec![(1.0, 30.0), (1.75, 0.0), (1.0, 0.0)],
                    gain: true,
                },
                FillPiece {
                    corners: vec![(1.75, 0.0), (2.0, -10.0), (2.0, 0.0)],
                    gain: false,
                },
            ]
        );
    }

    #[test]
    fn a_flat_walk_on_zero_fills_nothing() {
        assert!(walk(&[0, 0]).fill_pieces(1).is_empty());
    }

    #[test]
    fn a_downsampled_curve_keeps_the_start_and_the_last_trade() {
        let long = walk(&[1; 7]);
        assert_eq!(long.stride(3), 3);
        assert_eq!(long.drawn(3), vec![0, 3, 6, 7]);
        assert_eq!(long.drawn(1).len(), 8);
        assert_eq!(walk(&[]).drawn(1), vec![0], "no trades, the start alone");
    }
}
