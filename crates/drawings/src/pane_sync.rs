//! A pane's drawings kept in step with its market's store.
//!
//! Every chart of a market holds that market's all-timeframes marks itself.
//! When another chart writes them, this one takes the new set in place of its
//! own copy and keeps everything that is its own: its chart-only marks, their
//! ids, its selection and its undo and redo history
//! ([`Drawings::replace_shared`]). Ids are reserved above every id the market
//! holds, so a mark one chart adds never takes an id another chart's mark has.
//!
//! The set is written against what the pane last loaded (its base): a chart
//! that missed a refresh, a gesture being in flight on it or an edit not yet
//! written, writes only what it changed, and catches up with the rest
//! ([`PaneMarks::follow_write`]).
//!
//! Known limitation: a pane shows its market's all-timeframes marks under its
//! own chart-only marks, whatever order they were drawn in.
use crate::{Drawing, DrawingId, DrawingTool, Drawings, SavedDrawingExt, UndoEntry};
use quantick_workspace::layout_document::SavedDrawing;
use quantick_workspace::market_settings::{DrawingKey, MarketSettings, PaneWrite};
use quantick_workspace::market_settings_store::MarketSettingsStore;
use quantick_workspace::pane_layout::PaneLayout;

#[cfg(test)]
#[path = "pane_sync_tests.rs"]
mod pane_sync_tests;

impl<T: Clone + PartialEq> Drawings<T> {
    /// Never hand out an id at or below `floor`: the highest id the market's
    /// store holds, so a new mark cannot collide with another chart's.
    pub fn reserve_ids(&mut self, floor: u64) {
        self.next_id = self.next_id.max(floor);
    }

    /// Put `shared` (the market's all-timeframes marks) in place of this
    /// store's copy of them, keeping its own: chart-only marks and their ids,
    /// the selection, and the undo and redo history. Each step keeps what this
    /// chart changed of the set laid over the new set ([`rebase`]), so Ctrl+Z
    /// here still undoes a placement, move or delete made here; a step that
    /// changes nothing any more is dropped.
    ///
    /// A mark of this chart's own under an id the set gives another chart's
    /// mark takes a fresh id, here and in every step: a chart-only mark (two
    /// charts drew in one frame), or one only a step still holds (an id that
    /// left the market, issued again by a chart that never saw it). Nothing
    /// else is renumbered.
    pub fn replace_shared(&mut self, shared: Vec<Drawing<T>>) {
        let set: Vec<DrawingId> = shared.iter().map(|drawing| drawing.id).collect();
        self.reserve_ids(set.iter().map(|id| id.0).max().unwrap_or(0));
        let renamed = self.clashes_with(&set);
        let fresh = |id: DrawingId| {
            renamed
                .iter()
                .find(|(old, _)| *old == id)
                .map(|pair| pair.1)
        };
        let steps = self.undo.iter_mut().chain(self.redo.iter_mut());
        let marks = steps.flat_map(|entry| entry.items.iter_mut());
        for drawing in marks.chain(self.items.iter_mut()) {
            drawing.id = fresh(drawing.id).unwrap_or(drawing.id);
        }
        let held: Vec<Drawing<T>> = self
            .items
            .iter()
            .filter(|drawing| drawing.all_timeframes())
            .cloned()
            .collect();
        let rewrite = |items: &[Drawing<T>]| -> Vec<Drawing<T>> {
            let then: Vec<&Drawing<T>> = items.iter().filter(|d| d.all_timeframes()).collect();
            let mut next = rebase(&shared, &held, &then);
            let own = items.iter().filter(|drawing| !drawing.all_timeframes());
            next.extend(own.cloned());
            next
        };
        self.items = rewrite(&self.items);
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            entry.items = rewrite(&entry.items);
        }
        let now = self.snapshot();
        drop_still_steps(&mut self.undo, &now);
        drop_still_steps(&mut self.redo, &now);
        self.revision += 1;
        let selected = self.selected_id().map(|id| {
            let moved = renamed.iter().find(|(old, _)| *old == id);
            moved.map_or(id, |(_, fresh)| *fresh)
        });
        let index = selected.and_then(|id| self.index_of(id));
        self.select(index);
    }

    /// The ids `set` holds that name another mark than this chart's under
    /// them, each paired with a fresh id: see [`Self::replace_shared`].
    fn clashes_with(&mut self, set: &[DrawingId]) -> Vec<(DrawingId, DrawingId)> {
        let live: Vec<DrawingId> = self.items.iter().map(|drawing| drawing.id).collect();
        let chart_only = self
            .items
            .iter()
            .filter(|drawing| !drawing.all_timeframes());
        let steps = self.undo.iter().chain(self.redo.iter());
        let gone = steps
            .flat_map(|entry| entry.items.iter())
            .filter(|drawing| !live.contains(&drawing.id));
        let mut clashing: Vec<DrawingId> = Vec::new();
        for drawing in chart_only.chain(gone) {
            if set.contains(&drawing.id) && !clashing.contains(&drawing.id) {
                clashing.push(drawing.id);
            }
        }
        clashing
            .into_iter()
            .map(|id| (id, self.alloc_id()))
            .collect()
    }
}

/// `set` with what one step of a chart's history (`then`) holds otherwise
/// than the chart holds now (`held`) laid over it, by id: a mark the step
/// lacks is taken out, and one it holds otherwise, or alone, is put in. A mark
/// the step did not touch stays as `set` has it, another chart's edit
/// included.
fn rebase<T: Clone + PartialEq>(
    set: &[Drawing<T>],
    held: &[Drawing<T>],
    then: &[&Drawing<T>],
) -> Vec<Drawing<T>> {
    let mut next = set.to_vec();
    for gone in held
        .iter()
        .filter(|now| !then.iter().any(|old| old.id == now.id))
    {
        next.retain(|drawing| drawing.id != gone.id);
    }
    for &old in then {
        if held.iter().any(|now| now.id == old.id && now == old) {
            continue;
        }
        match next.iter_mut().find(|drawing| drawing.id == old.id) {
            Some(slot) => {
                let identity = slot.durable_identity.clone();
                *slot = old.clone();
                if identity.is_some() {
                    slot.durable_identity = identity;
                }
            }
            None => next.push(old.clone()),
        }
    }
    next
}

/// Drop the steps of `stack` (oldest first) that no longer change anything:
/// equal to the state above them.
fn drop_still_steps<T: PartialEq>(stack: &mut Vec<UndoEntry<T>>, now: &UndoEntry<T>) {
    let mut index = stack.len();
    while index > 0 {
        index -= 1;
        let above = stack.get(index + 1).unwrap_or(now);
        if stack[index] == *above {
            stack.remove(index);
        }
    }
}

/// The all-timeframes marks a pane holds, as its next write's base.
fn shared_base(drawings: &Drawings<DrawingTool>) -> Vec<SavedDrawing> {
    let shared = drawings
        .items()
        .iter()
        .filter(|drawing| drawing.all_timeframes());
    shared.map(SavedDrawing::from_drawing).collect()
}

/// Fill an emptied pane holding `key`, cut at `bars`, with what it shows, and
/// record what it loaded of the market's set in `base`.
pub fn adopt_pane(
    drawings: &mut Drawings<DrawingTool>,
    base: &mut Vec<SavedDrawing>,
    settings: &MarketSettings,
    key: &DrawingKey,
    bars: &str,
) {
    drawings.reserve_ids(settings.max_drawing_id(&key.market));
    drawings.adopt(crate::pane_marks(settings, key, bars));
    *base = shared_base(drawings);
}

/// Bring a pane's copy of its market's all-timeframes marks up to the store,
/// keeping what is its own (see [`Drawings::replace_shared`]). The caller
/// re-anchors the pane after, as after an adoption.
pub fn refresh_shared(
    drawings: &mut Drawings<DrawingTool>,
    base: &mut Vec<SavedDrawing>,
    settings: &MarketSettings,
    key: &DrawingKey,
    _bars: &str,
) {
    drawings.reserve_ids(settings.max_drawing_id(&key.market));
    let shared = crate::saved::live(&settings.shared_drawings(&key.market), key);
    drawings.replace_shared(shared);
    *base = shared_base(drawings);
}

/// Write what a pane holding `key`, cut at `bars`, holds, against what it last
/// loaded. A pane that was behind the store is brought up to it
/// ([`PaneWrite::merged`]); the caller re-anchors it then.
pub fn persist_pane(
    drawings: &mut Drawings<DrawingTool>,
    base: &mut Vec<SavedDrawing>,
    settings: &mut MarketSettings,
    key: &DrawingKey,
    bars: &str,
) -> PaneWrite {
    let write = crate::store_marks(settings, key, bars, drawings.items(), base);
    if write.merged {
        refresh_shared(drawings, base, settings, key, bars);
    } else {
        *base = shared_base(drawings);
    }
    drawings.reserve_ids(settings.max_drawing_id(&key.market));
    write
}

/// How a pane takes its marks from the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneSync {
    /// An emptied pane fills: see [`adopt_pane`].
    Adopt,
    /// A pane drops what it holds and takes it again, its selection kept by
    /// id: the set that comes back may hold one more object, and an index
    /// would point at another mark.
    Readopt,
    /// Only the market's all-timeframes marks: see [`refresh_shared`].
    Refresh,
}

/// Bring a pane holding `key`, cut at `bars`, to the store, the `sync` way.
pub fn sync_pane(
    sync: PaneSync,
    drawings: &mut Drawings<DrawingTool>,
    base: &mut Vec<SavedDrawing>,
    settings: &MarketSettings,
    key: &DrawingKey,
    bars: &str,
) {
    match sync {
        PaneSync::Adopt => adopt_pane(drawings, base, settings, key, bars),
        PaneSync::Readopt => {
            let selected = drawings.selected_id();
            drawings.take_all();
            adopt_pane(drawings, base, settings, key, bars);
            let index = selected.and_then(|id| drawings.index_of(id));
            drawings.select(index);
        }
        PaneSync::Refresh => refresh_shared(drawings, base, settings, key, bars),
    }
}

/// Empty a pane holding `key`, cut at `bars`, into the store.
pub fn put_away_pane(
    drawings: &mut Drawings<DrawingTool>,
    base: &mut Vec<SavedDrawing>,
    settings: &mut MarketSettings,
    key: &DrawingKey,
    bars: &str,
) -> PaneWrite {
    let marks = drawings.take_all();
    crate::store_marks(settings, key, bars, &marks, &std::mem::take(base))
}

/// One pane's side of the store: its marks, its layout bookkeeping, the bars
/// it is cut to now and how its host writes a bar rule. The host keeps only
/// what needs its series: re-anchoring after the pane takes from the store.
pub struct PaneMarks<'a, B> {
    pub drawings: &'a mut Drawings<DrawingTool>,
    pub layout: &'a mut PaneLayout<B>,
    pub now: B,
    pub rule: fn(B) -> String,
}

impl<B: Copy + PartialEq> PaneMarks<'_, B> {
    /// The bars the pane's marks were brought out at, and are written back at.
    fn bars(&self) -> String {
        (self.rule)(self.layout.drawings_spec.unwrap_or(self.now))
    }

    /// Empty the pane into the store under the key it holds. A pane never
    /// loaded drops what a hook or a test placed before seeding, with the key.
    pub fn put_away(self, settings: &mut MarketSettings) -> PaneWrite {
        let bars = self.bars();
        self.layout.drawings_spec = None;
        self.layout.recut_checked = None;
        self.layout.behind = PaneWrite::default();
        let Some(key) = self.layout.drawings_key.take() else {
            self.drawings.take_all();
            return PaneWrite::default();
        };
        let base = &mut self.layout.shared_base;
        let write = put_away_pane(self.drawings, base, settings, &key, &bars);
        self.layout.saved_revision = self.drawings.revision();
        write
    }

    /// Take what `key` shows at the pane's held bars, the `sync` way.
    pub fn sync(self, sync: PaneSync, settings: &MarketSettings, key: &DrawingKey) {
        let bars = self.bars();
        self.layout.behind = PaneWrite::default();
        sync_pane(
            sync,
            self.drawings,
            &mut self.layout.shared_base,
            settings,
            key,
            &bars,
        );
    }

    /// Write the pane's marks back; `None` for a pane that holds no key.
    pub fn persist(self, settings: &mut MarketSettings) -> Option<(DrawingKey, PaneWrite)> {
        let key = self.layout.drawings_key.clone()?;
        let bars = self.bars();
        let base = &mut self.layout.shared_base;
        let write = persist_pane(self.drawings, base, settings, &key, &bars);
        self.layout.saved_revision = self.drawings.revision();
        // Written against its base, it is level with the store now.
        self.layout.behind = PaneWrite::default();
        Some((key, write))
    }

    /// Whether the pane takes a write now, and how: `write` names the
    /// writer's key and what it changed, `None` asks after one it waited out.
    /// A pane busy (a gesture or a draft in flight) or holding an edit not yet
    /// written takes nothing: it is marked behind ([`PaneLayout::behind`]),
    /// and its own write merges against its base, or it is asked again once
    /// free. The caller syncs the answer and re-anchors.
    #[must_use]
    pub fn follow_write(
        self,
        write: Option<(&DrawingKey, PaneWrite)>,
        busy: bool,
    ) -> Option<(DrawingKey, PaneSync)> {
        let key = self.layout.drawings_key.clone()?;
        if let Some((writer, write)) = write.filter(|(writer, write)| write.reaches(writer, &key)) {
            self.layout.behind.all_timeframes |= write.all_timeframes;
            self.layout.behind.own |= write.own && *writer == key;
        }
        let behind = self.layout.behind;
        let dirty = self.layout.drawings_dirty(self.drawings.revision());
        if !behind.changed() || busy || dirty {
            return None;
        }
        let sync = if behind.own {
            PaneSync::Readopt
        } else {
            PaneSync::Refresh
        };
        Some((key, sync))
    }

    /// Each frame before a write: ids stay above every id the market holds,
    /// and a quiet re-cut moves only bars (`PaneLayout::recut_in_place`),
    /// asked again whenever the store has moved.
    pub fn follow_store(self, store: &MarketSettingsStore) {
        let settings = store.settings();
        if let Some(key) = &self.layout.drawings_key {
            self.drawings
                .reserve_ids(settings.max_drawing_id(&key.market));
        }
        let rule = self.rule;
        self.layout
            .recut_in_place(self.now, store.revision(), |key, held, now| {
                settings.same_marks_at(key, &rule(*held), &rule(*now))
            });
    }
}
