//! Pack immutable FLOW geometry, rims and labels once; frames only copy the final mesh.
use super::flow_execution::FlowDrawing;
use eframe::egui::{
    self,
    util::cache::{ComputerMut, FrameCache},
};
use quantick_chart::flow_execution::FlowExecutionGeometry;
use quantick_orderflow::projection::flow_tape::FlowTapeFrame;
use std::{
    hash::{Hash, Hasher},
    sync::{Arc, OnceLock, Weak},
};

struct Entry {
    // Keep the allocation identity alive without retaining its source members.
    _source: Weak<FlowTapeFrame>,
    mesh: OnceLock<Arc<FlowDrawing>>,
}
#[derive(Default)]
struct Computer;
#[derive(Clone, Copy)]
struct Key<'a> {
    frame: &'a Arc<FlowTapeFrame>,
    geometry: FlowExecutionGeometry,
    clip: egui::Rect,
    backing: Option<egui::Color32>,
    background: egui::Color32,
    tick: Option<quantick_orderflow::TickLook>,
    /// The pixel density tick rims and labels follow; regional geometry is logical.
    density: Option<u32>,
}
impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(self.frame).hash(state);
        self.density.hash(state);
        self.geometry.fingerprint().hash(state);
        self.clip.min.x.to_bits().hash(state);
        self.clip.min.y.to_bits().hash(state);
        self.clip.max.x.to_bits().hash(state);
        self.clip.max.y.to_bits().hash(state);
        self.backing.hash(state);
        self.background.hash(state);
        self.tick
            .map(|(style, theme)| (style.fingerprint(), theme))
            .hash(state);
    }
}
impl ComputerMut<Key<'_>, Arc<Entry>> for Computer {
    fn compute(&mut self, key: Key<'_>) -> Arc<Entry> {
        Arc::new(Entry {
            _source: Arc::downgrade(key.frame),
            mesh: OnceLock::new(),
        })
    }
}
type Cache = FrameCache<Arc<Entry>, Computer>;

pub(crate) fn cached_flow(
    painter: &egui::Painter,
    history: egui::Rect,
    frame: &Arc<FlowTapeFrame>,
    geometry: FlowExecutionGeometry,
    background: egui::Color32,
    backing: Option<egui::Color32>,
    tick: Option<quantick_orderflow::TickLook>,
) -> Arc<FlowDrawing> {
    let clip = history.intersect(painter.clip_rect());
    let key = Key {
        frame,
        geometry,
        clip,
        backing,
        background,
        tick,
        density: tick.map(|_| painter.ctx().pixels_per_point().to_bits()),
    };
    let entry = painter
        .ctx()
        .memory_mut(|memory| memory.caches.cache::<Cache>().get(key));
    // Build outside the Context memory lock. Logical sector vertices use only
    // WHITE_UV, independent of pixel density and the growing font atlas.
    Arc::clone(entry.mesh.get_or_init(|| {
        let (plan, viewport_frame) = geometry.presentation(
            frame,
            tick.map(|(style, _)| style),
            [clip.min.into(), clip.max.into()],
        );
        let displayed = viewport_frame.as_deref().unwrap_or(frame);
        let mut drawing =
            FlowDrawing::styled(displayed, plan, background, backing, tick, painter, history);
        drawing.viewport_frame = viewport_frame;
        Arc::new(drawing)
    }))
}

#[cfg(test)]
pub(crate) fn draw_cached_flow(
    painter: &egui::Painter,
    history: egui::Rect,
    frame: &Arc<FlowTapeFrame>,
    geometry: FlowExecutionGeometry,
    backing: Option<egui::Color32>,
) {
    let drawing = cached_flow(
        painter,
        history,
        frame,
        geometry,
        crate::theme::CANVAS,
        backing,
        None,
    );
    drawing.paint(painter, history, false);
    drawing.paint(painter, history, true);
}

#[cfg(test)]
#[path = "tests/flow_cache.rs"]
mod tests;
