//! Painting one bar of the footprint: its plate and spine, then each
//! display row in the style in force — the split ladder, the sell|buy
//! ladder, the mirrored `bidask` bars or the boxed cluster — plus the POC
//! line, the zone marks and the extreme-ratio badges.
//!
//! The frame decides levels, grouping and thresholds once in the root and
//! lends them here as a [`BarPaint`]; a bar contributes only its own rows
//! and its own x. Nothing here allocates per row.

use std::collections::BTreeMap;

use eframe::egui;
use quantick_engine::{Extreme, FootprintLevel, Side};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::footprint_config::StylePlate;
use crate::theme;

use super::heat::{HEAT_INK_FLIP_STEP, HeatScale, heat_fill, heat_ink, heat_step};
use super::{
    CENTER_GUTTER_PX, CLUSTER_BOX_PAD_PX, CLUSTER_GUTTER_PX, DetailLevel, GLYPH_EM,
    LADDER_MIN_FONT_PX, LayerFrame, QUANTITY_GLYPHS, ZoneMark, fmt_delta, fmt_qty, poc_of,
};

/// Where one display row lands on screen, and what the signals say about it.
///
/// Bundled because the per-style row painters all need the same seven facts
/// and none of them need anything else: passing the bundle keeps a new style
/// from reaching back into `draw_bar`'s locals, which is how the four
/// style conditions grew in the first place.
struct RowGeometry {
    row: i64,
    top: f32,
    bottom: f32,
    row_height: f32,
    is_poc: bool,
    buy_imbalance: bool,
    sell_imbalance: bool,
}

/// What every bar in a frame is painted against — resolved once, then lent to
/// each bar.
///
/// The split is the point, not the parameter count: these seven are facts
/// about the *frame* (the level the zoom supports, the style in force after
/// any handover, the row geometry, the thresholds, the heat cuts), while a bar
/// contributes only its own rows and its own x. Passing them one at a time
/// invited each new one to be threaded through by hand, and the signature had
/// grown to nine on exactly that path.
pub(super) struct BarPaint<'a> {
    pub(super) frame: &'a LayerFrame<'a>,
    pub(super) level: DetailLevel,
    pub(super) style: crate::footprint_config::FootprintStyle,
    pub(super) row_group: f64,
    pub(super) ratio: Decimal,
    pub(super) min_qty: Decimal,
    pub(super) heat: Option<HeatScale>,
}

pub(super) fn draw_bar(
    paint: &BarPaint<'_>,
    rows: &BTreeMap<i64, FootprintLevel>,
    xc: f32,
    cells_left: &mut usize,
) {
    let &BarPaint {
        frame,
        level,
        style,
        row_group,
        ratio,
        min_qty,
        heat,
    } = paint;
    let painter = frame.painter;
    let poc = frame.config.show_poc.then(|| poc_of(rows)).flatten();
    let max_volume = rows
        .values()
        .map(|level| level.volume().to_f64().unwrap_or(0.0))
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let max_abs_delta = rows
        .values()
        .map(|level| level.delta().to_f64().unwrap_or(0.0).abs())
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    // `bidask` mirrors two bars against one shared scale, and the scale has to
    // be the larger *side*, never the row total: halving the total would make
    // a one-sided row look like a balanced one at full width.
    let max_side_volume = rows
        .values()
        .map(|level| {
            level
                .buy
                .to_f64()
                .unwrap_or(0.0)
                .max(level.sell.to_f64().unwrap_or(0.0))
        })
        .fold(0.0_f64, f64::max)
        .max(f64::EPSILON);
    let dominates = |qty: Decimal, other: Decimal| -> bool {
        qty >= ratio.saturating_mul(other) && qty.saturating_sub(other) >= min_qty
    };
    let neighbour = |row: i64, side: Side| -> Decimal {
        rows.get(&row)
            .map(|l| match side {
                Side::Buy => l.buy,
                Side::Sell => l.sell,
            })
            .unwrap_or(Decimal::ZERO)
    };

    // The plate: what a style paints under its own content so the content has
    // a floor it controls.
    //
    // The floor matters exactly as much as the content is *digits*. A bar's
    // length reads the same over any background, so the shape styles ask for
    // the light backdrop and leave the map visible; a number does not degrade
    // gracefully, so the digit styles ask for the full casing. Until this was
    // a style's own answer, the ladder had no plate at all, and its contrast
    // floor was whatever the candle preset, the canvas switch and the bucket
    // arithmetic happened to leave behind — 2.2:1 on the `Classic` preset,
    // 4.1:1 on `Glass`.
    let plate = match style.plate() {
        StylePlate::Backdrop => canvas_backdrop(),
        StylePlate::Casing => theme::CASING,
    };
    if level >= DetailLevel::Profile
        && let (Some(&first), Some(&last)) = (rows.keys().next(), rows.keys().next_back())
    {
        // Composed from both rows' screen bands, not from the price names:
        // upside down the highest bucket renders lowest, and reading "top"
        // off it would hand the rect a negative height.
        let (first_top, first_bottom) = row_band(frame, first, row_group);
        let (last_top, last_bottom) = row_band(frame, last, row_group);
        let bar_top = first_top.min(last_top);
        let bar_bottom = first_bottom.max(last_bottom);
        let inset = style.candle_treatment().content_inset();
        let reach = (frame.half - 1.0).max(1.0);
        // One plate per bar, never one per cell: thirty rects instead of one,
        // and — worse — a hairline seam of whatever is behind between every
        // pair of rows.
        let box_rect = egui::Rect::from_min_max(
            egui::pos2(xc - reach + inset, bar_top),
            egui::pos2(xc + reach, bar_bottom),
        );
        painter.rect_filled(box_rect, egui::Rounding::same(2.0), plate);
        if inset > 0.0 {
            // A frame, so the box reads as one object rather than a dark
            // patch — the reference chart's boxed ladder. Border, never a
            // competitor: 2.3:1 against the casing.
            painter.rect_stroke(
                box_rect,
                egui::Rounding::same(2.0),
                egui::Stroke::new(1.0_f32, theme::BORDER),
            );
        }
        // A hairline spine at the central axis keeps the bar's midline
        // readable after the candle body fades to outline — the reference
        // charts' thin gray candle spine. On the ladder it does more: it is
        // the ruler between the two number columns, without which `123 456`
        // reads as one number, so it is drawn firmer there.
        let spine = match style {
            crate::footprint_config::FootprintStyle::Ladder => 0.55,
            _ => 0.3,
        };
        if inset == 0.0 {
            painter.line_segment(
                [egui::pos2(xc, bar_top), egui::pos2(xc, bar_bottom)],
                egui::Stroke::new(1.0_f32, theme::TEXT_FAINT.gamma_multiply(spine)),
            );
        }
    }

    for (&row, cell) in rows {
        if *cells_left == 0 {
            return;
        }
        let (top, bottom) = row_band(frame, row, row_group);
        if bottom < frame.chart_rect.top() || top > frame.chart_rect.bottom() {
            // Off-screen rows cost no budget: the cap exists to bound what
            // is *painted*, and spending it on invisible rows would starve
            // the visible bars of a tall ladder.
            continue;
        }
        *cells_left -= 1;
        let row_height = (bottom - top).max(1.0);
        let is_poc = poc == Some(row);
        let buy_imbalance = dominates(cell.buy, neighbour(row - 1, Side::Sell));
        let sell_imbalance = dominates(cell.sell, neighbour(row + 1, Side::Buy));

        // Styles that own their whole row paint it here and return; the two
        // that share the LOD ladder's generic cell fall through to the match
        // below. `draws_own_rows` is what decides, so a style added to the
        // registry declares which half it belongs to instead of being written
        // into a condition here.
        if style.draws_own_rows() && level >= DetailLevel::Profile {
            let geometry = RowGeometry {
                row,
                top,
                bottom,
                row_height,
                is_poc,
                buy_imbalance,
                sell_imbalance,
            };
            match style {
                crate::footprint_config::FootprintStyle::BidAsk => {
                    draw_bidask_row(frame, cell, &geometry, xc, max_side_volume, row_group);
                    continue;
                }
                crate::footprint_config::FootprintStyle::Cluster => {
                    draw_cluster_row(frame, level, cell, &geometry, xc, heat, max_volume);
                    continue;
                }
                _ => {}
            }
            // The reference look, inside the candle: a central axis at the
            // candle's middle, the total-volume profile growing rightward in
            // neutral light (the exocharts silhouette), and a delta bar per
            // row growing leftward in the winner's color — "who won the
            // fight" readable at a glance, the volume shape behind it.
            let volume_frac = (cell.volume().to_f64().unwrap_or(0.0) / max_volume) as f32;
            let delta = cell.delta();
            let delta_frac = (delta.to_f64().unwrap_or(0.0).abs() / max_abs_delta) as f32;
            let reach = (frame.half - 1.0).max(1.0);
            let delta_text = fmt_delta(delta);
            // A row balanced at display resolution has no winner: neutral
            // sliver, no color, no number — a teal bar on a tie is a
            // wrong-side read (panel must-fix).
            let winner = delta_text.as_ref().map(|_| {
                if delta > Decimal::ZERO {
                    Side::Buy
                } else {
                    Side::Sell
                }
            });
            // The imbalance chip is colored by the side that IS imbalanced,
            // never by the delta winner: a sell-imbalanced row boxed in teal
            // because its own delta leans positive is the one thing a
            // footprint must never say (panel must-fix). Both sides at once
            // is contested — no chip rather than a coin flip.
            let chip_side = match (buy_imbalance, sell_imbalance) {
                (true, false) => Some(Side::Buy),
                (false, true) => Some(Side::Sell),
                _ => None,
            };
            // Right: the profile. The POC row is the brightest thing in the
            // silhouette even before its yellow line lands on it.
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(xc, top + 0.5),
                    egui::pos2(xc + reach * volume_frac, bottom - 0.5),
                ),
                egui::Rounding::ZERO,
                PROFILE_COLOR.gamma_multiply(if is_poc { 0.95 } else { 0.60 }),
            );
            // Left: the fight.
            let (left_from, left_color) = match winner {
                Some(side) => (
                    xc - reach * delta_frac.max(0.04),
                    theme::side_color(side).gamma_multiply(if chip_side.is_some() {
                        0.55
                    } else {
                        0.5
                    }),
                ),
                None => (xc - 2.0, theme::TEXT_FAINT.gamma_multiply(0.35)),
            };
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left_from, top + 0.5),
                    egui::pos2(xc, bottom - 0.5),
                ),
                egui::Rounding::ZERO,
                left_color,
            );
            if let Some(side) = chip_side
                && row_height >= 4.0
            {
                // The chip hugs its own row's content — bar or number — and
                // is inset vertically so neighbouring chips never fuse into
                // one shouting rectangle (panel must-fix); the full-width
                // multi-row box stays reserved for the stacked-zone mark.
                let chip_reach = (reach * delta_frac).max(MIN_CHIP_PX).min(reach);
                painter.rect_stroke(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - chip_reach - 2.0, top + 1.5),
                        egui::pos2(xc - 1.0, bottom - 1.5),
                    ),
                    egui::Rounding::ZERO,
                    egui::Stroke::new(1.0_f32, theme::side_color(side)),
                );
            }
            // Deep zoom: the delta number over the left half, side-colored
            // so the column scans without reading (panel must-fix), width-
            // clamped so it never bleeds out.
            if level == DetailLevel::Detailed
                && frame.config.show_numbers
                && let Some(text) = delta_text
            {
                let width_budget = (frame.half - 6.0) / 3.0;
                let font = egui::FontId::monospace(
                    (row_height - 2.0)
                        .min(width_budget)
                        .clamp(LADDER_MIN_FONT_PX, 13.0),
                );
                painter.text(
                    egui::pos2(xc - CENTER_GUTTER_PX, (top + bottom) / 2.0),
                    egui::Align2::RIGHT_CENTER,
                    text,
                    font,
                    winner.map_or(theme::TEXT_MUTED, theme::side_color),
                );
            }
            if is_poc {
                // Right half only in the split style: a full-width line
                // would strike through the delta digits it shares the row
                // with; the profile half alone carries it unambiguously.
                draw_poc_line(frame, xc, xc + frame.half, row, row_group);
            }
            continue;
        }

        match level {
            DetailLevel::Profile => {
                // Textless histogram, anchored on the candle's left edge and
                // colored by the row's delta sign — neutral by design, so the
                // POC and the zone marks stay the only saturated things.
                let frac = (cell.volume().to_f64().unwrap_or(0.0) / max_volume) as f32;
                let width = (2.0 * frame.half - 1.0).max(1.0) * frac;
                let color = if cell.delta() >= Decimal::ZERO {
                    theme::BUY
                } else {
                    theme::SELL
                };
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - frame.half, top + 0.5),
                        egui::pos2(xc - frame.half + width, bottom - 0.5),
                    ),
                    egui::Rounding::ZERO,
                    color.gamma_multiply(if is_poc { 0.55 } else { 0.28 }),
                );
                if is_poc {
                    draw_poc_dot(frame, xc, row, row_group);
                }
            }
            DetailLevel::Compact | DetailLevel::Detailed => {
                // The font answers to the row height AND the cell width: a
                // five-glyph quantity ("58.1k") is ~3 em of monospace per
                // side, and a number that outgrows its candle bleeds into
                // the neighbour — worse than a smaller number.
                let width_budget = match level {
                    DetailLevel::Detailed => (frame.half - 6.0) / 3.0,
                    _ => (2.0 * frame.half - 6.0) / 3.0,
                };
                let font = egui::FontId::monospace(
                    (row_height - 2.0)
                        .min(width_budget)
                        .clamp(LADDER_MIN_FONT_PX, 13.0),
                );
                if is_poc {
                    // A ring around the row, not a wash under it. The old
                    // tint cost the row its contrast (6.8:1 → 4.4:1 — below
                    // AA on the one row the trader reads first) to say
                    // something an outline says louder, and the full-width
                    // line that came with it struck straight through both
                    // number columns. The split style already refuses that
                    // line for exactly this reason; the ladder had never been
                    // told. The ring also gives the POC a *shape*: the only
                    // framed row in the bar, readable without relying on hue.
                    painter.rect_stroke(
                        egui::Rect::from_min_max(
                            egui::pos2(xc - frame.half + 0.5, top + 0.5),
                            egui::pos2(xc + frame.half - 0.5, bottom - 0.5),
                        ),
                        egui::Rounding::ZERO,
                        egui::Stroke::new(1.5_f32, theme::POC),
                    );
                }
                let mid = (top + bottom) / 2.0;
                if level == DetailLevel::Compact {
                    if !frame.config.show_numbers {
                        continue;
                    }
                    let delta = cell.delta();
                    let side = if delta >= Decimal::ZERO {
                        Side::Buy
                    } else {
                        Side::Sell
                    };
                    painter.text(
                        egui::pos2(xc, mid),
                        egui::Align2::CENTER_CENTER,
                        fmt_qty(delta),
                        font,
                        theme::ink(side),
                    );
                } else {
                    // sell | buy, the tape's own left-to-right: taker-sells
                    // hit the bid printed on the left, taker-buys lift the
                    // ask on the right.
                    for (side, qty, imbalanced, align, x) in [
                        (
                            Side::Sell,
                            cell.sell,
                            sell_imbalance,
                            egui::Align2::RIGHT_CENTER,
                            xc - CENTER_GUTTER_PX,
                        ),
                        (
                            Side::Buy,
                            cell.buy,
                            buy_imbalance,
                            egui::Align2::LEFT_CENTER,
                            xc + CENTER_GUTTER_PX,
                        ),
                    ] {
                        if imbalanced {
                            let cell_rect = match side {
                                Side::Sell => egui::Rect::from_min_max(
                                    egui::pos2(xc - frame.half, top + 0.5),
                                    egui::pos2(xc - 1.0, bottom - 0.5),
                                ),
                                Side::Buy => egui::Rect::from_min_max(
                                    egui::pos2(xc + 1.0, top + 0.5),
                                    egui::pos2(xc + frame.half, bottom - 0.5),
                                ),
                            };
                            // The cell goes *deeper* than the plate, not
                            // brighter. A light pill under text of its own hue
                            // is arithmetically a trap — it raises the floor
                            // exactly beneath the digits it means to
                            // emphasise, and the row carrying the layer's most
                            // important signal ends up its least legible
                            // (3.2:1). Darker cell plus lighter ink inverts
                            // that: 8.6:1 on the same row.
                            painter.rect_filled(
                                cell_rect,
                                egui::Rounding::same(2.0),
                                theme::side_color(side).gamma_multiply(IMBALANCE_CELL_ALPHA),
                            );
                            // And an edge on the *outer* border of the column
                            // that dominated — sell on the body's left, buy on
                            // its right. Position carries the side on its own,
                            // so the colour is redundancy rather than the
                            // channel, and the mark survives colour blindness.
                            let edge = match side {
                                Side::Sell => egui::Rect::from_min_max(
                                    egui::pos2(cell_rect.left(), cell_rect.top()),
                                    egui::pos2(
                                        cell_rect.left() + IMBALANCE_EDGE_PX,
                                        cell_rect.bottom(),
                                    ),
                                ),
                                Side::Buy => egui::Rect::from_min_max(
                                    egui::pos2(
                                        cell_rect.right() - IMBALANCE_EDGE_PX,
                                        cell_rect.top(),
                                    ),
                                    egui::pos2(cell_rect.right(), cell_rect.bottom()),
                                ),
                            };
                            painter.rect_filled(
                                edge,
                                egui::Rounding::ZERO,
                                theme::side_color(side),
                            );
                        }
                        if !frame.config.show_numbers {
                            // Numbers off leaves the imbalance cells above:
                            // the shape of the fight without the digits.
                            continue;
                        }
                        // Over the plate, the ordinary number can afford the
                        // primary ink (14.2:1 instead of the muted grey's
                        // 6.8:1 on canvas — and the muted grey's 2.2:1 on a
                        // `Classic` candle body, which is the reading this
                        // plate exists to end).
                        let color = if imbalanced {
                            theme::ink(side)
                        } else {
                            theme::TEXT_PRIMARY
                        };
                        painter.text(egui::pos2(x, mid), align, fmt_qty(qty), font.clone(), color);
                    }
                }
            }
            DetailLevel::Marks | DetailLevel::Off => {}
        }
    }

    // The extreme ratio badges, Detailed only: the aggression ratio printed
    // beside the bar's low and high — the reference chart's "9.82 at the
    // low". One-sided extremes have no finite ratio and draw nothing rather
    // than an invented stand-in.
    // The badge describes the row the trader can *see*: the ratio is
    // computed on the display rows, not the finer capture grid — a "9.8x"
    // beside a merged row must be that row's own number. The "x" suffix
    // keeps it out of the price vocabulary.
    if level == DetailLevel::Detailed && frame.config.extreme_ratio_badge {
        for extreme in [Extreme::Low, Extreme::High] {
            let cell = match extreme {
                Extreme::Low => rows.iter().next(),
                Extreme::High => rows.iter().next_back(),
            };
            let Some((&row, cell)) = cell else { continue };
            let (dominant, other) = if cell.buy >= cell.sell {
                (cell.buy, cell.sell)
            } else {
                (cell.sell, cell.buy)
            };
            if other.is_zero() {
                continue;
            }
            let Some(ratio_value) = dominant.checked_div(other) else {
                continue;
            };
            // Below the threshold a badge is anti-signal ("1.0x" = nothing
            // happened); survivors get a chip anchored in the dominant
            // side's color, so the exhaustion cue registers as a marker
            // instead of a stray number (panel should-fix).
            if ratio_value < frame.config.badge_min_ratio {
                continue;
            }
            let dominant_side = if cell.buy >= cell.sell {
                Side::Buy
            } else {
                Side::Sell
            };
            // The badge sits *outside* the bar's extent — which end of the row
            // that is on screen follows the scale's orientation, so the chip
            // never lands on the ladder it is describing.
            let (band_top, band_bottom) = row_band(frame, row, row_group);
            let outward_down = match extreme {
                Extreme::Low => !frame.scale.is_inverted(),
                Extreme::High => frame.scale.is_inverted(),
            };
            let (align, y) = if outward_down {
                (egui::Align2::CENTER_TOP, band_bottom + EXTREME_BADGE_GAP_PX)
            } else {
                (egui::Align2::CENTER_BOTTOM, band_top - EXTREME_BADGE_GAP_PX)
            };
            let text = format!("{:.1}x", ratio_value.to_f64().unwrap_or(0.0));
            let galley =
                painter.layout_no_wrap(text, egui::FontId::monospace(10.0), theme::TEXT_PRIMARY);
            let anchor = align.anchor_size(egui::pos2(xc, y), galley.size());
            painter.rect_filled(
                anchor.expand(2.0),
                egui::Rounding::same(2.0),
                canvas_backdrop(),
            );
            painter.rect_stroke(
                anchor.expand(2.0),
                egui::Rounding::same(2.0),
                egui::Stroke::new(1.0_f32, theme::side_color(dominant_side)),
            );
            painter.galley(anchor.min, galley, theme::TEXT_PRIMARY);
        }
    }
}

/// The POC line over `[x_from, x_to]`, with a background under-stroke so it
/// registers against the candle outline it crosses at every level.
fn draw_poc_line(frame: &LayerFrame<'_>, x_from: f32, x_to: f32, row: i64, row_group: f64) {
    let (top, bottom) = row_band(frame, row, row_group);
    let y = (top + bottom) / 2.0;
    frame.painter.line_segment(
        [egui::pos2(x_from, y), egui::pos2(x_to, y)],
        egui::Stroke::new(3.5_f32, canvas_backdrop()),
    );
    frame.painter.line_segment(
        [egui::pos2(x_from, y), egui::pos2(x_to, y)],
        egui::Stroke::new(1.5_f32, theme::POC),
    );
}

/// How far the cluster's grey silhouette is allowed to lighten its column.
///
/// Hard ceiling: past ~0.42 the silhouette pushes the column into the
/// forbidden luminance band and the total column would need a flip rule of its
/// own. Staying under it is what buys the column a single ink.
pub(super) const CLUSTER_TOTAL_SILHOUETTE_ALPHA: f32 = 0.35;

/// Alpha of a `bidask` bar. Low enough that a POC line crosses it readably,
/// high enough that the two sides separate from the plate at a glance.
const BIDASK_BAR_ALPHA: f32 = 0.62;

/// The bevel's two faces.
///
/// Complementary by construction, and the asymmetry is the design rather than
/// a compromise: on a dark cell the white edge does all the work and the
/// shadow has nowhere to go, on a light cell the reverse. Measured in L*, the
/// highlight buys +19 on the floor step and +0.4 on the top one; the shadow
/// buys +5 on the floor and +29 on the top. So only the face that can be seen
/// is drawn — painting both always meant painting one for nothing.
///
/// Raising the alpha does not rescue the losing face: even at full opacity,
/// white over the top step is worth +17 L* — the cell simply turns white.
/// Which is why this is a choice of face, not a choice of number.
const BEVEL_HIGHLIGHT: egui::Color32 = egui::Color32::from_rgba_premultiplied(56, 56, 56, 56);

const BEVEL_SHADOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 120);

/// The step at and above which the shadow carries the relief instead of the
/// highlight. Shares the ink flip's boundary because both answer the same
/// question: is this cell light or dark.
const BEVEL_SHADOW_FROM_STEP: usize = HEAT_INK_FLIP_STEP;

/// Thickness of a bevel face, in pixels.
///
/// Two, not one. A one-pixel rect lands between two device pixels wherever the
/// row band falls on a fraction — which is always, the band being a
/// price-to-y projection — and antialiasing then spreads it until nothing is
/// left: measured, the top step's highlight arrived at +0.26 L* against a
/// theoretical +2.98, well under a just-noticeable difference of ~2.3.
const BEVEL_PX: f32 = 2.0;

/// Under this cell width the bevel is more edge than cell.
const BEVEL_MIN_CELL_PX: f32 = 16.0;

const BEVEL_MIN_ROW_PX: f32 = 10.0;

/// A light top edge and a dark bottom edge — the cheap bevel.
///
/// Two rects rather than four: the left and right faces are the least
/// informative of the four in a field of cells that already touch sideways,
/// and they cost 57% more. Rects rather than strokes, because a stroke goes
/// through the tessellator's feathering, and feathering is exactly what blurs
/// a one-pixel edge into nothing.
fn paint_bevel(
    painter: &egui::Painter,
    rect: egui::Rect,
    row_height: f32,
    step: usize,
    pixels_per_point: f32,
) {
    if row_height < BEVEL_MIN_ROW_PX || rect.width() < BEVEL_MIN_CELL_PX {
        return;
    }
    // Snapped to whole *device* pixels before anything is drawn. The rect
    // arrives on a fraction — the row band is a price-to-y projection — and a
    // bevel is the one thing that cannot survive being antialiased across two
    // rows, because the edge carrying the relief is exactly as wide as the
    // blur would be.
    //
    // Device pixels, not egui points: at 125% or 150% display scaling a whole
    // point is 1.25 or 1.5 physical pixels, so rounding before the scale is
    // applied lands the edge back on a fraction — which is the very thing this
    // is here to avoid, and it would only show on the machines that scale.
    //
    // Handed in, never asked for here: `Context::pixels_per_point` takes an
    // *exclusive* lock on the context, and this runs two or three times per
    // row against a twelve-thousand-row budget. The value is constant for the
    // frame, so asking once and lending it is the difference between one lock
    // and tens of thousands.
    let snap = |v: f32| (v * pixels_per_point).round() / pixels_per_point;
    let rect = egui::Rect::from_min_max(
        egui::pos2(snap(rect.left()), snap(rect.top())),
        egui::pos2(snap(rect.right()), snap(rect.bottom())),
    );
    let (face, lit_top) = if step >= BEVEL_SHADOW_FROM_STEP {
        (BEVEL_SHADOW, false)
    } else {
        (BEVEL_HIGHLIGHT, true)
    };
    // Two edges meeting at a corner, not two opposite bars: relief is read at
    // the corner, and a top and a bottom with no sides read as a rule.
    let (horizontal, vertical) = if lit_top {
        (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + BEVEL_PX)),
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + BEVEL_PX, rect.bottom())),
        )
    } else {
        (
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - BEVEL_PX), rect.max),
            egui::Rect::from_min_max(egui::pos2(rect.right() - BEVEL_PX, rect.top()), rect.max),
        )
    };
    painter.rect_filled(horizontal, egui::Rounding::ZERO, face);
    painter.rect_filled(vertical, egui::Rounding::ZERO, face);
}

/// How wide one cluster column is, given the body width it shares.
///
/// The painter and the floor read the *same* function. They used to compute it
/// apart, which is how the floor came to declare a width legible that the
/// painter then drew three overlapping numbers into.
pub(super) fn cluster_column_px_from(body_width: f32, columns: f32) -> f32 {
    let inner = body_width
        - crate::footprint_config::CANDLE_LANE_PX
        - 2.0 * CLUSTER_BOX_PAD_PX
        - (columns - 1.0) * CLUSTER_GUTTER_PX;
    (inner / columns).max(1.0)
}

/// How many number columns the cluster draws: three with the total, two
/// without. The knob exists because the third column costs ~33 px of candle
/// width, and a trader who would rather see more bars than one more number
/// should not have to leave the style to get them.
fn cluster_columns(config: &crate::footprint_config::FootprintConfig) -> f32 {
    if config.cluster_show_total { 3.0 } else { 2.0 }
}

/// One row of the `bidask` style: both sides at their real size, mirrored
/// around the bar's axis on one shared scale.
///
/// The split answers "who won, and how much traded"; this answers "how big was
/// each side" — a question the split's single delta bar cannot, because 400×380
/// and 40×20 share a delta and are not the same market. Two mirrored lengths
/// on one scale make that difference the first thing the eye gets, with no
/// digit involved, which is why this style survives down to Profile where the
/// number styles cannot go.
fn draw_bidask_row(
    frame: &LayerFrame<'_>,
    cell: &FootprintLevel,
    geometry: &RowGeometry,
    xc: f32,
    max_side_volume: f64,
    row_group: f64,
) {
    let painter = frame.painter;
    let reach = (frame.half - 1.0).max(1.0);
    let top = geometry.top + 0.5;
    let bottom = geometry.bottom - 0.5;
    for (side, qty, imbalanced) in [
        (Side::Sell, cell.sell, geometry.sell_imbalance),
        (Side::Buy, cell.buy, geometry.buy_imbalance),
    ] {
        let frac = (qty.to_f64().unwrap_or(0.0) / max_side_volume) as f32;
        let span = reach * frac.clamp(0.0, 1.0);
        if span <= 0.0 {
            continue;
        }
        // Sell grows left, buy grows right: the tape's own left-to-right, the
        // same one the ladder's columns keep.
        let bar = match side {
            Side::Sell => {
                egui::Rect::from_min_max(egui::pos2(xc - span, top), egui::pos2(xc, bottom))
            }
            Side::Buy => {
                egui::Rect::from_min_max(egui::pos2(xc, top), egui::pos2(xc + span, bottom))
            }
        };
        painter.rect_filled(
            bar,
            egui::Rounding::ZERO,
            theme::side_color(side).gamma_multiply(BIDASK_BAR_ALPHA),
        );
        if imbalanced {
            // A cap on the growing end, in the side's own ink. It reads as a
            // tipped bar rather than a coloured one — form first, hue as
            // backup — and it lands where the eye already is, at the end of
            // the longest bar in the row.
            let cap = match side {
                Side::Sell => egui::Rect::from_min_max(
                    egui::pos2(bar.left(), top),
                    egui::pos2(bar.left() + IMBALANCE_EDGE_PX, bottom),
                ),
                Side::Buy => egui::Rect::from_min_max(
                    egui::pos2(bar.right() - IMBALANCE_EDGE_PX, top),
                    egui::pos2(bar.right(), bottom),
                ),
            };
            painter.rect_filled(cap, egui::Rounding::ZERO, theme::ink(side));
        }
    }
    if geometry.is_poc {
        draw_poc_dot(frame, xc, geometry.row, row_group);
    }
}

/// One row of the `cluster` style: the reference chart's boxed ladder — bid,
/// ask and the row total, each cell shaded by how much volume it holds.
#[allow(clippy::too_many_arguments)]
fn draw_cluster_row(
    frame: &LayerFrame<'_>,
    level: DetailLevel,
    cell: &FootprintLevel,
    geometry: &RowGeometry,
    xc: f32,
    heat: Option<HeatScale>,
    max_volume: f64,
) {
    let painter = frame.painter;
    let reach = (frame.half - 1.0).max(1.0);
    let inset = crate::footprint_config::CANDLE_LANE_PX;
    // No inset: a half pixel a side became a four-pixel seam in a sixteen-pixel
    // row once antialiasing had spread both edges — a quarter of the row given
    // to gaps, which reads as a black grid rather than as raised cells. The
    // bevel is what separates one row from the next here.
    let top = geometry.top;
    let bottom = geometry.bottom;
    let columns = cluster_columns(frame.config);
    let column_width = cluster_column_px_from(2.0 * reach, columns);
    let bevel = frame.config.cluster_bevel && level == DetailLevel::Detailed;
    let pixels_per_point = frame.pixels_per_point;
    // The width budget is a *font size*, not a width: a five-glyph quantity is
    // `QUANTITY_GLYPHS * GLYPH_EM` ≈ 3 em of monospace, so the room a column
    // has buys a third of that in point size. Handing the column's raw width
    // to the font instead is how three columns of numbers end up written over
    // each other — which is exactly what the first capture of this style
    // showed. Same arithmetic the ladder's own budget uses.
    let width_budget = (column_width - 2.0) / (QUANTITY_GLYPHS * GLYPH_EM);
    let font = egui::FontId::monospace(
        (geometry.row_height - 2.0)
            .min(width_budget)
            .clamp(LADDER_MIN_FONT_PX, 13.0),
    );

    let mut x = xc - reach + inset + CLUSTER_BOX_PAD_PX;
    let mut column_rect = || -> egui::Rect {
        let rect =
            egui::Rect::from_min_max(egui::pos2(x, top), egui::pos2(x + column_width, bottom));
        x += column_width + CLUSTER_GUTTER_PX;
        rect
    };

    // Bid then ask, in that order and never the other: the ramp is
    // isoluminant between the two sides, so under deuteranopia the hues
    // collapse and *position* is what still says which side a number is.
    // That is a deliberate trade — luminance carries the ordinal reading,
    // which works for everyone — and it only holds while the columns stay put.
    for (side, qty, imbalanced) in [
        (Side::Sell, cell.sell, geometry.sell_imbalance),
        (Side::Buy, cell.buy, geometry.buy_imbalance),
    ] {
        let rect = column_rect();
        let step = heat_step(qty, heat);
        painter.rect_filled(rect, egui::Rounding::ZERO, heat_fill(side, step));
        if bevel {
            paint_bevel(painter, rect, geometry.row_height, step, pixels_per_point);
        }
        if imbalanced {
            // The outline flips with the ink, and for the same reason. A
            // side's lightened ink is *darker* than the ramp's top steps —
            // measured, 1.27:1 — so the mark that says "look here" was
            // vanishing into the cell it was meant to ring, precisely on the
            // busiest rows.
            let outline = if step >= HEAT_INK_FLIP_STEP {
                theme::CHIP_INK
            } else {
                theme::ink(side)
            };
            painter.rect_stroke(
                rect.shrink(0.5),
                egui::Rounding::ZERO,
                egui::Stroke::new(1.5_f32, outline),
            );
        }
        if frame.config.show_numbers && !qty.is_zero() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                fmt_qty(qty),
                font.clone(),
                heat_ink(step),
            );
        }
    }

    // The total column carries a silhouette, not a heat step. The grey bar
    // answers "where did volume concentrate" on an axis — length — that does
    // not compete with the digit for contrast, so the whole column lives on a
    // single ink with no flip rule. It is also the same silhouette the split
    // style draws, which is the point: one visual idea, two places.
    if columns > 2.0 {
        let rect = column_rect();
        let volume = cell.volume();
        // Against the bar's own busiest row, not the screen-wide side scale.
        // A row total is structurally about twice one side, so measuring it
        // with a per-side scale pinned nine rows in ten at full width —
        // wallpaper with a number on it rather than a histogram. Per bar is
        // also the right question here: "which row of *this* bar held the
        // volume", which is what the split style's silhouette answers too.
        let frac = if max_volume > 0.0 {
            (volume.to_f64().unwrap_or(0.0) / max_volume).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        painter.rect_filled(
            egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + rect.width() * frac, rect.bottom()),
            ),
            egui::Rounding::ZERO,
            PROFILE_COLOR.gamma_multiply(CLUSTER_TOTAL_SILHOUETTE_ALPHA),
        );
        if bevel {
            paint_bevel(painter, rect, geometry.row_height, 0, pixels_per_point);
        }
        if frame.config.show_numbers {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                fmt_qty(volume),
                font,
                theme::TEXT_PRIMARY,
            );
        }
    }

    if geometry.is_poc {
        // A ring, with the casing under-stroke the ramp makes mandatory:
        // POC yellow over the ramp's brightest step is 1.1:1, and a signal
        // that vanishes on the busiest rows is worse than no signal.
        let ring = egui::Rect::from_min_max(
            egui::pos2(xc - reach + inset, geometry.top + 0.5),
            egui::pos2(xc + reach, geometry.bottom - 0.5),
        );
        painter.rect_stroke(
            ring,
            egui::Rounding::ZERO,
            egui::Stroke::new(1.5 + theme::CASING_EXTRA_PX, theme::CASING),
        );
        painter.rect_stroke(
            ring,
            egui::Rounding::ZERO,
            egui::Stroke::new(1.5_f32, theme::POC),
        );
    }
}

/// Full-candle POC line, the non-split styles' and Marks level's shape.
pub(super) fn draw_poc_dot(frame: &LayerFrame<'_>, xc: f32, row: i64, row_group: f64) {
    draw_poc_line(frame, xc - frame.half, xc + frame.half, row, row_group);
}

pub(super) fn draw_zone_mark(frame: &LayerFrame<'_>, mark: &ZoneMark, row_group: f64) {
    // Composed from both rows' screen bands: upside down the high bucket
    // renders below the low one, and reading "top" off it would hand both
    // rects a negative height (see the Split backdrop above).
    let (high_top, high_bottom) = row_band(frame, mark.high_bucket, row_group);
    let (low_top, low_bottom) = row_band(frame, mark.low_bucket, row_group);
    let top = high_top.min(low_top);
    let bottom = high_bottom.max(low_bottom);
    let left = (frame.x_center)(mark.first_slot) - frame.half;
    let right = (frame.x_center)(mark.last_slot) + frame.half;
    // A zone marks the bars that formed it with a wash, and closes on a
    // firmer band at the right — the side that dominated, one bar past the
    // last one that re-formed it.
    //
    // It does *not* yet outlive those bars. A zone's trading value is memory
    // — a level to watch for a retest — and memory needs a life after its
    // origin plus a rule for when it dies (a stacked imbalance dies on a
    // print through the far edge; absorption dies on a *close* through it,
    // because a wick that pierces and returns is the defender holding). That
    // is a level-memory of its own, shared with naked POCs and absorption,
    // and it is not this change.
    let color = theme::side_color(mark.side);
    frame.painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom)),
        egui::Rounding::ZERO,
        color.gamma_multiply(0.10),
    );
    let edge_x = right + 1.0;
    frame.painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(edge_x, top), egui::pos2(edge_x + 2.0, bottom)),
        egui::Rounding::ZERO,
        color.gamma_multiply(0.7),
    );
}

/// How far an imbalanced cell sinks *below* its plate.
///
/// Below, never above: a light pill under text of the cell's own hue raises
/// the floor exactly beneath the digits it means to emphasise. Measured, the
/// old 0.35 pill left its number at 3.2:1 — the layer's most important row as
/// its least legible one. Sinking the cell and lightening the ink puts the
/// same row at 8.6:1.
pub(super) const IMBALANCE_CELL_ALPHA: f32 = 0.16;

/// Width of the solid edge on the dominant column's outer border, in pixels.
/// The side is carried by *which* border it is, so the colour is redundancy.
const IMBALANCE_EDGE_PX: f32 = 2.0;

/// The split style's volume-profile silhouette: neutral light, after the
/// reference charts' white/gray histograms — color stays reserved for the
/// fight (the delta side) and the POC.
pub(super) const PROFILE_COLOR: egui::Color32 = egui::Color32::from_gray(0xD8);

/// The least an imbalance chip spans, in pixels: enough to ring the number
/// on a short bar without swallowing the whole half.
const MIN_CHIP_PX: f32 = 14.0;

/// Gap between an extreme-ratio badge and the row it describes, in pixels —
/// just off the bar's end, never on the ladder itself.
const EXTREME_BADGE_GAP_PX: f32 = 3.0;

/// How much of the canvas the split style's per-bar backdrop keeps: enough
/// that the footprint owns its interior over the heatmap, little enough
/// that the map stays visible between candles.
const BACKDROP_ALPHA: f32 = 0.65;

/// That backdrop, derived from the theme rather than hand-premultiplied —
/// a canvas color copied by hand goes stale the day the theme moves, with
/// no test to notice.
fn canvas_backdrop() -> egui::Color32 {
    theme::CANVAS.gamma_multiply(BACKDROP_ALPHA)
}

/// Pixel band of display row `row` (rows are `row_group` of price tall).
///
/// Ordered on screen, not by price: on an inverted scale the row's high edge
/// is the *lower* pixel, and a band handed out as `(high_edge, low_edge)`
/// would give every rect a negative height.
fn row_band(frame: &LayerFrame<'_>, row: i64, row_group: f64) -> (f32, f32) {
    let low = row as f64 * row_group;
    frame.scale.band(low, low + row_group)
}
