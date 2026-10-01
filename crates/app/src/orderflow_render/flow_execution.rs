//! Compact history execution groups on the FLOW candle coordinate system.
use super::bubbles::{BubbleColors, BubbleMark, draw_centered_bubble};
use eframe::egui;
use quantick_orderflow::{
    BubbleRenderMode, HeatmapConfig,
    projection::{
        PriceWindow,
        flow_tape::{FlowProgress, FlowScaleBasis, FlowTapeDot, FlowTapeFrame},
    },
};
use rust_decimal::Decimal;

// Opaque backing hides the base candles; full colour restores regional contrast.
const FLOW_FILL_OPACITY: f32 = 1.0;
// Quarter-reference regions show both sides under the seven-pixel radius cap.
const FLOW_PIE_MIN_RADIUS_PX: f32 = 3.5;
// A proportional inset preserves coloured area ratios between regions.
const FLOW_INNER_RIM_RADIUS_FRACTION: f32 = 0.125;

pub(crate) fn draw_flow_executions(
    painter: &egui::Painter,
    rect: egui::Rect,
    lane_width: f32,
    frame: &FlowTapeFrame,
    background: egui::Color32,
    config: &HeatmapConfig,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) {
    let history = rect.with_max_x(rect.right() - lane_width);
    let clip = painter.with_clip_rect(history);
    let mut bubbles = config.bubbles.clone();
    bubbles.opacity = FLOW_FILL_OPACITY;
    // Uniform colour makes regional area comparable without sphere lighting.
    bubbles.render_mode = BubbleRenderMode::Flat;
    bubbles.max_radius = frame.view.radius_limit;
    bubbles.halo_strength = 0.0;
    bubbles.outline_width = 0.0;
    bubbles.detail_min_radius = bubbles.detail_min_radius.min(FLOW_PIE_MIN_RADIUS_PX);
    bubbles.readable_min_radius = bubbles.readable_min_radius.min(FLOW_PIE_MIN_RADIUS_PX);
    let palette = super::palette_for_theme(config.theme);
    let colors = BubbleColors::resolve(&palette, &bubbles);
    for dot in &frame.dots {
        let Some(center) = center(dot) else {
            continue;
        };
        clip.circle_filled(center, dot.radius, background.to_opaque());
        draw_centered_bubble(
            &clip,
            BubbleMark {
                center,
                radius: dot.radius,
                side: dot.mark.side,
                size: dot.mark.size,
                matched: None,
                buy_share: dot.mark.buy_share,
                folded: 0,
            },
            &bubbles,
            &colors,
        );
        clip.add(inner_separator(center, dot.radius, background));
    }
}

fn inner_separator(center: egui::Pos2, radius: f32, background: egui::Color32) -> egui::Shape {
    let width = radius * FLOW_INNER_RIM_RADIUS_FRACTION;
    let neutral =
        ((u16::from(background.r()) + u16::from(background.g()) + u16::from(background.b())) / 3)
            as u8;
    // Stroke is centred on the inset path: its outside stays at the factual radius.
    egui::Shape::circle_stroke(
        center,
        radius - width * 0.5,
        egui::Stroke::new(width, egui::Color32::from_gray(neutral)),
    )
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
        hints.push("Area relative to visible regions".to_owned());
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
