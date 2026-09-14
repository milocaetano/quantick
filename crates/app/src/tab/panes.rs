//! Addressing the tab's panes: the context column's collapse and order,
//! the index and side accessors, focus, and the tape the flow pane shows.

use super::Tab;
use crate::canvas_layout::PaneKind;
use crate::chart_layers::{ChartLayer, LayerBlock};
use crate::config::FeedCapabilities;
use crate::orderflow_view::OrderflowView;
use crate::pane::{ChartPane, PaneIndex, PaneSide};
use crate::style::ChartStyle;
use quantick_feed::{FeedConnectionState, FeedNotice};

impl Tab {
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
        if self.context_collapsed == collapsed {
            return false;
        }
        self.context_collapsed = collapsed;
        true
    }

    /// Move the context pane at `from` to `to`, keeping the rest in order.
    ///
    /// **The one reposition path.** The View menu, the keyboard and the
    /// control plane all arrive here, so none of them can grow its own idea of
    /// what moving a pane does; a drag gesture, when it lands, is sugar over
    /// this call rather than a second implementation of it.
    ///
    /// Addresses are [`PaneIndex`]es, so `0` names the flow pane. The flow
    /// pane does not move: it is the protagonist and its column is the one
    /// thing every preset agrees on. Refused rather than clamped — a caller
    /// that asked to move the heatmap meant something this cannot do, and
    /// quietly moving a different pane would be worse than saying no.
    ///
    /// Returns whether anything moved.
    pub fn move_context_pane(&mut self, from: PaneIndex, to: PaneIndex) -> bool {
        let (Some(from_slot), Some(to_slot)) = (from.checked_sub(1), to.checked_sub(1)) else {
            return false;
        };
        if from_slot >= self.time_panes.len() || to_slot >= self.time_panes.len() {
            return false;
        }
        if from_slot == to_slot {
            return false;
        }
        let pane = self.time_panes.remove(from_slot);
        self.time_panes.insert(to_slot, pane);
        // Focus follows the pane the trader just moved, so the next command
        // lands on the chart they were working with rather than on whichever
        // one slid into its place.
        self.focus = PaneSide::Time(to_slot);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUT_PANE_MOVED",
            tab_id = self.id,
            from = from,
            to = to,
            "a context pane was moved within the stack"
        );
        true
    }

    /// How many panes this tab holds, drawn or not.
    #[must_use]
    pub fn pane_count(&self) -> PaneIndex {
        1 + self.time_panes.len()
    }

    /// The pane at `index`: `0` is the flow pane, `1..` the context stack.
    #[must_use]
    pub fn pane_at(&self, index: PaneIndex) -> Option<&ChartPane> {
        match index {
            0 => Some(&self.flow_pane),
            other => self.time_panes.get(other - 1),
        }
    }

    /// The pane at `index`, mutably.
    pub fn pane_at_mut(&mut self, index: PaneIndex) -> Option<&mut ChartPane> {
        match index {
            0 => Some(&mut self.flow_pane),
            other => self.time_panes.get_mut(other - 1),
        }
    }

    /// The first context pane, if this tab has built one.
    ///
    /// Most of the chrome speaks about *the* context chart because most
    /// layouts show one. The ones that show more reach for `time_panes`
    /// directly; this is the convenience, not the truth.
    #[must_use]
    pub fn time_pane(&self) -> Option<&ChartPane> {
        self.time_panes.first()
    }

    /// The first context pane, mutably.
    pub fn time_pane_mut(&mut self) -> Option<&mut ChartPane> {
        self.time_panes.first_mut()
    }

    /// Whether this tab has built any context pane at all.
    #[must_use]
    pub fn has_time_pane(&self) -> bool {
        !self.time_panes.is_empty()
    }

    /// Whether this tab has trouble worth showing on its chip while it sits in
    /// the background (§11: honesty at a glance).
    ///
    /// A recording never reports transport trouble — it has no transport — and
    /// "still connecting" is not trouble either. This is a feed that had a
    /// connection and lost it, or one asking the user to fix something.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.replay.is_none()
            && (self.feed_connection == FeedConnectionState::Reconnecting
                || matches!(self.notice, FeedNotice::Attention { .. }))
    }

    /// The canvas width the last drawn frame used, for callers that have to
    /// reason in pixels without a frame in hand — the control plane's resize,
    /// which must honour the same floor a drag does.
    ///
    /// Zero before the first frame, which callers read as "no opinion" rather
    /// than as a canvas of no width.
    #[must_use]
    pub fn last_canvas_width(&self) -> f32 {
        self.last_canvas_width
    }

    /// Put the tab back in the state it opens in: never drawn, so no canvas
    /// width is known — how a background tab looks after a workspace restore.
    #[cfg(test)]
    pub fn forget_canvas_width_for_test(&mut self) {
        self.last_canvas_width = 0.0;
    }

    /// Whether any context chart is actually on screen.
    ///
    /// The layout has to *hold* one, the tab has to have *built* one, and the
    /// column must not be collapsed. Three conditions that chrome kept
    /// re-deriving one variant at a time — and got wrong twice: the legend
    /// gate matched `TimeAndFlow` alone, so the stacked charts drew no legend
    /// and a collapsed column drew one over the flow chart against a stale
    /// rect.
    #[must_use]
    pub fn shows_context_charts(&self) -> bool {
        self.layout.shows_time() && self.has_time_pane() && !self.context_collapsed
    }

    /// The pane the chrome speaks for. Only a split canvas has a choice to
    /// make: a single-pane layout *is* its one visible pane, whatever the
    /// last split left `focus` set to. Time falls back to the flow pane for
    /// the frame between asking for the layout and the pane being built.
    pub fn focused_side(&self) -> PaneSide {
        // Read from what the layout *holds*, never from which variant it is.
        // Matching one variant is how the three-pane canvas shipped with dead
        // focus: a click set `self.focus` and every reader threw it away,
        // because `TimeTimeAndFlow` was not in the arm.
        // A collapsed column is not on screen, and focus on a pane nobody can
        // see is worse than useless: `paper_hud_here` goes false for the one
        // pane that *is* drawn, so order entry, the ladder and the trade HUD
        // all go dead on the heatmap until the trader expands again.
        if !self.has_time_pane() || !self.layout.shows_time() {
            return PaneSide::Flow;
        }
        if !self.layout.shows_flow() {
            // A layout with no flow pane has nothing to fall back *to*: the
            // context chart is the only pane drawn, and the collapse flag
            // names a column this layout does not carve. Read before the flag,
            // or `Ctrl+0` on the Timeframe layout sent focus to a pane nobody
            // draws — order entry, the ladder and the trade HUD all dead on
            // the one chart on screen, with no rail to click to undo it.
            return PaneSide::Time(0);
        }
        if self.context_collapsed {
            return PaneSide::Flow;
        }
        // A slot the stack no longer shows — the focused pane was the bottom
        // of a three-pane layout and the trader switched to two — falls back
        // to the top context chart, never to a pane that is not drawn.
        match self.focus {
            PaneSide::Time(slot) if slot >= self.context_panes_shown() => PaneSide::Time(0),
            focus => focus,
        }
    }

    /// How many context panes the layout draws — bounded by how many exist,
    /// for the frame between asking for a layout and its panes being built.
    pub fn context_panes_shown(&self) -> usize {
        self.layout
            .kinds()
            .iter()
            .filter(|kind| **kind == PaneKind::Time)
            .count()
            .min(self.time_panes.len())
    }

    /// The pane on `side`, falling back to the flow pane when the time pane
    /// has never been opened.
    pub fn pane(&self, side: PaneSide) -> &ChartPane {
        match side {
            PaneSide::Time(slot) => self.time_panes.get(slot).unwrap_or(&self.flow_pane),
            PaneSide::Flow => &self.flow_pane,
        }
    }

    /// Whether a LAYERS lamp reads as on, and what blocks it if anything.
    ///
    /// One reading, two callers: the toolbar model built each frame and the
    /// semantic scene an operator captures on demand. A lamp that told the
    /// trader one thing and an assistant another would be worse than no scene
    /// at all, so neither side gets its own copy of the question — both come
    /// here, and here asks [`ChartPane::layer_switched_on`] and
    /// [`ChartPane::layer_blocked`], which already resolve every layer to the
    /// one field and the one gate that own it.
    ///
    /// The reading is the *switch*, not what the source lets through it: a
    /// lamp lit from `layer_visible` reads dark while book capture is starting
    /// and forever on a source with no book, so the trader presses an unlit
    /// button and switches the layer they wanted off. The block beside it is
    /// what says the source cannot fill it.
    pub(crate) fn layer_toggle_state(
        &self,
        layer: ChartLayer,
        style: &ChartStyle,
        capabilities: FeedCapabilities,
    ) -> (bool, Option<LayerBlock>) {
        let pane = self.pane(self.layer_toggle_side(layer));
        (
            pane.layer_switched_on(layer, style),
            pane.layer_blocked(layer, capabilities),
        )
    }

    /// Which pane a LAYERS button speaks for.
    ///
    /// The footprint folds the pane's *own* retained trades, so its lamp
    /// answers for the pane with focus: one lit from the flow pane while the
    /// time pane has focus would report a layer the trader is not looking at.
    /// The other three read the tape, and only the flow pane has one.
    fn layer_toggle_side(&self, layer: ChartLayer) -> PaneSide {
        match layer {
            // Read off the tape, and only the flow pane has one. A time pane
            // asked about these answers for machinery it does not own, which
            // is what `ChartPane::layer_blocked` says in words.
            ChartLayer::TapeChart
            | ChartLayer::TapeHeatmap
            | ChartLayer::TapeBubbles
            | ChartLayer::Heatmap
            | ChartLayer::Bubbles
            | ChartLayer::LiveStrip
            | ChartLayer::LaneMarks
            | ChartLayer::FlowLegend
            | ChartLayer::BookStatus
            | ChartLayer::DepthGaps => PaneSide::Flow,
            // The pane's own: the footprint folds the pane's retained trades,
            // the rest are that canvas's chrome and its objects. A lamp lit
            // from the flow pane while the time pane has focus would report a
            // layer the trader is not looking at.
            ChartLayer::Footprint
            | ChartLayer::Grid
            | ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
            | ChartLayer::PointerPrice
            | ChartLayer::PointerTime
            | ChartLayer::PaperTrading
            | ChartLayer::TradePaint
            | ChartLayer::Drawings => self.focused_side(),
        }
    }

    /// See [`Self::pane`].
    /// Point exactly one of this tab's panes at the object whose content is
    /// being typed off-canvas, and clear the other.
    ///
    /// Both are written every time on purpose: the flag suppresses an
    /// object's own painting, so a pane left holding a stale index keeps a
    /// note invisible for the rest of the session. The tab is what knows
    /// whether a time pane exists at all, which is why the loop lives here
    /// rather than in the host.
    pub fn set_content_editing(&mut self, target: Option<(PaneSide, usize)>) {
        self.flow_pane.gestures.content_editing = target
            .filter(|(side, _)| *side == PaneSide::Flow)
            .map(|(_, index)| index);
        for (slot, time) in self.time_panes.iter_mut().enumerate() {
            time.gestures.content_editing = target
                .filter(|(side, _)| *side == PaneSide::Time(slot))
                .map(|(_, index)| index);
        }
    }

    pub fn pane_mut(&mut self, side: PaneSide) -> &mut ChartPane {
        match side {
            PaneSide::Time(slot) => self.time_panes.get_mut(slot).unwrap_or(&mut self.flow_pane),
            PaneSide::Flow => &mut self.flow_pane,
        }
    }

    /// Every side this tab can address, flow first — the order
    /// [`Self::panes`] walks. The one place "which panes exist" is spelled
    /// out for the chrome, so a surface that asks each pane a question walks
    /// the stack rather than the two sides the split used to have.
    pub fn sides(&self) -> impl Iterator<Item = PaneSide> + '_ {
        (0..self.pane_count()).map(PaneSide::from_index)
    }

    /// The pane every chrome surface reads from — see [`Self::focused_side`].
    pub fn focused_pane(&self) -> &ChartPane {
        self.pane(self.focused_side())
    }

    /// See [`Self::focused_pane`].
    pub fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.pane_mut(self.focused_side())
    }

    /// The pane holding the drawing selection — which is not always the
    /// focused one.
    ///
    /// A shared mark can be taken from either chart it appears on, and it
    /// stays in the store of the pane it was drawn on. So the inspector, the
    /// keyboard and the object manager follow the *object*, not the pane the
    /// pointer happens to be over: selecting a level on the time pane and
    /// pressing Delete has to delete that level, wherever it lives.
    ///
    /// Exactly one pane holds a selection at a time
    /// ([`Self::apply_shared_interactions`] drops the other's), so this asks
    /// the focused pane first and takes the answer it finds.
    pub fn drawing_side(&self) -> PaneSide {
        let focused = self.focused_side();
        if self.pane(focused).drawings.selected().is_some() || self.time_panes.is_empty() {
            return focused;
        }
        self.sides()
            .find(|side| *side != focused && self.pane(*side).drawings.selected().is_some())
            .unwrap_or(focused)
    }

    /// The pane every drawing surface reads from — see [`Self::drawing_side`].
    pub fn drawing_pane(&self) -> &ChartPane {
        self.pane(self.drawing_side())
    }

    /// See [`Self::drawing_pane`].
    pub fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.pane_mut(self.drawing_side())
    }

    /// Every pane holding this market's bars, on screen or not, each beside
    /// the side it answers to — flow first, so a walk of one tab is stable and
    /// a capture stays diffable against the one before it.
    ///
    /// An iterator and not a `Vec`: this is walked per frame by the control
    /// plane's journal comparison, which must not touch the allocator on a
    /// quiet frame.
    pub fn panes(&self) -> impl Iterator<Item = (&ChartPane, PaneSide)> {
        std::iter::once((&self.flow_pane, PaneSide::Flow)).chain(
            self.time_panes
                .iter()
                .enumerate()
                .map(|(slot, time)| (time, PaneSide::Time(slot))),
        )
    }

    /// Every pane holding this market's bars, on screen or not. One tape, and
    /// however many charts the layout has ever shown read off it.
    pub fn panes_with_sides_mut(&mut self) -> impl Iterator<Item = (&mut ChartPane, PaneSide)> {
        let Self {
            flow_pane,
            time_panes,
            ..
        } = self;
        std::iter::once((flow_pane, PaneSide::Flow)).chain(
            time_panes
                .iter_mut()
                .enumerate()
                .map(|(slot, pane)| (pane, PaneSide::Time(slot))),
        )
    }

    /// See [`Self::panes_with_sides_mut`], without the addresses.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut ChartPane> {
        // Destructured rather than borrowed field by field: the flow pane and
        // the context stack are two disjoint parts of `self`, and the compiler
        // only knows that when it is told in one pattern.
        let Self {
            flow_pane,
            time_panes,
            ..
        } = self;
        std::iter::once(flow_pane).chain(time_panes.iter_mut())
    }

    /// The flow pane's tape.
    ///
    /// The flow pane is built with one ([`ChartPane::flow`]) and never gives it
    /// up; the `Option` on the pane exists so a *time* pane can go without a
    /// book worker, not because this one can be missing.
    pub fn tape(&self) -> &OrderflowView {
        self.flow_pane
            .orderflow
            .as_ref()
            .expect("the flow pane is built with a tape and never drops it")
    }

    /// See [`Self::tape`].
    pub fn tape_mut(&mut self) -> &mut OrderflowView {
        self.flow_pane
            .orderflow
            .as_mut()
            .expect("the flow pane is built with a tape and never drops it")
    }
}
