//! Compact history execution groups on the FLOW candle coordinate system.
use super::bubbles::BubbleColors;
use eframe::egui;
use quantick_orderflow::{
    HeatmapConfig,
    projection::{
        PriceWindow,
        flow_tape::{FlowProgress, FlowScaleBasis, FlowTapeDot, FlowTapeFrame},
    },
};
use rust_decimal::Decimal;

// Separate both sides without implying another quantity or a third region.
const FLOW_SIDE_GAP_PX: f32 = 1.0;

#[derive(Clone, Copy)]
pub(super) struct FlowDisc {
    pub center: egui::Pos2,
    pub radius: f32,
}
impl FlowDisc {
    pub fn visible(self, history: egui::Rect) -> bool {
        let edge = egui::pos2(
            self.center.x.clamp(history.left(), history.right()),
            self.center.y.clamp(history.top(), history.bottom()),
        );
        self.center.distance_sq(edge) <= self.radius.powi(2)
    }
    pub fn hit_distance(self, history: egui::Rect, pointer: egui::Pos2) -> Option<f32> {
        let distance = self.center.distance_sq(pointer);
        (self.visible(history) && distance <= self.radius.max(6.0).powi(2)).then_some(distance)
    }
}

/// Sell then buy, with the combined envelope centred at the factual regional centroid.
pub(super) fn flow_discs(dot: &FlowTapeDot, center: egui::Pos2) -> [Option<FlowDisc>; 2] {
    if !center.is_finite() || !dot.radius.is_finite() || dot.radius <= 0.0 {
        return [None, None];
    }
    let (buy, sell) = dot.side_radii();
    let paired = buy > 0.0 && sell > 0.0;
    [
        (sell > 0.0).then_some(FlowDisc {
            center: center
                - egui::vec2(
                    if paired {
                        buy + FLOW_SIDE_GAP_PX * 0.5
                    } else {
                        0.0
                    },
                    0.0,
                ),
            radius: sell,
        }),
        (buy > 0.0).then_some(FlowDisc {
            center: center
                + egui::vec2(
                    if paired {
                        sell + FLOW_SIDE_GAP_PX * 0.5
                    } else {
                        0.0
                    },
                    0.0,
                ),
            radius: buy,
        }),
    ]
}

pub(crate) fn draw_flow_executions(
    painter: &egui::Painter,
    rect: egui::Rect,
    lane_width: f32,
    frame: &FlowTapeFrame,
    config: &HeatmapConfig,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) {
    let history = rect.with_max_x(rect.right() - lane_width);
    let clip = painter.with_clip_rect(history);
    let palette = super::palette_for_theme(config.theme);
    let colors = BubbleColors::resolve(&palette, &config.bubbles);
    for dot in &frame.dots {
        let Some(center) = center(dot) else {
            continue;
        };
        for (disc, color) in flow_discs(dot, center)
            .into_iter()
            .zip([colors.sell, colors.buy])
        {
            if let Some(disc) = disc.filter(|disc| disc.visible(history)) {
                clip.circle_filled(disc.center, disc.radius, color);
            }
        }
    }
}

pub(crate) fn current_price_y(
    prices: PriceWindow,
    price: Decimal,
    rect: egui::Rect,
    inverted: bool,
) -> f32 {
    let normalized = prices.y_unclamped(price).unwrap_or_default() as f32;
    rect.top()
        + rect.height()
            * if inverted {
                1.0 - normalized
            } else {
                normalized
            }
}
/// Regional chrome is docked after the native legend, including incomplete first frames.
pub(crate) fn flow_caption(
    painter: &egui::Painter,
    history: egui::Rect,
    top: f32,
    frame: Option<&FlowTapeFrame>,
    progress: FlowProgress,
    legend_visible: bool,
) -> Option<egui::Rect> {
    let mut hints = Vec::new();
    if legend_visible {
        hints.push("Sell / buy area on one scale".to_owned());
    }
    if progress.pending {
        hints.push(
            if frame.is_some_and(|f| f.cache_limit_reached) {
                "Partial regional volume"
            } else {
                "Regional volume updating"
            }
            .to_owned(),
        );
    }
    if let Some(frame) = frame {
        if frame.ineligible_executions > 0 {
            hints.push(format!("{} records excluded", frame.ineligible_executions));
        }
        if frame.opening_exclusion_effective {
            hints.push("First recorded burst excluded from scale".to_owned());
        } else if frame.scale_basis == FlowScaleBasis::OpeningOnlyFallback {
            hints.push("Opening-only scale fallback".to_owned());
        }
    }
    if hints.is_empty() {
        return None;
    }
    let galley = painter.layout(
        hints.join(" | "),
        egui::FontId::proportional(10.0),
        crate::theme::TEXT_MUTED,
        (history.width() - 16.0).max(1.0),
    );
    let rect = egui::Rect::from_min_size(egui::pos2(history.left() + 8.0, top), galley.size());
    painter
        .with_clip_rect(history)
        .galley(rect.min, galley, crate::theme::TEXT_MUTED);
    Some(rect)
}
#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::{Side, Trade};
    use quantick_orderflow::projection::flow_tape::{
        FlowExecution, FlowReference, FlowTapeView, project_flow_tape,
    };

    fn painted_region(buy: i64, sell: i64) -> (FlowTapeFrame, Vec<egui::Shape>) {
        let trades = [(buy, Side::Buy), (sell, Side::Sell)].map(|(quantity, side)| Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: 100.into(),
            quantity: quantity.into(),
            side,
        });
        let mut frame = project_flow_tape(
            trades
                .iter()
                .enumerate()
                .map(|(ordinal, trade)| FlowExecution {
                    ordinal,
                    slot: 0,
                    accepted_ordinal: ordinal,
                    ticks_per_bar: 2.into(),
                    trade,
                    opening: false,
                }),
            1,
            2,
            2,
            FlowTapeView {
                first_slot: 0,
                end_slot: 1,
                clip_left: 0.into(),
                clip_right: 1.into(),
                width_px: 1.0,
                height_px: 100.0,
                prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
                reference: FlowReference::Typed(9187.into()),
                radius_limit: 7.0,
                merge_support_radius: 12.0,
                exclude_opening: false,
            },
        );
        // Rendering must read exact quantities, not this approximate cached share.
        frame.dots[0].mark.buy_share = 0.0;
        let mut config = HeatmapConfig::default();
        config.bubbles.hollow_small_buys = true;
        config.bubbles.detail_min_radius = 32.0;
        config.bubbles.readable_min_radius = 32.0;
        config.bubbles.halo_strength = 1.0;
        config.bubbles.outline_width = 4.0;
        config.bubbles.opacity = 0.0;
        config.bubbles.buy_color = Some([0, 255, 0]);
        config.bubbles.sell_color = Some([255, 0, 0]);
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            draw_flow_executions(
                &ctx.layer_painter(egui::LayerId::background()),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0)),
                0.0,
                &frame,
                &config,
                |_| Some(egui::pos2(50.0, 50.0)),
            );
        });
        (
            frame,
            output.shapes.into_iter().map(|shape| shape.shape).collect(),
        )
    }

    #[test]
    fn small_flow_pairs_show_each_side_area_without_inherited_dressing() {
        let mut areas = Vec::new();
        for (buy, sell) in [
            (51, 49),
            (100, 0),
            (0, 100),
            (1, 99),
            (25, 25),
            (4, 96),
            (1150, 6938),
            (5114, 4073),
        ] {
            let (frame, shapes) = painted_region(buy, sell);
            let circles: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Circle(circle) => Some(circle),
                    _ => None,
                })
                .collect();
            assert_eq!(circles.len(), usize::from(buy > 0) + usize::from(sell > 0));
            assert_eq!(
                shapes.len(),
                circles.len(),
                "only factual side discs are painted"
            );
            let left = circles
                .iter()
                .map(|c| c.center.x - c.radius)
                .fold(f32::INFINITY, f32::min);
            let right = circles
                .iter()
                .map(|c| c.center.x + c.radius)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(((left + right) * 0.5 - 50.0).abs() < 0.00001);
            assert!(
                right - left
                    <= 2.0 * frame.dots[0].radius * 2.0_f32.sqrt() + FLOW_SIDE_GAP_PX + 0.00001
            );
            if buy > 0 && sell > 0 {
                assert_eq!(circles[0].fill, egui::Color32::RED);
                assert_eq!(circles[1].fill, egui::Color32::GREEN);
                let gap = circles[1].center.x
                    - circles[1].radius
                    - circles[0].center.x
                    - circles[0].radius;
                assert!((gap - FLOW_SIDE_GAP_PX).abs() < 0.00001);
            } else {
                assert_eq!(circles[0].center.x, 50.0);
            }
            let mut side_areas = [0.0; 2];
            for circle in circles {
                assert_eq!(circle.stroke, egui::Stroke::NONE);
                assert_eq!(circle.center.y, 50.0);
                let index = if circle.fill == egui::Color32::GREEN {
                    0
                } else {
                    assert_eq!(circle.fill, egui::Color32::RED);
                    1
                };
                side_areas[index] = circle.radius.powi(2);
            }
            assert!((side_areas[0] - buy as f32 * 49.0 / 9187.0).abs() < 0.00001);
            assert!((side_areas[1] - sell as f32 * 49.0 / 9187.0).abs() < 0.00001);
            assert!(
                (side_areas.iter().sum::<f32>() - frame.dots[0].radius.powi(2)).abs() < 0.00001
            );
            areas.push(side_areas);
        }
        assert!((areas[0][0] / areas[4][0] - 51.0 / 25.0).abs() < 0.00001);
        assert!((areas[0][1] / areas[4][1] - 49.0 / 25.0).abs() < 0.00001);
        assert!((areas[5][0] / areas[3][0] - 4.0).abs() < 0.00001);
        assert!((areas[6][1] / areas[7][1] - 6938.0 / 4073.0).abs() < 0.00001);
    }
    #[test]
    fn paired_tips_share_the_painter_and_hover_clip_boundary() {
        let (frame, _) = painted_region(4593, 4594);
        let history = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        for (x, pointer_x, side) in [(-8.0, 1.0, 1), (108.0, 99.0, 0)] {
            let discs = flow_discs(&frame.dots[0], egui::pos2(x, 50.0));
            let visible = discs[side].unwrap();
            let clipped = discs[1 - side].unwrap();
            assert!(
                visible.visible(history),
                "a tip extends beyond the old gross-radius clip"
            );
            assert!(
                visible
                    .hit_distance(history, egui::pos2(pointer_x, 50.0))
                    .is_some()
            );
            assert!(!clipped.visible(history));
            assert!(clipped.hit_distance(history, clipped.center).is_none());
        }
        for x in [-20.0, 120.0] {
            assert!(
                flow_discs(&frame.dots[0], egui::pos2(x, 50.0))
                    .into_iter()
                    .flatten()
                    .all(|disc| !disc.visible(history)
                        && disc.hit_distance(history, disc.center).is_none())
            );
        }
    }
    #[test]
    fn retained_flow_prices_follow_the_current_axis_while_layout_is_pending() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 10.0), egui::vec2(100.0, 200.0));
        let old = PriceWindow::new(90.into(), 110.into()).unwrap();
        let current = PriceWindow::new(90.into(), 130.into()).unwrap();
        assert_eq!(current_price_y(old, 100.into(), rect, false), 110.0);
        assert_eq!(current_price_y(current, 100.into(), rect, false), 160.0);
        assert_eq!(current_price_y(current, 100.into(), rect, true), 60.0);
    }
}
