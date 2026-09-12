//! The deterministic sample the settings panel shows: fixed synthetic
//! geometry drawn with the same painter vocabulary as the live chart, so a
//! slider can be tuned before a live snapshot exists.
//!
//! The two sample prints go through [`super::bubbles::draw_bubble`], never
//! an approximation of it — a preview that drew its own picture would send
//! the trader tuning against something the chart never produces.

use eframe::egui;
use quantick_engine::Side;
use quantick_orderbook::BookSide;
use quantick_orderflow::{BubbleStyle, HeatmapConfig, HeatmapTheme};

use super::bubbles::{
    BubbleColors, BubbleMark, bubble_radius, draw_bubble, front_half_length, trail_rect,
};
use super::layout::EventBand;
use super::{
    OrderflowRenderStyle, Palette, add_gradient_rect, finite_unit, resting_rgb, rgba, thermal_rgb,
};

/// Normalized sizes of the two sample prints in the settings preview: one
/// near full size and one routine print, so the radius range is visible.
pub(super) const PREVIEW_LARGE_PRINT_SIZE: f32 = 0.85;

/// See [`PREVIEW_LARGE_PRINT_SIZE`].
const PREVIEW_SMALL_PRINT_SIZE: f32 = 0.45;

/// Matched fraction of the preview's consuming print. Mid-range, so the
/// impact ring shows neither its floor nor its ceiling.
pub(super) const PREVIEW_MATCHED_FRACTION: f32 = 0.6;

/// Buy share of the preview's summarized print. Lopsided rather than even, so
/// the two sectors are visibly unequal and the mark reads as a proportion.
const PREVIEW_SUMMARY_BUY_SHARE: f32 = 0.62;

/// Deterministic visual sample used by the settings panel and screenshot tests.
///
/// It intentionally does not construct market-domain records. The preview
/// demonstrates the exact painter vocabulary with fixed synthetic geometry:
/// persistent walls, one aggression-aligned bite and one unattributed L2
/// reduction. It therefore works before a live snapshot is available.
pub(crate) fn draw_preview(ui: &mut egui::Ui, config: &HeatmapConfig) -> egui::Response {
    let width = ui.available_width().clamp(240.0, 680.0);
    let desired = egui::vec2(width, 196.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let style =
        OrderflowRenderStyle::from_config(config, egui::Color32::from_rgb(19, 23, 34)).sanitized();
    let palette = Palette::for_theme(style.theme);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, egui::Rounding::same(4.0), style.canvas_background);
    painter.rect_stroke(
        rect,
        egui::Rounding::same(4.0),
        egui::Stroke::new(0.75_f32, palette.legend_border),
    );

    let title = egui::pos2(rect.left() + 9.0, rect.top() + 7.0);
    painter.text(
        title,
        egui::Align2::LEFT_TOP,
        "synthetic order-flow preview",
        egui::FontId::proportional(10.0),
        palette.muted_text,
    );
    let chart = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(8.0, 24.0),
        rect.right_bottom() - egui::vec2(8.0, if config.show_legend { 30.0 } else { 8.0 }),
    );

    for step in 1..5 {
        let y = egui::lerp(chart.top()..=chart.bottom(), step as f32 / 5.0);
        painter.line_segment(
            [egui::pos2(chart.left(), y), egui::pos2(chart.right(), y)],
            egui::Stroke::new(0.5_f32, egui::Color32::from_white_alpha(16)),
        );
    }

    // Each tuple is `(y, height, x0, x1, intensity, side)`. Segment boundaries
    // make additions and reductions visible without animation or live data.
    let segments = [
        (0.16, 0.034, 0.00, 0.48, 0.34, BookSide::Ask),
        (0.16, 0.034, 0.48, 0.76, 0.73, BookSide::Ask),
        (0.27, 0.042, 0.00, 0.58, 0.92, BookSide::Ask),
        (0.27, 0.042, 0.58, 0.98, 0.40, BookSide::Ask),
        (0.39, 0.030, 0.08, 0.88, 0.50, BookSide::Ask),
        (0.61, 0.032, 0.00, 0.44, 0.36, BookSide::Bid),
        (0.61, 0.032, 0.44, 1.00, 0.66, BookSide::Bid),
        (0.72, 0.045, 0.00, 0.43, 0.88, BookSide::Bid),
        (0.72, 0.045, 0.43, 0.82, 0.24, BookSide::Bid),
        (0.84, 0.032, 0.10, 1.00, 0.54, BookSide::Bid),
    ];
    let mut heat_mesh = egui::Mesh::default();
    for (y, height, x0, x1, intensity, side) in segments {
        let band = normalized_rect(chart, x0, x1, y - height / 2.0, y + height / 2.0);
        let rgb = resting_rgb(style.theme, side, intensity);
        let alpha = config.opacity.clamp(0.0, 1.0) * 0.94;
        let glow = egui::Rect::from_min_max(
            egui::pos2(band.left(), band.top() - 0.7),
            egui::pos2(band.right(), band.bottom() + 0.7),
        );
        add_gradient_rect(
            &mut heat_mesh,
            glow,
            rgba(rgb, alpha * style.edge_glow),
            rgba(rgb, alpha * style.edge_glow),
        );
        // Solid fill, matching the live heatmap's crisp bands.
        add_gradient_rect(&mut heat_mesh, band, rgba(rgb, alpha), rgba(rgb, alpha));
    }
    painter.add(egui::Shape::mesh(heat_mesh));

    // A subdued price path gives the liquidity/trade interaction context while
    // keeping the preview focused on the order-flow layers.
    let price_points = [
        (0.00, 0.59),
        (0.15, 0.57),
        (0.29, 0.63),
        (0.43, 0.68),
        (0.56, 0.53),
        (0.66, 0.29),
        (0.79, 0.36),
        (1.00, 0.25),
    ]
    .into_iter()
    .map(|(x, y)| {
        egui::pos2(
            egui::lerp(chart.left()..=chart.right(), x),
            egui::lerp(chart.top()..=chart.bottom(), y),
        )
    })
    .collect();
    painter.add(egui::Shape::line(
        price_points,
        egui::Stroke::new(1.1_f32, egui::Color32::from_white_alpha(145)),
    ));

    if config.show_aligned_depletion {
        // Aggression-aligned consumption front with a glow leaking into the
        // consumed side.
        let aligned = EventBand {
            x: egui::lerp(chart.left()..=chart.right(), 0.58),
            top: egui::lerp(chart.top()..=chart.bottom(), 0.27 - 0.042 / 2.0),
            bottom: egui::lerp(chart.top()..=chart.bottom(), 0.27 + 0.042 / 2.0),
        };
        let mut front_mesh = egui::Mesh::default();
        add_gradient_rect(
            &mut front_mesh,
            egui::Rect::from_min_max(
                egui::pos2(aligned.x, aligned.top),
                egui::pos2((aligned.x + 14.0).min(chart.right()), aligned.bottom),
            ),
            palette.consumption.gamma_multiply(0.24),
            egui::Color32::TRANSPARENT,
        );
        painter.add(egui::Shape::mesh(front_mesh));
        painter.line_segment(
            [
                egui::pos2(aligned.x, aligned.top - 2.0),
                egui::pos2(aligned.x, aligned.bottom + 2.0),
            ],
            egui::Stroke::new(1.6_f32, palette.consumption),
        );
    }

    if config.show_unattributed_reductions {
        // Depth-only withdrawal: a calm violet fade with a thin cap.
        let depth_only = EventBand {
            x: egui::lerp(chart.left()..=chart.right(), 0.76),
            top: egui::lerp(chart.top()..=chart.bottom(), 0.16 - 0.034 / 2.0),
            bottom: egui::lerp(chart.top()..=chart.bottom(), 0.16 + 0.034 / 2.0),
        };
        let mut ghost_mesh = egui::Mesh::default();
        add_gradient_rect(
            &mut ghost_mesh,
            egui::Rect::from_min_max(
                egui::pos2(depth_only.x, depth_only.top),
                egui::pos2((depth_only.x + 20.0).min(chart.right()), depth_only.bottom),
            ),
            palette.depth_only.gamma_multiply(0.42),
            egui::Color32::TRANSPARENT,
        );
        painter.add(egui::Shape::mesh(ghost_mesh));
        painter.line_segment(
            [
                egui::pos2(depth_only.x, depth_only.top - 1.0),
                egui::pos2(depth_only.x, depth_only.bottom + 1.0),
            ],
            egui::Stroke::new(1.4_f32, palette.depth_only),
        );
    }

    if config.show_aggressions {
        let bubbles = &style.bubbles;
        let colors = BubbleColors::resolve(&palette, bubbles);
        // Two prints at fixed normalized sizes, so every slider (radius range,
        // opacity, rim, front, trail, side offset) shows its effect here.
        draw_preview_bubble(
            &painter,
            PreviewBubble {
                center: egui::pos2(
                    egui::lerp(chart.left()..=chart.right(), 0.58),
                    egui::lerp(chart.top()..=chart.bottom(), 0.27),
                ),
                size: PREVIEW_LARGE_PRINT_SIZE,
                side: Side::Buy,
                linked_reduction: config.show_aligned_depletion,
                // With the closed-bar summary on, the large sample is what a
                // summarized bar actually produces: one mark, both sides.
                buy_share: if config.bubble_candle_summary {
                    PREVIEW_SUMMARY_BUY_SHARE
                } else {
                    1.0
                },
            },
            chart.right(),
            bubbles,
            &colors,
        );
        draw_preview_bubble(
            &painter,
            PreviewBubble {
                center: egui::pos2(
                    egui::lerp(chart.left()..=chart.right(), 0.43),
                    egui::lerp(chart.top()..=chart.bottom(), 0.72),
                ),
                size: PREVIEW_SMALL_PRINT_SIZE,
                side: Side::Sell,
                linked_reduction: false,
                buy_share: 0.0,
            },
            chart.right(),
            bubbles,
            &colors,
        );
    }

    if config.show_legend {
        draw_preview_legend(&painter, rect, &palette, style.theme);
    }
    response.on_hover_text(
        "Deterministic visual sample: green/red dots are confirmed trades; \
         a bright bite is aggression-aligned depletion; violet is an \
         unattributed L2 reduction.",
    )
}

fn normalized_rect(bounds: egui::Rect, x0: f32, x1: f32, y0: f32, y1: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            egui::lerp(bounds.left()..=bounds.right(), finite_unit(x0)),
            egui::lerp(bounds.top()..=bounds.bottom(), finite_unit(y0)),
        ),
        egui::pos2(
            egui::lerp(bounds.left()..=bounds.right(), finite_unit(x1)),
            egui::lerp(bounds.top()..=bounds.bottom(), finite_unit(y1)),
        ),
    )
}

/// One sample print in the settings preview.
#[derive(Debug, Clone, Copy)]
pub(super) struct PreviewBubble {
    pub(super) center: egui::Pos2,
    /// Normalized print size, as the projection would report it.
    pub(super) size: f32,
    pub(super) side: Side,
    /// Whether this sample ate resting liquidity, so it shows the marks.
    pub(super) linked_reduction: bool,
    /// Buy share of the sample, so a preview of the closed-bar summary shows
    /// the pie the chart would actually draw.
    pub(super) buy_share: f32,
}

/// Draw one preview print exactly the way the chart would draw it.
///
/// Everything past the trail goes through [`draw_bubble`], the same function
/// the live chart uses: a preview that renders its own approximation would
/// send the user tuning sliders against a picture the chart never produces.
pub(super) fn draw_preview_bubble(
    painter: &egui::Painter,
    preview: PreviewBubble,
    right_edge: f32,
    bubbles: &BubbleStyle,
    colors: &BubbleColors,
) {
    let PreviewBubble {
        center,
        size,
        side,
        linked_reduction,
        buy_share,
    } = preview;
    let lean = (finite_unit(buy_share) - 0.5) * 2.0;
    let center = center + egui::vec2(0.0, -lean * bubbles.side_offset);
    let radius = bubble_radius(size, bubbles.min_radius, bubbles.max_radius);
    if linked_reduction && bubbles.trail_length > 0.0 {
        // Consumption trail behind the bubble (drawn first so the fill sits on
        // top). On the chart this is batched across every bubble; here there
        // are two, so one mesh each costs nothing.
        let mut mesh = egui::Mesh::default();
        add_gradient_rect(
            &mut mesh,
            trail_rect(
                center,
                front_half_length(radius, bubbles),
                bubbles.trail_length,
                right_edge,
            ),
            colors.trail.gamma_multiply(bubbles.trail_opacity),
            egui::Color32::TRANSPARENT,
        );
        painter.add(egui::Shape::mesh(mesh));
    }
    draw_bubble(
        painter,
        BubbleMark {
            center,
            radius,
            side,
            size,
            matched: linked_reduction.then_some(PREVIEW_MATCHED_FRACTION),
            buy_share,
            folded: 0,
        },
        bubbles,
        colors,
    );
}

fn draw_preview_legend(
    painter: &egui::Painter,
    bounds: egui::Rect,
    palette: &Palette,
    theme: HeatmapTheme,
) {
    let baseline = bounds.bottom() - 13.0;
    let mut x = bounds.left() + 10.0;
    let font = egui::FontId::proportional(9.5);

    let heat_rect = egui::Rect::from_min_size(egui::pos2(x, baseline - 4.0), egui::vec2(34.0, 7.0));
    let mut mesh = egui::Mesh::default();
    for index in 0..8 {
        let t0 = index as f32 / 8.0;
        let t1 = (index + 1) as f32 / 8.0;
        add_gradient_rect(
            &mut mesh,
            egui::Rect::from_min_max(
                egui::pos2(
                    egui::lerp(heat_rect.left()..=heat_rect.right(), t0),
                    heat_rect.top(),
                ),
                egui::pos2(
                    egui::lerp(heat_rect.left()..=heat_rect.right(), t1),
                    heat_rect.bottom(),
                ),
            ),
            rgba(thermal_rgb(theme, t0), 1.0),
            rgba(thermal_rgb(theme, t1), 1.0),
        );
    }
    painter.add(egui::Shape::mesh(mesh));
    x += 39.0;
    painter.text(
        egui::pos2(x, baseline),
        egui::Align2::LEFT_CENTER,
        "liquidity",
        font.clone(),
        palette.legend_text,
    );
    x += 54.0;

    for (color, label) in [(palette.buy, "buy"), (palette.sell, "sell")] {
        painter.circle_filled(
            egui::pos2(x + 4.0, baseline),
            4.0,
            color.gamma_multiply(0.82),
        );
        painter.circle_stroke(
            egui::pos2(x + 4.0, baseline),
            4.0,
            egui::Stroke::new(0.75_f32, color),
        );
        painter.text(
            egui::pos2(x + 11.0, baseline),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            palette.legend_text,
        );
        x += 41.0;
    }

    // On narrow settings windows the hover text remains the complete legend.
    if x + 200.0 > bounds.right() {
        return;
    }
    painter.line_segment(
        [
            egui::pos2(x + 3.0, baseline - 5.0),
            egui::pos2(x + 3.0, baseline + 5.0),
        ],
        egui::Stroke::new(1.3_f32, palette.consumption),
    );
    painter.text(
        egui::pos2(x + 9.0, baseline),
        egui::Align2::LEFT_CENTER,
        "aligned depletion",
        font.clone(),
        palette.legend_text,
    );
    x += 105.0;
    painter.line_segment(
        [
            egui::pos2(x + 3.0, baseline - 5.0),
            egui::pos2(x + 3.0, baseline + 5.0),
        ],
        egui::Stroke::new(1.3_f32, palette.depth_only),
    );
    painter.text(
        egui::pos2(x + 9.0, baseline),
        egui::Align2::LEFT_CENTER,
        "unattributed L2 reduction",
        font,
        palette.legend_text,
    );
}
