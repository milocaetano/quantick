//! Which chart layers this pane shows: the switches, who owns each one, and
//! why a layer can be unavailable rather than merely hidden.
//!
//! Every arm reads the one field that already owns its layer, so the menu, the
//! toolbar and the state file can never disagree about a pixel. A pure move
//! out of `pane.rs`; the tape's own switch is in `tape_switch.rs`.

use crate::chart_layers::{ChartLayer, LayerActions, LayerBlock, blocks};
use crate::config::FeedCapabilities;
use crate::orderflow_view::OrderflowView;
use crate::style::ChartStyle;
use crate::toolrail::Tool;

use super::{ChartPane, PaneChrome};

impl ChartPane {
    /// Whether `layer` is painted on this pane right now.
    ///
    /// Every arm reads the one field that already owns that layer, so the menu
    /// and the toolbar/dock can never disagree about a pixel. `style` is the
    /// window's, passed in because the grid lives there and a pane holding a
    /// copy of it is exactly the disagreement this avoids.
    ///
    /// A layer this pane has no machinery for reports hidden: a time pane runs
    /// no tape (§11), so it has no heatmap to show.
    pub fn layer_visible(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        let tape = self.orderflow.as_ref();
        match layer {
            // The tape's three report the *switch*, not what survives the tape
            // being off: a trader who takes the band away and puts it back gets
            // the tape they had, and the state file records the same.
            ChartLayer::TapeChart => tape.is_some_and(OrderflowView::lane_enabled),
            ChartLayer::TapeHeatmap => tape.is_some_and(OrderflowView::lane_depth_visible),
            ChartLayer::TapeBubbles => tape.is_some_and(OrderflowView::lane_bubbles_enabled),
            ChartLayer::Heatmap => tape.is_some_and(OrderflowView::depth_visible),
            ChartLayer::Bubbles => tape.is_some_and(OrderflowView::bubbles_enabled),
            // Footprint reads the pane's own retained trades, not the tape
            // machinery, so it works on flow and time panes alike.
            ChartLayer::Footprint => self.footprint.visible,
            ChartLayer::LiveStrip => self.orderflow.is_some() && self.live_strip_visible,
            ChartLayer::LaneMarks => tape.is_some_and(OrderflowView::lane_marks_visible),
            ChartLayer::FlowLegend => tape.is_some_and(OrderflowView::legend_visible),
            ChartLayer::BookStatus => tape.is_some_and(OrderflowView::status_badge_visible),
            ChartLayer::DepthGaps => tape.is_some_and(OrderflowView::gaps_visible),
            ChartLayer::Grid => style.canvas.grid_enabled,
            // The toolbox's global eye already owns this one, undo history and
            // all; the menu is a second door to the same switch.
            ChartLayer::Drawings => !self.drawings.all_hidden(),
            ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
            | ChartLayer::PointerPrice
            | ChartLayer::PointerTime
            | ChartLayer::PaperTrading
            | ChartLayer::TradePaint => !self.hidden_layers.contains(&layer),
        }
    }

    /// Show or hide `layer`, writing through to whoever owns it.
    ///
    /// Display only: nothing here stops depth capture, bar building, indicator
    /// computation or a working order, so unhiding repaints the retained past
    /// instead of opening a hole in it. The grid is the window's, so that one
    /// is left in `actions` for the app to apply.
    pub fn set_layer_visible(
        &mut self,
        layer: ChartLayer,
        visible: bool,
        actions: &mut LayerActions,
    ) {
        match layer {
            ChartLayer::TapeChart => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_enabled(visible);
                }
            }
            ChartLayer::TapeHeatmap => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_depth_visible(visible);
                }
            }
            ChartLayer::TapeBubbles => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_bubbles_enabled(visible);
                }
            }
            ChartLayer::Heatmap => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_depth_visible(visible);
                }
            }
            ChartLayer::Bubbles => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_bubbles_enabled(visible);
                }
            }
            ChartLayer::Footprint => self.footprint.visible = visible,
            ChartLayer::LiveStrip => self.live_strip_visible = visible,
            ChartLayer::LaneMarks => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_lane_marks_visible(visible);
                }
            }
            ChartLayer::FlowLegend => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_legend_visible(visible);
                }
            }
            ChartLayer::BookStatus => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_status_badge_visible(visible);
                }
            }
            ChartLayer::DepthGaps => {
                if let Some(tape) = self.orderflow.as_mut() {
                    tape.set_gaps_visible(visible);
                }
            }
            ChartLayer::Grid => actions.grid = Some(visible),
            ChartLayer::Drawings => self.drawings.set_all_hidden(!visible),
            ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
            | ChartLayer::PointerPrice
            | ChartLayer::PointerTime
            | ChartLayer::PaperTrading
            | ChartLayer::TradePaint => {
                if visible {
                    self.hidden_layers.remove(&layer);
                } else {
                    self.hidden_layers.insert(layer);
                }
            }
        }
    }

    /// Whether a surface that is neither the depth map nor the bubbles needs
    /// the order-flow projection this frame.
    ///
    /// Two do: the live strip draws the same clusters the bubbles would, and
    /// the lane's marks need the frame's live edge. Without this, switching
    /// the bubbles off blanked the strip, and switching every flow layer off
    /// left the lane reserved but unmarked — a band indistinguishable from a
    /// dead feed, while its menu entry still read as on.
    pub(super) fn projection_demand(&self) -> bool {
        self.live_strip_visible
            || self
                .orderflow
                .as_ref()
                .is_some_and(OrderflowView::lane_marks_visible)
    }

    /// Whether this pane draws `layer` at all, whatever the source can produce.
    ///
    /// §11 keeps the tape and everything read off it on the flow pane, so a
    /// time pane has no machinery for those five and never will.
    fn draws_layer(&self, layer: ChartLayer) -> bool {
        self.orderflow.is_some()
            || !(layer.on_tape()
                || matches!(
                    layer,
                    ChartLayer::Heatmap
                        | ChartLayer::Bubbles
                        | ChartLayer::LiveStrip
                        | ChartLayer::LaneMarks
                        | ChartLayer::FlowLegend
                        | ChartLayer::BookStatus
                        | ChartLayer::DepthGaps
                ))
    }

    /// Why `layer` cannot be shown here, if it cannot.
    ///
    /// A layer the source cannot produce — or that this pane does not draw at
    /// all — is *unavailable*, not hidden: the menu shows the entry disabled
    /// with the reason, the same wording the toolbar uses, rather than offering
    /// a switch that would do nothing.
    ///
    /// `capabilities` is passed in rather than read here so one menu frame
    /// resolves the running feed once instead of once per entry.
    pub fn layer_blocked(
        &self,
        layer: ChartLayer,
        capabilities: FeedCapabilities,
    ) -> Option<LayerBlock> {
        if !self.draws_layer(layer) {
            return Some(blocks::WRONG_PANE);
        }
        // A layer of a tape that is not on the canvas: the switch is real and
        // remembered, but ticking it now would draw nothing. Offered disabled
        // with the reason, the same way the status badge refuses while the map
        // it reports on is hidden.
        if layer.on_tape()
            && layer != ChartLayer::TapeChart
            && !self
                .orderflow
                .as_ref()
                .is_some_and(OrderflowView::lane_enabled)
        {
            return Some(blocks::TAPE_OFF);
        }
        match layer {
            ChartLayer::TapeHeatmap => (!capabilities.book_capture).then_some(blocks::NO_BOOK),
            ChartLayer::TapeBubbles => {
                (!capabilities.traded_volume).then_some(blocks::NO_TRADED_VOLUME)
            }
            ChartLayer::Heatmap | ChartLayer::DepthGaps => {
                (!capabilities.book_capture).then_some(blocks::NO_BOOK)
            }
            // The strip draws the book and the aggressions landing into it, so
            // it takes either one and is empty only without both. Disabled
            // with the reason rather than offered as a switch that would
            // reserve width for a blank band.
            ChartLayer::LiveStrip => (!capabilities.book_capture && !capabilities.traded_volume)
                .then_some(blocks::NO_BOOK_AND_NO_VOLUME),
            // The badge reports on the book feed, so a source with no book has
            // nothing for it to say — and it is drawn with the map, so while
            // the map is hidden the switch would tick a box that draws
            // nothing. Offered disabled with the reason instead.
            ChartLayer::BookStatus => {
                if capabilities.book_capture {
                    (!self
                        .orderflow
                        .as_ref()
                        .is_some_and(OrderflowView::depth_visible))
                    .then_some(blocks::DEPTH_MAP_HIDDEN)
                } else {
                    Some(blocks::NO_BOOK)
                }
            }
            // The footprint is the buy/sell split per price: on a source that
            // prints no traded volume every cell would be an identical
            // synthetic unit — the same reason the bubbles refuse.
            ChartLayer::Bubbles | ChartLayer::Footprint => {
                (!capabilities.traded_volume).then_some(blocks::NO_TRADED_VOLUME)
            }
            _ => None,
        }
    }

    /// The same layers as [`Self::layer_visible`], reporting the *switch*
    /// rather than what the source lets through it.
    ///
    /// Only the two depth layers differ, and only because their "is it drawn"
    /// answer folds in book capture: on a source with no book they read
    /// undrawn however the switch stands. That is the right answer for a
    /// renderer and the wrong one for a file — persisting it would record a
    /// capability as the trader's choice, and their file outranks the shipped
    /// default on every market from then on, including the ones that do have a
    /// book. The setters already compare against the switch for this exact
    /// reason (`OrderflowView::set_depth_visible`); this is the reading half.
    pub fn layer_switched_on(&self, layer: ChartLayer, style: &ChartStyle) -> bool {
        let tape = self.orderflow.as_ref();
        match layer {
            ChartLayer::Heatmap => tape.is_some_and(OrderflowView::depth_switched_on),
            ChartLayer::TapeHeatmap => tape.is_some_and(OrderflowView::lane_depth_switched_on),
            other => self.layer_visible(other, style),
        }
    }

    /// Every layer this pane persists, and whether it is switched on.
    ///
    /// `style` comes from the window for the same reason it does in
    /// [`Self::layer_visible`].
    pub fn layer_states(&self, style: &ChartStyle) -> std::collections::BTreeMap<ChartLayer, bool> {
        ChartLayer::ALL
            .into_iter()
            .filter(|layer| layer.persisted())
            .map(|layer| (layer, self.layer_switched_on(layer, style)))
            .collect()
    }

    /// The same visibility as one bit per persisted layer, for change
    /// detection.
    ///
    /// The bit is the layer's index in `ALL`, which is now sixteen entries —
    /// the last bit `u32` has room for after this widening, and the reason the
    /// accumulator is not `u16` any more: a seventeenth layer would have
    /// shifted by 16, panicking in debug and silently colliding in release.
    /// The assertion below fails the build rather than the chart.
    pub fn layer_mask(&self, style: &ChartStyle) -> u32 {
        ChartLayer::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, layer)| layer.persisted() && self.layer_switched_on(*layer, style))
            .fold(0_u32, |mask, (bit, _)| mask | (1 << bit))
    }

    /// Apply saved visibility to this pane, ignoring layers it cannot draw.
    ///
    /// The grid is not applied here — one window, one grid, and the app sets
    /// that once rather than once per pane.
    pub fn apply_layer_states(&mut self, states: &std::collections::BTreeMap<ChartLayer, bool>) {
        let mut discarded = LayerActions::default();
        for (layer, visible) in states {
            if *layer == ChartLayer::Grid || !self.draws_layer(*layer) {
                continue;
            }
            self.set_layer_visible(*layer, *visible, &mut discarded);
        }
    }

    /// Arming a tool brings back the layer it draws on.
    ///
    /// A crosshair that draws no cross, or a line tool that places invisible
    /// objects, reads as a broken tool rather than as a hidden layer — and
    /// reaching for the tool is the user saying they want to see it.
    pub(super) fn unhide_layer_for_armed_tool(&mut self, chrome: &mut PaneChrome<'_>) {
        let layer = match chrome.toolrail.tool() {
            Tool::Crosshair => ChartLayer::Crosshair,
            Tool::Drawing(_) => ChartLayer::Drawings,
            Tool::Pointer => return,
        };
        if !self.layer_visible(layer, chrome.style) {
            self.set_layer_visible(layer, true, chrome.layers);
        }
    }
}
