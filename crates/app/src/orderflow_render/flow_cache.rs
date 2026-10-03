//! Pack immutable FLOW geometry once; ordinary frames only copy the final mesh.
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
    mesh: OnceLock<egui::Mesh>,
}
#[derive(Default)]
struct Computer;
#[derive(Clone, Copy)]
struct Key<'a> {
    frame: &'a Arc<FlowTapeFrame>,
    geometry: FlowExecutionGeometry,
    clip: egui::Rect,
    backing: Option<egui::Color32>,
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

pub(crate) fn draw_cached_flow(
    painter: &egui::Painter,
    history: egui::Rect,
    frame: &Arc<FlowTapeFrame>,
    geometry: FlowExecutionGeometry,
    backing: Option<egui::Color32>,
) {
    let clip = history.intersect(painter.clip_rect());
    let key = Key {
        frame,
        geometry,
        clip,
        backing,
    };
    let entry = painter
        .ctx()
        .memory_mut(|memory| memory.caches.cache::<Cache>().get(key));
    // Build outside the Context memory lock. Logical sector vertices use only
    // WHITE_UV, independent of pixel density and the growing font atlas.
    let mesh = entry.mesh.get_or_init(|| {
        super::flow_execution::flow_mesh(clip, frame, backing, |dot| {
            geometry.point(dot).map(|(x, y)| egui::pos2(x, y))
        })
    });
    painter
        .with_clip_rect(clip)
        .add(egui::Shape::mesh(mesh.clone()));
}

#[cfg(test)]
#[path = "tests/flow_cache.rs"]
mod tests;
