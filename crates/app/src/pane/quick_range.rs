//! The pane half of the temporary secondary-button range.
//!
//! Input resolves pixels into the pane's chart coordinates; paint projects
//! those coordinates back through the registered ruler. The window-level
//! state and its action bar stay in `surfaces::drawing_chrome`; the headless
//! owner is `quantick_chart_interaction::quick_range`. What the pane lends
//! both is a [`QuickRangeView`]: who it is, as the owner's key, and the
//! projection its pixels are read through — never the pane itself.
//!
//! The range lives on the band the press landed on — the candles or one
//! indicator pane — and is dragged, measured and painted through that band's
//! own scale and unit. A drag never crosses into the next band.

use eframe::egui;

use crate::bands::{self, Band};
use crate::drawings::{self, DrawContext, MarkScene};
use crate::plot_area::PlotAreas;
use crate::surfaces::drawing_chrome::{QuickRange, QuickRangeOwner};
use crate::toolrail::Tool;

use super::PaneChrome;
use super::render_registry::{TextFonts, paint_scene};
use quantick_drawings::gesture::DRAWING_DRAG_THRESHOLD_PX;
use quantick_drawings::gesture::DrawingProjection;

/// Enough history for the demo's two anchors to span a real, visible range.
#[cfg(any(feature = "quick-range-harness", test))]
const DEMO_MIN_BARS: usize = 72;

/// The band the current range lives on among this pane's `bands`; `None`
/// with no range, or once that band is not carved here.
fn range_band<'b>(quick: &QuickRange, bands: &'b [Band]) -> Option<&'b Band> {
    let key = quick.band()?;
    bands.iter().find(|band| band.key == *key)
}

/// One pane's side of the quick range for one frame.
pub(super) struct QuickRangeView<'a> {
    /// The pane as the window-level owner keys it: tab, side, pane id, the
    /// series revision and the layout it shows.
    pub(super) owner: QuickRangeOwner,
    pub(super) projection: DrawingProjection<'a>,
}

impl QuickRangeView<'_> {
    pub(super) fn handle(
        &self,
        ui: &egui::Ui,
        (bands, areas): (&[Band], &PlotAreas),
        history_right: f32,
        total: usize,
        magnet: bool,
        chrome: &mut PaneChrome<'_>,
    ) {
        let owner = self.owner;
        let quick = &mut chrome.drawing_chrome.quick_range;
        quick.reconcile(Some(owner));
        // A range whose band went with its pane (hidden, removed, collapsed)
        // goes too, rather than holding Escape and coming back with the pane.
        if quick.owner().is_some_and(|range| range.pane == owner.pane)
            && quick.band().is_some()
            && range_band(quick, bands)
                .is_none_or(|band| band.refusal == Some(bands::COLLAPSED_BAND_REFUSAL))
        {
            quick.dismiss();
        }
        let area = |band: &Band| quantick_chart_interaction::quick_range::GestureArea {
            min: [band.rect.left(), band.rect.top()],
            max: [history_right.max(band.rect.left()), band.rect.bottom()],
        };
        let (pressed, down, released, pointer, shift) = ui.input(|input| {
            (
                input.pointer.secondary_pressed(),
                input.pointer.secondary_down(),
                input.pointer.secondary_released(),
                input.pointer.latest_pos(),
                input.modifiers.shift,
            )
        });
        // A held secondary button borrows the ruler's projection and saved
        // look without arming or placing the persistent ruler tool. A press
        // that never crosses the threshold remains the context-menu gesture.
        if let Some(measure) = drawings::DrawingTool::by_id("measure") {
            // The press picks the band: never a band that cannot honestly
            // take an anchor, nor an indicator pane's divider or header. The
            // candles keep every pixel they had before panes took ranges.
            if pressed
                && let Some(position) = pointer
                && let Some(band) = bands::band_at(bands, drawings::px(position))
                    .filter(|band| band.drawable())
                    .filter(|band| {
                        band.key == drawings::DrawingBand::Price
                            || !super::primary_button::pane_chrome_hit(areas, position)
                    })
                && let Some(anchor) = self.projection.drawing_point_at(
                    drawings::px(position),
                    history_right,
                    total,
                    magnet,
                    measure.anchor_snap(),
                    band,
                )
            {
                chrome.drawing_chrome.quick_range.press(
                    owner,
                    position,
                    anchor,
                    quantick_chart_interaction::quick_range::GestureEligibility {
                        pointer_tool: chrome.toolrail.tool() == Tool::Pointer,
                        unoccluded: !super::gesture_input::over_chrome(ui, position),
                        area: area(band),
                    },
                    band.key.clone(),
                );
            }
            // Past egui's own click distance, not only the drawing drag's:
            // a right-press that slips less than that is still a click, which
            // opens the chart menu, and must not raise a range beside it.
            let range_threshold_px = ui
                .ctx()
                .options(|options| options.input_options.max_click_dist)
                .max(DRAWING_DRAG_THRESHOLD_PX);
            // Every drag frame reads through the band the press chose.
            let pressed_band = range_band(&chrome.drawing_chrome.quick_range, bands);
            if (down || released)
                && let Some(position) = pointer
                && let Some(band) = pressed_band
            {
                // The divider is last frame's and can sit left of the band
                // after a layout change; `clamp` panics on an inverted range.
                let [x, y] = area(band).clamp([position.x, position.y]);
                let position = egui::pos2(x, y);
                if let Some(anchor) = self.projection.drawing_point_at(
                    drawings::px(position),
                    history_right,
                    total,
                    magnet,
                    measure.anchor_snap(),
                    band,
                ) {
                    chrome.drawing_chrome.quick_range.drag(
                        owner,
                        position,
                        anchor,
                        range_threshold_px,
                        // Levels the ruler; `primary_button.rs` keeps it from trading.
                        shift,
                        || drawings::new_drawing_from_defaults(chrome.presets, measure),
                    );
                }
            }
        }
        if released {
            chrome.drawing_chrome.quick_range.release(owner);
        }
    }

    pub(super) fn draw(
        &self,
        painter: &egui::Painter,
        bands: &[Band],
        history_right: f32,
        total: usize,
        chrome: &mut PaneChrome<'_>,
    ) {
        let owner = self.owner;
        chrome.drawing_chrome.quick_range.reconcile(Some(owner));
        #[cfg(any(feature = "quick-range-harness", test))]
        if chrome.side == super::PaneSide::Flow {
            chrome
                .drawing_chrome
                .quick_range
                .stage_demo(owner, |key, future| {
                    if total < DEMO_MIN_BARS {
                        return Ok(None);
                    }
                    let Some(band) = quantick_drawings::gesture::asked_band(bands, key)? else {
                        return Ok(None);
                    };
                    let (Some(scale), Some(measure)) =
                        (band.scale.as_ref(), drawings::DrawingTool::by_id("measure"))
                    else {
                        return Ok(None);
                    };
                    Ok(Some({
                        let (start, end) = if future {
                            (total.saturating_sub(28), total.saturating_add(2))
                        } else {
                            (total.saturating_sub(70), total.saturating_sub(12))
                        };
                        let point = |slot: usize, y: f32| {
                            drawings::ChartPoint::at_time(
                                slot as f32 + 0.5,
                                scale.price_at(y),
                                self.projection.series.anchor_time(slot as f32 + 0.5),
                            )
                        };
                        (
                            [
                                point(start, band.rect.center().y + band.rect.height() * 0.12),
                                point(end, band.rect.center().y - band.rect.height() * 0.12),
                            ],
                            drawings::new_drawing_from_defaults(chrome.presets, measure),
                        )
                    }))
                });
        }
        let geometry = chrome
            .drawing_chrome
            .quick_range
            .paint(owner)
            .and_then(|quick| {
                let band = range_band(&chrome.drawing_chrome.quick_range, bands)?;
                let scale = band.scale.as_ref()?;
                let tool = drawings::DrawingTool::by_id("measure")?;
                let points = quick.anchors.map(|anchor| {
                    self.projection
                        .drawing_screen_point(anchor, history_right, total, scale)
                });
                let ctxt = DrawContext {
                    payload: quick.payload,
                    anchors: &quick.anchors,
                    scale,
                    px_per_bar: self.projection.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: true,
                    style: quick.style,
                    selected: false,
                    halo: false,
                    content_editing: false,
                    text: &TextFonts(painter.ctx()),
                };
                let mut scene = MarkScene::default();
                tool.paint(&mut scene, band.rect, quick.style, &points, &ctxt, false);
                let rect = drawings::egui_rect(band.rect);
                paint_scene(&painter.with_clip_rect(rect), &scene);
                let mut bounds = quantick_drawings::PxRect::from_min_max(points[0], points[0]);
                bounds.extend_with(points[1]);
                Some((
                    rect,
                    drawings::egui_rect(tool.painted_bounds(bounds, band.rect)),
                ))
            })
            .or_else(|| {
                let quick = &chrome.drawing_chrome.quick_range;
                quick
                    .stale(owner)
                    .then(|| {
                        // The notice sits on the band the range was drawn on.
                        let band = range_band(quick, bands).or_else(|| bands.first())?;
                        let chart = drawings::egui_rect(band.rect);
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
