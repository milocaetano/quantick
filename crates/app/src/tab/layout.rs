//! The tab's canvas arrangement, owned whole: which panes the canvas shows,
//! the width and heights the trader gave them, the folds, the divider drag in
//! flight, and what the context panes not built yet will open with.

use eframe::egui;
use smallvec::SmallVec;

use super::CanvasLayout;
use super::context_resize::ContextStack;
use crate::canvas_layout::{MAX_CONTEXT_PANES, PaneIdAllocator, PaneKind};
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
    /// Whether the context column is collapsed to its rail.
    pub context_collapsed: bool,
    /// Whether the flow pane is folded against the right canvas edge.
    pub flow_collapsed: bool,
    /// Retained context heights and the geometry of their last drawn stack.
    pub(super) context_stack: ContextStack,
    /// Pixel width and opening direction of the divider drag in flight.
    pub(super) drag: Option<(f32, bool, bool)>,
    /// The canvas the last drawn frame used. See `Tab::last_canvas_width`.
    pub(crate) last_canvas: egui::Rect,
    /// Every pane band the last drawn frame laid out, header to strip.
    pub(crate) drawn_bands: SmallVec<[(PaneSide, egui::Rect); crate::canvas_layout::MAX_CANVAS_PANES]>,
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
            context_collapsed: false,
            flow_collapsed: false,
            context_stack: ContextStack::default(),
            drag: None,
            last_canvas: egui::Rect::ZERO,
            drawn_bands: SmallVec::new(),
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
    /// reveals, or `None` when the layout did not change.
    pub(super) fn switch_layout(
        &mut self,
        layout: CanvasLayout,
        built_context_panes: usize,
        focus: PaneSide,
    ) -> Option<PaneSide> {
        let previous = self.layout;
        // Leaving the column collapsed meant the cell lit, two panes were
        // promised, and an 8 px rail arrived instead. Before the early return,
        // because picking the arrangement that is *already* selected is
        // exactly how a trader asks for the charts they can see promised in a
        // lit cell — and a return above this line answered that with the rail.
        self.context_collapsed = false;
        self.flow_collapsed = false;
        self.expand_context_stack();
        self.state.apply_preset(layout.preset());
        if layout == previous {
            return None;
        }
        self.layout = layout;
        let wanted = layout
            .kinds()
            .iter()
            .filter(|kind| matches!(kind, PaneKind::Time))
            .count();
        // Set, never raised: switching to a three-pane layout and back before
        // the next frame used to leave the count where the wider layout put
        // it, and the tab then built a pane no layout had asked for — seeded
        // from the whole retained tape and fed every trade thereafter.
        self.pending_context_panes = wanted.saturating_sub(built_context_panes);
        Some(match layout {
            CanvasLayout::Single => PaneSide::Flow,
            CanvasLayout::Time => PaneSide::Time(0),
            // The split reveals whichever pane the previous layout was not
            // showing: the time pane coming from Single, the flow pane coming
            // from Time.
            _ if !layout.shows_flow() => PaneSide::Time(0),
            _ if !layout.shows_time() => PaneSide::Flow,
            _ if !previous.shows_time() => PaneSide::Time(0),
            _ if !previous.shows_flow() => PaneSide::Flow,
            _ => focus,
        })
    }

    /// The context pane due in `slot`, empty and opening on what a restored
    /// workspace named for that slot, or `None` when no layout change asked.
    pub(super) fn take_pending_pane(
        &mut self,
        slot: usize,
        ids: &mut PaneIdAllocator,
    ) -> Option<ChartPane> {
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

    /// Put the context column away, or bring it back.
    ///
    /// **The one collapse path.** The divider drag, the rail, the View menu,
    /// `Ctrl+0` and `layout.pane.collapse` all arrive here. Before this
    /// existed the only way a *trader* could collapse the column was a mouse
    /// drag, while an assistant had a named call for it — the second operator
    /// holding a capability the first could not reach from the keyboard,
    /// which is the rule inverted.
    ///
    /// `split_fraction` is deliberately untouched: it is the width the column
    /// springs back to, and spending it here would hand back a different chart
    /// from the one that was put away.
    ///
    /// Returns whether anything changed.
    pub fn set_context_collapsed(&mut self, collapsed: bool) -> bool {
        if self.context_collapsed == collapsed && (!collapsed || !self.flow_collapsed) {
            return false;
        }
        self.context_collapsed = collapsed;
        if collapsed {
            self.flow_collapsed = false;
        }
        true
    }

    /// Whether the column painter draws this frame: a layout it knows, laid
    /// out as its preset, nothing folded or maximized by the canvas model.
    /// Anything else is painted from [`Self::state`]'s tree.
    pub(crate) fn column_painted(&self) -> bool {
        self.layout.is_column()
            && self.state.maximized().is_none()
            && self.state.minimized().is_empty()
            && self.state.preset() == Some(self.layout.preset())
    }

    /// Put the column painter's split where the layout's tree puts the flow
    /// pane, so a picked layout draws exactly what its thumbnail drew.
    pub(crate) fn split_to_preset(&mut self) {
        if !(self.layout.is_column() && self.layout.shows_time() && self.layout.shows_flow()) {
            return;
        }
        let unit = quantick_workspace::canvas::CanvasRect::new(0.0, 0.0, 1.0, 1.0);
        let preset = self.layout.preset();
        let flow = preset.kinds.iter().position(|kind| *kind == PaneKind::Flow);
        let rects = preset.tree.rects(unit);
        let flow = flow.and_then(|slot| rects.iter().find(|(held, _)| *held as usize == slot));
        if let Some((_, rect)) = flow {
            self.split_fraction = rect.x;
        }
    }

    /// Fold or restore the flow pane while retaining the split's open width.
    pub fn set_flow_collapsed(&mut self, collapsed: bool) -> bool {
        if self.flow_collapsed == collapsed && (!collapsed || !self.context_collapsed) {
            return false;
        }
        self.flow_collapsed = collapsed;
        if collapsed {
            self.context_collapsed = false;
        }
        true
    }
}
