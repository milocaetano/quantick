//! The cluster style's heat ramp: six quantised steps, the percentile
//! cuts that place a quantity on one of them, and the ink each step takes.
//!
//! The deciding half of the cluster's colour — a table lookup at paint time
//! and nothing more — kept apart from the painter so the ramp can be pinned
//! by tests that never touch egui.

use std::collections::BTreeMap;

use eframe::egui;
use quantick_engine::{FootprintLevel, Side};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::theme;

/// How many steps the heat ramp has.
///
/// Quantised, never a gradient. Three reasons, in order: rounding a float into
/// a colour every frame is how a pixel moves between two identical frames; the
/// depth map already owns the "continuous gradient" channel on this same
/// screen; and steps can be counted, which a gradient cannot.
pub(super) const HEAT_STEP_COUNT: usize = 6;

/// The cuts and the colours describe the same ramp from two sides, and only
/// agree by construction: every cut opens a step, and the floor below the
/// first cut is free. Adding a colour without a cut leaves one unreachable.
const _: () = assert!(
    HEAT_PERCENTILES.len() + 1 == HEAT_STEP_COUNT,
    "the heat ramp needs exactly one more colour than it has cuts"
);

/// The heat ramp, one colour per step, darkest first.
///
/// **Derived, then written down.** Each entry was resolved in CIELCh — a
/// chosen lightness, a chosen hue, and the most chroma that lightness and hue
/// can hold inside sRGB — and pasted here as a literal. Three things follow
/// from doing it that way rather than mixing toward black and white at paint
/// time:
///
/// - **Chroma is an input.** A linear mix toward a neutral is a mix with grey,
///   so it launders the colour out at both ends: the old top sell step had
///   thrown away 68% of its base hue's chroma, and that vividness is exactly
///   what the reference charts get their heat from. Here the top step carries
///   more chroma than the token it came from.
/// - **The hue is free to travel.** Heat reads as a drift toward orange, and a
///   mix toward white cannot drift. The sell ramp walks toward it and stops
///   with **25.9° to spare** against [`crate::theme::AMBER`], which stays
///   reserved for provenance — measured the way
///   `the_heat_ramp_stays_clear_of_the_reserved_hues` measures it, and that
///   test's floor of 25° is under a degree below. The margin is thin on
///   purpose: it is the statement that the top step cannot be warmed any
///   further without taking a hue the app has already spent. Yellow is never
///   reached at all; at this lightness it would land on AMBER itself.
/// - **No arithmetic at paint time.** No binary search, no float compared per
///   cell, and the ramp is bit-exact for ever — a search in `f32` can walk an
///   8-bit level the day anything upstream of it moves.
///
/// Lightness is the ladder the ink rule reads, so it is the one axis chosen
/// rather than maximised: 16, 22, 32, 40, 56, 78 in L*. Step 1 sits at 22
/// because [`theme::TEXT_MUTED`] stops clearing 4.5:1 above L* 23, and the two
/// quiet steps keep the muted ink.
///
/// The top step takes nearly all the chroma its lightness allows, and that is
/// deliberate rather than incidental: a first pass at this table held it back
/// and landed on C* 22 — within half a unit of the washed-out colour the whole
/// rewrite was meant to replace. The defect had survived its own fix, at the
/// one step that carries the heat. Lightness is untouched by the correction,
/// so every ink ratio is identical.
const HEAT_SELL: [egui::Color32; HEAT_STEP_COUNT] = [
    egui::Color32::from_rgb(0x51, 0x0E, 0x16),
    egui::Color32::from_rgb(0x6A, 0x11, 0x1A),
    egui::Color32::from_rgb(0x95, 0x15, 0x20),
    egui::Color32::from_rgb(0xBD, 0x12, 0x17),
    egui::Color32::from_rgb(0xFE, 0x38, 0x00),
    egui::Color32::from_rgb(0xFD, 0xAF, 0x89),
];

/// See [`HEAT_SELL`]. The buy ramp walks 191° to 170°, away from the sell hue
/// at every step so the two can never converge.
const HEAT_BUY: [egui::Color32; HEAT_STEP_COUNT] = [
    egui::Color32::from_rgb(0x0A, 0x2D, 0x2B),
    egui::Color32::from_rgb(0x0D, 0x3B, 0x38),
    egui::Color32::from_rgb(0x11, 0x55, 0x4E),
    egui::Color32::from_rgb(0x0D, 0x6A, 0x5F),
    egui::Color32::from_rgb(0x00, 0x98, 0x80),
    egui::Color32::from_rgb(0x29, 0xD9, 0xAE),
];

/// The step at and above which the ink turns dark. See [`HEAT_LUMINANCE`].
pub(super) const HEAT_INK_FLIP_STEP: usize = 4;

/// Below this step the ink is muted rather than primary.
///
/// The ramp builds a hierarchy and a two-valued ink erases half of it: a cell
/// on the floor and a cell three steps up read with the same weight of text,
/// so the eye has to decode the background to know which one matters. Letting
/// the quiet cells keep quiet numbers means the digits agree with the colour
/// instead of arguing with it.
pub(super) const HEAT_INK_MUTED_BELOW_STEP: usize = 2;

/// The heat ramp's scale: where each step's boundary falls, in quantity, for
/// the ladders currently on screen.
///
/// **Ranks, not ratios.** Dividing a cell by a fixed reference sounds right
/// and is not: per-cell volume is heavily skewed and the shape of that skew
/// changes with the market, so one denominator paints every cell on the floor
/// in a quiet stretch and saturates half of them in a busy one. Measured on a
/// real capture, ratio-to-p95 put **47% of cells in the top step** — the
/// brightest colour on screen was also the most common one, which leaves
/// nothing for it to stand out against.
///
/// Cutting the visible distribution at fixed *percentiles* fixes both ends by
/// construction: the busiest cells are always the top step and the quiet ones
/// always the floor, whatever the regime. The cuts are uneven on purpose —
/// most rows are ordinary, so the ramp spends its bright steps on the tail
/// that is worth seeing.
///
/// Visible ladders, not the newest N of the series: the denominator has to
/// describe what the trader is looking at. Reading the series instead made
/// the colours depend on where the replay's live edge happened to be.
const HEAT_PERCENTILES: [usize; 5] = [45, 68, 83, 93, 98];

/// One step's lower bound in quantity, ascending.  when there is
/// nothing on screen to measure — a ramp with an invented scale is a colour
/// key that means whatever it likes.
pub(super) type HeatScale = [f64; 5];

pub(super) fn heat_scale<'a>(
    rows: impl Iterator<Item = &'a BTreeMap<i64, FootprintLevel>>,
) -> Option<HeatScale> {
    let mut sides: Vec<f64> = rows
        .flat_map(BTreeMap::values)
        .flat_map(|level| {
            [
                level.buy.to_f64().unwrap_or(0.0),
                level.sell.to_f64().unwrap_or(0.0),
            ]
        })
        .filter(|volume| *volume > 0.0)
        .collect();
    // Display rows, not capture buckets. Cutting the raw grid and colouring
    // the merged one is a scale for a different chart: a drawn cell is the sum
    // of `k` buckets, so the same cut lands at a different place in the
    // distribution at every zoom and every instrument tick.
    if sides.is_empty() {
        return None;
    }
    sides.sort_by(f64::total_cmp);
    let last = sides.len() - 1;
    Some(HEAT_PERCENTILES.map(|pct| sides[last * pct / 100]))
}

/// Which heat step a quantity falls in. `reference` absent (no closed bars
/// yet) puts everything on the floor rather than inventing a scale.
pub(super) fn heat_step(qty: Decimal, scale: Option<HeatScale>) -> usize {
    let Some(scale) = scale else { return 0 };
    let value = qty.to_f64().unwrap_or(0.0);
    // One past the last boundary it clears: below every cut is the floor.
    scale.iter().filter(|cut| value >= **cut).count()
}

/// The fill for a step: a table lookup, and deliberately nothing more.
pub(super) fn heat_fill(side: Side, step: usize) -> egui::Color32 {
    let ramp = match side {
        Side::Buy => HEAT_BUY,
        Side::Sell => HEAT_SELL,
    };
    ramp[step.min(ramp.len() - 1)]
}

/// The ink for a step. A function of the *step*, never of the colour: no
/// luminance arithmetic at paint time, no float compared per frame.
pub(super) fn heat_ink(step: usize) -> egui::Color32 {
    if step >= HEAT_INK_FLIP_STEP {
        theme::CHIP_INK
    } else if step < HEAT_INK_MUTED_BELOW_STEP {
        theme::TEXT_MUTED
    } else {
        theme::TEXT_PRIMARY
    }
}
