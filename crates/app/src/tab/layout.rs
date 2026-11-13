//! The tab's canvas arrangement, owned whole: which panes the canvas shows,
//! the width and heights the trader gave them, the folds, the divider drag in
//! flight, and what the context panes not built yet will open with.

use eframe::egui;
use quantick_workspace::canvas::revealed;
use smallvec::SmallVec;

use super::CanvasLayout;
use super::context_resize::ContextStack;
use crate::canvas_layout::{LayoutPreset, MAX_CONTEXT_PANES, PaneIdAllocator, PaneKind};
use crate::pane::{ChartPane, DEFAULT_PANE_FRACTION, PaneSide};

/// The geometry owner of one tab's canvas. The tab holds one and reads its
/// panes against it; the painter writes the drawn rectangles back.
pub struct TabCanvas {
    /// Which panes this tab's canvas shows. In-session only for now: per-tab
    /// chrome persistence is the open question §14 leaves to `ui-state.toml`,
    /// and this field with `split_fraction` and the tab's focus is what it
    /// would write.
    pub layout: CanvasLayout,
    /// The canvas as the headless model holds it: the pane tree, the folds,
    /// the maximized pane and the floating ones. Picked layouts and the
    /// `canvas.*` capabilities change it; a layout the column painter cannot
    /// draw, or any fold or maximize, is painted from it.
    pub state: quantick_workspace::canvas::CanvasState,
    /// The context column's share of the canvas width while it is shown.
    ///
    /// Kept while the column is collapsed, which is what it springs back to.
    /// One number and one flag rather than two numbers: a separate "restore"
    /// field would be a second opinion about the same width.
    pub split_fraction: f32,
    /// Retained context heights and the geometry of their last drawn stack.
    pub(super) context_stack: ContextStack,
    /// Pixel width and opening direction of the divider drag in flight.
    pub(super) drag: Option<(f32, (bool, bool))>,
    /// The tree seam drag in flight, its travel counted from the press.
    pub(super) seam_drag: Option<quantick_workspace::canvas::SeamDrag>,
    /// The canvas the last drawn frame used. See `Tab::last_canvas_width`.
    pub(crate) last_canvas: egui::Rect,
    /// Which built time pane each time slot of [`Self::state`] shows, kept
    /// across edits so a removed chart never moves another; a picked layout
    /// seats them afresh.
    pub(crate) seats: quantick_workspace::canvas::Seats,
    /// The floating chart held by its grip or an edge, and what letting go
    /// or its own window asked for.
    pub(crate) floats: crate::floating_panes::FloatingPanes,
    /// The charts' ⋯ menus, their strips and the remove dialog.
    pub(crate) menus: crate::pane_menu::PaneMenus,
    /// Every pane band the last drawn frame laid out, header to strip.
    pub(crate) drawn_bands:
        SmallVec<[(PaneSide, egui::Rect); crate::canvas_layout::MAX_CANVAS_PANES]>,
    /// Set when the split is asked for and the time pane does not exist yet;
    /// drained by `Tab::apply_pending_layout` on the following frame.
    pub(super) pending_context_panes: usize,
    /// The interval the time pane opens on when it is first built. The
    /// header's default unless the feed declared one (`default_bars` with a
    /// time-showing `default_layout`); once the pane exists, its own header
    /// owns the interval and this is never read again.
    pub(super) time_pane_opening_interval_ms: i64,
    /// The interval each context pane opens on, by slot, when a restored
    /// workspace recorded more than one. A slot past the end of this list
    /// opens on `time_pane_opening_interval_ms`, which is what every tab did
    /// while the stack held one chart.
    context_opening_intervals_ms: SmallVec<[i64; MAX_CONTEXT_PANES]>,
    /// The layout each context pane opens on, by slot, from a restored
    /// workspace. A slot past the end takes what a fresh pane takes: the
    /// focused pane's layout.
    pub(super) context_opening: quantick_workspace::context_opening::ContextOpening,
    /// Whether the time pane opens with its indicator legend folded, for the
    /// same reason the interval above is stashed: a restored workspace names
    /// the fold a frame before the pane it belongs to exists.
    pub(super) time_pane_opening_legend_collapsed: bool,
}

impl Default for TabCanvas {
    fn default() -> Self {
        Self {
            layout: CanvasLayout::Single,
            state: quantick_workspace::canvas::CanvasState::from_preset(
                CanvasLayout::Single.preset(),
            ),
            split_fraction: DEFAULT_PANE_FRACTION,
            context_stack: ContextStack::default(),
            drag: None,
            seam_drag: None,
            last_canvas: egui::Rect::ZERO,
            drawn_bands: SmallVec::new(),
            menus: Default::default(),
            seats: Default::default(),
            floats: Default::default(),
            pending_context_panes: 0,
            time_pane_opening_interval_ms: crate::time_header::DEFAULT_INTERVAL_MS,
            context_opening_intervals_ms: SmallVec::new(),
            context_opening: Default::default(),
            time_pane_opening_legend_collapsed: false,
        }
    }
}

impl TabCanvas {
    /// The canvas half of `Tab::set_layout`. A layout chosen from the picker
    /// shows the panes its thumbnail drew, so the folds open even when the
    /// layout is the one already selected. Returns the focus the switch
    /// reveals, or `None` when the layout did not change, as past the cap.
    pub(super) fn switch_layout(
        &mut self,
        layout: CanvasLayout,
        built_context_panes: usize,
        focus: PaneSide,
    ) -> Option<PaneSide> {
        let previous = self.layout;
        let preset = layout.preset();
        if !self.state.apply_preset(preset) && self.state.preset() != Some(preset) {
            return None;
        }
        // `apply_preset` opened every fold. Leaving the column collapsed
        // meant the cell lit, two panes were
        // promised, and an 8 px rail arrived instead. Before the early return,
        // because picking the arrangement that is *already* selected is
        // exactly how a trader asks for the charts they can see promised in a
        // lit cell — and a return above this line answered that with the rail.
        self.expand_context_stack();
        self.seats.clear();
        if layout == previous {
            return None;
        }
        self.layout = layout;
        // Set, never raised: switching to a three-pane layout and back before
        // the next frame used to leave the count where the wider layout put
        // it, and the tab then built a pane no layout had asked for — seeded
        // from the whole retained tape and fed every trade thereafter.
        let wanted = preset.count(PaneKind::Time);
        self.pending_context_panes = wanted.saturating_sub(built_context_panes);
        Some(match revealed(previous.preset(), preset) {
            Some(PaneKind::Time) => PaneSide::Time(0),
            Some(_) => PaneSide::Flow,
            None => focus,
        })
    }

    /// The context pane due in `slot`, empty and opening on what a restored
    /// workspace named for that slot, or `None` when no layout change asked.
    pub(super) fn take_pending_pane(
        &mut self,
        slot: usize,
        ids: &mut PaneIdAllocator,
    ) -> Option<ChartPane> {
        // A chart the canvas holds beyond what a layout asked for (one added
        // or floating) is built like any other.
        let time = |(_, kind, _): &(_, PaneKind, _)| *kind == PaneKind::Time;
        let wanted = self.state.panes().iter().filter(|pane| time(pane)).count();
        self.pending_context_panes = (self.pending_context_panes).max(wanted.saturating_sub(slot));
        if self.pending_context_panes == 0 {
            return None;
        }
        self.pending_context_panes -= 1;
        let interval_ms = self
            .context_opening_intervals_ms
            .get(slot)
            .copied()
            .unwrap_or(self.time_pane_opening_interval_ms);
        let mut pane = ChartPane::time(ids.alloc(), interval_ms);
        let saved_style = self.context_opening.take_style(slot + 1);
        pane.presentation.candles =
            quantick_stores::pane_style::restore(saved_style.as_slice(), slot + 1);
        pane.legend_collapsed = self.time_pane_opening_legend_collapsed;
        pane.layout.opening = Some(
            self.context_opening
                .layouts
                .get(slot)
                .copied()
                .flatten()
                .map(crate::layouts::LayoutId),
        );
        Some(pane)
    }

    /// A new chart for `seat`, opening as a chart nobody saved: the canvas's
    /// opening interval, no saved layout.
    pub(super) fn fresh_pane(&self, seat: usize, ids: &mut PaneIdAllocator) -> ChartPane {
        let mut pane = ChartPane::time(ids.alloc(), self.time_pane_opening_interval_ms);
        pane.presentation.candles = quantick_stores::pane_style::restore(&[], seat + 1);
        pane.layout.opening = Some(None);
        pane
    }

    /// Name the layout a context pane will open on when the stack builds it.
    ///
    /// Only for a pane that is not standing yet — a canvas change lands its
    /// panes a frame after the layout that asked for them. A pane already on
    /// the canvas is moved by `QuantickApp::switch_pane_layout`, which
    /// carries its indicators and its drawings across too; writing
    /// `pane.layout` under a standing pane would leave the field disagreeing
    /// with what that chart is showing.
    #[cfg(any(feature = "scenario-harness", test))]
    pub fn set_opening_layout(&mut self, side: PaneSide, id: crate::layouts::LayoutId) {
        // The flow pane is built with the tab and is never pending, so the
        // only address that can be waiting is a context slot.
        let PaneSide::Time(slot) = side else {
            debug_assert!(false, "the flow pane is never waiting to be built");
            return;
        };
        if slot >= MAX_CONTEXT_PANES {
            return;
        }
        if self.context_opening.layouts.len() <= slot {
            self.context_opening.layouts.resize(slot + 1, None);
        }
        self.context_opening.layouts[slot] = Some(id.0);
    }

    /// The intervals the context charts not built yet open on, top first.
    /// None leaves what they would open on: the timeframe is the tab's.
    pub fn open_context_on(&mut self, context_intervals_ms: &[i64]) {
        // The top chart's interval is also the one every slot past the list
        // opens on, which is what a one-chart file has always meant.
        let Some(ms) = context_intervals_ms.first() else {
            return;
        };
        self.time_pane_opening_interval_ms = *ms;
        let intervals = context_intervals_ms.iter().copied();
        self.context_opening_intervals_ms = intervals.take(MAX_CONTEXT_PANES).collect();
    }

    /// Put the context column, or the flow pane (`flow`), away to its rail or
    /// bring it back: one fold in the canvas model, whichever painter draws.
    /// Folding one side opens the other. Answers whether anything changed.
    pub fn fold(&mut self, flow: bool, collapsed: bool) -> bool {
        self.state.fold_flow_side(flow, collapsed)
    }

    /// Whether the context column and the flow pane are folded.
    pub(crate) fn folds(&self) -> (bool, bool) {
        self.state.flow_side_folds()
    }

    /// Whether the column painter draws this frame; anything else is painted
    /// from [`Self::state`]'s tree.
    pub(crate) fn column_painted(&self) -> bool {
        (self.state).column_painted(self.layout.preset(), self.layout.is_column())
    }

    /// One split on a column layout, owned by the painter drawing: see
    /// `CanvasState::mirror_split`.
    pub(crate) fn mirror_split(&mut self, columns: bool) {
        let (preset, split) = (self.layout.preset(), &mut self.split_fraction);
        let _ = self.layout.is_column() && self.state.mirror_split(preset, split, columns);
    }

    /// Put the column painter's split where the layout's tree puts the flow
    /// pane, so a picked layout draws exactly what its thumbnail drew.
    pub(crate) fn split_to_preset(&mut self) {
        if self.layout.is_column() && self.layout.shows_time() {
            self.split_fraction = (self.layout.preset().flow_left()).unwrap_or(self.split_fraction);
        }
    }
}

/// The layout a hook or a config names: a config word first (so
/// `time+time+flow` stays the stacked column it always was), else a preset id.
#[cfg(any(feature = "scenario-harness", test))]
pub(crate) fn named(name: &str) -> Option<CanvasLayout> {
    let declared = crate::config::DeclaredLayout::parse(name).map(CanvasLayout::from);
    declared.or_else(|| crate::canvas_layout::preset(name).map(CanvasLayout::from_preset))
}

/// Lay `tab` out as `preset`, the split where the preset's tree puts it:
/// the one way every picker, menu and call applies a layout. Answers whether
/// the canvas changed.
pub(crate) fn pick(tab: &mut super::Tab, preset: &'static LayoutPreset) -> bool {
    let read = |canvas: &TabCanvas| (canvas.layout, canvas.state.clone(), canvas.split_fraction);
    let before = read(&tab.canvas);
    tab.set_layout(CanvasLayout::from_preset(preset));
    if tab.canvas.layout.preset() == preset {
        tab.canvas.split_to_preset();
    }
    before != read(&tab.canvas)
}
