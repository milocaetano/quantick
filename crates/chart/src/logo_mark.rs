//! The small pixel mark the connection overlay stands above its words.
//!
//! Sampled once from the logo's alpha onto a coarse grid, so it reads as pixels
//! at any window size and is a handful of rectangles to draw. Static unless an
//! attempt is running, when it breathes; nothing here reads a clock.

/// Side of the mark on screen, in points.
pub const MARK_PX: f32 = 56.0;
/// A cell is this fraction of the mark's side, down to [`MIN_CELL_PX`].
const CELLS_PER_MARK: f32 = 40.0;
/// The smallest cell, in points: under this the mark stops reading as pixels.
pub const MIN_CELL_PX: f32 = 4.0;
/// A cell whose coverage falls under this is not drawn.
const MIN_COVERAGE: f32 = 0.25;
/// The mark's alpha at rest.
pub const REST_ALPHA: f32 = 0.35;
/// The breath's two ends and its period.
const BREATH_LOW_ALPHA: f32 = 0.25;
const BREATH_HIGH_ALPHA: f32 = 0.45;
const BREATH_PERIOD_MS: i64 = 2_400;

/// One drawn cell, in cell units from the mark's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkCell {
    pub column: u16,
    pub row: u16,
    /// How much of the cell the logo covers, in `0.0..=1.0`.
    pub coverage: f32,
}

/// The side of one cell for a mark of `mark_px`.
#[must_use]
pub fn cell_px(mark_px: f32) -> f32 {
    (mark_px / CELLS_PER_MARK).max(MIN_CELL_PX)
}

/// The cells of a `side` by `side` alpha image laid over a `cells` by `cells`
/// grid, leaving out what would be a stray speck.
#[must_use]
pub fn sample(alpha: &[u8], side: usize, cells: usize) -> Vec<MarkCell> {
    let mut out = Vec::new();
    if side == 0 || cells == 0 || alpha.len() < side * side {
        return out;
    }
    for row in 0..cells {
        let (top, bottom) = (row * side / cells, (row + 1) * side / cells);
        for column in 0..cells {
            let (left, right) = (column * side / cells, (column + 1) * side / cells);
            let mut sum = 0_u64;
            let mut count = 0_u64;
            for y in top..bottom {
                for value in &alpha[y * side + left..y * side + right] {
                    sum += u64::from(*value);
                    count += 1;
                }
            }
            let coverage = if count == 0 {
                0.0
            } else {
                sum as f32 / (count as f32 * 255.0)
            };
            if coverage >= MIN_COVERAGE {
                out.push(MarkCell {
                    column: u16::try_from(column).unwrap_or(u16::MAX),
                    row: u16::try_from(row).unwrap_or(u16::MAX),
                    coverage,
                });
            }
        }
    }
    out
}

/// The mark's alpha at `now_ms`: a slow breath during an attempt, otherwise a
/// constant that needs no repaint.
#[must_use]
pub fn alpha_at(breathing: bool, now_ms: i64) -> f32 {
    if !breathing {
        return REST_ALPHA;
    }
    let phase = now_ms.rem_euclid(BREATH_PERIOD_MS) as f32 / BREATH_PERIOD_MS as f32;
    let wave = (1.0 - (phase * std::f32::consts::TAU).cos()) / 2.0;
    BREATH_LOW_ALPHA + (BREATH_HIGH_ALPHA - BREATH_LOW_ALPHA) * wave
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_mark_keeps_the_minimum_cell() {
        assert_eq!(cell_px(MARK_PX), MIN_CELL_PX);
        assert_eq!(cell_px(400.0), 10.0);
    }

    /// Half a two-cell image is covered: only that half is a cell, at its own
    /// measured coverage, and the plate-less corners draw nothing.
    #[test]
    fn sampling_keeps_what_the_logo_covers_and_drops_the_rest() {
        let side = 4;
        let mut alpha = vec![0_u8; side * side];
        for y in 0..2 {
            for x in 0..2 {
                alpha[y * side + x] = 255;
            }
        }
        // One lone pixel: 1/4 of its cell is at the threshold's edge, kept
        // only when strictly enough of it is there.
        alpha[2 * side + 2] = 200;
        let cells = sample(&alpha, side, 2);
        assert_eq!(
            cells,
            vec![MarkCell {
                column: 0,
                row: 0,
                coverage: 1.0
            }]
        );
    }

    #[test]
    fn a_short_image_is_no_mark_rather_than_a_panic() {
        assert!(sample(&[0; 3], 4, 2).is_empty());
        assert!(sample(&[], 0, 2).is_empty());
    }

    #[test]
    fn at_rest_the_mark_is_constant_and_during_an_attempt_it_breathes_in_range() {
        for now in [0, 700, 1_200, 99_999] {
            assert_eq!(alpha_at(false, now), REST_ALPHA);
        }
        assert!((alpha_at(true, 0) - BREATH_LOW_ALPHA).abs() < 1e-5);
        assert!((alpha_at(true, BREATH_PERIOD_MS / 2) - BREATH_HIGH_ALPHA).abs() < 1e-5);
        assert!((alpha_at(true, BREATH_PERIOD_MS) - BREATH_LOW_ALPHA).abs() < 1e-5);
        for now in (0..5_000).step_by(37) {
            let alpha = alpha_at(true, now);
            assert!((BREATH_LOW_ALPHA..=BREATH_HIGH_ALPHA + 1e-5).contains(&alpha));
        }
    }
}
