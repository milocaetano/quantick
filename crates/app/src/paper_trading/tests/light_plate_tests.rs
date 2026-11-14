//! Light's chart plates, read off what the paint helpers actually emit: the
//! aim and the stop and target it carries wear the paper plate (Dark keeps
//! its fills), a pending TP and SL still tell apart, and a plated close
//! glyph still answers a hover.
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

/// What the Alt aim paints in `ctx`'s theme, carrying a 1:1 stop and target
/// when `bracket`.
fn aim_shapes(ctx: &egui::Context, bracket: bool) -> Vec<egui::Shape> {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    if bracket {
        paper.set_ruler_ticks(3);
    }
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let at = egui::pos2(400.0, 300.0);
    paper.handle_chart_input(&cmd_frame(chart, &scale, at, shift, false));
    shapes_of(ctx, at, |paint| paper.draw_cmd_preview(paint, true))
}

/// The colours of every line segment painted, dashes included.
fn segment_colors(shapes: &[egui::Shape]) -> Vec<egui::Color32> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::LineSegment { stroke, .. } => match stroke.color {
                egui::epaint::ColorMode::Solid(color) => Some(color),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Every filled rectangle, as (fill, edge colour, edge width).
fn rects(shapes: &[egui::Shape]) -> Vec<(egui::Color32, egui::Color32, f32)> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(rect) => Some((rect.fill, rect.stroke.color, rect.stroke.width)),
            _ => None,
        })
        .collect()
}

/// The Alt aim on a light chart: its line keeps the order plate, its label
/// is the paper plate - white, edged dashed in the side's tag colour because
/// nothing is sent yet, dark words - and its price chip is filled in the
/// side's tag colour with white digits. No black plate is left.
#[test]
fn the_cmd_preview_lays_the_dashed_paper_plate_in_light() {
    let ctx = light_ctx();
    let palette = crate::theme::get(&ctx);
    let shapes = aim_shapes(&ctx, false);
    let segments = segment_colors(&shapes);
    assert!(
        segments.iter().all(|color| *color != palette.buy()),
        "no raw side colour on the plated chart: {segments:?}"
    );
    assert!(segments.contains(&palette.order_plate()), "the line");
    let edge_dashes = segments
        .iter()
        .filter(|color| **color == *palette.get(quantick_appearance::Token::PaperTagGain))
        .count();
    assert!(
        edge_dashes >= 8,
        "a dashed edge round the label: {segments:?}"
    );
    let rects = rects(&shapes);
    assert!(
        rects
            .iter()
            .all(|(fill, ..)| *fill != palette.order_plate()),
        "no black plate: {rects:?}"
    );
    assert!(
        rects.iter().any(|(fill, ..)| *fill == palette.inset()),
        "the white label"
    );
    let tag = *palette.get(quantick_appearance::Token::PaperTagGain);
    assert!(
        rects.iter().any(|(fill, ..)| *fill == tag),
        "the filled price chip"
    );
    assert_eq!(glyph_ink(&shapes, "BUY "), palette.text_primary());
    let digits: Vec<_> = shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Text(text)
                if text
                    .galley
                    .text()
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.') =>
            {
                Some(text.galley.job.sections[0].format.color)
            }
            _ => None,
        })
        .collect();
    assert_eq!(digits, vec![palette.chip_ink()], "white digits on the chip");
}

/// Dark's aim is untouched: a filled side-colour label in the chip ink, a
/// side-colour chip, no paper plate and no dashed edge.
#[test]
fn the_cmd_preview_keeps_its_filled_label_in_dark() {
    let ctx = egui::Context::default();
    let palette = crate::theme::get(&ctx);
    let shapes = aim_shapes(&ctx, false);
    let rects = rects(&shapes);
    let filled = rects
        .iter()
        .filter(|(fill, ..)| *fill == palette.buy())
        .count();
    assert!(filled >= 2, "the label and the gutter chip: {rects:?}");
    assert!(
        rects.iter().all(|(_, _, width)| *width == 0.0),
        "no edge: {rects:?}"
    );
    assert_eq!(glyph_ink(&shapes, "BUY "), palette.chip_ink());
    let segments = segment_colors(&shapes);
    assert!(
        segments.iter().all(|color| *color == palette.buy()),
        "only the line is dashed: {segments:?}"
    );
}

/// The stop and target the aim carries take the pinned leg's plate on a
/// light chart - white, a solid edge in the side's tag colour, dark
/// words - with chips filled in the side's tag colour. Dark fills them in
/// the side colours, as before.
#[test]
fn the_aim_s_stop_and_target_wear_the_pinned_leg_plate_in_light() {
    let light = light_ctx();
    let palette = crate::theme::get(&light);
    let shapes = aim_shapes(&light, true);
    let rects = rects(&shapes);
    // One colour a side: the plate's edge is its filled chip's colour.
    for token in [
        quantick_appearance::Token::PaperTagGain,
        quantick_appearance::Token::PaperTagLoss,
    ] {
        let edge = *palette.get(token);
        // `chip_tag` fills the plate, then strokes its edge solid on top.
        assert!(
            rects
                .iter()
                .any(|(_, stroke, width)| *stroke == edge && *width == 1.0),
            "an edge solid in {edge:?}: {rects:?}"
        );
        assert!(
            rects.iter().any(|(fill, ..)| *fill == edge),
            "{edge:?}: {rects:?}"
        );
    }
    let white = rects.iter().filter(|(fill, ..)| *fill == palette.inset());
    assert!(white.count() >= 2, "white TP and SL plates: {rects:?}");
    assert!(
        rects
            .iter()
            .all(|(fill, ..)| *fill != palette.order_plate())
    );
    assert_eq!(glyph_ink(&shapes, "TP"), palette.text_primary());
    assert_eq!(glyph_ink(&shapes, "SL"), palette.text_primary());

    let dark = egui::Context::default();
    let palette = crate::theme::get(&dark);
    let dark_rects = self::rects(&aim_shapes(&dark, true));
    for side in [palette.buy(), palette.sell()] {
        let filled = dark_rects.iter().filter(|(fill, ..)| *fill == side).count();
        assert!(
            filled >= 2,
            "tag and chip filled in {side:?}: {dark_rects:?}"
        );
    }
    assert!(dark_rects.iter().all(|(fill, ..)| *fill != palette.inset()));
}

/// A pending TP and SL are outlined in their sides' tag colours, so the two
/// ghosts no longer look identical.
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
    let tag = |token| *palette.get(token);
    assert_eq!(take_profit, tag(quantick_appearance::Token::PaperTagGain));
    assert_eq!(stop, tag(quantick_appearance::Token::PaperTagLoss));
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
