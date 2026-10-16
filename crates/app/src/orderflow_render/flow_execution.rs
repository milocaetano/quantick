//! Compact history execution groups on the FLOW candle coordinate system.
use super::bubbles::{BubbleColors, SPHERE_LIGHT_OFFSET, SphereShading, add_sector};
use super::{bubbles, constants, palette_for_theme, premultiplied};
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
use quantick_orderflow::projection::flow_tape::{CaptionInk, FlowProgress, FlowTapeFrame};
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
    let look = (background, backing);
    let drawing = FlowDrawing::styled((frame, plan), look, None, (&painter, history), None);
    let [mut mesh, peaks] = drawing.meshes();
    mesh.append(peaks);
    mesh
}

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
    labels: Vec<flow_labels::PlacedLabel>,
}
impl FlowDrawing {
    /// `painter` lends tessellation and fonts; `history` is the area painted in. Runs
    /// whose keys held since `previous`, this look's drawing before, are kept from it.
    pub fn styled(
        (frame, plan): (&FlowTapeFrame, FlowPresentation),
        look: (egui::Color32, Option<egui::Color32>),
        tick_look: Option<quantick_orderflow::TickLook>,
        (painter, history): (&egui::Painter, egui::Rect),
        previous: Option<&Self>,
    ) -> Self {
        let (made, held) = ((frame, &plan), previous.map(|previous| &previous.runs));
        let runs = flow_runs::flow_runs(made, look, tick_look, (painter, history), held);
        Self {
            runs,
            labels: flow_labels::placed_labels(made, tick_look, (painter, history)),
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
    spheres: Option<flow::SphereChrome<'_>>,
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
        if flow::place_flow_chrome(area, panel, chrome.regions).is_none() {
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
    segments: &[(String, CaptionInk)],
    spheres: Option<flow::SphereChrome<'_>>,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    let wrap = (history.width() - FLOW_CAPTION_WRAP_MARGIN_PX).max(1.0);
    let chrome = flow::FLOW_CHROME_MAX_WIDTH_PX;
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
mod flow_labels;
mod flow_runs;
#[cfg(test)]
#[path = "tests/flow_execution.rs"]
mod tests;
