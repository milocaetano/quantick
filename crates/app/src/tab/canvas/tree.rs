//! The canvas painted from the model's pane tree: every layout the column
//! painter does not draw, and any fold or maximize the canvas model holds.
//!
//! The geometry is `quantick_workspace::canvas`'s, unchanged: each docked
//! pane takes its tree rectangle, inset by half a divider on every edge it
//! shares with a neighbour, so the seams read as the column painter's
//! dividers do, and drag as they do. Nothing here decides a layout.

use eframe::egui;
use quantick_chart::pane_header::{PaneHeader, PaneIdentity};
use quantick_engine::bar_selection::BarInputAvailability;
use quantick_workspace::canvas::{
    CanvasRect, FloatHost, PaneKind, PaneSlot, SeamRail, SplitAxis, slot_seat, strip_label,
    tree_bands, tree_rails, tree_seams,
};
use smallvec::SmallVec;

use super::{CanvasChrome, CanvasFrame, PaneRects, Tab};
use crate::canvas_layout::{CANVAS_DIVIDER_PX, MAX_CANVAS_PANES};
use crate::pane::canvas_split::PaneAreas;
use crate::pane::{
    CANVAS_DIVIDER_HANDLE_PX, PaneSide, split_flow_pane, split_pane_header, split_pane_layout_strip,
};
use crate::{floating_panes, theme};

/// The resize cursor over a seam laying charts top to bottom, then side by side.
const CURSORS: [egui::CursorIcon; 2] = [
    egui::CursorIcon::ResizeVertical,
    egui::CursorIcon::ResizeHorizontal,
];

impl Tab {
    /// Lay the canvas out from the model's tree and spend each band's header,
    /// as the column painter does for its own bands.
    pub(super) fn draw_tree(
        &mut self,
        tab_id: u64,
        ui: &mut egui::Ui,
        area: egui::Rect,
        chrome: &mut CanvasChrome<'_>,
    ) -> (CanvasFrame, PaneRects) {
        self.canvas.last_canvas = area;
        let palette = theme::get(ui.ctx());
        let bar_inputs = self.bar_inputs(chrome);
        let canvas = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
        let bands = tree_bands(
            &self.canvas.state,
            &mut self.canvas.seats,
            canvas,
            CANVAS_DIVIDER_PX,
        );
        // A rail is painted over its whole node with the seams.
        let bands = bands.into_iter().filter(|band| !band.railed).map(|band| {
            (
                band.seat.map_or(PaneSide::Flow, PaneSide::Time),
                floating_panes::screen_rect(band.rect),
                band.minimized,
                band.slot,
            )
        });
        let bands: SmallVec<[_; MAX_CANVAS_PANES]> = bands.collect();
        let built = self.time_panes.len();
        self.canvas.context_stack.at_least(built);
        self.canvas.context_stack.frame = None;
        #[cfg(test)]
        self.context_dividers.clear();
        let mut rects = PaneRects {
            context_charts: SmallVec::from_elem(None, built),
            kept: (self.floating_sides().iter())
                .map(|(_, side, _)| side.index())
                .collect(),
            ..PaneRects::default()
        };
        let flow_band = (bands.iter())
            .find(|(side, _, minimized, _)| *side == PaneSide::Flow && !minimized)
            .map(|(_, rect, ..)| *rect);
        let mut job_slot = flow_band
            .is_none()
            .then(|| self.connection.slot(self.session.history_note()))
            .flatten();
        let mut focus_bands: SmallVec<[egui::Rect; MAX_CANVAS_PANES]> =
            SmallVec::from_elem(egui::Rect::NOTHING, built);
        for &(side, rect, minimized, slot) in &bands {
            if minimized {
                let name = self.strip_name(side);
                chrome.surfaces.remove_chart.with_scoped_updates(
                    tab_id,
                    |_| {},
                    |_, session| {
                        let menus = &mut self.canvas.menus;
                        let door = crate::pane_menu::strip(ui, rect, &name, slot, menus, session);
                        let (at, ink) = ((slot, &self.canvas.state), palette.text_muted());
                        crate::pane_menu::button(ui, door, at, menus, ink, session);
                    },
                );
                continue;
            }
            let PaneSide::Time(slot) = side else {
                continue;
            };
            if slot >= built {
                // Still being built: the canvas, not a stand-in chart.
                ui.painter()
                    .rect_filled(rect, egui::Rounding::ZERO, palette.chrome());
                continue;
            }
            rects.pane_bands.push((side, rect));
            focus_bands[slot] = rect;
            let pane_areas = split_pane_layout_strip(rect);
            let areas = split_pane_header(
                pane_areas.body,
                palette.layout(crate::time_header::HEIGHT_PX),
            );
            self.draw_time_header(
                ui,
                slot,
                areas.header,
                bar_inputs,
                job_slot.take(),
                (tab_id, chrome.surfaces),
            );
            rects.context_charts[slot] = Some((areas.chart, pane_areas.layout_strip));
        }
        if rects.pane_bands.len() + usize::from(flow_band.is_some()) > 1 {
            self.focus_from_pointer(ui, &focus_bands, flow_band.unwrap_or(egui::Rect::NOTHING));
        }
        if let Some(band) = flow_band {
            self.draw_flow_band(tab_id, ui, (band, false), chrome, &mut rects);
        }
        let frame = CanvasFrame {
            area,
            context_shown: built,
            show_flow: flow_band.is_some(),
            split: false,
            time_area: None,
            divider: None,
            flow_band: flow_band.unwrap_or(egui::Rect::NOTHING),
            collapsed_rail: None,
            flow_rail: None,
            tree: true,
        };
        (frame, rects)
    }

    /// The seams between the docked bands, painted as the column painter's
    /// dividers and dragged as they are: a resize cursor over each, and a
    /// drag moving it through the canvas model, folding a side to its rail
    /// at the edge. Then every rail, as the column painter paints its own.
    /// After the panes, so a seam's grab margin wins over the chart beside it.
    pub(super) fn draw_tree_seams(&mut self, tab_id: u64, ui: &egui::Ui, area: egui::Rect) {
        let palette = theme::get(ui.ctx());
        let canvas = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
        let rect = floating_panes::screen_rect;
        let rails = tree_rails(&self.canvas.state, canvas, CANVAS_DIVIDER_PX);
        let words: Vec<String> = rails.iter().map(|rail| self.rail_words(rail)).collect();
        let of = |slot| rails.iter().position(|rail: &SeamRail| rail.slot == slot);
        let mut grabs = vec![Vec::new(); rails.len()];
        for seam in tree_seams(&self.canvas.state, canvas, CANVAS_DIVIDER_PX) {
            let ink = [egui::Color32::TRANSPARENT, palette.border()][usize::from(seam.is_line())];
            ui.painter().rect_filled(rect(seam.rect), 0.0, ink);
            if !seam.resizable {
                continue;
            }
            let row = seam.axis == SplitAxis::Row;
            let id = egui::Id::new(("tree_seam", tab_id, &seam.address));
            let grab = rect(seam.grab(CANVAS_DIVIDER_HANDLE_PX));
            let mut handle = ui.interact(grab, id, egui::Sense::drag());
            if let Some(at) = seam.rail.and_then(|rail| of(rail.slot)) {
                handle = handle.on_hover_text(&words[at]);
                grabs[at].push(grab);
            }
            if handle.hovered() || handle.dragged() {
                ui.ctx().set_cursor_icon(CURSORS[usize::from(row)]);
            }
            // Every step the pointer took since the press, as the column
            // divider adds them up: an off-centre grab never jumps the seam,
            // and travel past a floor still reaches the fold.
            let (state, held) = (&mut self.canvas.state, &mut self.canvas.seam_drag);
            state.seam_hand(held, (&seam.address, super::hand(&handle, row)), canvas);
        }
        for ((rail, words), hits) in rails.iter().zip(words).zip(&grabs) {
            let (at, lit) = (rect(rail.rect), super::rail_hot(ui.painter(), hits));
            super::paint_rail(ui.painter(), at, rail.look(canvas), lit);
            self.canvas.menus.publish_rail(rail.control_id(), at, words);
        }
    }

    /// What the bars controls may offer on this tab's feed.
    pub(super) fn bar_inputs(&self, chrome: &CanvasChrome<'_>) -> BarInputAvailability {
        let recording = self.deal_recording_view();
        crate::toolbar::available_bar_inputs(chrome.capabilities, recording.as_ref())
    }

    /// The flow pane's surface, header and panel, without a footer when floating.
    fn draw_flow_band(
        &mut self,
        tab_id: u64,
        ui: &mut egui::Ui,
        (band, floating): (egui::Rect, bool),
        chrome: &mut CanvasChrome<'_>,
        rects: &mut PaneRects,
    ) {
        let palette = theme::get(ui.ctx());
        let bar_inputs = self.bar_inputs(chrome);
        rects.pane_bands.push((PaneSide::Flow, band));
        // A floating chart keeps no layout footer: it would draw under it.
        let mut pane_areas = split_pane_layout_strip(band);
        if floating {
            pane_areas.body = band;
        }
        let areas = split_flow_pane(pane_areas.body, palette.layout(1.0));
        ui.painter().rect_filled(
            pane_areas.body,
            egui::Rounding::ZERO,
            palette.flow_surface(),
        );
        let header = areas.band;
        self.draw_flow_header(tab_id, ui, header, chrome);
        rects.flow_side_row = Some((areas.side_row, bar_inputs));
        self.flow_pane.frame.side_row = Some(areas.side_row);
        rects.flow_areas = Some(PaneAreas {
            body: crate::orderflow_panel::draw(self, ui, areas.chart, chrome.panel_folds),
            layout_strip: pane_areas.layout_strip,
        });
    }

    /// Every pane outside the docked tree: its slot, the chart it shows and
    /// where it stands.
    pub(crate) fn floating_sides(&mut self) -> SmallVec<[(PaneSlot, PaneSide, FloatHost); 4]> {
        let canvas = &mut self.canvas;
        let floating = canvas.state.floating().to_vec();
        let mut side = |slot| match slot_seat(&canvas.state, &mut canvas.seats, slot)? {
            Some(seat) => Some(PaneSide::Time(seat)),
            None => Some(PaneSide::Flow),
        };
        (floating.into_iter())
            .filter_map(|pane| Some((pane.slot, side(pane.slot)?, pane.host)))
            .collect()
    }

    /// The charts floating over the canvas, each on a layer of its own above
    /// it, and those standing in their own OS windows: after the docked
    /// panes, with the drop preview over all of them.
    pub(super) fn draw_floating(
        &mut self,
        tab_id: u64,
        ui: &mut egui::Ui,
        chrome: &mut CanvasChrome<'_>,
    ) {
        let floating = self.floating_sides();
        let (ctx, canvas) = (ui.ctx().clone(), self.canvas.last_canvas);
        self.canvas.floats.drawn.clear();
        for (slot, side, host) in floating {
            let window = match host {
                FloatHost::Canvas { rect } => {
                    self.canvas.floats.placed.remove(&slot);
                    let rect = floating_panes::placed(&self.canvas.floats, slot, rect, &ctx);
                    let mut layer = floating_panes::layer(&ctx, (tab_id, slot), rect);
                    let floats = &mut self.canvas.floats;
                    let content = floating_panes::frame(&mut layer, rect, slot, floats);
                    self.draw_solo(tab_id, &mut layer, chrome, (side, content, false));
                    continue;
                }
                FloatHost::OsWindow { rect, .. } => rect,
            };
            let placed = self.canvas.floats.placed.entry(slot);
            let mut at = *placed.or_insert((window, window));
            let events = floating_panes::os_window(&ctx, (tab_id, slot), (window, &mut at), |ui| {
                let content = ui.max_rect();
                self.draw_solo(tab_id, ui, chrome, (side, content, true));
            });
            self.canvas.floats.placed.insert(slot, at);
            self.canvas.floats.windows.push((slot, events));
        }
        let canvas_state = &mut self.canvas;
        let floats = &mut canvas_state.floats;
        floating_panes::preview(&ctx, floats, &canvas_state.state, canvas);
        let drawn = floats
            .drawn
            .drain(..)
            .map(|(id, label, rect, capability, selected)| {
                let (rect, capability) = (floating_panes::screen_rect(rect), Some(capability));
                crate::toolbar::DrawnControl {
                    id,
                    label,
                    rect,
                    selected,
                    capability,
                    description: None,
                }
            });
        canvas_state.menus.drawn.extend(drawn);
    }

    /// One floating chart's pane pass on its layer or native window.
    fn draw_solo(
        &mut self,
        tab_id: u64,
        ui: &mut egui::Ui,
        chrome: &mut CanvasChrome<'_>,
        (side, content, windowed): (PaneSide, egui::Rect, bool),
    ) {
        let palette = theme::get(ui.ctx());
        let built = self.time_panes.len();
        let mut rects = PaneRects {
            context_charts: SmallVec::from_elem(None, built),
            kept: (0..=built).filter(|index| *index != side.index()).collect(),
            solo: true,
            ..PaneRects::default()
        };
        match side {
            PaneSide::Time(seat) if seat < built => {
                let bar_inputs = self.bar_inputs(chrome);
                let height = palette.layout(crate::time_header::HEIGHT_PX);
                let areas = split_pane_header(content, height);
                let header = areas.header;
                self.draw_time_header(
                    ui,
                    seat,
                    header,
                    bar_inputs,
                    None,
                    (tab_id, chrome.surfaces),
                );
                rects.context_charts[seat] = Some((areas.chart, egui::Rect::NOTHING));
                rects.pane_bands.push((side, content));
            }
            PaneSide::Time(_) => {
                // Still being built: the canvas, not a stand-in chart.
                ui.painter().rect_filled(content, 0.0, palette.chrome());
                return;
            }
            PaneSide::Flow => self.draw_flow_band(tab_id, ui, (content, true), chrome, &mut rects),
        }
        // A window's own coordinates mean nothing to the canvas's readers.
        let bands = rects.pane_bands.iter().filter(|_| !windowed);
        self.canvas.drawn_bands.extend(bands.copied());
        if ui.input(|input| input.pointer.primary_pressed()) && ui.rect_contains_pointer(content) {
            self.focus = side;
        }
        let picks = self.shared_picks(ui);
        let edits = self.draw_panes(tab_id, ui, chrome, &rects, &picks);
        self.apply_shared_interactions(&edits);
        self.draw_layer_capsules(ui, chrome, |drawn| drawn == side);
        self.draw_flow_sides(ui, rects.flow_side_row, chrome);
        let frame = &mut self.pane_mut(side).frame;
        frame.layout_strip = None;
        frame.area = frame.area.filter(|_| !windowed);
    }

    /// What `rail` says on hover and in the scene: a lone pane by its name.
    fn rail_words(&mut self, rail: &SeamRail) -> String {
        let seat = slot_seat(&self.canvas.state, &mut self.canvas.seats, rail.slot);
        let side = seat.map(|seat| seat.map_or(PaneSide::Flow, PaneSide::Time));
        rail.holds.words(|| side.map(|side| self.strip_name(side)))
    }

    /// What a folded chart's strip calls it: "Time · 1m · BTCUSDT".
    pub(super) fn strip_name(&self, side: PaneSide) -> String {
        let (kind, identity, pane) = match side {
            PaneSide::Time(seat) => (
                PaneKind::Time,
                PaneIdentity::Time,
                self.time_panes.get(seat),
            ),
            PaneSide::Flow => (
                PaneKind::Flow,
                PaneIdentity::OrderFlow,
                Some(&self.flow_pane),
            ),
        };
        let spec = pane.map(|pane| pane.spec.spec());
        // The header's own words, less a "Time" the strip's name already says.
        let bars = spec.map(|spec| {
            if identity == PaneIdentity::Time && spec.time_interval_ms().is_some() {
                spec.parameter_summary()
            } else {
                PaneHeader::of(identity, spec).bars_text
            }
        });
        strip_label(kind, &bars.unwrap_or_default(), &self.symbol)
    }
}
