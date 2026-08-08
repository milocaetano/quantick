//! The on-chart indicator legend (audit S1; spec'd in
//! `docs/ux/ui-design-model.md` §9 and never built): one row per indicator
//! on the pane, pinned to the chart's top-left — colour dot, name, last
//! value, and the eye / gear / remove controls beside it. Double-clicking
//! the name opens settings, the TradingView gesture the audit's pain 3
//! found nowhere to happen.
//!
//! Error and stale rows say so *on the chart*, red and amber, with the full
//! message on hover. An errored indicator used to disappear from the render
//! with its message computed and never shown (B1) — against the project's
//! data-honesty rule; this surface is where that statement now lives.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_indicators::Rgba8;

use crate::chart;
use crate::indicator_worker::SlotId;
use crate::indicators::IndicatorView;
use crate::theme;

/// Gap between the pane's corner and the legend, in pixels.
const LEGEND_MARGIN_PX: f32 = 8.0;
/// Diameter of a row's colour dot, in pixels.
const DOT_DIAMETER_PX: f32 = 8.0;
/// How much of a hidden row's dot colour survives — enough to still name
/// the plot, faded enough to read as off.
const HIDDEN_DOT_FADE: f32 = 0.4;
/// Height of one row, in pixels: a small button plus egui's button padding,
/// which is the tallest widget in the row.
const ROW_HEIGHT_PX: f32 = 20.0;
/// Vertical gap between two rows, matching the spacing the frame sets.
const ROW_SPACING_PX: f32 = 3.0;
/// The frame's own vertical padding, top and bottom.
const FRAME_PADDING_Y_PX: f32 = 5.0;
/// How far this legend drops below the position HUD when both claim the
/// chart's top-left corner. See [`hud_offset_px`].
const HUD_OFFSET_PX: f32 = 64.0;

/// What a legend row asked of the app this frame. The caller applies each
/// against the pane the legend was drawn for — never the focused pane, so a
/// row can never act on the chart beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegendAction {
    /// Flip the render-side eye (no recompute).
    ToggleHidden(SlotId),
    /// Open the settings dialog.
    OpenSettings(SlotId),
    /// Remove the indicator.
    Remove(SlotId),
}

/// How far down this legend starts when the position HUD owns the very
/// corner. Zero with no open position.
///
/// One owner for one number: the app places the legend with it, and the pane
/// stacks the order-flow key below the same corner with it. They disagreed
/// once — the pane also demanded input focus, so in a split with a position
/// open and the *other* pane focused the chips dropped and the key did not,
/// straight back onto them. The HUD does not care who has focus, so neither
/// does this.
pub(crate) fn hud_offset_px(position_open: bool) -> f32 {
    if position_open { HUD_OFFSET_PX } else { 0.0 }
}

/// How much of the canvas's top-left corner this legend claims, measured
/// from the corner itself — its own margin included, zero when it draws
/// nothing.
///
/// Predicted rather than measured, so whoever stacks below it (the order-flow
/// key) lands correctly on the very first frame instead of printing over a
/// legend that had not been laid out yet. `the_predicted_stack_height_covers_
/// what_the_legend_actually_draws` keeps the prediction honest against the
/// real layout.
pub(crate) fn stack_height_px(views: &[IndicatorView]) -> f32 {
    if views.is_empty() {
        return 0.0;
    }
    let rows = views.len() as f32;
    LEGEND_MARGIN_PX
        + FRAME_PADDING_Y_PX * 2.0
        + rows * ROW_HEIGHT_PX
        + (rows - 1.0) * ROW_SPACING_PX
}

/// Draw the legend over `chart_rect`. A no-op with no indicators — an empty
/// legend frame would be chart chrome with nothing to say.
pub(crate) fn draw(
    ctx: &egui::Context,
    pane_id: u64,
    chart_rect: egui::Rect,
    views: &[IndicatorView],
) -> Vec<LegendAction> {
    let mut actions = Vec::new();
    if views.is_empty() {
        return actions;
    }
    egui::Area::new(egui::Id::new(("indicator_legend", pane_id)))
        .fixed_pos(chart_rect.left_top() + egui::vec2(LEGEND_MARGIN_PX, LEGEND_MARGIN_PX))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            // The HUD's card grammar, quieter: sunken fill, hairline border,
            // no rail — the rows' own colour dots carry the identity.
            egui::Frame::none()
                .fill(theme::INSET)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0);
                    for view in views {
                        draw_row(ui, view, &mut actions);
                    }
                });
        });
    actions
}

/// One legend row. Status precedence matches the toolbar menu's dot:
/// errored beats stale beats hidden — a broken indicator must never read as
/// merely hidden.
fn draw_row(ui: &mut egui::Ui, view: &IndicatorView, actions: &mut Vec<LegendAction>) {
    ui.horizontal(|ui| {
        draw_status_dot(ui, view);
        let name_color = if view.error.is_some() {
            theme::SELL
        } else if view.stale.is_some() {
            theme::ACCENT
        } else if view.hidden {
            theme::TEXT_MUTED
        } else {
            theme::TEXT_PRIMARY
        };
        let name = ui
            .add(
                egui::Label::new(egui::RichText::new(view.label()).color(name_color).small())
                    .sense(egui::Sense::click()),
            )
            .on_hover_text("double-click to open settings");
        if name.double_clicked() {
            actions.push(LegendAction::OpenSettings(view.slot));
        }
        // The last committed value of the first plot — the number a trader
        // reads an overlay by. Withheld while hidden or errored: a value
        // from a line that is not on screen answers nothing.
        if !view.hidden
            && view.error.is_none()
            && let Some(value) = view.columns.first().and_then(|column| column.last())
        {
            ui.label(
                egui::RichText::new(chart::compact_value(*value))
                    .monospace()
                    .small()
                    .color(theme::TEXT_MUTED),
            );
        }
        if let Some(error) = &view.error {
            ui.label(egui::RichText::new("error").small().color(theme::SELL))
                .on_hover_text(error.to_string());
        } else if let Some(stale) = &view.stale {
            ui.label(egui::RichText::new("stale").small().color(theme::ACCENT))
                .on_hover_text(stale.clone());
        }
        if ui
            .small_button(if view.hidden {
                icons::EYE
            } else {
                icons::EYE_SLASH
            })
            .on_hover_text("hide/show without removing (no recompute)")
            .clicked()
        {
            actions.push(LegendAction::ToggleHidden(view.slot));
        }
        if ui
            .small_button(icons::GEAR)
            .on_hover_text("settings")
            .clicked()
        {
            actions.push(LegendAction::OpenSettings(view.slot));
        }
        if ui
            .small_button(icons::TRASH)
            .on_hover_text("remove this indicator")
            .clicked()
        {
            actions.push(LegendAction::Remove(view.slot));
        }
    });
}

/// The row's colour dot: the first plot's own colour, so the row and the
/// line it names can never disagree; error and stale wear the same glyphs
/// as the toolbar menu's status dot.
fn draw_status_dot(ui: &mut egui::Ui, view: &IndicatorView) {
    if view.error.is_some() {
        ui.label(
            egui::RichText::new(icons::WARNING_CIRCLE)
                .small()
                .color(theme::SELL),
        );
        return;
    }
    if view.stale.is_some() {
        ui.label(
            egui::RichText::new(icons::WARNING)
                .small()
                .color(theme::ACCENT),
        );
        return;
    }
    let color = view
        .descriptor
        .plots
        .first()
        .map_or(theme::TEXT_MUTED, |plot| rgba(plot.base_color));
    let color = if view.hidden {
        color.gamma_multiply(HIDDEN_DOT_FADE)
    } else {
        color
    };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(DOT_DIAMETER_PX, DOT_DIAMETER_PX),
        egui::Sense::hover(),
    );
    ui.painter()
        .circle_filled(rect.center(), DOT_DIAMETER_PX / 2.0, color);
}

fn rgba(color: Rgba8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator_worker::IndicatorEvent;
    use crate::indicators::IndicatorViews;
    use quantick_indicators::{EvalError, IndicatorDescriptor, PlotId, PlotSpec, PlotStyle};

    fn descriptor(title: &str) -> IndicatorDescriptor {
        IndicatorDescriptor {
            title: title.to_owned(),
            short_title: None,
            overlay: true,
            plots: vec![PlotSpec {
                id: PlotId::new(0),
                title: "p0".to_owned(),
                style: PlotStyle::Line,
                base_color: Rgba8::opaque(0x26, 0xA6, 0x9A),
                width: 1.0,
                offset: 0,
                marker: None,
            }],
            inputs: Vec::new(),
            fills: Vec::new(),
        }
    }

    fn views_with(build: impl FnOnce(&mut IndicatorViews, SlotId)) -> IndicatorViews {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        build(&mut views, slot);
        views
    }

    fn painted(ctx: &egui::Context, views: &IndicatorViews) -> String {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let mut text = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let actions = draw(ctx, 0, chart, views.all());
                assert!(actions.is_empty(), "no clicks, no actions");
            });
            text.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = shape.shape {
                    text.push_str(galley.galley.text());
                    text.push(' ');
                }
            }
        }
        text
    }

    /// Whoever stacks under this legend — the order-flow key at the same
    /// corner — places itself from [`stack_height_px`] before the legend has
    /// ever been laid out. The prediction therefore has to cover what the
    /// legend really draws, at every row count, or the two print over each
    /// other on the frame an indicator is added.
    #[test]
    fn the_predicted_stack_height_covers_what_the_legend_actually_draws() {
        let ctx = egui::Context::default();
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        assert_eq!(stack_height_px(&[]), 0.0, "nothing drawn, nothing claimed");

        let mut views = IndicatorViews::new();
        for rows in 1..=3_usize {
            let slot = views.allocate_slot("test.indicator");
            views.apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor(&format!("EMA({rows}, close)")),
                columns: vec![vec![101.5, 1_234.0]],
                inputs: Vec::new(),
                stale: None,
            });

            let mut bottom = f32::NEG_INFINITY;
            for _ in 0..2 {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    draw(ctx, 0, chart, views.all());
                });
                bottom = f32::NEG_INFINITY;
                for shape in output.shapes {
                    let rect = shape.shape.visual_bounding_rect();
                    if rect.is_positive() {
                        bottom = bottom.max(rect.bottom());
                    }
                }
            }
            let claimed = chart.top() + stack_height_px(views.all());
            assert!(
                bottom <= claimed,
                "{rows} row(s): the legend reaches {bottom}, the prediction claims {claimed}"
            );
        }
    }

    /// A healthy row reaches pixels: name and last value, in the axis'
    /// compact spelling.
    #[test]
    fn the_legend_names_the_indicator_and_its_last_value() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor("EMA(9, close)"),
                columns: vec![vec![101.5, 1_234.0]],
                inputs: Vec::new(),
                stale: None,
            });
        });
        let text = painted(&ctx, &views);
        assert!(text.contains("EMA(9, close)"), "painted: {text}");
        assert!(text.contains("1.23K"), "painted: {text}");
    }

    /// B1: an errored indicator states its error on the chart, message
    /// reachable — it must never simply disappear.
    #[test]
    fn an_errored_indicator_states_its_error_on_the_chart() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor("zigzag.pine"),
                columns: vec![vec![1.0]],
                inputs: Vec::new(),
                stale: None,
            });
            views.apply(IndicatorEvent::Error {
                slot,
                error: EvalError {
                    bar_index: 7,
                    message: "PINE_NO_SECURITY: security() is not supported".to_owned(),
                },
            });
        });
        let text = painted(&ctx, &views);
        assert!(text.contains("zigzag.pine"), "painted: {text}");
        assert!(text.contains("error"), "painted: {text}");
    }

    /// A stale row wears the amber word; a flat list draws nothing at all.
    #[test]
    fn stale_rows_say_stale_and_an_empty_list_paints_nothing() {
        let ctx = egui::Context::default();
        let views = views_with(|views, slot| {
            views.apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor("cvd.pine"),
                columns: vec![vec![2.0]],
                inputs: Vec::new(),
                stale: Some("edit has errors; running the previous version".to_owned()),
            });
        });
        let text = painted(&ctx, &views);
        assert!(text.contains("stale"), "painted: {text}");

        let empty = IndicatorViews::new();
        assert!(painted(&ctx, &empty).is_empty(), "no rows, no chrome");
    }
}
