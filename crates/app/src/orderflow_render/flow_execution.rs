//! Compact history execution groups on the FLOW candle coordinate system.
use super::bubbles::{BubbleColors, SPHERE_LIGHT_OFFSET, SphereShading, add_sector};
use crate::orderflow_render::constants::{
    FLOW_BUY, FLOW_CAPTION_FONT_PX, FLOW_CAPTION_INSET_PX, FLOW_CAPTION_WRAP_MARGIN_PX, FLOW_SELL,
    LABEL_MAX_HEIGHT_SCALE, LABEL_MAX_WIDTH_SCALE, PIE_START_ANGLE,
};
use eframe::egui;
#[cfg(test)]
pub(super) use quantick_chart::flow_execution::FlowDisc;
use quantick_chart::flow_execution::{self as flow, FlowPresentation, FlowQuantityLabel};
#[cfg(test)]
use quantick_orderflow::HeatmapConfig;
#[cfg(test)]
use quantick_orderflow::projection::flow_tape::FlowTapeDot;
use quantick_orderflow::projection::flow_tape::{FlowProgress, FlowTapeFrame};
#[cfg(test)]
use rust_decimal::Decimal;
use std::sync::Arc;

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
    let drawing = FlowDrawing::styled(
        (frame, plan),
        background,
        backing,
        None,
        &painter,
        history,
        None,
    );
    let [mut mesh, peaks] = drawing.meshes();
    mesh.append(peaks);
    mesh
}

/// A placed quantity label: where its galley goes, and the label.
type PlacedLabel = ([f32; 2], FlowQuantityLabel);
/// One FLOW frame as every frame paints it, built once per cached drawing: each pass's
/// runs, their sectors then their rims as egui would tessellate them, and placed labels.
pub(crate) struct FlowDrawing {
    pub plan: FlowPresentation,
    /// The area painted in, which clips every pass and holds the labels.
    pub history: egui::Rect,
    /// Each run's pass, then its sectors and rims, keyed for the next drawing to keep.
    runs: flow::FlowRunCache<Arc<[egui::Mesh; 2]>>,
    /// Exact presentation frame when the current camera resizes tick spheres.
    pub viewport_frame: Option<Arc<FlowTapeFrame>>,
    labels: Vec<PlacedLabel>,
}
impl FlowDrawing {
    /// `painter` lends tessellation and fonts; `history` is the area painted in. Runs
    /// whose keys held since `previous`, this look's drawing before, are kept from it.
    pub fn styled(
        (frame, plan): (&FlowTapeFrame, FlowPresentation),
        background: egui::Color32,
        backing: Option<egui::Color32>,
        tick_look: Option<quantick_orderflow::TickLook>,
        painter: &egui::Painter,
        history: egui::Rect,
        previous: Option<&Self>,
    ) -> Self {
        let ppp = painter.ctx().pixels_per_point();
        let options = painter.ctx().options(|o| o.tessellation_options);
        let mut tessellator = egui::epaint::Tessellator::new(ppp, options, [1, 1], Vec::new());
        tessellator.set_clip_rect(history.intersect(painter.clip_rect()));
        let tick = tick_look.map(|(tick, _)| tick);
        let tick_style = tick.map(quantick_orderflow::TickBubbleStyle::bubbles);
        let tick_colors = tick_look.map(|(tick, theme)| {
            BubbleColors::resolve(&super::palette_for_theme(theme), &tick.bubbles())
        });
        let canvas = background.to_array();
        let underlay = backing.as_ref().map(egui::Color32::to_array);
        let keys = plan.region_keys(frame, tick.is_some());
        let hair_color = egui::Color32::from_black_alpha(super::bubbles::SEPARATOR_RING_ALPHA);
        let light = tick_style.as_ref().map_or(0.0, |_| SPHERE_LIGHT_OFFSET);
        let shade = |color| match &tick_style {
            Some(style) => SphereShading::sphere(color, style).faded(style.opacity),
            None => SphereShading::flat(color),
        };
        let mut build = |regions: &[flow::FlowRegionVisual]| {
            let [mut mesh, mut rim] = [egui::Mesh::default(), egui::Mesh::default()];
            for region in regions {
                let dot = &frame.dots[region.dot_index];
                let disc = region.disc;
                let [buy_color, sell_color] = match tick_colors {
                    Some(colors) => [colors.buy, colors.sell],
                    None => region
                        .regional_fill(frame, canvas, underlay)
                        .map(super::premultiplied),
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
                            egui::Stroke::new(hair, hair_color),
                        ),
                        &mut rim,
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
                        &mut mesh,
                        disc.center.into(),
                        disc.radius,
                        angle as f32,
                        sweep as f32,
                        shade(color),
                        light,
                    );
                    angle += sweep;
                }
            }
            Arc::new([mesh, rim])
        };
        let empty = flow::FlowRunCache::default();
        let held = previous.map_or(&empty, |previous| &previous.runs);
        let runs = held.rebuild(keys, |cut| build(&plan.regions[cut]));
        let labels = plan.quantity_labels(frame, tick);
        let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
        let measure = |label: &FlowQuantityLabel| quantity_galley(painter, label).size().into();
        let area = [history.min.into(), history.max.into()];
        Self {
            runs,
            labels: flow::place_quantity_labels(labels, area, fit, measure),
            history,
            viewport_frame: None,
            plan,
        }
    }

    /// The pass's runs, sectors then rims, on FLOW's two white texels in
    /// turn, so egui moves each whole into a primitive of its own.
    pub fn paint(&self, painter: &egui::Painter, foreground: bool) {
        let clip = painter.with_clip_rect(self.history);
        let texels = flow_texels(painter.ctx());
        let pass = self
            .runs
            .runs()
            .iter()
            .filter(|run| run.0 == usize::from(foreground));
        let sectors = pass.clone().map(|run| &run.1[0]);
        let meshes = sectors.chain(pass.map(|run| &run.1[1]));
        for (index, mesh) in meshes.filter(|mesh| !mesh.is_empty()).enumerate() {
            let mut mesh = mesh.clone();
            mesh.texture_id = texels[index % 2];
            clip.add(egui::Shape::mesh(mesh));
        }
    }

    /// Labels finish the candle pass, so no candle contour crosses a quantity.
    pub fn paint_labels(&self, painter: &egui::Painter) {
        paint_quantity_labels(painter, self.history, &self.labels);
    }
}

#[cfg(test)]
impl FlowDrawing {
    /// Each pass's runs joined into one mesh, sectors then rims, as painted.
    pub(super) fn meshes(&self) -> [egui::Mesh; 2] {
        std::array::from_fn(|pass| {
            let mut mesh = egui::Mesh::default();
            for part in [0, 1] {
                for (_, run) in self.runs.runs().iter().filter(|(at, _)| *at == pass) {
                    mesh.append_ref(&run[part]);
                }
            }
            mesh
        })
    }
}

/// FLOW's two white texels: egui copies consecutive meshes of one texture and clip
/// into one buffer each frame, but moves runs drawn on the two in turn. White under
/// `WHITE_UV`, like the font atlas's texel, so every triangle paints its vertex colour.
pub(crate) fn flow_texels(ctx: &egui::Context) -> [egui::TextureId; 2] {
    let id = egui::Id::new("flow white texels");
    if let Some(held) = ctx.data(|data| data.get_temp::<[egui::TextureHandle; 2]>(id)) {
        return held.each_ref().map(egui::TextureHandle::id);
    }
    let white = || egui::ColorImage::new([1, 1], egui::Color32::WHITE);
    let options = egui::TextureOptions::LINEAR;
    let held = ["flow texel", "flow texel 2"].map(|name| ctx.load_texture(name, white(), options));
    ctx.data_mut(|data| data.insert_temp(id, held.clone()));
    held.each_ref().map(egui::TextureHandle::id)
}

fn paint_quantity_labels(painter: &egui::Painter, history: egui::Rect, labels: &[PlacedLabel]) {
    let clip = painter.with_clip_rect(history);
    for (pos, label) in labels {
        let pos = egui::Pos2::from(*pos);
        // Laid out each frame (egui's galley cache answers): a galley kept across
        // frames would outlive a font atlas egui recreates.
        let galley = quantity_galley(painter, label);
        let shadow = egui::Color32::from_black_alpha(super::constants::LABEL_SHADOW_ALPHA);
        let offset = super::constants::LABEL_SHADOW_OFFSET_PX;
        clip.galley_with_override_text_color(pos + offset, galley.clone(), shadow);
        clip.galley(pos, galley, crate::theme::TEXT_PRIMARY);
    }
}

fn quantity_galley(painter: &egui::Painter, label: &FlowQuantityLabel) -> Arc<egui::Galley> {
    use super::constants::{LABEL_FONT_SCALE, LABEL_MAX_FONT_PX, LABEL_MIN_FONT_PX};
    let font = (label.radius * LABEL_FONT_SCALE).clamp(LABEL_MIN_FONT_PX, LABEL_MAX_FONT_PX);
    let font = egui::FontId::proportional(font);
    painter.layout_no_wrap(label.text.clone(), font, crate::theme::TEXT_PRIMARY)
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
            let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
            let measure =
                |label: &FlowQuantityLabel| quantity_galley(&painter, label).size().into();
            let area = [history.min.into(), history.max.into()];
            let placed = flow::place_quantity_labels(labels.to_vec(), area, fit, measure);
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
