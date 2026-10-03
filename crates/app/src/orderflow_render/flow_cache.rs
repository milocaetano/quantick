//! Pack immutable FLOW geometry once; ordinary frames only copy the final mesh.
use super::flow_execution::FlowDrawing;
use eframe::egui::{
    self,
    util::cache::{ComputerMut, FrameCache},
};
use quantick_chart::flow_execution::{FlowExecutionGeometry, FlowPresentation};
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
}
impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(self.frame).hash(state);
        self.geometry.fingerprint().hash(state);
        self.clip.min.x.to_bits().hash(state);
        self.clip.min.y.to_bits().hash(state);
        self.clip.max.x.to_bits().hash(state);
        self.clip.max.y.to_bits().hash(state);
        self.backing.hash(state);
        self.background.hash(state);
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
) -> Arc<FlowDrawing> {
    let clip = history.intersect(painter.clip_rect());
    let key = Key {
        frame,
        geometry,
        clip,
        backing,
        background,
    };
    let entry = painter
        .ctx()
        .memory_mut(|memory| memory.caches.cache::<Cache>().get(key));
    // Build outside the Context memory lock. Logical sector vertices use only
    // WHITE_UV, independent of pixel density and the growing font atlas.
    Arc::clone(entry.mesh.get_or_init(|| {
        let plan = FlowPresentation::new(frame, [clip.min.into(), clip.max.into()], |dot| {
            geometry.point(dot).map(|(x, y)| [x, y])
        });
        Arc::new(FlowDrawing::new(frame, plan, background, backing))
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
    );
    drawing.paint(painter, history, false);
    drawing.paint(painter, history, true);
}

#[cfg(test)]
#[path = "tests/flow_cache.rs"]
mod tests;
