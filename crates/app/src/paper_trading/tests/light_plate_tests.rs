//! Light's chart plates, read off what the paint helpers actually emit: the
//! aim's preview plates like a placed order, a pending TP and SL still tell
//! apart, and a plated close glyph still answers a hover.
use super::*;
use crate::paper_trading::paint_ctx::close_button_rect;

fn light_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply(quantick_appearance::LIGHT).unwrap();
    crate::theme::sync(&ctx, &appearance);
    ctx
}

/// Every shape `paint` emits through a `PaintCtx` over the test chart, in
/// `ctx`'s theme, with the pointer at `pointer`.
fn shapes_of(
    ctx: &egui::Context,
    pointer: egui::Pos2,
    paint: impl Fn(&PaintCtx<'_>),
) -> Vec<egui::Shape> {
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        paint(&PaintCtx {
            palette: crate::theme::get(ctx),
            painter: &painter,
            chart_rect: chart,
            tag_right: chart.right(),
            axis_x: chart.right(),
            scale: &scale,
            reserved_chip_y: None,
            pointer: Some(pointer),
        });
    });
    output
        .shapes
        .into_iter()
        .map(|clipped| clipped.shape)
        .collect()
}

/// The colour the first text starting with `glyph` was laid out in.
fn glyph_ink(shapes: &[egui::Shape], glyph: &str) -> egui::Color32 {
    shapes
        .iter()
        .find_map(|shape| match shape {
            egui::Shape::Text(text) if text.galley.text().starts_with(glyph) => {
                Some(text.galley.job.sections[0].format.color)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {glyph} painted"))
}

/// The cmd-trading preview's dashed line and label take the order plate, as
/// a placed order's line does, never the raw side colour.
#[test]
fn the_cmd_preview_plates_like_a_placed_order_in_light() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let at = egui::pos2(400.0, 300.0);
    paper.handle_chart_input(&cmd_frame(chart, &scale, at, shift, false));
    let ctx = light_ctx();
    let palette = crate::theme::get(&ctx);
    let shapes = shapes_of(&ctx, at, |paint| paper.draw_cmd_preview(paint, true));
    let segments: Vec<_> = shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::LineSegment { stroke, .. } => match stroke.color {
                egui::epaint::ColorMode::Solid(color) => Some(color),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert!(segments.len() >= 8, "a dashed line: {segments:?}");
    assert!(
        segments.iter().all(|color| *color != palette.buy()),
        "no raw side colour on the plated chart: {segments:?}"
    );
    assert!(segments.contains(&palette.order_plate()));
    let plate_fills = shapes
        .iter()
        .filter(
            |shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == palette.order_plate()),
        )
        .count();
    assert!(plate_fills >= 2, "the label and the gutter chip are plates");
    // The label reads its side's plate ink, as the gutter chip beside it.
    assert_eq!(glyph_ink(&shapes, "BUY "), palette.plate_gain());
}

/// A pending TP and SL are outlined in the plate's gain and loss inks, so
/// the two ghosts no longer look identical.
#[test]
fn light_ghost_legs_tell_take_profit_from_stop() {
    let ctx = light_ctx();
    let palette = crate::theme::get(&ctx);
    let outline = |side: egui::Color32| {
        let shapes = shapes_of(&ctx, egui::pos2(10.0, 10.0), |paint| {
            paint.chip_tag(200.0, side, "TP 2", false, true);
        });
        shapes
            .iter()
            .find_map(|shape| match shape {
                egui::Shape::Rect(rect) if rect.stroke.width > 0.0 => Some(rect.stroke.color),
                _ => None,
            })
            .expect("a ghost is outlined")
    };
    let take_profit = outline(palette.buy());
    let stop = outline(palette.sell());
    assert_eq!(take_profit, palette.plate_gain());
    assert_eq!(stop, palette.plate_loss());
    assert_ne!(take_profit, stop);
}

/// A plated close glyph rests dimmed and takes the plate's full ink under
/// the pointer, on an order chip and on the position tag; Dark's position
/// tag keeps its muted and primary pair.
#[test]
fn plated_close_glyphs_answer_a_hover() {
    let y = 200.0;
    let (chart, _) = chart_and_scale(80.0, 120.0);
    let button = close_button_rect(chart.right(), y).center();
    let away = egui::pos2(10.0, 10.0);
    for dark in [false, true] {
        let ctx = if dark {
            egui::Context::default()
        } else {
            light_ctx()
        };
        let palette = crate::theme::get(&ctx);
        let ink = |pointer, position: bool| {
            let shapes = shapes_of(&ctx, pointer, |paint| {
                if position {
                    paint.position_tag(y, palette.buy(), "SIM LONG 2", None);
                } else {
                    paint.chip_tag(y, palette.accent(), "BUY LMT 2", true, false);
                }
            });
            glyph_ink(&shapes, "×")
        };
        for position in [false, true] {
            let (rest, lit) = (ink(away, position), ink(button, position));
            if dark && !position {
                let before = crate::paper_chrome::trade_action_ink(&palette, palette.accent());
                assert_eq!((rest, lit), (before, before), "Dark's order chip stays");
                continue;
            }
            assert_ne!(rest, lit, "dark {dark}, position tag {position}");
            if dark {
                assert_eq!((rest, lit), (palette.text_muted(), palette.text_primary()));
            } else {
                let full = palette.order_plate_ink();
                assert_eq!(lit, full);
                assert_eq!(
                    rest,
                    crate::theme::with_opacity(
                        full,
                        quantick_appearance::paper::close_glyph_alpha(false)
                    )
                );
            }
        }
    }
}
