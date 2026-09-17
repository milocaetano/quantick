//! The window side of the drawing chrome (§8): inspector, manager, notes.
//!
//! [`crate::surfaces::drawing_chrome`] draws; this file assembles what it
//! draws from and applies what it returns. [`drawing_env`] and
//! [`DrawingViewCosts`] are the seam — free items rather than methods, so a
//! caller can hand the surface a disjoint borrow of the window.
//!
//! Editing a drawing *on the canvas* is [`super::DrawingController`]; this is
//! the chrome around it.

use eframe::egui;

use crate::drawings::{self, DrawingAuthor};
use crate::pane::DRAWING_ANCHOR_RADIUS_PX;
use crate::toolrail::{Tool, ToolRail};

use super::{DrawingAccess, DrawingController, DrawingReadAccess};

/// The slice the drawing chrome reads, assembled from the pieces of the
/// application it is allowed to see.
///
/// A free function rather than a method for the reason
/// the frame composition is one: every caller has already
/// split `QuantickApp` into disjoint borrows to draw a surface through `&mut`,
/// and a method would want the whole of `self` back. That the compiler insists
/// on the split is the port working.
///
/// `manager_rows` is handed in rather than gathered here. Only one of the two
/// call sites draws the list, and building a row per object for the site that
/// does not would be a per-frame allocation for a window nobody is looking at.
fn drawing_env<'a>(
    host: &'a DrawingReadAccess<'_>,
    toolrail: &ToolRail,
    presets: &'a drawings::presets::PresetStore,
    read: DrawingViewCosts<'a>,
) -> crate::surfaces::DrawingEnv<'a> {
    let facts = host.chrome_facts();
    let store = host.drawings();
    let selected = store.selected().and_then(|index| {
        store
            .items()
            .get(index)
            .map(|drawing| crate::surfaces::drawing_chrome::SelectedDrawing { index, drawing })
    });
    let automatic_bar_area = selected.as_ref().and_then(|selection| {
        if !matches!(selection.drawing.band, drawings::DrawingBand::Indicator(_)) {
            return None;
        }
        let band = host.band(selection.drawing)?;
        let header = crate::indicator_render::pane_header_rect(band.rect, false);
        Some(egui::Rect::from_min_max(
            egui::pos2(
                band.rect.left(),
                header.bottom() + drawings::context_bar::OBJECT_GAP_PX,
            ),
            band.rect.max,
        ))
    });
    crate::surfaces::DrawingEnv {
        pane_id: facts.pane_id,
        selected,
        chart_area: facts.chart_area,
        automatic_bar_area,
        focused_chart_area: facts.focused_chart_area,
        lane_divider_x: facts.lane_divider_x,
        legends: facts.legends,
        auto_range: facts.auto_range,
        selected_bbox: read.selected_bbox,
        selected_band: read.selected_band,
        tab: facts.tab,
        side: facts.side,
        drawing_tool_armed: matches!(toolrail.tool(), Tool::Drawing(_)),
        toolbox_dock: toolrail.dock(),
        authored_objects: read.authored_objects,
        manager_rows: read.manager_rows,
        presets,
    }
}

/// The parts of [`drawing_env`] that cost something to work out, gathered by
/// the caller so each pass pays only for what it draws.
///
/// Three fields and three prices. Projecting the selection's painted bounds
/// walks its anchors through the price scale; naming its band formats a
/// string; counting an assistant's objects walks every pane of every tab. All
/// three are per-frame while a selection is on screen, which is why the pass
/// that only runs the capture hooks gathers none of them and says so.
#[derive(Default)]
struct DrawingViewCosts<'a> {
    selected_bbox: Option<egui::Rect>,
    selected_band: Option<String>,
    authored_objects: usize,
    manager_rows: &'a [crate::surfaces::drawing_chrome::ManagerRow],
}

impl DrawingController {
    fn drawing_manager_rows(
        &self,
        host: &DrawingReadAccess<'_>,
    ) -> Vec<crate::surfaces::drawing_chrome::ManagerRow> {
        let store = host.drawings();
        let selected = store.selected();
        store
            .items()
            .iter()
            .enumerate()
            .map(
                |(index, drawing)| crate::surfaces::drawing_chrome::ManagerRow {
                    name: drawing.display_label(index),
                    selected: selected == Some(index),
                    locked: drawing.locked,
                    hidden: drawing.hidden,
                    shared: drawing.scope == drawings::DrawingScope::AllCharts,
                    off_series: drawing.off_series,
                    foreign_market: drawing.foreign_market,
                    author: drawing.author.as_ref().map(DrawingAuthor::label),
                    band: host.band_label(drawing),
                },
            )
            .collect()
    }
    fn selected_drawing_bbox(&self, host: &DrawingReadAccess<'_>) -> Option<egui::Rect> {
        let store = host.drawings();
        let index = store.selected()?;
        let chart = host.chart_area()?;
        self.drawing_bbox_on_screen(host, chart, index)
    }
    fn selected_drawing_band(&self, host: &DrawingReadAccess<'_>) -> Option<String> {
        let store = host.drawings();
        let index = store.selected()?;
        host.band_label(store.items().get(index)?).chip()
    }
    pub(crate) fn drawing_bbox_on_screen(
        &self,
        host: &DrawingReadAccess<'_>,
        chart: egui::Rect,
        index: usize,
    ) -> Option<egui::Rect> {
        let store = host.drawings();
        let total = host.slots();
        let drawing = store.items().get(index)?;
        let band = host.band(drawing)?;
        let scale = band.scale?;
        let history_right = host.history_right(chart);
        let points = host.projected_points(drawing, history_right, total, &scale);
        let first = points.first()?;
        let mut bbox = egui::Rect::from_min_max(*first, *first);
        for point in &points {
            bbox.extend_with(*point);
        }
        // What the tool paints, which is not always where its anchors are: a
        // fixed-range profile anchors at one price and covers the axis. Every
        // popup that keeps clear of an object reads this rectangle, so asking
        // the anchors alone is what let a panel land in the middle of a
        // profile while believing it had walked around it.
        let bbox = drawing.tool.painted_bounds(bbox, band.rect);
        Some(bbox.expand(DRAWING_ANCHOR_RADIUS_PX))
    }
    pub(crate) fn draw_pinned_inspector(
        &mut self,
        ctx: &egui::Context,
        host: &DrawingReadAccess<'_>,
        tools: &ToolRail,
    ) -> Option<crate::surfaces::drawing_chrome::DrawingChromeAsk> {
        if !self.chrome.inspector_pinned() {
            return None;
        }
        // No painted bounds: a docked panel has no placement rule to keep
        // clear of the object, so the projection the floating one needs is not
        // gathered here. The band still is — it is in the title.
        let read = DrawingViewCosts {
            selected_band: self.selected_drawing_band(host),
            ..DrawingViewCosts::default()
        };
        let ask = self.draw_chrome_pass(ctx, host, tools, read, false);
        Some(ask)
    }
    pub(crate) fn draw_drawing_chrome(
        &mut self,
        ctx: &egui::Context,
        host: &DrawingReadAccess<'_>,
        tools: &ToolRail,
    ) -> crate::surfaces::drawing_chrome::DrawingChromeAsk {
        let manager_open = self.chrome.manager_open();
        let rows = if manager_open {
            self.drawing_manager_rows(host)
        } else {
            Vec::new()
        };
        // The band name goes in the inspector's title and nowhere else, so
        // it is formatted only when one of the two inspector hosts is on
        // screen. A selection alone raises the context bar, which never shows
        // it — and `band_label` scans the pane's indicator views and `chip`
        // allocates, every frame, for a value nothing would read.
        let inspector_showing = self.chrome.inspector_open() || self.chrome.inspector_pinned();
        let read = DrawingViewCosts {
            selected_bbox: self.selected_drawing_bbox(host),
            selected_band: inspector_showing
                .then(|| self.selected_drawing_band(host))
                .flatten(),
            // Counted only for the window that offers to take them back, and
            // over every tab: an object an assistant placed on another chart
            // still belongs in that count.
            authored_objects: if manager_open {
                host.authored_count()
            } else {
                0
            },
            manager_rows: &rows,
        };
        self.draw_chrome_pass(ctx, host, tools, read, true)
    }
    fn draw_chrome_pass(
        &mut self,
        ctx: &egui::Context,
        host: &DrawingReadAccess<'_>,
        tools: &ToolRail,
        read: DrawingViewCosts<'_>,
        floating: bool,
    ) -> crate::surfaces::drawing_chrome::DrawingChromeAsk {
        let env = drawing_env(host, tools, &self.presets, read);
        let current = floating
            .then(|| host.current_range_owner(self.chrome.quick_range.owner()))
            .flatten();
        self.chrome.draw_pass(ctx, &env, current, floating)
    }

    pub(crate) fn place_text_note(&mut self, host: &mut DrawingAccess<'_>) -> bool {
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
        else {
            return false;
        };
        let Some(point) = host.text_note_point() else {
            return false;
        };
        // Through the same door the click path uses, saved defaults and all —
        // and on the same pane every drawing surface reads, so the index the
        // editor opens on is the object this just placed.
        let fresh = drawings::new_drawing_from_defaults(&self.presets, tool);
        let placed =
            host.drawings_mut()
                .place_with(tool, &drawings::DrawingBand::Price, point, |_| fresh);
        if placed && let Some(index) = host.drawings().selected() {
            self.begin_inline_text_edit(host, index);
        }
        placed
    }
    pub(crate) fn begin_inline_text_edit(
        &mut self,
        host: &mut DrawingAccess<'_>,
        index: usize,
    ) -> bool {
        let (tab, side) = host.target();
        let Some(drawing) = host.drawings().items().get(index) else {
            return false;
        };
        if !self
            .chrome
            .begin_inline_text_edit(tab, side, index, drawing)
        {
            return false;
        }
        host.drawings_mut().select(Some(index));
        host.sync_inline(&self.chrome);
        true
    }
}
