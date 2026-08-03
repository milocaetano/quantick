//! Draw objects: lines, boxes and labels an indicator retains across bars.
//!
//! Coordinates are **bar index + price** only (`xloc.bar_time` is outside
//! the dialect): bar indices map 1:1 onto chart slots, which is the whole
//! point of scripting on activity-sampled bars.
//!
//! Each indicator owns one [`ObjectStore`] with a hard cap of
//! [`MAX_OBJECTS_PER_KIND`] per kind — creating one past the cap
//! garbage-collects the oldest (Pine's rule, and our render-cost
//! guarantee). The store is `Clone` with bounded size, which is what makes
//! the preview snapshot discipline affordable; a preview's objects travel
//! to the renderer as a [`ObjectSnapshot`] and are discarded on the next
//! run.

use crate::output::Rgba8;

/// Hard cap per object kind per indicator. Creating the 501st line
/// garbage-collects the oldest — bounded render and clone cost by
/// construction.
pub const MAX_OBJECTS_PER_KIND: usize = 500;

/// A straight segment between two (bar index, price) anchors.
/// The bar index of an object whose x coordinate is missing (`na`).
///
/// Negative on purpose: every renderer already drops objects at a negative
/// bar, so "no bar" draws nothing instead of being silently patched to bar
/// zero. The y side of the same call answers NaN for the same reason.
pub const OFF_CHART_BAR: i64 = -1;

#[derive(Debug, Clone, PartialEq)]
pub struct LineObj {
    /// First anchor's bar index.
    pub x1: i64,
    /// First anchor's price.
    pub y1: f64,
    /// Second anchor's bar index.
    pub x2: i64,
    /// Second anchor's price.
    pub y2: f64,
    /// Stroke color.
    pub color: Rgba8,
    /// Stroke width in points.
    pub width: f32,
}

/// An axis-aligned rectangle between two (bar index, price) corners.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxObj {
    /// Left edge's bar index.
    pub left: i64,
    /// Top edge's price.
    pub top: f64,
    /// Right edge's bar index.
    pub right: i64,
    /// Bottom edge's price.
    pub bottom: f64,
    /// Border color.
    pub border_color: Rgba8,
    /// Fill color (transparent = no fill).
    pub bg_color: Rgba8,
    /// Border width in points.
    pub border_width: f32,
}

/// Where a label sits relative to its anchor price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelStyle {
    /// Pointer below the anchor, text above (`label.style_label_up` — used
    /// at lows).
    Up,
    /// Pointer above the anchor, text below (`label.style_label_down` —
    /// used at highs).
    Down,
    /// Plain text centred on the anchor (`label.style_none`).
    None,
}

/// A text marker at one (bar index, price) anchor.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelObj {
    /// Anchor bar index.
    pub x: i64,
    /// Anchor price.
    pub y: f64,
    /// The text shown.
    pub text: String,
    /// Background color.
    pub color: Rgba8,
    /// Text color.
    pub text_color: Rgba8,
    /// Placement relative to the anchor.
    pub style: LabelStyle,
}

/// Handle to one object (id within its indicator's store, per kind). Ids
/// are never reused within a store's lifetime — a stale handle misses
/// instead of mutating a stranger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectId(pub u32);

/// The retained draw objects of one indicator.
///
/// Every mutation bumps `revision`, which is how the worker knows to
/// re-publish the set — cheaper than diffing and impossible to forget.
#[derive(Debug, Clone, Default)]
pub struct ObjectStore {
    lines: Vec<(u32, LineObj)>,
    boxes: Vec<(u32, BoxObj)>,
    labels: Vec<(u32, LabelObj)>,
    next_id: u32,
    revision: u64,
}

impl ObjectStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Monotone change counter (any create/set/delete bumps it).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn allocate(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.revision += 1;
        id
    }

    /// Create a line; the 501st collects the oldest.
    pub fn new_line(&mut self, line: LineObj) -> ObjectId {
        let id = self.allocate();
        self.lines.push((id, line));
        if self.lines.len() > MAX_OBJECTS_PER_KIND {
            self.lines.remove(0);
        }
        ObjectId(id)
    }

    /// Create a box; the 501st collects the oldest.
    pub fn new_box(&mut self, object: BoxObj) -> ObjectId {
        let id = self.allocate();
        self.boxes.push((id, object));
        if self.boxes.len() > MAX_OBJECTS_PER_KIND {
            self.boxes.remove(0);
        }
        ObjectId(id)
    }

    /// Create a label; the 501st collects the oldest.
    pub fn new_label(&mut self, label: LabelObj) -> ObjectId {
        let id = self.allocate();
        self.labels.push((id, label));
        if self.labels.len() > MAX_OBJECTS_PER_KIND {
            self.labels.remove(0);
        }
        ObjectId(id)
    }

    /// Mutable access for a `set_*` call. Newest-first search: scripts
    /// overwhelmingly mutate the object they just created.
    ///
    /// The revision moves only on a hit. A handle the cap already collected —
    /// the documented outcome once a script passes the per-kind limit — used
    /// to bump it on every bar, and the worker reads that as "the set
    /// changed": a full snapshot of up to 1500 objects republished every bar,
    /// forever, for no visible difference.
    pub fn line_mut(&mut self, id: ObjectId) -> Option<&mut LineObj> {
        let hit = self.lines.iter().rev().any(|(i, _)| *i == id.0);
        if !hit {
            return None;
        }
        self.revision += 1;
        self.lines
            .iter_mut()
            .rev()
            .find(|(i, _)| *i == id.0)
            .map(|(_, line)| line)
    }

    /// See [`line_mut`](Self::line_mut).
    pub fn box_mut(&mut self, id: ObjectId) -> Option<&mut BoxObj> {
        let hit = self.boxes.iter().rev().any(|(i, _)| *i == id.0);
        if !hit {
            return None;
        }
        self.revision += 1;
        self.boxes
            .iter_mut()
            .rev()
            .find(|(i, _)| *i == id.0)
            .map(|(_, object)| object)
    }

    /// See [`line_mut`](Self::line_mut).
    pub fn label_mut(&mut self, id: ObjectId) -> Option<&mut LabelObj> {
        let hit = self.labels.iter().rev().any(|(i, _)| *i == id.0);
        if !hit {
            return None;
        }
        self.revision += 1;
        self.labels
            .iter_mut()
            .rev()
            .find(|(i, _)| *i == id.0)
            .map(|(_, label)| label)
    }

    /// Delete a line (a stale id is a no-op, never a panic).
    pub fn delete_line(&mut self, id: ObjectId) {
        let before = self.lines.len();
        self.lines.retain(|(i, _)| *i != id.0);
        if self.lines.len() != before {
            self.revision += 1;
        }
    }

    /// Delete a box.
    pub fn delete_box(&mut self, id: ObjectId) {
        let before = self.boxes.len();
        self.boxes.retain(|(i, _)| *i != id.0);
        if self.boxes.len() != before {
            self.revision += 1;
        }
    }

    /// Delete a label.
    pub fn delete_label(&mut self, id: ObjectId) {
        let before = self.labels.len();
        self.labels.retain(|(i, _)| *i != id.0);
        if self.labels.len() != before {
            self.revision += 1;
        }
    }

    /// Drop everything (indicator reset).
    pub fn clear(&mut self) {
        self.lines.clear();
        self.boxes.clear();
        self.labels.clear();
        self.revision += 1;
    }

    /// Roll the store back to `previous` without rewinding the id counter.
    ///
    /// A preview run is discarded wholesale, but ids handed out during it
    /// must not be handed out again: a `varip` handle survives the rollback
    /// (that is what `varip` means), and if `next_id` rewound with the rest
    /// it would later point at a different object — the exact "mutating a
    /// stranger" this store's ids exist to prevent.
    pub fn restore_from(&mut self, previous: Self) {
        let next_id = self.next_id.max(previous.next_id);
        *self = previous;
        self.next_id = next_id;
    }

    /// The renderable view of the store, creation order per kind.
    #[must_use]
    pub fn snapshot(&self) -> ObjectSnapshot {
        ObjectSnapshot {
            lines: self.lines.iter().map(|(_, l)| l.clone()).collect(),
            boxes: self.boxes.iter().map(|(_, b)| b.clone()).collect(),
            labels: self.labels.iter().map(|(_, l)| l.clone()).collect(),
        }
    }

    /// Object counts per kind: (lines, boxes, labels).
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.lines.len(), self.boxes.len(), self.labels.len())
    }
}

/// What the renderer receives: plain object lists, no ids. Committed sets
/// travel on the worker's `Objects` event; a preview's set rides its
/// [`PreviewFrame`](crate::PreviewFrame) and replaces the committed view
/// while the bar forms (latest-wins, like plot previews).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectSnapshot {
    /// Lines, oldest first.
    pub lines: Vec<LineObj>,
    /// Boxes, oldest first.
    pub boxes: Vec<BoxObj>,
    /// Labels, oldest first.
    pub labels: Vec<LabelObj>,
}

impl ObjectSnapshot {
    /// True when there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.boxes.is_empty() && self.labels.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x: i64) -> LineObj {
        LineObj {
            x1: x,
            y1: 1.0,
            x2: x + 1,
            y2: 2.0,
            color: Rgba8::opaque(255, 255, 255),
            width: 1.0,
        }
    }

    #[test]
    fn create_mutate_delete_round_trip() {
        let mut store = ObjectStore::new();
        let id = store.new_line(line(0));
        store.line_mut(id).expect("exists").x2 = 42;
        assert_eq!(store.snapshot().lines[0].x2, 42);
        store.delete_line(id);
        assert!(store.snapshot().lines.is_empty());
        assert!(store.line_mut(id).is_none(), "stale ids miss, never panic");
    }

    #[test]
    fn the_cap_collects_the_oldest() {
        let mut store = ObjectStore::new();
        for i in 0..(MAX_OBJECTS_PER_KIND as i64 + 3) {
            store.new_line(line(i));
        }
        let snapshot = store.snapshot();
        assert_eq!(snapshot.lines.len(), MAX_OBJECTS_PER_KIND);
        assert_eq!(snapshot.lines[0].x1, 3, "the three oldest were collected");
    }

    #[test]
    fn every_mutation_bumps_the_revision() {
        let mut store = ObjectStore::new();
        let r0 = store.revision();
        let id = store.new_label(LabelObj {
            x: 0,
            y: 1.0,
            text: "HH".to_owned(),
            color: Rgba8::opaque(0, 0, 0),
            text_color: Rgba8::opaque(255, 255, 255),
            style: LabelStyle::Down,
        });
        let r1 = store.revision();
        assert!(r1 > r0);
        store.label_mut(id);
        assert!(store.revision() > r1);
    }

    #[test]
    fn clones_are_bounded_by_the_caps() {
        let mut store = ObjectStore::new();
        for i in 0..2_000 {
            store.new_line(line(i));
        }
        let clone = store.clone();
        assert_eq!(clone.counts().0, MAX_OBJECTS_PER_KIND);
    }
}
