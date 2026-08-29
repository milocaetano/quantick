//! UI-side indicator state: the columns the renderer reads.
//!
//! The worker owns the truth (the [`IndicatorHost`]); this module owns the
//! UI's *copy* of it, kept in sync by applying delta events each frame — the
//! same shape as the `FeedEvent` pattern. No locks anywhere near the render
//! path: the renderer reads plain vectors this struct owns.
//!
//! [`IndicatorHost`]: quantick_indicators::IndicatorHost

pub(crate) mod library;
pub(crate) mod preset_file;
pub(crate) mod state_file;

use eframe::egui;
use quantick_indicators::{
    EvalError, IndicatorDescriptor, InputValue, ObjectSnapshot, PreviewFrame, Rgba8,
    resolve_bar_paint,
};

use std::sync::Arc;

use crate::indicator_style::ResolvedPlot;
use crate::indicator_worker::{IndicatorEvent, LaneSample, SlotId};
use crate::price_view::PriceView;

/// Fraction of the chart's height each indicator pane takes (plan §4.3:
/// fixed fraction v1, draggable dividers later).
pub(crate) const PANE_HEIGHT_FRAC: f32 = 0.20;
/// At most this many panes; further pane indicators wait until one is
/// removed (the honest alternative to shrinking panes into unreadability).
pub(crate) const MAX_PANES: usize = 3;

/// One indicator as the UI sees it.
pub(crate) struct IndicatorView {
    /// The UI-allocated slot this instance answers to.
    pub slot: SlotId,
    /// The constructor it was added through (`native.cvd`, `script.zigzag`),
    /// durable across remove + re-add in a way the slot id is not — see
    /// [`crate::indicator_worker::IndicatorSource::kind_id`]. Drawings
    /// anchored to this pane are keyed on it.
    ///
    /// Shared rather than cloned: it is copied into a [`crate::drawings::PaneKey`]
    /// on every band carve, which runs twice per pane per frame.
    pub kind: Arc<str>,
    /// Which instance of that kind this is, assigned **once, at birth** as
    /// the lowest ordinal no live view of the kind is using.
    ///
    /// Deliberately not a position in `views`: a positional ordinal would be
    /// renumbered by removing an earlier pane of the same kind, and the
    /// survivor would inherit the removed pane's annotations — one pane's
    /// marks painted on another's axis, which is the data-honesty failure
    /// this key exists to prevent.
    pub ordinal: u8,
    /// Descriptor as of the last rebuild (title, plots, overlay flag).
    pub descriptor: IndicatorDescriptor,
    /// The descriptor's display name, shareable and kept in step with it.
    pub label: Arc<str>,
    /// Committed plot columns, one per descriptor plot, kept in lockstep
    /// with the worker via Rebuilt/Appended deltas.
    pub columns: Vec<Vec<f64>>,
    /// Committed rows, tracked rather than derived from `columns`: an
    /// indicator whose whole output is candle paint declares no plots at all,
    /// and `columns.first()` would answer 0 for every bar it ever evaluated.
    pub rows: usize,
    /// The candle paint of each committed bar (`barcolor`), mirrored from the
    /// worker. Empty for the indicators that never paint — which is all of
    /// them until a script asks — and otherwise as long as the last painted
    /// bar; reads past the end are "no paint", exactly as in the buffer this
    /// mirrors.
    ///
    /// Plural for the column, singular for one bar:
    /// [`bar_paint`](IndicatorView::bar_paint) answers about a row and applies
    /// the eye toggle, which reading this field directly does not.
    pub bar_paints: Vec<Option<Rgba8>>,
    /// Latest forming-bar frame, if a bar is forming.
    pub preview: Option<PreviewFrame>,
    /// The forming bar sampled across the live lane's window, oldest rung
    /// first — what this indicator showed at each instant on the tape.
    ///
    /// Transient like `preview`, and for the same reason: it describes a bar
    /// that has not closed. Empty whenever the chart has no lane.
    pub lane: Vec<LaneSample>,
    /// Error state (indicator disabled worker-side until rebuilt).
    pub error: Option<EvalError>,
    /// Eye toggle: hidden is render-side only — no recompute, state keeps
    /// flowing so unhiding is instant.
    pub hidden: bool,
    /// Committed draw objects (a preview's transient set, when present,
    /// replaces this at render time).
    pub objects: ObjectSnapshot,
    /// The values currently bound to the declared inputs (what the
    /// settings dialog opens with).
    pub input_values: Vec<InputValue>,
    /// A failed hot reload's errors: the running version is stale relative
    /// to the file on disk, and the panel says so.
    pub stale: Option<String>,
    /// This pane's vertical scale: auto-fits its visible values until the
    /// user drags the pane's own y-axis, then holds the range they set — the
    /// candles' price axis rule, applied per pane.
    ///
    /// Render-side state, like `hidden`, and it lives on the view rather than
    /// in a slot-keyed map on the side: a removed indicator takes its scale
    /// with it, so a later pane can never inherit a range someone set for a
    /// different series.
    pub scale: PriceView,
    /// How tall this pane asks to be: the layout's call until the user drags
    /// its divider or collapses it by hand.
    ///
    /// Render-side state like `hidden` and `scale`, and on the view for the
    /// same reason: a removed indicator takes its height with it, so a later
    /// pane can never inherit a size someone set for a different series.
    pub sizing: PaneSizing,
    /// Per-plot style the trader set in the settings dialog, layered over what
    /// the indicator declared.
    ///
    /// Render-side state like `hidden`, `scale` and `sizing`, and on the view
    /// for the same reason: a removed indicator takes its colours with it, so a
    /// later pane can never inherit a palette someone chose for a different
    /// series. It also survives the rebuild an input edit triggers, which
    /// replaces `descriptor` wholesale — styling an EMA and then changing its
    /// period must not throw the colour away.
    pub style: crate::indicator_style::StyleOverride,
    /// The auto-fitted `(lo, hi)` the last frame drew this pane with.
    ///
    /// The gesture that zooms the pane runs before the frame that draws it,
    /// so it needs the range the renderer actually used — the same handshake
    /// `last_auto_range` performs for the candles.
    pub last_auto: Option<(f64, f64)>,
}

impl IndicatorView {
    /// The candle paint this indicator asks for on one committed bar.
    ///
    /// A hidden indicator asks for nothing: the eye toggle is the trader
    /// saying "not now", and a pane that vanished while its colours stayed on
    /// the candles would be unexplainable from the screen.
    pub(crate) fn bar_paint(&self, row: usize) -> Option<Rgba8> {
        if self.hidden {
            return None;
        }
        self.bar_paints.get(row).copied().flatten()
    }

    /// The paint this indicator asks for on the bar that is forming.
    pub(crate) fn forming_paint(&self) -> Option<Rgba8> {
        if self.hidden {
            return None;
        }
        self.preview.as_ref().and_then(|frame| frame.paint)
    }

    /// The draw objects to render right now: the forming bar's transient
    /// set while a preview is live, else the committed set (latest-wins,
    /// like plot previews).
    pub(crate) fn render_objects(&self) -> &ObjectSnapshot {
        self.preview
            .as_ref()
            .and_then(|frame| frame.objects.as_ref())
            .unwrap_or(&self.objects)
    }

    /// The style plot `index` draws with right now: the declaration with the
    /// trader's layer applied, or `None` when this indicator has no such plot.
    ///
    /// The one door between the style layer and the renderer — every plot the
    /// chart draws is resolved through here, so a colour changed in the dialog
    /// cannot reach one drawing path and miss another. Two `Vec` lookups and a
    /// few `Option`s: it runs once per plot per frame and allocates nothing.
    pub(crate) fn plot_style(&self, index: usize) -> Option<ResolvedPlot> {
        let spec = self.descriptor.plots.get(index)?;
        Some(self.style.resolve(index, spec))
    }

    /// The label the UI shows for this indicator.
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    /// The same label, shareable. The band carve runs twice per pane per
    /// frame and would otherwise clone this string every time.
    pub(crate) fn label_shared(&self) -> Arc<str> {
        Arc::clone(&self.label)
    }

    /// What a descriptor calls itself: the short title when it has one.
    fn label_of(descriptor: &IndicatorDescriptor) -> Arc<str> {
        Arc::from(
            descriptor
                .short_title
                .as_deref()
                .unwrap_or(&descriptor.title),
        )
    }
}

/// Every indicator the UI knows about, in add order, plus the slot counter.
#[derive(Default)]
pub(crate) struct IndicatorViews {
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
    /// [`plot_split`](crate::app::plot_split) runs more than once per frame,
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
fn is_visible_pane(view: &IndicatorView) -> bool {
    !view.descriptor.overlay && !view.hidden && view.error.is_none()
}

/// Shortest a pane may be drawn and still be read.
///
/// Added up rather than guessed, from what a pane actually has to fit:
///
/// | Piece | Pixels |
/// | --- | --- |
/// | Title + live value row (`PANE_LABEL_FONT_PX` + its inset, top and bottom) | 28 |
/// | Two axis labels: `AXIS_LABEL_MIN_GAP_PX` between, `AXIS_LABEL_EDGE_MARGIN_PX` clear of each edge | 32 |
/// | Curve amplitude worth reading a shape from | 40 |
///
/// A hundred pixels. Below it the labels start dropping out and the trace has
/// nowhere to move, so the pane is chrome with a squiggle in it — and the
/// honest move is to stop drawing the curve and say so
/// ([`PaneSlot::collapsed`]) rather than to draw something unreadable.
///
/// The number matters: at the smallest window the app allows, three panes come
/// to about 81 px each, so a floor set below that would never bite and this
/// whole rule would be dead code.
pub(crate) const MIN_PANE_HEIGHT_PX: f32 = 100.0;
/// Height of a collapsed pane: one row, enough for its name and its live
/// value. Never zero — a pane that vanished would be an indicator the user
/// added and the chart silently dropped.
pub(crate) const COLLAPSED_PANE_HEIGHT_PX: f32 = 20.0;
/// Shortest the candles may be squeezed to by the pane band. The candles are
/// what the panes are *about*; a band that leaves them a sliver has inverted
/// the chart.
pub(crate) const MIN_CHART_HEIGHT_PX: f32 = 120.0;

/// What decides one pane's height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PaneSizing {
    /// The layout decides: a share of the band, floored, collapsing when the
    /// band cannot hold it.
    Auto,
    /// The user dragged this pane's divider to a height in pixels. Still
    /// floored — a drag cannot produce a pane too short to read either.
    Manual(f32),
    /// The user collapsed it by hand. Stays collapsed however much room
    /// appears, until they expand it again.
    Collapsed,
}

impl PaneSizing {
    /// Height this sizing asks for in a band `band_height_px` tall, before the
    /// layout decides whether there is room for it.
    fn desired(self, band_height_px: f32) -> f32 {
        match self {
            Self::Auto => (band_height_px * PANE_HEIGHT_FRAC).max(MIN_PANE_HEIGHT_PX),
            Self::Manual(px) => px.max(MIN_PANE_HEIGHT_PX),
            Self::Collapsed => COLLAPSED_PANE_HEIGHT_PX,
        }
    }
}

/// One pane's band, and whether it is showing its curve or only its name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PaneSlot {
    pub rect: egui::Rect,
    /// `true`: too little room to draw this pane legibly, so it is a labelled
    /// strip instead. Collapsing hides the *curve*; the name and the live
    /// value are still written, because those are what the user added the
    /// indicator for.
    pub collapsed: bool,
}

/// Carve the pane band off the bottom of the chart rect.
///
/// Two rules the old fraction-only split had neither of:
///
/// - **Nothing is drawn below the height at which it can be read.** A pane
///   that cannot have [`MIN_PANE_HEIGHT_PX`] becomes a collapsed strip rather
///   than a squeezed one. Three slivers are worth less than one readable pane
///   and two labelled strips.
/// - **The candles keep [`MIN_CHART_HEIGHT_PX`].** Panes are read against the
///   bars; a band that eats the bars has inverted the chart.
///
/// Room is granted top-down, so the first pane — the one the user added first,
/// and the one the eye lands on — is the last to lose its curve. Every pane
/// always gets at least a strip: an indicator that is on must never be
/// silently absent.
///
/// Pure, so the split is unit-testable without a display.
pub(crate) fn split_panes(chart: egui::Rect, sizing: &[PaneSizing]) -> (egui::Rect, Vec<PaneSlot>) {
    let count = sizing.len().min(MAX_PANES);
    if count == 0 {
        return (chart, Vec::new());
    }
    let band_height = chart.height().max(0.0);
    // Strips are mandatory chrome; only the expansion above a strip has to be
    // negotiated for.
    let mandatory = COLLAPSED_PANE_HEIGHT_PX * count as f32;
    let mut budget = (band_height - MIN_CHART_HEIGHT_PX - mandatory).max(0.0);

    // Explicit choices are served first, in two passes over the same order:
    // a pane the user dragged or expanded by hand must get its height even
    // when the automatic ones above it would have spent the budget. Without
    // this, clicking a collapsed strip open is a click that does nothing.
    let mut heights = vec![COLLAPSED_PANE_HEIGHT_PX; count];
    for explicit in [true, false] {
        for (index, sizing) in sizing[..count].iter().enumerate() {
            if matches!(sizing, PaneSizing::Manual(_)) != explicit {
                continue;
            }
            if matches!(sizing, PaneSizing::Collapsed) {
                continue;
            }
            let desired = sizing.desired(band_height);
            let extra = desired - COLLAPSED_PANE_HEIGHT_PX;
            if extra <= budget {
                budget -= extra;
                heights[index] = desired;
            }
        }
    }

    let total: f32 = heights.iter().sum();
    let chart_bottom = (chart.bottom() - total).max(chart.top());
    let shrunk = egui::Rect::from_min_max(chart.min, egui::pos2(chart.right(), chart_bottom));
    let mut top = chart_bottom;
    let panes = heights
        .into_iter()
        .map(|height| {
            let rect = egui::Rect::from_min_max(
                egui::pos2(chart.left(), top),
                egui::pos2(chart.right(), top + height),
            );
            top += height;
            PaneSlot {
                rect,
                collapsed: height <= COLLAPSED_PANE_HEIGHT_PX,
            }
        })
        .collect();
    (shrunk, panes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_indicators::{PlotId, PlotSpec, PlotStyle, Rgba8};

    fn descriptor(overlay: bool, plots: usize) -> IndicatorDescriptor {
        IndicatorDescriptor {
            title: "test".to_owned(),
            short_title: None,
            overlay,
            plots: (0..plots)
                .map(|i| PlotSpec {
                    id: PlotId::new(i),
                    title: format!("p{i}"),
                    style: PlotStyle::Line,
                    base_color: Rgba8::opaque(255, 255, 255),
                    width: 1.0,
                    offset: 0,
                    marker: None,
                })
                .collect(),
            inputs: Vec::new(),
            fills: Vec::new(),
        }
    }

    #[test]
    fn deltas_reconstruct_the_columns() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.apply(IndicatorEvent::rebuilt(
            slot,
            descriptor(true, 2),
            vec![vec![1.0], vec![10.0]],
        ));
        views.apply(IndicatorEvent::appended(slot, vec![2.0, 20.0]));
        let view = &views.all()[0];
        assert_eq!(view.rows, 2);
        assert_eq!(view.columns[0], vec![1.0, 2.0]);
        assert_eq!(view.columns[1], vec![10.0, 20.0]);
    }

    #[test]
    fn appended_row_invalidates_the_preview() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.apply(IndicatorEvent::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![]],
        ));
        views.apply(IndicatorEvent::Preview {
            slot,
            frame: Some(PreviewFrame::new(vec![5.0])),
        });
        assert!(views.all()[0].preview.is_some());
        views.apply(IndicatorEvent::appended(slot, vec![1.0]));
        assert!(
            views.all()[0].preview.is_none(),
            "a frame describing the closed bar must not draw one slot further right"
        );
    }

    const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
    const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);

    /// A paint-only indicator: no plots at all, which is exactly the shape
    /// `force_bar.pine` has and the shape a row count derived from `columns`
    /// would get wrong.
    fn paint_only(views: &mut IndicatorViews, rows: usize) -> SlotId {
        let slot = views.allocate_slot("script.force_bar");
        views.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: descriptor(true, 0),
            columns: Vec::new(),
            bar_paint: Vec::new(),
            rows,
            inputs: Vec::new(),
            stale: None,
        });
        slot
    }

    #[test]
    fn a_late_first_paint_lands_on_the_bar_that_asked_for_it() {
        // The trap: an indicator with no plots and no paint yet has nothing
        // to count rows from, and a colour arriving at bar 7 would land on
        // bar 0 — the wrong candle, silently.
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 7);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });

        assert_eq!(views.bar_paint(7), Some(RED), "the bar that asked");
        assert_eq!(views.bar_paint(0), None, "and no other");
        assert_eq!(views.all()[0].rows, 8);
    }

    /// Past a certain zoom the chart folds several bars into one candle. The
    /// paint has to survive that fold, or the marks disappear at exactly the
    /// zoom a trader reaches for to see the whole session.
    #[test]
    fn a_folded_slot_wears_the_newest_paint_it_contains() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        for paint in [Some(RED), None, Some(BLUE), None] {
            views.apply(IndicatorEvent::Appended {
                slot,
                row: Vec::new(),
                paint,
            });
        }

        // A slot folding bars 0..4: RED at 0, BLUE at 2, and BLUE is newer.
        assert_eq!(views.slot_paint(0..4, false), Some(BLUE));
        // A slot folding only 0..2 never sees the blue one.
        assert_eq!(views.slot_paint(0..2, false), Some(RED));
        // One that folds only unpainted bars stays unpainted.
        assert_eq!(views.slot_paint(3..4, false), None);
    }

    #[test]
    fn the_forming_bar_outranks_every_committed_bar_in_its_slot() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Preview {
            slot,
            frame: Some(PreviewFrame {
                paint: Some(BLUE),
                ..PreviewFrame::new(Vec::new())
            }),
        });

        assert_eq!(
            views.slot_paint(0..4, true),
            Some(BLUE),
            "the forming bar is the newest end of the fold"
        );
        assert_eq!(
            views.slot_paint(0..4, false),
            Some(RED),
            "a slot the forming bar does not belong to ignores it"
        );
    }

    #[test]
    fn a_hidden_indicator_paints_nothing() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Preview {
            slot,
            frame: Some(PreviewFrame {
                paint: Some(RED),
                ..PreviewFrame::new(Vec::new())
            }),
        });
        assert_eq!(views.bar_paint(0), Some(RED));
        assert_eq!(views.forming_paint(), Some(RED));

        views.toggle_hidden(slot);
        assert_eq!(
            views.bar_paint(0),
            None,
            "the eye is the trader saying not now; colours left on the \
             candles would be unexplainable from the screen"
        );
        assert_eq!(views.forming_paint(), None);
        assert!(!views.paints_any());
    }

    #[test]
    fn the_last_view_to_ask_paints_the_bar() {
        let mut views = IndicatorViews::new();
        let lower = paint_only(&mut views, 0);
        let upper = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot: lower,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Appended {
            slot: upper,
            row: Vec::new(),
            paint: Some(BLUE),
        });
        assert_eq!(views.bar_paint(0), Some(BLUE));

        // Bar 1: only the lower one asks, so it shows through.
        views.apply(IndicatorEvent::Appended {
            slot: lower,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Appended {
            slot: upper,
            row: Vec::new(),
            paint: None,
        });
        assert_eq!(views.bar_paint(1), Some(RED));
    }

    #[test]
    fn prepended_history_carries_the_paint_with_it() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });
        assert_eq!(views.bar_paint(0), Some(RED));

        views.shift_rows(3);
        assert_eq!(
            views.bar_paint(3),
            Some(RED),
            "older bars arrived in front; the colour moved with its bar"
        );
        assert_eq!(views.bar_paint(0), None);
    }

    #[test]
    fn an_ordinary_chart_never_looks_up_paint() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("native.ema");
        views.apply(IndicatorEvent::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![1.0, 2.0]],
        ));
        assert!(
            !views.paints_any(),
            "no indicator paints, so the renderer skips the channel entirely"
        );
        assert_eq!(views.bar_paint(0), None);
    }

    #[test]
    fn events_for_removed_slots_are_dropped() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.apply(IndicatorEvent::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![]],
        ));
        views.remove(slot);
        views.apply(IndicatorEvent::appended(slot, vec![1.0]));
        assert!(views.all().is_empty(), "the remove always wins the race");
    }

    /// A slot removed before its first build answered: the answer must not
    /// resurrect it. The layout switch is the production path that races
    /// this way — the add's `Rebuilt` was still on the channel.
    #[test]
    fn a_first_build_for_a_removed_slot_is_dropped_too() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.remove(slot);
        views.apply(IndicatorEvent::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![]],
        ));
        assert!(
            views.all().is_empty(),
            "no ghost view for a slot the remove took"
        );
    }

    #[test]
    fn overlay_and_pane_filters_respect_state() {
        let mut views = IndicatorViews::new();
        for (overlay, hidden, errored) in [
            (true, false, false),  // drawn overlay
            (true, true, false),   // hidden overlay
            (false, false, false), // drawn pane
            (false, false, true),  // errored pane
        ] {
            let slot = views.allocate_slot("test.indicator");
            views.apply(IndicatorEvent::rebuilt(
                slot,
                descriptor(overlay, 1),
                vec![vec![]],
            ));
            if hidden {
                views.toggle_hidden(slot);
            }
            if errored {
                views.apply(IndicatorEvent::Error {
                    slot,
                    error: EvalError {
                        bar_index: 0,
                        message: "boom".to_owned(),
                    },
                });
            }
        }
        assert_eq!(views.visible_overlays().count(), 1);
        assert_eq!(views.visible_panes().count(), 1);
    }

    /// A pane's scale belongs to the indicator, not to the slot's position on
    /// screen: removing one and adding another must not hand the newcomer a
    /// range someone set for a different series.
    #[test]
    fn a_removed_pane_takes_its_scale_with_it() {
        let mut views = IndicatorViews::new();
        let first = views.allocate_slot("test.indicator");
        views.apply(IndicatorEvent::rebuilt(
            first,
            descriptor(false, 1),
            vec![vec![1.0]],
        ));
        let view = views.view_mut(first).expect("the pane is there");
        view.last_auto = Some((0.0, 10.0));
        view.scale.zoom(0.5, (0.0, 10.0));
        assert!(!views.all()[0].scale.is_auto());

        views.remove(first);
        let second = views.allocate_slot("test.indicator");
        views.apply(IndicatorEvent::rebuilt(
            second,
            descriptor(false, 1),
            vec![vec![1.0]],
        ));
        let fresh = &views.all()[0];
        assert!(fresh.scale.is_auto(), "a new pane fits its own values");
        assert!(fresh.last_auto.is_none(), "and has not been drawn yet");
    }

    fn band(height: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, height))
    }

    fn auto(n: usize) -> Vec<PaneSizing> {
        vec![PaneSizing::Auto; n]
    }

    #[test]
    fn pane_split_is_stacked_and_bounded() {
        let chart = band(1000.0);
        let (shrunk, panes) = split_panes(chart, &auto(2));
        assert_eq!(shrunk.height(), 600.0, "two panes take 2 x 20%");
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].rect.top(), 600.0);
        assert_eq!(panes[1].rect.top(), 800.0);
        assert_eq!(panes[1].rect.bottom(), 1000.0);
        assert!(panes.iter().all(|pane| !pane.collapsed), "there is room");

        let (unchanged, none) = split_panes(chart, &auto(0));
        assert_eq!(unchanged, chart);
        assert!(none.is_empty());

        let (_, capped) = split_panes(chart, &auto(9));
        assert_eq!(capped.len(), MAX_PANES);
    }

    /// The headline of this change: a band too short for three readable panes
    /// gives room to the ones it can and collapses the rest, instead of
    /// handing all three a sliver. Room goes top-down, so the pane the user
    /// added first is the last to lose its curve.
    #[test]
    fn a_short_band_collapses_the_panes_it_cannot_draw_legibly() {
        let (chart, panes) = split_panes(band(300.0), &auto(3));

        assert_eq!(panes.len(), 3, "every pane is still present");
        assert!(!panes[0].collapsed, "the first keeps its curve");
        assert!(
            panes[1].collapsed && panes[2].collapsed,
            "300 px cannot hold two readable panes and the candles' own floor"
        );
        for pane in &panes {
            assert!(
                pane.rect.height() >= COLLAPSED_PANE_HEIGHT_PX,
                "no pane is ever thinner than its own name"
            );
            assert!(
                pane.collapsed || pane.rect.height() >= MIN_PANE_HEIGHT_PX,
                "an expanded pane always clears the readable floor: {pane:?}"
            );
        }
        assert!(
            chart.height() >= MIN_CHART_HEIGHT_PX,
            "and the candles keep their floor: {}",
            chart.height()
        );
    }

    /// The floor holds all the way down. At the smallest window the app
    /// allows, three panes are three labelled strips and the candles are still
    /// candles — the state the old split turned into three unreadable bands.
    #[test]
    fn the_floors_hold_at_every_band_height() {
        for height in [0.0_f32, 60.0, 120.0, 200.0, 300.0, 560.0, 1000.0] {
            let (chart, panes) = split_panes(band(height), &auto(MAX_PANES));
            let total: f32 = panes.iter().map(|pane| pane.rect.height()).sum();
            assert!(
                (chart.height() + total - height.max(0.0)).abs() < 0.01 || chart.height() == 0.0,
                "height {height}: the band and the chart must tile the space"
            );
            for pane in &panes {
                assert!(
                    pane.collapsed || pane.rect.height() >= MIN_PANE_HEIGHT_PX,
                    "height {height}: an expanded pane below the floor: {pane:?}"
                );
            }
            assert!(
                panes
                    .windows(2)
                    .all(|pair| { (pair[0].rect.bottom() - pair[1].rect.top()).abs() < 0.01 }),
                "height {height}: panes must not overlap or leave a seam"
            );
        }
    }

    /// A pane the user collapsed by hand stays collapsed however much room
    /// appears — the automatic rule decides what *cannot* be shown, never what
    /// the user decided not to show.
    #[test]
    fn a_hand_collapsed_pane_stays_collapsed_in_a_tall_band() {
        let (_, panes) = split_panes(band(1000.0), &[PaneSizing::Collapsed, PaneSizing::Auto]);
        assert!(panes[0].collapsed, "the user's choice survives the room");
        assert!(!panes[1].collapsed);
    }

    /// A dragged height is honoured, and still cannot go below the floor: the
    /// divider stops rather than producing a pane nobody can read.
    #[test]
    fn a_dragged_height_is_honoured_but_never_below_the_floor() {
        let (_, tall) = split_panes(band(1000.0), &[PaneSizing::Manual(300.0)]);
        assert!((tall[0].rect.height() - 300.0).abs() < 0.01);

        let (_, squeezed) = split_panes(band(1000.0), &[PaneSizing::Manual(5.0)]);
        assert!(
            (squeezed[0].rect.height() - MIN_PANE_HEIGHT_PX).abs() < 0.01,
            "the drag stops at the floor: {:?}",
            squeezed[0]
        );
    }
}
