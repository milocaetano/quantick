//! Target-keyed frames for reads that wait for a native paint.
//! Native viewport validation stays at the UI adapter; this state cannot route
//! a delivered image into another window's pending capture.
use quantick_control::wire::WireU64;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct WindowFrames<S, I> {
    frames: BTreeMap<WireU64, Frame<S, I>>,
    next_generation: u64,
}
struct Frame<S, I> {
    scene: S,
    image: Option<I>,
    armed: bool,
    ticket: CaptureTicket,
}
impl<S, I> Default for WindowFrames<S, I> {
    fn default() -> Self {
        Self {
            frames: BTreeMap::new(),
            next_generation: 1,
        }
    }
}
impl<S, I> WindowFrames<S, I> {
    pub fn arm(&mut self, window: WireU64, scene: S) -> CaptureTicket {
        if self.has_image(window) {
            return self.frames[&window].ticket.clone();
        }
        if let Some(frame) = self.frames.get(&window) {
            frame.ticket.cancel();
        }
        let ticket = CaptureTicket {
            generation: self.next_generation,
            active: Arc::new(AtomicBool::new(true)),
        };
        self.next_generation = self.next_generation.saturating_add(1);
        self.frames.insert(
            window,
            Frame {
                scene,
                image: None,
                armed: true,
                ticket: ticket.clone(),
            },
        );
        ticket
    }
    pub fn rearm(&mut self, window: WireU64) -> bool {
        if let Some(frame) = self.frames.get_mut(&window) {
            frame.armed = true;
            true
        } else {
            false
        }
    }
    pub fn accept_generation(
        &mut self,
        target: WireU64,
        source: WireU64,
        generation: u64,
        image: I,
    ) -> bool {
        if !self
            .frames
            .get(&target)
            .is_some_and(|frame| frame.ticket.generation == generation && frame.ticket.is_active())
        {
            return false;
        }
        self.accept(target, source, image)
    }
    pub fn armed(&self, window: WireU64) -> bool {
        self.frames.get(&window).is_some_and(|frame| frame.armed)
    }
    pub fn any_armed(&self) -> bool {
        self.frames.values().any(|frame| frame.armed)
    }
    pub fn has_image(&self, window: WireU64) -> bool {
        self.frames
            .get(&window)
            .is_some_and(|frame| frame.image.is_some())
    }
    pub fn scene(&self, window: WireU64) -> Option<&S> {
        self.frames.get(&window).map(|frame| &frame.scene)
    }
    pub fn accept(&mut self, target: WireU64, source: WireU64, image: I) -> bool {
        if target != source {
            return false;
        }
        let Some(frame) = self.frames.get_mut(&target).filter(|frame| frame.armed) else {
            return false;
        };
        frame.ticket.cancel();
        frame.image = Some(image);
        frame.armed = false;
        true
    }
    pub fn take_image(&mut self, window: WireU64) -> Option<I> {
        self.frames.get_mut(&window)?.image.take()
    }
    pub fn retain(&mut self, mut needed: impl FnMut(WireU64) -> bool) {
        self.frames.retain(|window, frame| {
            let keep = needed(*window);
            if !keep {
                frame.ticket.cancel();
            }
            keep
        });
    }
    pub fn clear(&mut self) {
        for frame in self.frames.values() {
            frame.ticket.cancel();
        }
        self.frames.clear();
    }
}
impl<S, I> Drop for WindowFrames<S, I> {
    fn drop(&mut self) {
        for frame in self.frames.values() {
            frame.ticket.cancel();
        }
    }
}
/// A queued GPU callback loses permission when its frame is replaced or discarded.
#[derive(Clone)]
pub struct CaptureTicket {
    generation: u64,
    active: Arc<AtomicBool>,
}
impl CaptureTicket {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    fn cancel(&self) {
        self.active.store(false, Ordering::Release);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interleaved_frames_keep_their_source_scene_and_target_identity() {
        let (root, child) = (WireU64::new(0), WireU64::new(7));
        let mut frames = WindowFrames::default();
        frames.arm(root, "root scene");
        frames.arm(child, "child scene");
        assert!(!frames.accept(root, child, "wrong pixels"));
        assert!(frames.accept(child, child, "child pixels"));
        frames.arm(child, "replacement scene");
        assert_eq!(frames.scene(child), Some(&"child scene"));
        assert!(frames.accept(root, root, "root pixels"));
        assert_eq!(frames.take_image(root), Some("root pixels"));
        assert_eq!(frames.take_image(child), Some("child pixels"));
        frames.retain(|window| window == child);
        assert_eq!(frames.scene(root), None);
    }
}

#[cfg(test)]
mod ticket_tests {
    use super::*;
    #[test]
    fn replaced_closed_and_dropped_paints_cannot_publish_into_a_new_capture() {
        let window = WireU64::new(1);
        let mut frames = WindowFrames::<(), ()>::default();
        let old = frames.arm(window, ());
        let current = frames.arm(window, ());
        assert!(!old.is_active());
        assert!(!frames.accept_generation(window, window, old.generation(), ()));
        assert!(frames.accept_generation(window, window, current.generation(), ()));
        frames.take_image(window).unwrap();
        let dropped = frames.arm(window, ());
        assert!(dropped.is_active());
        drop(frames);
        assert!(!dropped.is_active());
    }
}

/// Model/system reads retain their original immediate UI dispatch. Only scopes
/// reading shared painted chrome need the target viewport's paint boundary.
pub fn needs_painted_scopes(scopes: &[quantick_control::id::SnapshotScopeId]) -> bool {
    scopes.iter().any(|scope| {
        matches!(
            scope.as_str(),
            "scene.controls" | "interaction.cursor" | "interaction.selection"
        )
    })
}
#[cfg(test)]
mod scope_tests {
    use super::*;
    use quantick_control::id::SnapshotScopeId;
    #[test]
    fn ordinary_snapshots_do_not_spend_image_waiter_slots() {
        let scopes = |names: &[&str]| {
            names
                .iter()
                .map(|name| SnapshotScopeId::new(*name).unwrap())
                .collect::<Vec<_>>()
        };
        assert!(!needs_painted_scopes(&scopes(&[
            "system.info",
            "workspace.summary",
            "session.paper"
        ])));
        for paint in [
            "scene.controls",
            "interaction.cursor",
            "interaction.selection",
        ] {
            assert!(needs_painted_scopes(&scopes(&["system.info", paint])));
        }
    }
}

/// Leave one admission slot available while native paint reads wait, including
/// when their target cannot present. Completion and admission share the same
/// documented per-frame count; neither stream can monopolize all four slots.
pub fn reserved_paint_slots(pending: usize, served: usize) -> usize {
    const MIN_ADMISSION_SLOTS: usize = 1;
    let limit = quantick_control::limits::CONTROL_UI_MAX_REQUESTS_PER_FRAME;
    pending
        .min(limit.saturating_sub(served))
        .min(limit.saturating_sub(MIN_ADMISSION_SLOTS))
}
#[cfg(test)]
mod reservation_tests {
    use super::*;
    #[test]
    fn unpainted_targets_leave_admission_capacity_and_cannot_exceed_frame_budget() {
        assert_eq!(reserved_paint_slots(8, 0), 3);
        assert_eq!(reserved_paint_slots(8, 3), 1);
        assert_eq!(reserved_paint_slots(8, 4), 0);
        assert_eq!(reserved_paint_slots(1, 0), 1);
    }
}
