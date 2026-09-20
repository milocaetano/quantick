//! One indicator as the UI sees it: the columns, the paint and the pane
//! geometry of a single instance.

use std::sync::Arc;

use quantick_indicators::{EvalError, IndicatorDescriptor, InputValue, ObjectSnapshot, PreviewFrame, Rgba8};

use crate::indicator_style::ResolvedPlot;
use crate::indicator_worker::{LaneSample, SlotId};
use crate::price_view::PriceView;

use super::PaneSizing;

/// Fraction of the chart's height each indicator pane takes (plan §4.3:
/// fixed fraction v1, draggable dividers later).
pub const PANE_HEIGHT_FRAC: f32 = 0.20;

/// At most this many panes; further pane indicators wait until one is
/// removed (the honest alternative to shrinking panes into unreadability).
pub const MAX_PANES: usize = 3;

/// One indicator as the UI sees it.
pub struct IndicatorView {
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
    /// Mirror the price chart's hovered x coordinate into this non-overlay
    /// pane as a subtle vertical guide. Render-only and persisted by layout.
    pub mouse_vertical_line: bool,
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
    /// [`crate::pane::PaneFrame::auto_range`] performs for the candles.
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
    pub(super) fn label_of(descriptor: &IndicatorDescriptor) -> Arc<str> {
        Arc::from(
            descriptor
                .short_title
                .as_deref()
                .unwrap_or(&descriptor.title),
        )
    }
}
