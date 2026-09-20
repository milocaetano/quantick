//! The collection: every indicator the UI knows about, in add order, and
//! the worker deltas that keep it in step.

use std::sync::Arc;

use quantick_indicators::{ObjectSnapshot, Rgba8, resolve_bar_paint};

use crate::indicator_worker::{IndicatorEvent, SlotId};
use crate::price_view::PriceView;

use super::{IndicatorView, MAX_PANES, PaneSizing};

/// Every indicator the UI knows about, in add order, plus the slot counter.
#[derive(Default)]
pub struct IndicatorViews {
    views: Vec<IndicatorView>,
    next_slot: u64,
    /// Kind of each slot whose first delta has not arrived yet. The view is
    /// born from the worker's `Rebuilt` event, which knows nothing about the
    /// constructor, so the add path parks the kind here and the view takes it
    /// on birth.
    pending_kinds: std::collections::BTreeMap<SlotId, Arc<str>>,
}

impl IndicatorViews {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reserve a slot id for an add command about to be sent, remembering
    /// which constructor it answers to (see [`IndicatorView::kind`]).
    pub(crate) fn allocate_slot(&mut self, kind: impl AsRef<str>) -> SlotId {
        let slot = SlotId(self.next_slot);
        self.next_slot += 1;
        self.pending_kinds.insert(slot, Arc::from(kind.as_ref()));
        slot
    }

    /// The durable identity of the pane `view` draws in.
    ///
    /// Both halves are fixed at birth, so nothing a trader does to the *other*
    /// indicators — hiding one, collapsing one, removing one — can move a
    /// drawing from the pane it was placed on.
    pub(crate) fn pane_key(&self, view: &IndicatorView) -> crate::drawings::PaneKey {
        crate::drawings::PaneKey {
            kind: Arc::clone(&view.kind),
            ordinal: view.ordinal,
        }
    }

    /// The lowest ordinal no live view of `kind` is using.
    ///
    /// Counting instances would reuse an ordinal that a *later* view already
    /// holds; the free-slot rule cannot, so two live panes of one kind never
    /// share a key and a re-added pane lands back on the ordinal the removed
    /// one left behind — which is what makes its drawings come home.
    fn free_ordinal(&self, kind: &str) -> u8 {
        let mut taken: Vec<u8> = self
            .views
            .iter()
            .filter(|view| &*view.kind == kind)
            .map(|view| view.ordinal)
            .collect();
        taken.sort_unstable();
        let mut ordinal = 0u8;
        for used in taken {
            if used == ordinal {
                ordinal = ordinal.saturating_add(1);
            } else if used > ordinal {
                break;
            }
        }
        ordinal
    }

    /// Apply one worker delta. Events for slots the UI already removed are
    /// dropped silently — commands and events cross on the channel, and the
    /// remove always wins.
    pub(crate) fn apply(&mut self, event: IndicatorEvent) {
        match event {
            IndicatorEvent::Rebuilt {
                slot,
                descriptor,
                columns,
                bar_paint,
                inputs,
                rows,
                stale,
            } => {
                if let Some(view) = self.view_mut(slot) {
                    view.label = IndicatorView::label_of(&descriptor);
                    view.descriptor = descriptor;
                    view.columns = columns;
                    view.rows = rows;
                    view.bar_paints = bar_paint;
                    view.input_values = inputs;
                    view.preview = None;
                    view.lane.clear();
                    view.error = None;
                    // Mirrored from the worker, not cleared: `hidden` and
                    // `errored` both survive a rebuild, and this used to be
                    // the one status a routine chart interaction could erase
                    // while the pre-edit code was still what ran.
                    view.stale = stale;
                } else {
                    // A first answer for a slot nobody parked a kind for is
                    // an add the UI already removed — a layout switch takes
                    // a slot away before its worker has built it — and the
                    // remove always wins: a view born here would be a ghost
                    // with no registration behind it, on a chart the layout
                    // says is empty. The parked kind is what keeps two views
                    // from sharing a key and adopting each other's drawings.
                    let Some(kind) = self.pending_kinds.remove(&slot) else {
                        return;
                    };
                    let ordinal = self.free_ordinal(&kind);
                    self.views.push(IndicatorView {
                        slot,
                        kind,
                        ordinal,
                        label: IndicatorView::label_of(&descriptor),
                        descriptor,
                        columns,
                        rows,
                        bar_paints: bar_paint,
                        preview: None,
                        lane: Vec::new(),
                        error: None,
                        hidden: false,
                        mouse_vertical_line: false,
                        objects: ObjectSnapshot::default(),
                        input_values: inputs,
                        stale,
                        scale: PriceView::new(),
                        sizing: PaneSizing::Auto,
                        style: crate::indicator_style::StyleOverride::default(),
                        last_auto: None,
                    });
                }
            }
            IndicatorEvent::Appended { slot, row, paint } => {
                if let Some(view) = self.view_mut(slot) {
                    for (column, value) in view.columns.iter_mut().zip(&row) {
                        column.push(*value);
                    }
                    if let Some(color) = paint {
                        // Bars between the previous painted one and this one
                        // asked for nothing; filled in only now, so a chart
                        // where nothing paints never allocates the channel.
                        view.bar_paints.resize(view.rows, None);
                        view.bar_paints.push(Some(color));
                    }
                    view.rows += 1;
                    // The old preview described the bar that just closed;
                    // drawing it one slot further right would be a lie. The
                    // next Preview event replaces it within the same batch.
                    view.preview = None;
                    // Same for the rungs: they are prefixes of a bar that is
                    // no longer forming.
                    view.lane.clear();
                }
            }
            IndicatorEvent::Preview { slot, frame } => {
                if let Some(view) = self.view_mut(slot) {
                    view.preview = frame;
                }
            }
            IndicatorEvent::Lane { slot, samples } => {
                if let Some(view) = self.view_mut(slot) {
                    view.lane = samples;
                }
            }
            IndicatorEvent::Error { slot, error } => {
                if let Some(view) = self.view_mut(slot) {
                    view.error = Some(error);
                    view.preview = None;
                    view.lane.clear();
                }
            }
            IndicatorEvent::Objects { slot, objects } => {
                if let Some(view) = self.view_mut(slot) {
                    view.objects = objects;
                }
            }
            IndicatorEvent::ReloadFailed { slot, message } => {
                if let Some(view) = self.view_mut(slot) {
                    view.stale = Some(message);
                }
            }
        }
    }

    pub(crate) fn view_mut(&mut self, slot: SlotId) -> Option<&mut IndicatorView> {
        self.views.iter_mut().find(|v| v.slot == slot)
    }

    /// Whether any visible indicator paints candles at all.
    ///
    /// One question per frame instead of one per visible bar: on the ordinary
    /// chart, where nothing paints, this is the whole cost of the feature.
    pub(crate) fn paints_any(&self) -> bool {
        self.views.iter().any(|view| {
            !view.hidden
                && (!view.bar_paints.is_empty()
                    || view.preview.as_ref().is_some_and(|f| f.paint.is_some()))
        })
    }

    /// The candle paint for one committed bar, resolved across every visible
    /// indicator.
    ///
    /// Goes through [`resolve_bar_paint`] — the same function the host uses
    /// for the backtest and the bot — so a trader and a bot reading the same
    /// chart can never be shown different colours.
    pub(crate) fn bar_paint(&self, row: usize) -> Option<Rgba8> {
        resolve_bar_paint(self.views.iter().map(|view| view.bar_paint(row)))
    }

    /// The same, for the bar that is forming.
    pub(crate) fn forming_paint(&self) -> Option<Rgba8> {
        resolve_bar_paint(self.views.iter().map(IndicatorView::forming_paint))
    }

    /// The paint of one *drawn slot*, which past a certain zoom is several
    /// bars folded into one candle (`resample::for_each_group`).
    ///
    /// **The newest painted bar of the fold wins** — the channel's own
    /// last-one-wins rule, applied along time instead of across indicators.
    /// A compressed chart is a coarser record of the same tape, so a mark
    /// inside the fold stays visible rather than vanishing at exactly the
    /// zoom a trader reaches for to see the whole session. The colour then
    /// describes one of the bars in the slot, not the folded candle — which
    /// is the same bargain the fold itself already makes.
    ///
    /// `includes_forming` puts the forming bar at the newest end of the fold,
    /// where it outranks every committed bar sharing the slot.
    pub(crate) fn slot_paint(
        &self,
        rows: std::ops::Range<usize>,
        includes_forming: bool,
    ) -> Option<Rgba8> {
        if includes_forming && let Some(paint) = self.forming_paint() {
            return Some(paint);
        }
        rows.rev().find_map(|row| self.bar_paint(row))
    }

    /// Drop a slot UI-side (the worker gets the Remove command separately).
    pub(crate) fn remove(&mut self, slot: SlotId) {
        self.views.retain(|v| v.slot != slot);
        self.pending_kinds.remove(&slot);
    }

    /// Prepend `added` unknown rows to every column, keeping the views
    /// aligned with bars that just grew at the front.
    ///
    /// Older trades re-cut every bar, so the worker rebuilds from scratch —
    /// but that answer arrives a round-trip later, and until it does the
    /// renderer would draw every value `added` slots to the left of the
    /// candle it belongs to. `NaN` is the honest filler: it renders as a gap,
    /// which is exactly what "not computed yet" means here.
    pub(crate) fn shift_rows(&mut self, added: usize) {
        if added == 0 {
            return;
        }
        for view in &mut self.views {
            for column in &mut view.columns {
                column.splice(0..0, std::iter::repeat_n(f64::NAN, added));
            }
            // The paint channel is indexed by the same bar numbers, so it
            // shifts with them or every colour lands `added` candles to the
            // left of the bar that earned it. Only when it exists: an empty
            // channel has nothing to shift.
            if !view.bar_paints.is_empty() {
                view.bar_paints
                    .splice(0..0, std::iter::repeat_n(None, added));
            }
            view.rows += added;
            // The forming bar moved with the candles; its frame is stale.
            view.preview = None;
        }
    }

    /// Flip the render-side eye toggle.
    pub(crate) fn toggle_hidden(&mut self, slot: SlotId) {
        if let Some(view) = self.view_mut(slot) {
            view.hidden = !view.hidden;
        }
    }

    /// All views, in add order (for the manager UI).
    pub(crate) fn all(&self) -> &[IndicatorView] {
        &self.views
    }

    /// Overlay indicators that should draw on the price chart right now.
    pub(crate) fn visible_overlays(&self) -> impl Iterator<Item = &IndicatorView> {
        self.views
            .iter()
            .filter(|v| v.descriptor.overlay && !v.hidden && v.error.is_none())
    }

    /// Pane indicators that get a pane right now, capped at [`MAX_PANES`].
    pub(crate) fn visible_panes(&self) -> impl Iterator<Item = &IndicatorView> {
        self.views
            .iter()
            .filter(|v| is_visible_pane(v))
            .take(MAX_PANES)
    }

    /// What the layout needs to carve the pane band: one sizing per visible
    /// pane, top to bottom. Handed over as a whole rather than pane by pane,
    /// because how tall each one gets is one decision about all of them.
    ///
    /// Written into a caller-owned array rather than returned as a `Vec`:
    /// [`plot_split`](crate::plot_area::plot_split) runs more than once per frame,
    /// and there is no reason for a chart to reach the allocator sixty times a
    /// second for at most [`MAX_PANES`] copies of an eight-byte enum. Returns
    /// the slice actually written.
    pub(crate) fn pane_sizing<'a>(
        &self,
        buffer: &'a mut [PaneSizing; MAX_PANES],
    ) -> &'a [PaneSizing] {
        let mut count = 0;
        for view in self.visible_panes() {
            buffer[count] = view.sizing;
            count += 1;
        }
        &buffer[..count]
    }

    /// The same panes, in the same order, mutable: the renderer records the
    /// range it fitted and the axis gesture moves the scale, both on the view
    /// the pane rect belongs to.
    pub(crate) fn visible_panes_mut(&mut self) -> impl Iterator<Item = &mut IndicatorView> {
        self.views
            .iter_mut()
            .filter(|v| is_visible_pane(v))
            .take(MAX_PANES)
    }
}

/// Whether this indicator gets a pane of its own right now.
///
/// One predicate for both iterators, because the two are zipped against the
/// same rects: `plot_split` carves bands from `visible_panes().count()` and the
/// input pass walks `visible_panes_mut()`. A condition added to one and not the
/// other would misalign the zip silently — and a drag would then stretch the
/// pane next to the one whose numbers were grabbed.
pub fn is_visible_pane(view: &IndicatorView) -> bool {
    !view.descriptor.overlay && !view.hidden && view.error.is_none()
}
