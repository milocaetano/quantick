//! Compact history execution groups on the FLOW candle coordinate system.
use super::bubbles::{BubbleColors, SphereShading, add_sector};
use crate::orderflow_render::constants::{
    FLOW_BUY, FLOW_CAPTION_FONT_PX, FLOW_CAPTION_INSET_PX, FLOW_CAPTION_WRAP_MARGIN_PX, FLOW_SELL,
    PIE_START_ANGLE,
};
use eframe::egui;
#[cfg(test)]
pub(super) use quantick_chart::flow_execution::FlowDisc;
use quantick_chart::flow_execution::{FlowPresentation, FlowQuantityLabel, FlowRegionRole};
#[cfg(test)]
use quantick_orderflow::HeatmapConfig;
#[cfg(test)]
use quantick_orderflow::projection::flow_tape::FlowTapeDot;
use quantick_orderflow::projection::flow_tape::{FlowProgress, FlowTapeFrame};
#[cfg(test)]
use rust_decimal::Decimal;
use std::sync::Arc;

/// The clear margin around a sphere's quantity label that no other label
/// may enter.
const LABEL_CLEARANCE_PX: f32 = 2.0;

// Regional volume controls emphasis while candle contours stay in front.
#[cfg(test)]
const FLOW_FILL_OPACITY: f32 = quantick_orderflow::config::dressing::flow::PEAK_OPACITY;
#[cfg(test)]
const FLOW_OFFSET: egui::Vec2 = egui::vec2(
    quantick_chart::flow_execution::FLOW_EXECUTION_OFFSET[0],
    quantick_chart::flow_execution::FLOW_EXECUTION_OFFSET[1],
);

/// One factual gross-area disc, as paint places it.
#[cfg(test)]
pub(super) fn flow_disc(dot: &FlowTapeDot, center: egui::Pos2) -> Option<FlowDisc> {
    FlowDisc::new(dot, center.into())
}

#[cfg(test)]
fn draw_flow_executions(
    painter: &egui::Painter,
    rect: egui::Rect,
    lane_width: f32,
    frame: &FlowTapeFrame,
    _config: &HeatmapConfig,
    backing: Option<egui::Color32>,
    center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) {
    let history = rect.with_max_x(rect.right() - lane_width);
    painter
        .with_clip_rect(history)
        .add(egui::Shape::mesh(flow_mesh(
            history, frame, backing, center,
        )));
}

#[cfg(test)]
pub(super) fn flow_mesh(
    history: egui::Rect,
    frame: &FlowTapeFrame,
    backing: Option<egui::Color32>,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) -> egui::Mesh {
    let plan = FlowPresentation::new(frame, [history.min.into(), history.max.into()], |dot| {
        center(dot).map(Into::into)
    });
    let ctx = egui::Context::default();
    let painter = ctx.layer_painter(egui::LayerId::background());
    let background = backing.unwrap_or(crate::theme::CANVAS);
    let drawing = FlowDrawing::styled(frame, plan, background, backing, None, &painter, history);
    let mut mesh = drawing.meshes[0].clone();
    mesh.append(drawing.meshes[1].clone());
    mesh
}

/// A placed quantity label: where its galley goes, and the label.
type PlacedLabel = (egui::Pos2, FlowQuantityLabel);
/// One FLOW frame as every frame paints it, built once per cached drawing: each pass's
/// sectors with its rims tessellated in after them, as egui would, and placed labels.
pub(crate) struct FlowDrawing {
    pub plan: FlowPresentation,
    /// Exact presentation frame when the current camera resizes tick spheres.
    pub viewport_frame: Option<Arc<FlowTapeFrame>>,
    pub(super) meshes: [egui::Mesh; 2],
    labels: Vec<PlacedLabel>,
}
impl FlowDrawing {
    /// `painter` lends tessellation and fonts; `history` is the area painted in.
    pub fn styled(
        frame: &FlowTapeFrame,
        plan: FlowPresentation,
        background: egui::Color32,
        backing: Option<egui::Color32>,
        tick_look: Option<quantick_orderflow::TickLook>,
        painter: &egui::Painter,
        history: egui::Rect,
    ) -> Self {
        use quantick_orderflow::config::dressing::flow::{colors, ordinary_colors};
        let mut meshes = [egui::Mesh::default(), egui::Mesh::default()];
        let mut rims = [egui::Mesh::default(), egui::Mesh::default()];
        let ppp = painter.ctx().pixels_per_point();
        let options = painter.ctx().options(|o| o.tessellation_options);
        let mut tessellator = egui::epaint::Tessellator::new(ppp, options, [1, 1], Vec::new());
        tessellator.set_clip_rect(history.intersect(painter.clip_rect()));
        let tick = tick_look.map(|(tick, _)| tick);
        let tick_style = tick.map(quantick_orderflow::TickBubbleStyle::bubbles);
        let tick_colors = tick_look.map(|(tick, theme)| {
            BubbleColors::resolve(&super::palette_for_theme(theme), &tick.bubbles())
        });
        let mut labels = plan.quantity_labels(frame, tick);
        labels.sort_by(|a, b| b.radius.total_cmp(&a.radius));
        let colors = [
            colors(None, true, false),
            colors(Some(background.to_array()), false, true),
        ]
        .map(|palette| palette.map(super::premultiplied));
        for region in &plan.regions {
            let dot = &frame.dots[region.dot_index];
            let disc = region.disc;
            let mesh = &mut meshes[usize::from(region.role == FlowRegionRole::Peak)];
            let [buy_color, sell_color] = if let Some(colors) = tick_colors {
                [colors.buy, colors.sell]
            } else if region.role == FlowRegionRole::Peak {
                ordinary_colors(
                    backing.unwrap_or(background).to_array(),
                    frame.ordinary_region_opacity(dot),
                )
                .map(super::premultiplied)
            } else {
                colors[region.role as usize]
            };
            let (buy, sell) = dot.side_shares();
            if let Some(style) = &tick_style
                && disc.radius >= style.detail_min_radius
            {
                let hair = super::bubbles::separator_ring_width(disc.radius);
                tessellator.tessellate_shape(
                    egui::Shape::circle_stroke(
                        disc.center.into(),
                        disc.radius + hair / 2.0,
                        egui::Stroke::new(
                            hair,
                            egui::Color32::from_black_alpha(super::bubbles::SEPARATOR_RING_ALPHA),
                        ),
                    ),
                    &mut rims[usize::from(region.role == FlowRegionRole::Peak)],
                );
            }
            let mut angle = f64::from(PIE_START_ANGLE);
            for (share, color) in [(buy, buy_color), (sell, sell_color)] {
                if share <= 0.0 {
                    continue;
                }
                // Exact sides reach geometry directly; no cached share or readability cutoff.
                let sweep = share * std::f64::consts::TAU;
                add_sector(
                    mesh,
                    disc.center.into(),
                    disc.radius,
                    angle as f32,
                    sweep as f32,
                    tick_style
                        .as_ref()
                        .map_or(SphereShading::flat(color), |style| {
                            SphereShading::sphere(color, style).faded(style.opacity)
                        }),
                    tick_style
                        .as_ref()
                        .map_or(0.0, |_| super::bubbles::SPHERE_LIGHT_OFFSET),
                );
                angle += sweep;
            }
        }
        for (mesh, rims) in meshes.iter_mut().zip(rims) {
            mesh.append(rims);
        }
        Self {
            viewport_frame: None,
            labels: place_quantity_labels(painter, history, labels),
            plan,
            meshes,
        }
    }

    pub fn paint(&self, painter: &egui::Painter, history: egui::Rect, foreground: bool) {
        let mesh = &self.meshes[usize::from(foreground)];
        if !mesh.is_empty() {
            painter
                .with_clip_rect(history)
                .add(egui::Shape::mesh(mesh.clone()));
        }
    }

    /// Labels finish the candle pass, so no candle contour crosses a quantity.
    pub fn paint_labels(&self, painter: &egui::Painter, history: egui::Rect) {
        paint_quantity_labels(painter, history, &self.labels);
    }
}

fn paint_quantity_labels(painter: &egui::Painter, history: egui::Rect, labels: &[PlacedLabel]) {
    let clip = painter.with_clip_rect(history);
    for (pos, label) in labels {
        // Laid out each frame (egui's galley cache answers): a galley kept across
        // frames would outlive a font atlas egui recreates.
        let galley = quantity_galley(painter, label);
        let shadow = egui::Color32::from_black_alpha(super::constants::LABEL_SHADOW_ALPHA);
        let offset = super::constants::LABEL_SHADOW_OFFSET_PX;
        clip.galley_with_override_text_color(*pos + offset, galley.clone(), shadow);
        clip.galley(*pos, galley, crate::theme::TEXT_PRIMARY);
    }
}

fn quantity_galley(painter: &egui::Painter, label: &FlowQuantityLabel) -> Arc<egui::Galley> {
    use super::constants::{LABEL_FONT_SCALE, LABEL_MAX_FONT_PX, LABEL_MIN_FONT_PX};
    let font = (label.radius * LABEL_FONT_SCALE).clamp(LABEL_MIN_FONT_PX, LABEL_MAX_FONT_PX);
    let font = egui::FontId::proportional(font);
    painter.layout_no_wrap(label.text.clone(), font, crate::theme::TEXT_PRIMARY)
}

/// The labels that fit their sphere, inside `history` and clear of every
/// larger one placed before, in the order given (largest sphere first).
fn place_quantity_labels(
    painter: &egui::Painter,
    history: egui::Rect,
    labels: Vec<FlowQuantityLabel>,
) -> Vec<PlacedLabel> {
    use super::constants::{LABEL_MAX_HEIGHT_SCALE, LABEL_MAX_WIDTH_SCALE};
    let mut occupied = Vec::<egui::Rect>::new();
    let place = |label: FlowQuantityLabel| {
        let size = quantity_galley(painter, &label).size();
        let fits = size.x <= label.radius * LABEL_MAX_WIDTH_SCALE
            && size.y <= label.radius * LABEL_MAX_HEIGHT_SCALE;
        let pos = egui::Pos2::from(label.center) - size / 2.0;
        let bounds = egui::Rect::from_min_size(pos, size).expand(LABEL_CLEARANCE_PX);
        let free = fits
            && history.contains_rect(bounds)
            && !occupied.iter().any(|other| other.intersects(bounds));
        free.then(|| {
            occupied.push(bounds);
            (pos, label)
        })
    };
    labels.into_iter().filter_map(place).collect()
}

#[cfg(test)]
mod label_tests {
    use super::*;
    use quantick_chart::flow_execution::FlowQuantityLabel;

    #[test]
    fn tick_quantities_use_fitted_nonoverlapping_galleys() {
        let ctx = egui::Context::default();
        let history = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 180.0));
        let labels = [
            FlowQuantityLabel {
                center: [100.0, 90.0],
                radius: 30.0,
                text: "28.1".into(),
            },
            FlowQuantityLabel {
                center: [105.0, 90.0],
                radius: 20.0,
                text: "14.7".into(),
            },
            FlowQuantityLabel {
                center: [180.0, 90.0],
                radius: 15.0,
                text: "7.2".into(),
            },
            FlowQuantityLabel {
                center: [0.0, 90.0],
                radius: 15.0,
                text: "7.2".into(),
            },
        ];
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let placed = place_quantity_labels(&painter, history, labels.to_vec());
            paint_quantity_labels(&painter, history, &placed);
        });
        let bounds: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.override_text_color.is_none() => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            bounds.len(),
            2,
            "the overlapping and clipped labels are omitted"
        );
        assert!(!bounds[0].intersects(bounds[1]));
        assert!(bounds.iter().all(|bounds| history.contains_rect(*bounds)));
    }
}

/// Compose neutral isolation into the earned sectors, without extra circle geometry.
/// Glow blends premultiplied sRGBA in gamma space; linear Rgba would change the tone.
#[cfg(test)]
pub(super) fn flow_colors(backing: Option<egui::Color32>, oversized: bool) -> [egui::Color32; 2] {
    quantick_orderflow::config::dressing::flow::colors(
        backing.map(|color| color.to_array()),
        oversized,
        false,
    )
    .map(|[r, g, b, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a))
}

/// Regional chrome is docked after the native legend, including incomplete first frames.
pub(crate) fn flow_caption(
    painter: &egui::Painter,
    history: egui::Rect,
    top: f32,
    frame: Option<&FlowTapeFrame>,
    progress: FlowProgress,
    legend_visible: bool,
    spheres: Option<quantick_chart::flow_execution::SphereChrome<'_>>,
) -> Option<egui::Rect> {
    use quantick_orderflow::projection::flow_tape::{caption_segments, notes_segments};
    let segments = caption_segments(frame, progress, legend_visible, spheres.map(|s| s.unit));
    let at = egui::pos2(history.left() + FLOW_CAPTION_INSET_PX, top);
    let mut galley = caption_galley(painter, history, &segments, spheres);
    if let Some(chrome) = spheres {
        let rect = egui::Rect::from_min_size(at, galley.size());
        let area = [history.min.into(), history.max.into()];
        let panel = [rect.min.into(), rect.max.into()];
        // A caption that cannot be placed stands down, but its honesty notes
        // never vanish with it: they take its place, over a sphere if they must.
        if quantick_chart::flow_execution::place_flow_chrome(area, panel, chrome.regions).is_none()
        {
            galley = caption_galley(painter, history, &notes_segments(frame, progress)?, spheres);
        }
    }
    let rect = egui::Rect::from_min_size(at, galley.size());
    painter
        .with_clip_rect(history)
        .galley(at, galley, crate::theme::TEXT_MUTED);
    Some(rect)
}

/// Lay caption `segments` out at the caption font and wrap; tick spheres key
/// their Buy / Sell in the pane theme, regional FLOW in its own colours.
fn caption_galley(
    painter: &egui::Painter,
    history: egui::Rect,
    segments: &[(
        String,
        quantick_orderflow::projection::flow_tape::CaptionInk,
    )],
    spheres: Option<quantick_chart::flow_execution::SphereChrome<'_>>,
) -> std::sync::Arc<egui::Galley> {
    use quantick_orderflow::projection::flow_tape::CaptionInk;
    let mut job = egui::text::LayoutJob::default();
    let wrap = (history.width() - FLOW_CAPTION_WRAP_MARGIN_PX).max(1.0);
    let chrome = quantick_chart::flow_execution::FLOW_CHROME_MAX_WIDTH_PX;
    job.wrap.max_width = spheres.map_or(wrap, |_| wrap.min(chrome));
    let palette = spheres.map(|spheres| super::palette_for_theme(spheres.theme));
    for (text, ink) in segments {
        let color = match (ink, &palette) {
            (CaptionInk::Buy, Some(palette)) => palette.buy,
            (CaptionInk::Sell, Some(palette)) => palette.sell,
            (CaptionInk::Buy, None) => FLOW_BUY,
            (CaptionInk::Sell, None) => FLOW_SELL,
            (CaptionInk::Muted, _) => crate::theme::TEXT_MUTED,
        };
        let font = egui::FontId::proportional(FLOW_CAPTION_FONT_PX);
        job.append(text, 0.0, egui::TextFormat::simple(font, color));
    }
    painter.layout_job(job)
}
#[cfg(test)]
#[path = "tests/flow_execution.rs"]
mod tests;
