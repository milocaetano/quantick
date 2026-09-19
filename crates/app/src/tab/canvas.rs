//! Painting the canvas: the pane grid, the shared gestures over it, and the
//! chrome a frame borrows to draw one.
//!
//! Laying the area out for however many panes the layout asks for, running
//! each visible pane through it, and the interactions that belong to the
//! canvas rather than to any one pane — a gesture routed to the pane the
//! pointer is over, the marks one pane paints on another's behalf, the focus
//! rule, the collapsed rail and the divider between the columns.

use eframe::egui;
use smallvec::SmallVec;

use super::{FOCUS_RULE_PX, RAIL_GRIP_HEIGHT_PX, RAIL_GRIP_WIDTH_PX, SharedPicks, Tab};
use crate::canvas_layout::{self, MAX_CANVAS_PANES, MAX_CONTEXT_PANES, PaneKind};
use crate::config::FeedCapabilities;
use crate::pane::canvas_split::PaneAreas;
use crate::pane::{
    CANVAS_DIVIDER_HANDLE_PX, PaneChrome, PaneIndex, PaneSide, SharedEdit, SharedInteraction,
    SharedPick, clamp_pane_fraction, split_pane_layout_strip, split_time_pane,
};
use crate::state::BarKind;
use crate::style::ChartStyle;
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;

/// The window chrome a tab's canvas borrows for one frame. The tab completes
/// it with its own symbol to make the [`PaneChrome`] its panes read.
pub struct CanvasChrome<'a> {
    pub toolrail: &'a mut ToolRail,
    pub presets: &'a crate::drawings::presets::PresetStore,
    pub drawing_chrome: &'a mut crate::surfaces::DrawingChromeSurface,
    /// See [`PaneChrome::begin_text_edit`].
    pub begin_text_edit: &'a mut bool,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    /// What the running source can produce, for the layer menu's disabled
    /// entries. Resolved once by the app rather than per pane, per entry.
    pub capabilities: FeedCapabilities,
    /// Whether the source infers the aggressor side (see
    /// [`PaneChrome::side_inferred`]). Resolved once, like `capabilities`.
    pub side_inferred: bool,
    /// The footprint layer's signal tunables (see [`PaneChrome::footprint`]).
    pub footprint: &'a mut crate::footprint_config::FootprintConfig,
    /// Where a pane's layer menu leaves the switches it does not own.
    pub layers: &'a mut quantick_layers::LayerActions,
}

/// Which pane order entry belongs to this frame.
///
/// Every visible pane is a trading surface (§11): a price level is as true
/// on a context chart as on the flow chart, so the aim follows the
/// **pointer** rather than focus, and holding the buy modifier over any
/// pane places there without a focusing click first. That is the whole
/// change from "the focused pane trades" — a trader reading a level on the
/// context chart no longer has to click it into focus before they can act
/// on what they just saw.
///
/// A drag in flight overrides the pointer. The grabbed line is read against
/// one pane's price scale, and letting the pointer cross into a neighbour
/// mid-drag would reprice the order to whatever *that* pane's scale says —
/// a stop that jumps because the hand strayed. So the pane that started the
/// drag keeps it until the release.
///
/// With the pointer outside every pane (over the dock, off the window, or
/// on the chrome between panes) the focused pane answers, which is also the
/// unsplit case: one pane, always the answer, nothing changes.
pub(super) fn trading_pane(
    pointer: Option<egui::Pos2>,
    panes: &[(PaneIndex, egui::Rect)],
    dragging: bool,
    pinned: Option<PaneIndex>,
    focused: PaneIndex,
) -> PaneIndex {
    if dragging && let Some(pinned) = pinned {
        return pinned;
    }
    pointer
        .and_then(|pointer| {
            panes
                .iter()
                .find(|(_, rect)| rect.contains(pointer))
                .map(|(side, _)| *side)
        })
        .unwrap_or(focused)
}

/// How one frame of the canvas is carved up: which columns show, where the
/// divider and the collapsed rail sit. Measured once, before any pane draws,
/// and read by every stage after it.
struct CanvasFrame {
    area: egui::Rect,
    /// How many context charts are drawn this frame: the lower of what the
    /// layout asks for and what the tab has actually built.
    context_shown: usize,
    show_flow: bool,
    split: bool,
    /// The context column's rect, `None` when it is hidden or collapsed.
    time_area: Option<egui::Rect>,
    divider: Option<egui::Rect>,
    flow_band: egui::Rect,
    collapsed_rail: Option<egui::Rect>,
}

/// Where each visible pane draws this frame, after the context column has
/// spent its header strips.
struct PaneRects {
    /// Per context chart, top to bottom: the chart rect and its layout strip.
    context_charts: SmallVec<[(egui::Rect, egui::Rect); MAX_CONTEXT_PANES]>,
    context_dividers: SmallVec<[egui::Rect; MAX_CONTEXT_PANES]>,
    flow_areas: Option<PaneAreas>,
}

impl Tab {
    /// Lay the canvas out and run every visible pane through it (§11).
    ///
    /// Single is one pane over the whole area — the same code path the split
    /// takes, with one pane in it, so the default layout can never drift from
    /// the split one.
    pub fn draw_canvas(
        &mut self,
        tab_id: u64,
        ui: &mut egui::Ui,
        area: egui::Rect,
        chrome: &mut CanvasChrome<'_>,
    ) {
        let frame = self.canvas_frame(area);
        let rects = self.draw_context_column(ui, &frame);

        // Which shared mark the pointer is over, on each pane, against the
        // other pane's store. Answered here because answering it needs both
        // panes at once, and the pane pass holds them one at a time.
        let picks = self.shared_picks(ui);
        let edits = self.draw_panes(tab_id, ui, chrome, &rects, &picks);
        self.apply_shared_interactions(&edits);

        // Drawings marked "show on all charts" cross here, after both panes
        // have drawn and cached their projections. It happens outside the
        // pane pass because each pane paints the *other* pane's marks, and
        // that needs both panes borrowed at once — immutably, which is also
        // the guarantee that a foreign mark can only be looked at.
        self.paint_shared_drawings(ui.painter());

        // The position HUD rides the pane that owns order entry (the focused
        // one). It draws here, after the pane pass, because its buttons need
        // the paper host mutably — inside the pass that borrow is pinned
        // behind the shared chrome.
        if let Some((rect, scale)) = self.focused_pane().paper_hud_anchor() {
            crate::paper_hud::draw(ui.ctx(), rect, &mut self.paper, &scale);
        }

        self.draw_canvas_chrome(tab_id, ui, &frame, &rects);
    }

    /// Measure this frame's [`CanvasFrame`], clamping the split fraction so
    /// neither column is narrower than a pane.
    fn canvas_frame(&mut self, area: egui::Rect) -> CanvasFrame {
        // How many context charts this layout asks for, and how many the tab
        // has actually built. The lower of the two is what gets drawn: a
        // layout may name a pane the tab is still building, and half a canvas
        // is better than a frame of nothing.
        self.last_canvas_width = area.width();
        let context_wanted = self
            .layout
            .kinds()
            .iter()
            .filter(|kind| matches!(kind, PaneKind::Time))
            .count();
        let context_shown = context_wanted.min(self.time_panes.len());
        let show_time = context_shown > 0;
        // The flow pane also stands in for a time pane still being built, so
        // the frame between asking for the Time layout and the pane existing
        // shows the market rather than nothing.
        let show_flow = self.layout.shows_flow() || !show_time;
        let split = show_time && show_flow;
        if split && !self.context_collapsed && area.width() > 0.0 {
            let floor = canvas_layout::MIN_PANE_WIDTH_PX / area.width();
            self.split_fraction = clamp_pane_fraction(self.split_fraction.max(floor));
        }
        // One visible pane is handed the whole canvas and the rest of this
        // reduces to nothing: no divider, no focus rule — though a lone time
        // pane keeps its header.
        let (time_area, divider, flow_band) = if split {
            let width = if self.context_collapsed {
                canvas_layout::PaneWidth::Collapsed {
                    restore: self.split_fraction,
                }
            } else {
                canvas_layout::PaneWidth::Manual(self.split_fraction)
            };
            let row = canvas_layout::split_row(area, &[width, canvas_layout::PaneWidth::Auto]);
            (Some(row.panes[0]), Some(row.dividers[0]), row.panes[1])
        } else if show_time {
            (Some(area), None, area)
        } else {
            (None, None, area)
        };
        // A collapsed column paints a rail, not charts: eight pixels is a
        // handle, not a chart, and laying one out there would draw a price
        // axis and nothing else.
        let collapsed_rail = (split && self.context_collapsed)
            .then_some(time_area)
            .flatten();
        let time_area = if collapsed_rail.is_some() {
            None
        } else {
            time_area
        };
        CanvasFrame {
            area,
            context_shown,
            show_flow,
            split,
            time_area,
            divider,
            flow_band,
            collapsed_rail,
        }
    }

    /// The context column, carved into one band per chart it shows, top to
    /// bottom. Each band spends its own header strip and hands back the
    /// chart rect below it.
    fn draw_context_column(&mut self, ui: &mut egui::Ui, frame: &CanvasFrame) -> PaneRects {
        let context_shown = frame.context_shown;
        let mut rects = PaneRects {
            context_charts: SmallVec::new(),
            context_dividers: SmallVec::new(),
            flow_areas: None,
        };
        #[cfg(test)]
        self.context_dividers.clear();
        self.context_stack.frame =
            frame
                .time_area
                .map(|column| super::context_resize::StackFrame {
                    column,
                    pane_ids: self
                        .time_panes
                        .iter()
                        .take(context_shown)
                        .map(|pane| pane.id)
                        .collect(),
                });
        if let Some(column) = frame.time_area {
            // Focus before input, so the click that focuses a pane is also the
            // click that pane goes on to handle. Only a split has focus to
            // move: a single visible pane is the focused one by definition.
            if self.context_stack.heights.len() < context_shown {
                self.context_stack
                    .heights
                    .resize(context_shown, canvas_layout::PaneWidth::Auto);
            }
            let bands =
                canvas_layout::split_column(column, &self.context_stack.heights[..context_shown]);
            rects
                .context_dividers
                .extend(bands.dividers.iter().copied());
            if frame.split {
                self.focus_from_pointer(ui, &bands.panes[..context_shown], frame.flow_band);
            }
            for (slot, band) in bands.panes.iter().enumerate().take(context_shown) {
                let pane_areas = split_pane_layout_strip(*band);
                let areas = split_time_pane(pane_areas.body);
                self.draw_time_header(ui, slot, areas.header);
                rects
                    .context_charts
                    .push((areas.chart, pane_areas.layout_strip));
            }
        }
        rects.flow_areas = frame
            .show_flow
            .then(|| split_pane_layout_strip(frame.flow_band));
        rects
    }

    /// Each context chart carries its own timeframe selector (§11): its BARS
    /// group, beside the toolbar's, which keeps governing the flow pane.
    fn draw_time_header(&mut self, ui: &mut egui::Ui, slot: usize, header: egui::Rect) {
        let mut interval_ms = self.time_panes[slot]
            .spec
            .retained(BarKind::Time)
            .time_interval_ms()
            .unwrap_or(crate::time_header::DEFAULT_INTERVAL_MS);
        let header_layout = crate::time_header::draw(
            ui,
            header,
            &mut interval_ms,
            self.time_panes[slot].id,
            &self.time_panes[slot].layout_label,
        );
        #[cfg(test)]
        if slot == 0 {
            self.time_header_chips = header_layout.chips();
        }
        if header_layout.changed {
            let pane = &mut self.time_panes[slot];
            pane.spec
                .update(
                    quantick_engine::bar_selection::SelectionCommand::Replace(
                        crate::state::BarSpec::Time(interval_ms).into(),
                    ),
                    quantick_engine::bar_selection::BarInputAvailability::PRINTS,
                )
                .expect("time control stays in the interval domain");
        }
    }

    /// The pane pass: every visible pane handles its input and draws, time
    /// panes first, then flow. Returns what each pane did to the *other*
    /// panes' shared marks, for the caller to apply once nothing is borrowed.
    fn draw_panes(
        &mut self,
        tab_id: u64,
        ui: &mut egui::Ui,
        chrome: &mut CanvasChrome<'_>,
        rects: &PaneRects,
        picks: &SharedPicks,
    ) -> SmallVec<[(PaneIndex, SharedInteraction); MAX_CANVAS_PANES]> {
        let mut edits: SmallVec<[(PaneIndex, SharedInteraction); MAX_CANVAS_PANES]> =
            SmallVec::new();
        // Focus as an address, so the loop below compares like with like
        // however many panes it walks.
        let focused = self.focused_side().index();
        let Self {
            flow_pane,
            time_panes,
            symbol,
            paper,
            feed_gaps,
            paper_drag_pane,
            ..
        } = self;
        // Cleared before the loop, set by whichever panes it actually walks.
        // A collapsed context column draws nothing and would otherwise keep
        // the rect it had when it was last open, which `starved_pane` would
        // then offer as somewhere to paint the offline note — off the visible
        // canvas, on a chart that is not there.
        flow_pane.frame.area = None;
        flow_pane.frame.layout_strip = None;
        for pane in time_panes.iter_mut() {
            pane.frame.area = None;
            pane.frame.layout_strip = None;
        }
        // The time pane has no tape of its own (§11), so its footprint rows
        // adopt the flow pane's capture bucket — the instrument's grid is a
        // fact about the market, not about which pane shows it.
        //
        // Which is why there is no longer a gate here. This used to run only
        // while the time pane's *footprint layer* was visible, and that
        // contradicted the sentence above it: the ladders have a second
        // consumer now, and a fixed-range volume profile folds them with the
        // layer hidden. So the same profile, on the same market, read at the
        // flow pane's bucket on one chart and at the default on the other — a
        // hundredfold difference in row height on WDO, which paints as a slab
        // beside a wash. Two surfaces that are the same thing have to behave
        // the same way.
        //
        // Unconditional is also cheap: `set_footprint_group` returns
        // immediately when the bucket has not changed, which is every frame
        // but the one after a market switch.
        if let (Some(time), Some(base)) = (
            time_panes.first_mut(),
            flow_pane
                .orderflow
                .as_ref()
                .map(|tape| tape.base_capture_grouping()),
        ) {
            time.state.set_footprint_group(base);
        }
        // Context panes carry addresses `1..`, the flow pane `0` — the order
        // `Tab::pane_at` uses, never the order they sit in.
        let addressed: SmallVec<[(PaneIndex, egui::Rect); MAX_CANVAS_PANES]> = rects
            .context_charts
            .iter()
            .enumerate()
            .map(|(slot, (chart, _))| (slot + 1, *chart))
            .chain(rects.flow_areas.map(|areas| (0 as PaneIndex, areas.body)))
            .collect();
        let trading_pane = trading_pane(
            ui.ctx().pointer_latest_pos(),
            &addressed,
            paper.gesture_active(),
            *paper_drag_pane,
            focused,
        );
        let mut chrome = PaneChrome {
            tab: tab_id,
            side: PaneSide::Flow,
            toolrail: chrome.toolrail,
            presets: chrome.presets,
            drawing_chrome: chrome.drawing_chrome,
            begin_text_edit: chrome.begin_text_edit,
            style: chrome.style,
            tz: chrome.tz,
            symbol,
            paper,
            paper_takes_input: false,
            paper_hud_here: false,
            shared_pick: None,
            shared: SharedInteraction::default(),
            feed_gaps: &feed_gaps[..],
            capabilities: chrome.capabilities,
            side_inferred: chrome.side_inferred,
            footprint: chrome.footprint,
            layers: chrome.layers,
        };
        // Time pane first, then flow. Both take the same two steps in the
        // same order — which is what keeps the split honest: the second pane
        // cannot drift from the first, and one pane is this same loop with
        // one entry in it.
        // Context panes carry addresses `1..`, the flow pane `0` — the order
        // `Tab::pane_at` uses, never the order they sit in.
        let context = time_panes
            .iter_mut()
            .zip(rects.context_charts.iter().copied())
            .enumerate()
            .map(|(slot, (pane, (chart, strip)))| (pane, chart, strip, slot + 1));
        let flow = rects.flow_areas.map(|areas| {
            (
                &mut *flow_pane,
                areas.body,
                areas.layout_strip,
                0 as PaneIndex,
            )
        });
        for (pane, rect, strip, side) in context.chain(flow) {
            pane.frame.layout_strip = Some(strip);
            chrome.side = PaneSide::from_index(side);
            chrome.paper_takes_input = side == trading_pane;
            // The HUD is one card and follows focus, so it does not flicker
            // from pane to pane as the hand crosses them.
            chrome.paper_hud_here = side == focused;
            chrome.shared_pick = picks.for_pane(side);
            chrome.shared = SharedInteraction::default();
            pane.handle_navigation(ui, rect, &mut chrome);
            pane.draw_chart(ui.painter(), rect, &mut chrome);
            // Whatever this pane did to the other's marks travels out of the
            // loop: the store it belongs to is the pane that is not borrowed
            // right now.
            if chrome.shared != SharedInteraction::default() {
                edits.push((side, chrome.shared));
            }
        }
        // Written *after* the loop, not before it. The drag begins inside
        // `handle_chart_input`, so on the frame of the press
        // `gesture_active()` is still false up there and the pin would be
        // stored as `None` — leaving the very next frame, the first one that
        // actually drags, to fall through to the pointer. A flick across a
        // divider in one frame then repriced the order against the
        // neighbour's scale, which is the whole thing the pin exists to stop.
        *paper_drag_pane = paper.gesture_active().then_some(trading_pane);
        edits
    }

    /// The canvas's own chrome, over the panes: the collapsed rail, the
    /// dividers, and the focus rule.
    fn draw_canvas_chrome(
        &mut self,
        tab_id: u64,
        ui: &mut egui::Ui,
        frame: &CanvasFrame,
        rects: &PaneRects,
    ) {
        let area = frame.area;
        if let Some(rail) = frame.collapsed_rail {
            self.draw_collapsed_rail(tab_id, ui, rail, area.width());
        }
        if frame.time_area.is_some() {
            self.draw_context_dividers(tab_id, ui, &rects.context_dividers);
        }
        let (Some(time_area), Some(divider)) = (frame.time_area, frame.divider) else {
            return;
        };
        let drawn_width = divider.center().x - area.left();
        self.draw_canvas_divider(tab_id, ui, divider, drawn_width, area.width());
        // §11: a 1 px accent under the focused pane's top edge — no border
        // boxes around market data.
        let focused = match self.focused_side() {
            PaneSide::Time(slot) => rects
                .context_charts
                .get(slot)
                .map(|(chart, _)| *chart)
                .unwrap_or(time_area),
            PaneSide::Flow => rects.flow_areas.map_or(frame.flow_band, |areas| areas.body),
        };
        ui.painter().line_segment(
            [
                egui::pos2(focused.left(), focused.top() + FOCUS_RULE_PX / 2.0),
                egui::pos2(focused.right(), focused.top() + FOCUS_RULE_PX / 2.0),
            ],
            egui::Stroke::new(FOCUS_RULE_PX, theme::ACCENT),
        );
    }

    /// What the pointer is over, on each pane, among the *other* pane's
    /// shared marks.
    ///
    /// Resolved with both panes in hand and before either is borrowed for its
    /// input pass, which is the only moment a pane can be asked about marks it
    /// does not hold. Nothing to answer on an unsplit tab: one pane has no
    /// other pane to mirror.
    fn shared_picks(&self, ui: &egui::Ui) -> SharedPicks {
        let count = self.pane_count();
        let mut picks = SharedPicks {
            by_pane: SmallVec::from_elem(None, count),
        };
        if count < 2 {
            // One pane has no other pane to mirror.
            return picks;
        }
        let Some(position) = ui.input(|input| input.pointer.latest_pos()) else {
            return picks;
        };

        for viewer in 0..count {
            let Some(pane) = self.pane_at(viewer) else {
                continue;
            };
            // Only the pane the pointer is actually over is asked. Besides
            // saving the work, it is what stops a horizontal line — which
            // spans a whole chart — from reporting a hit on the pane beside
            // the one the pointer is in, at the same height.
            if !pane
                .frame
                .chart_area
                .is_some_and(|chart| chart.contains(position))
            {
                continue;
            }
            // The mark may belong to any other pane. First owner in address
            // order wins, which is stable frame to frame: a pick that
            // depended on iteration luck would move an object between charts
            // between frames.
            for owner in (0..count).filter(|owner| *owner != viewer) {
                let Some(source) = self.pane_at(owner) else {
                    continue;
                };
                if let Some((index, anchor)) =
                    pane.hit_test().shared_pick(&source.drawings, position)
                {
                    picks.by_pane[viewer] = Some(SharedPick {
                        owner,
                        index,
                        anchor,
                        locked: source.drawings.items()[index].locked,
                    });
                    break;
                }
            }
        }
        picks
    }

    /// Land what each pane did to the other's marks on the store that holds
    /// them.
    ///
    /// The gesture brackets travel with the edits so a whole drag started on
    /// the mirror lands as one undo entry on the owning store — the same
    /// coalescing the object gets on its own chart, because it is the same
    /// gesture on the same object. A selection taken on one pane is dropped on
    /// the other, so the tab never holds two.
    pub(super) fn apply_shared_interactions(&mut self, edits: &[(PaneIndex, SharedInteraction)]) {
        for (actor, interaction) in edits {
            // No owner means no mark was ever taken hold of, so there is
            // nothing to land. Refused rather than guessed: landing it on a
            // neighbour chosen by arithmetic would move an object the trader
            // drew onto a chart they were not working on.
            let Some(owner) = interaction.owner else {
                continue;
            };
            if self.pane_at(owner).is_none() {
                continue;
            }
            if interaction.begin_gesture {
                self.pane_at_mut(owner)
                    .expect("owner checked above")
                    .drawings
                    .begin_gesture();
            }
            if let Some(edit) = interaction.edit {
                if matches!(edit, SharedEdit::Select(_))
                    && let Some(pane) = self.pane_at_mut(*actor)
                {
                    // A selection taken on a mirror is dropped on the pane
                    // that took it, so the tab never holds two.
                    pane.drawings.select(None);
                }
                self.pane_at_mut(owner)
                    .expect("owner checked above")
                    .shared_marks_mut()
                    .apply_edit(edit);
            }
            if interaction.commit_gesture {
                self.pane_at_mut(owner)
                    .expect("owner checked above")
                    .drawings
                    .commit_gesture();
            }
        }
    }

    /// Cross-pane drawings: each pane paints the shared marks of the other.
    ///
    /// Nothing happens on an unsplit tab — one pane has no other pane to
    /// borrow marks from, and the drawing is already on the only chart there
    /// is. Scope stops here, at the tab: the panes below hold one symbol on
    /// one feed, which is what makes a price level mean the same thing on
    /// both (`docs/ux/drawing-tools-2026-08.md` §D7).
    fn paint_shared_drawings(&self, painter: &egui::Painter) {
        let count = self.pane_count();
        if count < 2 {
            return;
        }
        for viewer in 0..count {
            let Some(pane) = self.pane_at(viewer) else {
                continue;
            };
            for owner in (0..count).filter(|owner| *owner != viewer) {
                if let Some(source) = self.pane_at(owner) {
                    pane.hit_test()
                        .paint_shared_from(painter, source.shared_source());
                }
            }
        }
    }

    /// Clicking a pane focuses it (§11). Read from the raw pointer press
    /// rather than a widget response, so the press that starts a pan or picks
    /// up a drawing focuses the pane it landed in on that same frame.
    fn focus_from_pointer(
        &mut self,
        ui: &egui::Ui,
        context_bands: &[egui::Rect],
        flow_area: egui::Rect,
    ) {
        let pressed = ui.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let Some(position) = pressed else { return };
        // A press egui routed to another layer belongs to whatever floats
        // there — the toast, the object manager, the inspector — not to the
        // pane it happens to cover. Taking those as pane clicks made the
        // toast's Undo act on the chart the button floated over rather than
        // the one it was raised for.
        if ui.ctx().layer_id_at(position) != Some(ui.layer_id()) {
            return;
        }
        // Each band of the context column is its own pane (§11): the press
        // names the chart it landed in, not the column. Before this the whole
        // column was one target, which is how a third pane could never take
        // focus.
        if let Some(slot) = context_bands
            .iter()
            .position(|band| band.contains(position))
        {
            self.focus = PaneSide::Time(slot);
        } else if flow_area.contains(position) {
            self.focus = PaneSide::Flow;
        }
    }

    /// The collapsed context column: a rail with a grip, and the way back.
    ///
    /// A pane dragged to nothing has to leave something behind. Blender's
    /// manual puts the rule plainly — a hidden region leaves a little arrow to
    /// click — and the vertical axis in this app already refuses zero for the
    /// same reason (`indicators::COLLAPSED_PANE_HEIGHT_PX`). Eight pixels of a
    /// 1920 px canvas is four tenths of one percent: near enough to the "size
    /// zero" a trader asks for, and not so near that the chart is gone for
    /// good.
    ///
    /// The paint is 8 px wide; the *hit* area is 24 px, reaching into the
    /// chart beside it where it costs nothing but the pointer's first few
    /// pixels. A rail that photographed well but could not be hit would be a
    /// picture of an affordance rather than one.
    fn draw_collapsed_rail(
        &mut self,
        tab_id: u64,
        ui: &egui::Ui,
        rail: egui::Rect,
        canvas_width: f32,
    ) {
        #[cfg(test)]
        {
            self.collapsed_rail = Some(rail);
        }
        let painter = ui.painter();
        painter.rect_filled(rail, egui::Rounding::ZERO, theme::CHROME);
        // The inner edge, so the rail reads as chrome against the chart rather
        // than as a stripe the chart happens to start after.
        painter.line_segment(
            [
                egui::pos2(rail.right(), rail.top()),
                egui::pos2(rail.right(), rail.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );

        let hit = egui::Rect::from_min_max(
            rail.min,
            egui::pos2(
                rail.left() + canvas_layout::COLLAPSED_HIT_PX.max(rail.width()),
                rail.bottom(),
            ),
        );
        let response = ui
            .interact(
                hit,
                egui::Id::new(("canvas_divider", tab_id)),
                egui::Sense::drag(),
            )
            .on_hover_text("drag right to show the timeframe charts again");
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }

        // The grip: a short bar at the rail's middle, in the colour a reader
        // already knows means "chrome you can take hold of".
        let grip_colour = if response.hovered() {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        };
        let grip = egui::Rect::from_center_size(
            rail.center(),
            egui::vec2(RAIL_GRIP_WIDTH_PX, RAIL_GRIP_HEIGHT_PX),
        );
        ui.painter()
            .rect_filled(grip, egui::Rounding::same(1.0), grip_colour);

        self.apply_canvas_drag(&response, rail.width() / 2.0, canvas_width);
    }

    /// The divider between the panes, as a resize handle.
    ///
    /// Registered after both panes so it takes the drag that would otherwise
    /// pan the chart behind its grab area, exactly as the live lane's own
    /// divider does inside a pane.
    fn draw_canvas_divider(
        &mut self,
        tab_id: u64,
        ui: &egui::Ui,
        divider: egui::Rect,
        drawn_width: f32,
        canvas_width: f32,
    ) {
        #[cfg(test)]
        {
            self.canvas_divider = Some(divider);
        }
        ui.painter()
            .rect_filled(divider, egui::Rounding::ZERO, theme::BORDER);
        // Namespaced by tab for the same reason a pane namespaces its own ids
        // (see [`crate::pane`]): egui keeps drag state per id, so one shared
        // id would let a drag started on this tab's divider carry on into the
        // next tab's the moment Ctrl+Tab switches under a held button.
        let handle = ui.interact(
            divider.expand2(egui::vec2(CANVAS_DIVIDER_HANDLE_PX, 0.0)),
            egui::Id::new(("canvas_divider", tab_id)),
            egui::Sense::drag(),
        );
        if handle.hovered() || handle.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        self.apply_canvas_drag(&handle, drawn_width, canvas_width);
    }

    fn apply_canvas_drag(
        &mut self,
        response: &egui::Response,
        drawn_width: f32,
        canvas_width: f32,
    ) {
        if response.drag_started() {
            self.canvas_drag = Some((drawn_width, self.context_collapsed));
        }
        if response.dragged() && response.drag_delta().x.abs() > f32::EPSILON && canvas_width > 0.0
        {
            let (wanted_px, opening) = {
                let state = self
                    .canvas_drag
                    .get_or_insert((drawn_width, self.context_collapsed));
                state.0 += response.drag_delta().x;
                *state
            };
            if opening && wanted_px <= canvas_layout::COLLAPSED_PANE_WIDTH_PX / 2.0 {
                return;
            }
            if !opening && wanted_px < canvas_layout::COLLAPSE_AT_PX {
                self.set_context_collapsed(true);
            } else {
                self.set_context_collapsed(false);
                let width = wanted_px.max(canvas_layout::MIN_PANE_WIDTH_PX);
                self.split_fraction = clamp_pane_fraction(width / canvas_width);
            }
        }
        if response.drag_stopped() {
            self.canvas_drag = None;
        }
    }

    /// The horizontal dividers between context charts.
    ///
    /// Each drag moves one boundary, so the chart above and the chart below
    /// exchange height while every other boundary stays put. Registered after
    /// the pane bodies so a resize wins over the chart pan beneath its handle.
    fn draw_context_dividers(&mut self, tab_id: u64, ui: &egui::Ui, dividers: &[egui::Rect]) {
        #[cfg(test)]
        self.context_dividers.extend(dividers.iter().copied());

        for (index, divider) in dividers.iter().copied().enumerate() {
            ui.painter()
                .rect_filled(divider, egui::Rounding::ZERO, theme::BORDER);
            let handle = ui.interact(
                divider.expand2(egui::vec2(0.0, CANVAS_DIVIDER_HANDLE_PX)),
                egui::Id::new((
                    "context_divider",
                    tab_id,
                    self.time_panes[index].id,
                    self.time_panes[index + 1].id,
                )),
                egui::Sense::drag(),
            );
            if handle.hovered() || handle.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if handle.dragged()
                && let Some(pointer) = ui.ctx().pointer_interact_pos()
            {
                let _ = self.resize_context_pair(
                    tab_id,
                    super::context_resize::ResizeContextPair {
                        upper_pane_id: self.time_panes[index].id,
                        lower_pane_id: self.time_panes[index + 1].id,
                        wanted_y: pointer.y,
                    },
                    quantick_control::wire::ActorKind::HumanUi,
                );
            }
        }
    }
}
