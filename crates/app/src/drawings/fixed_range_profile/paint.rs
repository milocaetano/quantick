//! Painting the profile: one per-frame [`ProfileFrame`] measured once, then
//! the passes that read it — edges, histogram, silhouette, levels, labels.

use eframe::egui;
use quantick_anchored_studies::{FrvpEmpty, ProfileOutput};
use quantick_engine::{ValueArea, VolumeProfile};
use rust_decimal::Decimal;

use super::geometry::{self, ProfileGeometry};
use super::{
    FrvpCache, FrvpPayload, LABEL_OFFSET_PX, OUTLINE_IN_VA_PX, OUTLINE_OUT_VA_BRIGHTNESS,
    OUTLINE_OUT_VA_PX, ROW_ALPHA_IN_VA, ROW_ALPHA_OUT_VA, VA_DASH_PX, VA_GAP_PX, knockout_text,
    knockout_text_within, price_extent, range_edges, status_line,
};
use crate::chart::to_f64;
use crate::drawings::{DrawContext, DrawingStyle, drawing_stroke};
use crate::theme;
use crate::theme::{CASING, CASING_EXTRA_PX};

/// Which side of the candles one pass of the profile paints on.
///
/// A volume profile is two different kinds of thing wearing one name. Its
/// histogram is *context* — the shape the price is read against, like the
/// liquidity map — and painted over the candles it tints every body it
/// covers. Its edges, value-area lines, POC and status line are *annotation*,
/// and buried under the price they would simply be lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrvpPass {
    /// Before the candles: the histogram.
    Under,
    /// After them: everything else.
    Over,
}

/// One straight piece of the silhouette, with the value-area membership that
/// picks its ink weight. Collected first and stroked in two passes — every
/// casing under every ink — so a corner never has a later casing overpainting
/// an earlier ink.
struct SilhouetteSegment {
    from: egui::Pos2,
    to: egui::Pos2,
    in_va: bool,
}

/// Both halves of the object, from one function.
///
/// The two passes share every measurement — the range's edges, the price
/// extent, the level cap, the value-area test, the row geometry, the
/// fill/silhouette cut — so they share the [`ProfileFrame`] that holds them.
/// Split into two functions measuring on their own this would have copied
/// thirty lines of arithmetic that must agree exactly, and the day they
/// stopped agreeing the histogram would sit a pixel off the outline drawn
/// over it.
pub(super) fn paint_body(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    style: DrawingStyle,
    points: &[egui::Pos2],
    ctxt: &DrawContext<'_>,
    pass: FrvpPass,
) {
    let Some(payload) = ctxt.payload.as_any().downcast_ref::<FrvpPayload>() else {
        return;
    };
    let stroke = drawing_stroke(style);
    if points.len() < 2 {
        if pass == FrvpPass::Under {
            return;
        }
        // A one-anchor draft with no hover yet: mark the starting edge.
        if let Some(point) = points.first() {
            painter.line_segment(
                [
                    egui::pos2(point.x, chart_rect.top()),
                    egui::pos2(point.x, chart_rect.bottom()),
                ],
                stroke,
            );
        }
        return;
    }
    let frame = ProfileFrame::new(painter, chart_rect, style, points, ctxt, payload);

    // The range's edges — the stroke geometry, which is all the halo
    // pass paints. Over the candles: an edge is where the object *ends*,
    // and a boundary buried under the price is one the trader cannot
    // follow.
    if pass == FrvpPass::Over {
        frame.paint_edges(stroke);
    }
    if ctxt.halo {
        return;
    }

    let cache = payload.cache.as_ref().map(FrvpCache::output);
    let profile = cache.and_then(|cache| cache.profile);

    if let Some((profile, value_area)) = profile {
        let geometry = ProfileGeometry::new(profile, *value_area, payload, points, ctxt);
        match pass {
            FrvpPass::Under => frame.paint_histogram(&geometry, *value_area),
            FrvpPass::Over => {
                if frame.outline_active {
                    frame.paint_silhouette(&geometry, *value_area);
                }
                if let Some(area) = value_area {
                    frame.paint_levels(profile, *area);
                }
            }
        }
    }

    // Everything from here down is words and plates: the status line, the
    // POC/VAH/VAL prices, the handles. All annotation, none of it legible
    // under a candle.
    if pass == FrvpPass::Under || !ctxt.primary_band || !payload.show_labels {
        return;
    }
    frame.paint_labels(profile, cache);
}

/// Everything one frame of the profile measures once and every pass reads:
/// the range's screen edges and price extent, and where the fill gives way
/// to the silhouette.
struct ProfileFrame<'a> {
    painter: &'a egui::Painter,
    chart_rect: egui::Rect,
    style: DrawingStyle,
    ctxt: &'a DrawContext<'a>,
    payload: &'a FrvpPayload,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    /// Where the fill gives way to the silhouette: the left boundary of the
    /// liquidity map, expressed in this object's own coordinates. Left of
    /// the cut the profile composes over candles exactly as before; right of
    /// it a fill would compose into the map's cells (worst case measured at
    /// 1.002:1 contrast) — so the shape is drawn instead, and not one
    /// uncovered pixel of the map is altered.
    cut_x: Option<f32>,
    /// Whether the cut falls inside the range, so part of it is outlined.
    outline_active: bool,
}

impl<'a> ProfileFrame<'a> {
    fn new(
        painter: &'a egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &'a DrawContext<'a>,
        payload: &'a FrvpPayload,
    ) -> Self {
        let (left, right) = range_edges(payload, points, ctxt);
        let (top, bottom) = price_extent(payload, points, ctxt);
        let cut_x = geometry::silhouette_cut(payload, points, ctxt);
        Self {
            painter,
            chart_rect,
            style,
            ctxt,
            payload,
            left,
            right,
            top,
            bottom,
            cut_x,
            outline_active: cut_x.is_some_and(|x| x < right),
        }
    }

    /// Whether `bucket` paints with the value area's weight.
    fn in_va(&self, value_area: Option<ValueArea>, bucket: i64) -> bool {
        self.payload.show_value_area
            && value_area.is_some_and(|area| bucket >= area.val && bucket <= area.vah)
    }

    fn paint_edges(&self, stroke: egui::Stroke) {
        for x in [self.left, self.right] {
            self.painter.line_segment(
                [egui::pos2(x, self.top), egui::pos2(x, self.bottom)],
                stroke,
            );
        }
    }

    /// The histogram itself — the one part of this object that is *context*
    /// rather than annotation, and the reason the tool takes the
    /// under-candles pass at all.
    fn paint_histogram(&self, geometry: &ProfileGeometry<'_>, value_area: Option<ValueArea>) {
        let (painter, left) = (self.painter, self.left);
        let fill_limit = if self.outline_active {
            self.cut_x
        } else {
            None
        };
        for row in geometry.rows(self.chart_rect) {
            let (row_top, row_bottom) = (row.base.min(row.far), row.base.max(row.far));
            let bucket = row.bucket;
            let level = row.level;
            let tip = row.tip;
            let width = tip - left;
            // The fill stops at the map's boundary; the silhouette pass
            // carries the rest of the row.
            let fill_tip = fill_limit.map_or(tip, |cut| tip.min(cut));
            if fill_tip <= left {
                continue;
            }
            let alpha = if self.in_va(value_area, bucket) {
                ROW_ALPHA_IN_VA
            } else {
                ROW_ALPHA_OUT_VA
            };
            if self.payload.delta_coloring {
                // The row split by aggressor: buys from the edge, sells
                // continuing — the same quantities the footprint shows.
                let volume = to_f64(level.volume()).max(f64::MIN_POSITIVE);
                #[allow(clippy::cast_possible_truncation)]
                let buy_width = ((to_f64(level.buy) / volume) as f32) * width;
                let buy_tip = (left + buy_width).min(fill_tip);
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(left, row_top),
                        egui::pos2(buy_tip, row_bottom),
                    ),
                    egui::Rounding::ZERO,
                    theme::BUY.gamma_multiply(alpha),
                );
                if fill_tip > buy_tip {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(buy_tip, row_top),
                            egui::pos2(fill_tip, row_bottom),
                        ),
                        egui::Rounding::ZERO,
                        theme::SELL.gamma_multiply(alpha),
                    );
                }
            } else {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(left, row_top),
                        egui::pos2(fill_tip, row_bottom),
                    ),
                    egui::Rounding::ZERO,
                    self.style.color.gamma_multiply(alpha),
                );
            }
        }
    }

    /// The silhouette: the histogram's staircase envelope right of the cut,
    /// double-stroked — casing under ink, all casings first so a corner
    /// never has a later casing overpainting an earlier ink. The value area
    /// keeps its by-weight reading in the ink's width and brightness; rows
    /// short of the cut hug the boundary, so the filled and outlined halves
    /// read as one object.
    ///
    /// Over the candles, unlike the fill: it is a line, and a line is read
    /// as a shape rather than as a wash, so burying it would only lose it.
    fn paint_silhouette(&self, geometry: &ProfileGeometry<'_>, value_area: Option<ValueArea>) {
        let style = self.style;
        let mut segments: Vec<SilhouetteSegment> = Vec::new();
        geometry.visit_silhouette(self.chart_rect, |bucket, from, to| {
            segments.push(SilhouetteSegment {
                from,
                to,
                in_va: bucket.is_some_and(|bucket| self.in_va(value_area, bucket)),
            });
            false
        });
        let ink_width = |in_va: bool| {
            if in_va {
                style.width_px.max(OUTLINE_IN_VA_PX)
            } else {
                style.width_px.clamp(0.75, OUTLINE_OUT_VA_PX)
            }
        };
        for segment in &segments {
            self.painter.line_segment(
                [segment.from, segment.to],
                egui::Stroke::new(ink_width(segment.in_va) + CASING_EXTRA_PX, CASING),
            );
        }
        for segment in &segments {
            let color = if segment.in_va {
                style.color
            } else {
                style.color.gamma_multiply(OUTLINE_OUT_VA_BRIGHTNESS)
            };
            self.painter.line_segment(
                [segment.from, segment.to],
                egui::Stroke::new(ink_width(segment.in_va), color),
            );
        }
    }

    /// POC and the value-area bounds: levels, and a level is annotation — it
    /// is read *against* the price, so it goes over it.
    fn paint_levels(&self, profile: &VolumeProfile, area: ValueArea) {
        let (painter, style, left, right) = (self.painter, self.style, self.left, self.right);
        let scale = &self.ctxt.scale;
        if self.payload.show_poc {
            let y = scale.y(to_f64(
                profile
                    .bucket_price(area.poc)
                    .saturating_add(profile.group() / Decimal::TWO),
            ));
            let width = style.width_px.max(1.0);
            // The casing carries the POC over the map's yellow band —
            // #FFD54F against it is the worst number of the scene
            // (1.05:1); against the casing it is ~21:1. Over plain
            // canvas the casing is near-invisible, so it simply stays.
            painter.line_segment(
                [egui::pos2(left, y), egui::pos2(right, y)],
                egui::Stroke::new(width + CASING_EXTRA_PX, CASING),
            );
            painter.line_segment(
                [egui::pos2(left, y), egui::pos2(right, y)],
                egui::Stroke::new(width, theme::POC),
            );
        }
        if self.payload.show_value_area {
            // VAH tops its row, VAL bottoms its row: the dashes hug
            // the area they bound. Casing dashes share the geometry,
            // so the phase matches and the map shows through the gaps.
            let vah_y = scale.y(to_f64(profile.bucket_price(area.vah.saturating_add(1))));
            let val_y = scale.y(to_f64(profile.bucket_price(area.val)));
            let width = style.width_px.max(0.75);
            for y in [vah_y, val_y] {
                let ends = [egui::pos2(left, y), egui::pos2(right, y)];
                painter.add(egui::Shape::dashed_line(
                    &ends,
                    egui::Stroke::new(width + CASING_EXTRA_PX, CASING),
                    VA_DASH_PX,
                    VA_GAP_PX,
                ));
                painter.add(egui::Shape::dashed_line(
                    &ends,
                    egui::Stroke::new(width, style.color),
                    VA_DASH_PX,
                    VA_GAP_PX,
                ));
            }
        }
    }

    /// The status line under the range: what the profile is made of, in the
    /// footprint legend's language. Everything honesty demands lives here —
    /// coverage, effective rows, why the range is empty — plus the
    /// POC/VAH/VAL price plates at the range's right edge.
    fn paint_labels(
        &self,
        profile: Option<&(VolumeProfile, Option<ValueArea>)>,
        cache: Option<ProfileOutput<'_>>,
    ) {
        let payload = self.payload;
        let mut status = String::new();
        match (profile, cache) {
            (Some((profile, value_area)), Some(cache)) => {
                status.push_str(&status_line(profile, &cache, payload, self.outline_active));
                if let Some(area) = value_area {
                    // POC/VAH/VAL price plates at the right edge of the range.
                    let labels = [
                        ("POC", area.poc, theme::POC),
                        ("VAH", area.vah.saturating_add(1), theme::TEXT_MUTED),
                        ("VAL", area.val, theme::TEXT_MUTED),
                    ];
                    for (name, bucket, color) in labels {
                        if name != "POC" && !payload.show_value_area {
                            continue;
                        }
                        if name == "POC" && !payload.show_poc {
                            continue;
                        }
                        let price = profile.bucket_price(bucket);
                        knockout_text(
                            self.painter,
                            egui::pos2(
                                self.right + LABEL_OFFSET_PX,
                                self.ctxt.scale.y(to_f64(price)),
                            ),
                            egui::Align2::LEFT_CENTER,
                            &format!("{name} {price}"),
                            color,
                        );
                    }
                }
            }
            (None, Some(cache)) => status.push_str(&match (cache.folding, cache.empty) {
                // A fold that has not reached a bar with tape yet has nothing
                // to draw *yet* — which is not the same as a range with
                // nothing in it, and must not borrow that sentence.
                (true, _) => {
                    format!("loading {} of {} bars", cache.bars_folded, cache.bars_total)
                }
                (false, Some(FrvpEmpty::Blocked)) => "feed reports no traded volume".to_owned(),
                (false, _) => "no tape in range".to_owned(),
            }),
            // Not refreshed yet (first frame of a fresh object): say nothing
            // rather than guessing. A profile without a cache cannot exist —
            // the profile *lives in* the cache — but the tuple can't say so.
            (_, None) => {}
        }
        if !status.is_empty() {
            knockout_text_within(
                self.painter,
                egui::pos2(self.left, self.bottom + LABEL_OFFSET_PX),
                &status,
                theme::TEXT_MUTED,
                self.chart_rect,
                (self.top, self.bottom),
            );
        }
    }
}
