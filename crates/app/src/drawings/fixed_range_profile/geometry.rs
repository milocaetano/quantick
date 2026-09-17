//! Screen geometry shared by profile painting and pointer selection.
use super::*;
use crate::drawings::distance_to_segment;

/// Keep the profile precise even when the general drawing selector is generous.
pub(super) const HIT_SLOP_PX: f32 = 3.0;

pub(super) struct ProfileGeometry<'a> {
    profile: &'a VolumeProfile,
    scale: &'a crate::chart::PriceScale,
    pub left: f32,
    pub cut: Option<f32>,
    height: f32,
    max_volume: f64,
    width: f32,
}

pub(super) struct Row<'a> {
    pub bucket: i64,
    pub level: &'a quantick_engine::FootprintLevel,
    pub base: f32,
    pub far: f32,
    pub tip: f32,
}

impl ProfileGeometry<'_> {
    pub fn new<'a>(
        profile: &'a VolumeProfile,
        area: Option<ValueArea>,
        payload: &FrvpPayload,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'a>,
    ) -> ProfileGeometry<'a> {
        let (left, right) = range_edges(payload, points, ctxt);
        // The cached POC is the maximum-volume bucket even when its line is
        // hidden. Reuse it instead of scanning the ladder on every paint pass.
        let max_volume = area
            .and_then(|area| profile.levels().get(&area.poc))
            .map_or_else(
                || profile.max_level_volume(),
                quantick_engine::FootprintLevel::volume,
            );
        ProfileGeometry {
            profile,
            scale: ctxt.scale,
            left,
            cut: silhouette_cut(payload, points, ctxt).filter(|cut| *cut < right),
            height: row_height(profile, ctxt),
            max_volume: to_f64(max_volume).max(f64::MIN_POSITIVE),
            width: payload.width_frac * (right - left).max(1.0),
        }
    }

    pub fn rows(&self, clip: egui::Rect) -> impl Iterator<Item = Row<'_>> {
        // Include the full painted height: subpixel buckets occupy one pixel,
        // so several nearby buckets can overlap the same pointer coordinate.
        let group = to_f64(self.profile.group());
        let a = self.scale.price_at(clip.top() - self.height) / group;
        let b = self.scale.price_at(clip.bottom() + self.height) / group;
        #[allow(clippy::cast_possible_truncation)]
        let bounds = if self.scale.px_per_price().abs() > f64::EPSILON {
            (a.min(b).floor() as i64).saturating_sub(1)..=(a.max(b).ceil() as i64).saturating_add(1)
        } else {
            i64::MIN..=i64::MAX
        };
        self.profile
            .levels()
            .range(bounds)
            .filter_map(move |(&bucket, level)| {
                let base = self.scale.y(to_f64(self.profile.bucket_price(bucket)));
                let far = if self.scale.is_inverted() {
                    base + self.height
                } else {
                    base - self.height
                };
                if base.max(far) < clip.top() || base.min(far) > clip.bottom() {
                    return None;
                }
                #[allow(clippy::cast_possible_truncation)]
                let tip =
                    self.left + (to_f64(level.volume()) / self.max_volume) as f32 * self.width;
                Some(Row {
                    bucket,
                    level,
                    base,
                    far,
                    tip,
                })
            })
    }

    /// Visit the same staircase that paint strokes, without allocating for hits.
    /// Returning true stops at the first matching segment.
    pub fn visit_silhouette(
        &self,
        clip: egui::Rect,
        mut visit: impl FnMut(Option<i64>, egui::Pos2, egui::Pos2) -> bool,
    ) -> bool {
        let Some(base) = self.cut else { return false };
        let mut previous: Option<(i64, f32, f32)> = None;
        for row in self.rows(clip) {
            let tip = row.tip.max(base);
            match previous {
                Some((bucket, previous_tip, far)) if bucket.checked_add(1) == Some(row.bucket) => {
                    if visit(
                        Some(row.bucket),
                        egui::pos2(previous_tip, far),
                        egui::pos2(tip, row.base),
                    ) {
                        return true;
                    }
                }
                other => {
                    if let Some((_, previous_tip, far)) = other
                        && visit(
                            Some(row.bucket),
                            egui::pos2(previous_tip, far),
                            egui::pos2(base, far),
                        )
                    {
                        return true;
                    }
                    if visit(
                        Some(row.bucket),
                        egui::pos2(base, row.base),
                        egui::pos2(tip, row.base),
                    ) {
                        return true;
                    }
                }
            }
            if visit(
                Some(row.bucket),
                egui::pos2(tip, row.base),
                egui::pos2(tip, row.far),
            ) {
                return true;
            }
            previous = Some((row.bucket, tip, row.far));
        }
        previous
            .is_some_and(|(_, tip, far)| visit(None, egui::pos2(tip, far), egui::pos2(base, far)))
    }
}

pub(super) fn silhouette_cut(
    payload: &FrvpPayload,
    points: &[egui::Pos2],
    ctxt: &DrawContext<'_>,
) -> Option<f32> {
    let (left, _) = range_edges(payload, points, ctxt);
    payload
        .outline_over_heatmap
        .then(|| {
            payload
                .cache
                .as_ref()
                .and(payload.heat_first_slot)
        })
        .flatten()
        .and_then(|slot| {
            #[allow(clippy::cast_precision_loss)]
            bar_x(points, ctxt.anchors, slot as f32)
        })
        .map(|x| x.max(left))
}

pub(super) fn hit_profile(
    chart: egui::Rect,
    points: &[egui::Pos2],
    position: egui::Pos2,
    radius_px: f32,
    ctxt: &DrawContext<'_>,
) -> bool {
    if points.len() < 2 || !chart.contains(position) {
        return false;
    }
    let Some(payload) = ctxt.payload.as_any().downcast_ref::<FrvpPayload>() else {
        return false;
    };
    let radius = radius_px.min(HIT_SLOP_PX);
    let (left, right) = range_edges(payload, points, ctxt);
    let (top, bottom) = price_extent(payload, points, ctxt);
    let near = |from, to| distance_to_segment(position, from, to) <= radius;
    if [left, right]
        .into_iter()
        .any(|x| near(egui::pos2(x, top), egui::pos2(x, bottom)))
    {
        return true;
    }
    let Some((profile, area)) = payload
        .cache
        .as_ref()
        .and_then(|cache| cache.output().profile)
    else {
        return false;
    };
    if let Some(area) = area {
        let horizontal = |price| {
            let y = ctxt.scale.y(to_f64(price));
            near(egui::pos2(left, y), egui::pos2(right, y))
        };
        if payload.show_poc
            && horizontal(
                profile
                    .bucket_price(area.poc)
                    .saturating_add(profile.group() / Decimal::TWO),
            )
        {
            return true;
        }
        if payload.show_value_area
            && [area.val, area.vah.saturating_add(1)]
                .into_iter()
                .any(|bucket| horizontal(profile.bucket_price(bucket)))
        {
            return true;
        }
    }
    let geometry = ProfileGeometry::new(profile, *area, payload, points, ctxt);
    // Only rows touching the pointer need volume projection. The level cap
    // bounds the ladder independently of session length.
    let pointer_band = egui::Rect::from_min_max(
        egui::pos2(chart.left(), position.y),
        egui::pos2(chart.right(), position.y),
    );
    if geometry.rows(pointer_band).any(|row| {
        let tip = geometry.cut.map_or(row.tip, |cut| row.tip.min(cut));
        tip > left && position.x >= left && position.x <= tip
    }) {
        return true;
    }
    geometry.visit_silhouette(chart, |_, from, to| near(from, to))
}
