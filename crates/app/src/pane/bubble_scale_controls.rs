//! Pane-local size controls occupy their own input layer above the chart.
use super::constants::{TAPE_SWITCH_GAP_PX, TAPE_SWITCH_INSET, TAPE_SWITCH_SIZE};
use super::{ChartPane, PaneChrome};
use eframe::egui;
use quantick_orderflow::{
    BubbleScaleTarget, DEFAULT_BUBBLE_DISPLAY_SCALE, MAX_BUBBLE_DISPLAY_SCALE,
    MIN_BUBBLE_DISPLAY_SCALE,
};

const INSET: f32 = 8.0;
const TOP: f32 = TAPE_SWITCH_INSET.y + TAPE_SWITCH_SIZE.y + TAPE_SWITCH_GAP_PX;
const WIDTH: f32 = 156.0;
const HEIGHT: f32 = 26.0;
const MARGIN: f32 = 4.0;
const MIN_CONTROL_WIDTH: f32 = 24.0;
const MIN_LABELED_TRACK_WIDTH: f32 = 64.0;

#[derive(Clone, Copy)]
struct ScaleControl {
    target: BubbleScaleTarget,
    rect: egui::Rect,
    value: f32,
}

/// Register before navigation: both response-based navigation and raw
/// drawing gestures must see these floating controls as pointer owners.
pub(crate) fn show(
    pane: &mut ChartPane,
    ui: &egui::Ui,
    area: egui::Rect,
    chrome: &PaneChrome<'_>,
) -> [Option<egui::Rect>; 2] {
    let chart = pane.plot_areas(area, chrome.capabilities).chart;
    let mut controls = [None, None];
    for (slot, control) in layout(pane, chart).into_iter().enumerate() {
        let Some(control) = control else {
            continue;
        };
        let id = pane.interaction_id(match control.target {
            BubbleScaleTarget::Candle => "candle_bubble_scale",
            BubbleScaleTarget::Tape => "tape_bubble_scale",
        });
        let (changed, rect) = draw_control(ui.ctx(), id, control);
        controls[slot] = Some(rect);
        if let Some(value) = changed
            && let Some(owner) = pane.orderflow.as_mut()
        {
            owner.set_bubble_display_scale(control.target, value);
        }
    }
    controls
}

fn layout(pane: &ChartPane, chart: egui::Rect) -> [Option<ScaleControl>; 2] {
    let Some(owner) = pane.orderflow.as_ref() else {
        return [None, None];
    };
    let tape_only = pane.tape_modes().0;
    let width = if tape_only {
        chart.width()
    } else {
        owner.lane_width_px(chart.width())
    };
    let divider = chart.right() - width;
    let history = chart.with_max_x(divider);
    let tape = chart.with_min_x(divider);
    let candle_visible =
        owner.bubbles_enabled() && !tape_only && pane.state.tick_membership().is_some();
    let tape_visible = owner.lane_enabled() && owner.lane_bubbles_enabled();
    [
        (BubbleScaleTarget::Candle, history, candle_visible),
        (BubbleScaleTarget::Tape, tape, tape_visible),
    ]
    .map(|(target, region, visible)| {
        if !visible {
            return None;
        }
        Some(ScaleControl {
            target,
            rect: control_rect(region)?,
            value: owner.bubble_display_scale(target)?,
        })
    })
}

fn control_rect(region: egui::Rect) -> Option<egui::Rect> {
    let width = (region.width() - 2.0 * INSET).min(WIDTH);
    (width >= MIN_CONTROL_WIDTH && region.height() >= TOP + HEIGHT).then(|| {
        egui::Rect::from_min_size(
            egui::pos2(region.right() - INSET - width, region.top() + TOP),
            egui::vec2(width, HEIGHT),
        )
    })
}

fn draw_control(
    ctx: &egui::Context,
    id: egui::Id,
    control: ScaleControl,
) -> (Option<f32>, egui::Rect) {
    let mut value = control.value;
    let label = match control.target {
        BubbleScaleTarget::Candle => "Tick bubble size",
        BubbleScaleTarget::Tape => "Tape bubble size",
    };
    let shown = egui::Area::new(id)
        .fixed_pos(control.rect.min)
        .default_size(control.rect.size())
        .constrain_to(control.rect)
        .movable(false)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_black_alpha(165))
                .rounding(egui::Rounding::same(3.0))
                .inner_margin(egui::Margin::same(MARGIN))
                .show(ui, |ui| {
                    let font = egui::TextStyle::Monospace.resolve(ui.style());
                    let value_width = ui
                        .painter()
                        .layout_no_wrap("3.00x".into(), font, ui.visuals().text_color())
                        .size()
                        .x;
                    let inner_width = control.rect.width() - 2.0 * MARGIN;
                    let compact = inner_width - value_width - ui.spacing().item_spacing.x
                        < MIN_LABELED_TRACK_WIDTH;
                    let reserved = if compact {
                        0.0
                    } else {
                        value_width + ui.spacing().item_spacing.x
                    };
                    ui.set_max_width(inner_width);
                    ui.spacing_mut().slider_width = inner_width - reserved;
                    let response = ui
                        .horizontal(|ui| {
                            let response = ui.add(
                                egui::Slider::new(
                                    &mut value,
                                    MIN_BUBBLE_DISPLAY_SCALE..=MAX_BUBBLE_DISPLAY_SCALE,
                                )
                                .show_value(false),
                            );
                            if !compact {
                                ui.monospace(format!("{value:.2}x"));
                            }
                            response
                        })
                        .inner;
                    if response.double_clicked() {
                        value = DEFAULT_BUBBLE_DISPLAY_SCALE;
                    }
                    let response = response.on_hover_ui(|ui| {
                        ui.label(format!("{label}: {value:.2}x"));
                        ui.label("Changes visual size only. Double-click for 1x.");
                    });
                    response.rect
                })
                .inner
        });
    // egui registers Area's remembered bounds before measuring this frame.
    // Adding drag updates that same root widget to the measured bounds on resize.
    let _ = shown.response.interact(egui::Sense::drag());
    ((value != control.value).then_some(value), shown.inner)
}

#[cfg(test)]
#[path = "tests/bubble_scale_controls_tests.rs"]
mod tests;
