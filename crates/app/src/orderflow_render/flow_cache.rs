//! Pack immutable FLOW geometry, rims and labels once; frames only copy the final mesh.
use super::flow_execution::FlowDrawing;
use eframe::egui::{
    self,
    util::cache::{ComputerMut, FrameCache},
};
use quantick_chart::flow_execution::FlowExecutionGeometry;
use quantick_orderflow::projection::flow_tape::FlowTapeFrame;
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::{Arc, OnceLock, Weak},
};

/// The drawing each look last painted, by the look's hash, in one temp slot.
/// A look nobody paints any more leaves with its drawing: egui collects only
/// persisted temp values, so a slot per look would grow by one entry every
/// frame a divider drag or a resize changes the clip.
#[derive(Clone, Default)]
struct LastPainted(BTreeMap<u64, Weak<FlowDrawing>>);

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
impl Key<'_> {
    /// All but the frame and the camera: what a drawing keeps the runs of the one before by.
    fn look(&self) -> impl Hash {
        let (min, max) = (self.clip.min, self.clip.max);
        let corners = [min.x, min.y, max.x, max.y].map(f32::to_bits);
        let tick = self.tick.map(|(style, theme)| (style.fingerprint(), theme));
        (corners, self.backing, self.background, tick, self.density)
    }
}
impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(self.frame).hash(state);
        self.geometry.fingerprint().hash(state);
        self.look().hash(state);
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
    let ctx = painter.ctx();
    let entry = ctx.memory_mut(|memory| memory.caches.cache::<Cache>().get(key));
    let slot = egui::Id::new("quantick_flow_last_painted");
    let look = egui::Id::new(key.look()).value();
    // Build outside the Context memory lock. Logical sector vertices use only
    // WHITE_UV, independent of pixel density and the growing font atlas.
    Arc::clone(entry.mesh.get_or_init(|| {
        let (plan, viewport_frame) = geometry.presentation(
            frame,
            tick.map(|(style, _)| style),
            [clip.min.into(), clip.max.into()],
        );
        let previous = ctx.data(|data| {
            let painted = data.get_temp::<LastPainted>(slot)?;
            painted.0.get(&look).and_then(Weak::upgrade)
        });
        let displayed = viewport_frame.as_deref().unwrap_or(frame);
        let (made, held) = ((displayed, plan), previous.as_deref());
        let mut drawing =
            FlowDrawing::styled(made, (background, backing), tick, (painter, history), held);
        drawing.viewport_frame = viewport_frame;
        let drawing = Arc::new(drawing);
        ctx.data_mut(|data| {
            let painted = &mut data.get_temp_mut_or_default::<LastPainted>(slot).0;
            painted.retain(|_, drawing| drawing.strong_count() > 0);
            painted.insert(look, Arc::downgrade(&drawing));
        });
        drawing
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
    drawing.paint(painter, false);
    drawing.paint(painter, true);
}

#[cfg(test)]
#[path = "tests/flow_cache.rs"]
mod tests;
