//! The pane half of the temporary secondary-button range.
//!
//! Input resolves pixels into the pane's chart coordinates; paint projects
//! those coordinates back through the registered ruler. The window-level
//! state and its action bar stay in `surfaces::drawing_chrome`.

use eframe::egui;

use crate::bands::Band;
use crate::drawings::{self, DrawContext};
use crate::toolrail::Tool;

use super::{ChartPane, DRAWING_DRAG_THRESHOLD_PX, PaneChrome};

/// Enough history for the demo's two anchors to span a real, visible range.
#[cfg(feature = "quick-range-harness")]
const DEMO_MIN_BARS: usize = 72;

impl ChartPane {
    pub(super) fn handle_quick_range(
        &self,
        ui: &egui::Ui,
        price_band: &Band,
        history_right: f32,
        total: usize,
        magnet: bool,
        chrome: &mut PaneChrome<'_>,
    ) {
        let owner = crate::surfaces::drawing_chrome::QuickRangeOwner {
            tab: chrome.tab,
            side: chrome.side,
            pane: self.id,
            revision: self.pagination_revision(),
            layout: self.layout_id().map(|id| id.0),
        };
        chrome.drawing_chrome.quick_range.reconcile(Some(owner));
        let area = quantick_chart_interaction::quick_range::GestureArea {
            min: [price_band.rect.left(), price_band.rect.top()],
            max: [
                history_right.max(price_band.rect.left()),
                price_band.rect.bottom(),
            ],
        };
        let (pressed, down, released, pointer) = ui.input(|input| {
            (
                input.pointer.secondary_pressed(),
                input.pointer.secondary_down(),
                input.pointer.secondary_released(),
                input.pointer.latest_pos(),
            )
        });
        // A held secondary button borrows the ruler's projection and saved
        // look without arming or placing the persistent ruler tool. A press
        // that never crosses the threshold remains the context-menu gesture.
        if let Some(measure) = drawings::DrawingTool::by_id("measure") {
            if pressed
                && let Some(position) = pointer
                && let Some(anchor) = self.drawing_projection().drawing_point_at(
                    position,
                    history_right,
                    total,
                    magnet,
                    measure.anchor_snap(),
                    price_band,
                )
            {
                chrome.drawing_chrome.quick_range.press(
                    owner,
                    position,
                    anchor,
                    quantick_chart_interaction::quick_range::GestureEligibility {
                        pointer_tool: chrome.toolrail.tool() == Tool::Pointer,
                        unoccluded: ui
                            .ctx()
                            .layer_id_at(position)
                            .is_none_or(|layer| layer == ui.layer_id()),
                        area,
                    },
                );
            }
            // Past egui's own click distance, not only the drawing drag's:
            // a right-press that slips less than that is still a click, which
            // opens the chart menu, and must not raise a range beside it.
            let range_threshold_px = ui
                .ctx()
                .options(|options| options.input_options.max_click_dist)
                .max(DRAWING_DRAG_THRESHOLD_PX);
            if (down || released)
                && let Some(position) = pointer
            {
                // The divider is last frame's and can sit left of the band
                // after a layout change; `clamp` panics on an inverted range.
                let [x, y] = area.clamp([position.x, position.y]);
                let position = egui::pos2(x, y);
                if let Some(anchor) = self.drawing_projection().drawing_point_at(
                    position,
                    history_right,
                    total,
                    magnet,
                    measure.anchor_snap(),
                    price_band,
                ) {
                    chrome.drawing_chrome.quick_range.drag(
                        owner,
                        position,
                        anchor,
                        range_threshold_px,
                        || drawings::new_drawing_from_defaults(chrome.presets, measure),
                    );
                }
            }
        }
        if released {
            chrome.drawing_chrome.quick_range.release(owner);
        }
    }

    pub(super) fn draw_quick_range(
        &self,
        painter: &egui::Painter,
        bands: &[Band],
        history_right: f32,
        total: usize,
        chrome: &mut PaneChrome<'_>,
    ) {
        let owner = crate::surfaces::drawing_chrome::QuickRangeOwner {
            tab: chrome.tab,
            side: chrome.side,
            pane: self.id,
            revision: self.pagination_revision(),
            layout: self.layout_id().map(|id| id.0),
        };
        chrome.drawing_chrome.quick_range.reconcile(Some(owner));
        #[cfg(feature = "quick-range-harness")]
        if chrome.side == super::PaneSide::Flow {
            chrome
                .drawing_chrome
                .quick_range
                .stage_demo(owner, |future| {
                    let band = bands.first()?;
                    let scale = band.scale.as_ref()?;
                    let measure = drawings::DrawingTool::by_id("measure")?;
                    (total >= DEMO_MIN_BARS).then(|| {
                        let (start, end) = if future {
                            (total.saturating_sub(28), total.saturating_add(2))
                        } else {
                            (total.saturating_sub(70), total.saturating_sub(12))
                        };
                        let point = |slot: usize, y: f32| {
                            drawings::ChartPoint::at_time(
                                slot as f32 + 0.5,
                                scale.price_at(y),
                                self.series_read().anchor_time(slot as f32 + 0.5),
                            )
                        };
                        (
                            [
                                point(start, band.rect.center().y + band.rect.height() * 0.12),
                                point(end, band.rect.center().y - band.rect.height() * 0.12),
                            ],
                            drawings::new_drawing_from_defaults(chrome.presets, measure),
                        )
                    })
                });
        }
        let geometry = chrome
            .drawing_chrome
            .quick_range
            .paint(owner)
            .and_then(|quick| {
                let band = bands.first()?;
                let scale = band.scale.as_ref()?;
                let tool = drawings::DrawingTool::by_id("measure")?;
                let points = quick
                    .anchors
                    .map(|anchor| self.drawing_screen_point(anchor, history_right, total, scale));
                let ctxt = DrawContext {
                    payload: quick.payload,
                    anchors: &quick.anchors,
                    scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: true,
                    style: quick.style,
                    selected: false,
                    halo: false,
                    content_editing: false,
                };
                tool.paint(
                    &painter.with_clip_rect(band.rect),
                    band.rect,
                    quick.style,
                    &points,
                    &ctxt,
                    false,
                );
                let mut bounds = egui::Rect::from_min_max(points[0], points[0]);
                bounds.extend_with(points[1]);
                Some((band.rect, tool.painted_bounds(bounds, band.rect)))
            })
            .or_else(|| {
                chrome
                    .drawing_chrome
                    .quick_range
                    .stale(owner)
                    .then(|| {
                        let chart = bands.first()?.rect;
                        Some((
                            chart,
                            egui::Rect::from_center_size(chart.center(), egui::Vec2::ZERO),
                        ))
                    })
                    .flatten()
            });
        if let Some((chart, bounds)) = geometry {
            chrome.drawing_chrome.quick_range.remember_geometry(
                owner,
                chart,
                bounds,
                history_right,
            );
        }
    }
}
