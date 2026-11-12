//! The pixel contract of the drawing tools, frozen before they left `app`.
//!
//! For every registered tool, painted plain and selected, plus a profile and
//! an anchored VWAP with real cached rows, this records the exact egui shape
//! list a chart band receives: the tool's [`MarkScene`] through
//! [`super::paint_scene`], the window's one drawings renderer. The list lives in
//! `drawings_scene_golden.txt` beside this file and was written from the
//! tools as they painted *before* the move to `quantick_drawings`; the test
//! is what proves the move changed no pixel. Never edit the golden to make it
//! pass: a difference is a painting change, and it is the code that answers
//! for it.
//!
//! Setting `QUANTICK_BLESS_DRAWINGS_GOLDEN` rewrites the file; it exists for
//! the one recording, not for repairs.
//!
//! SHA-256 of the recorded file:
//! `8e69e050e4d42b31a4649f4972b25303cf00e4caf2803fdd28437729224e5108`.

use std::fmt::Write as _;

use eframe::egui;
use quantick_anchored_studies::{
    AnchoredAverage, AverageInputs, AverageRequest, ProfileInputs, ProfileRequest, RangeProfile,
};
use quantick_engine::{BarSpec, Side, Trade, bar_registry::BarConfiguration};
use rust_decimal::Decimal;

use super::{TextFonts, paint_scene};
use crate::chart::PriceScale;
use crate::drawings::{
    ChartPoint, DRAWING_TOOLS, DrawContext, DrawingPayload, DrawingStyle, DrawingTool, FrvpPayload,
    MarkScene, ValueUnit, px, px_rect,
};
use quantick_drawings::{AvwapPayload, Px};

const GOLDEN_FILE: &str = "src/pane/render_registry/drawings_scene_golden.txt";

const CHART: egui::Rect = egui::Rect {
    min: egui::pos2(0.0, 0.0),
    max: egui::pos2(500.0, 300.0),
};

/// Anchors a fixture takes from, in order; a tool takes as many as it needs.
const ANCHORS: [egui::Pos2; 3] = [
    egui::pos2(140.0, 90.0),
    egui::pos2(340.0, 210.0),
    egui::pos2(260.0, 250.0),
];

/// The path a freehand tool is given: a stroke, not a declared count.
const PATH: [egui::Pos2; 5] = [
    egui::pos2(100.0, 100.0),
    egui::pos2(130.0, 120.0),
    egui::pos2(170.0, 110.0),
    egui::pos2(200.0, 150.0),
    egui::pos2(240.0, 140.0),
];

struct Fixture {
    label: String,
    tool: DrawingTool,
    points: Vec<egui::Pos2>,
    payload: Box<dyn DrawingPayload>,
    scale: PriceScale,
}

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("registered fixture tool")
}

fn trade(id: u64, ms: i64, price: i64, quantity: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: ms,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side: Side::Buy,
    }
}

/// A profile at a real B3 price, grouped by 10, four rows deep.
fn profile_payload() -> FrvpPayload {
    let group = Decimal::from(10);
    let mut builder = BarConfiguration::from(BarSpec::Tick(1)).build();
    let mut footprint = quantick_engine::FootprintBuilder::new(group, 4096);
    let mut closed = Vec::new();
    for (index, &(price, quantity)) in [(95_010, 10), (95_020, 8), (95_000, 7), (94_990, 2)]
        .iter()
        .enumerate()
    {
        let trade = trade(index as u64 + 1, 0, price, quantity);
        builder.push_into(&trade, &mut closed);
        footprint.push(&trade);
    }
    let bars = vec![closed.pop().expect("the fixture closes a bar"); 20];
    let ladder = footprint.close().expect("the fixture traded");
    let mut cache = None;
    RangeProfile::refresh(
        &mut cache,
        ProfileRequest {
            min_bar: 0.0,
            max_bar: f32::MAX,
            extend_right: true,
            approximate_history: false,
            value_area_pct: 70,
        },
        &ProfileInputs {
            closed: &bars,
            ladders: &[ladder],
            prefix: &[],
            partial_ladder: None,
            group,
            series_revision: 0,
            partial_version: 0,
            blocked: false,
            side_inferred: false,
            partial_bucket_slot: None,
            budget: 1500,
        },
    );
    FrvpPayload {
        cache,
        ..FrvpPayload::default()
    }
}

/// An anchored VWAP over a twelve-print tape cut as tick(3) bars, anchored
/// on bar 1.
fn vwap_payload() -> AvwapPayload {
    let mut builder = BarConfiguration::from(BarSpec::Tick(3)).build();
    let mut bars = Vec::new();
    let prints = [
        (100, 1),
        (100, 1),
        (100, 1),
        (112, 2),
        (80, 2),
        (96, 4),
        (144, 2),
        (112, 2),
        (128, 4),
        (96, 4),
        (64, 4),
        (80, 8),
    ];
    for (index, &(price, quantity)) in prints.iter().enumerate() {
        let trade = trade(index as u64 + 1, 1_000 * index as i64, price, quantity);
        builder.push_into(&trade, &mut bars);
    }
    let mut payload = AvwapPayload::default();
    AnchoredAverage::refresh(
        &mut payload.cache,
        AverageRequest {
            anchor_bar: 1.0,
            source: payload.source,
            bands: payload.bands,
        },
        &AverageInputs {
            closed: &bars,
            partial: None,
            prefix: &[],
            timeline_revision: 0,
        },
    );
    payload
}

fn fixtures() -> Vec<Fixture> {
    let plain = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
    let mut fixtures: Vec<Fixture> = DRAWING_TOOLS
        .into_iter()
        .map(|drawing_tool| {
            let points = if drawing_tool.freehand() {
                PATH.to_vec()
            } else {
                ANCHORS[..drawing_tool.required_points()].to_vec()
            };
            let mut payload = drawing_tool.default_payload();
            // The golden pins how a projecting extension paints; a fresh one
            // now opens finite, so the fixture asks for the projection.
            if let Some(fib) = payload
                .as_any_mut()
                .downcast_mut::<quantick_drawings::FibPayload>()
                && drawing_tool.id() == "fib-extension"
            {
                fib.extend = quantick_drawings::FibExtend::Forward;
            }
            Fixture {
                label: drawing_tool.id().to_owned(),
                tool: drawing_tool,
                points,
                payload,
                scale: plain,
            }
        })
        .collect();
    fixtures.push(Fixture {
        label: "fixed-range-profile with rows".to_owned(),
        tool: tool("fixed-range-profile"),
        points: vec![egui::pos2(100.0, 150.0), egui::pos2(400.0, 150.0)],
        payload: Box::new(profile_payload()),
        scale: PriceScale::from_range(94_950.0, 95_070.0, 0.0, 300.0),
    });
    fixtures.push(Fixture {
        label: "anchored-vwap with rows".to_owned(),
        tool: tool("anchored-vwap"),
        points: vec![egui::pos2(120.0, 150.0)],
        payload: Box::new(vwap_payload()),
        scale: PriceScale::from_range(40.0, 160.0, 0.0, 300.0),
    });
    fixtures
}

/// Paint one fixture the way a chart band does: the under-candles body, then
/// the object itself, through a painter clipped to the band.
fn paint(fixture: &Fixture, selected: bool) -> Vec<egui::epaint::ClippedShape> {
    let anchors: Vec<ChartPoint> = fixture
        .points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            ChartPoint::at(index as f32 * 2.0 + 1.0, fixture.scale.price_at(point.y))
        })
        .collect();
    let style = DrawingStyle::default();
    let ctxt = DrawContext {
        appearance: quantick_appearance::ResolvedTheme::bundled_dark(),
        payload: fixture.payload.as_ref(),
        anchors: &anchors,
        scale: &fixture.scale,
        px_per_bar: 20.0,
        unit: ValueUnit::Price,
        primary_band: true,
        style,
        selected,
        halo: false,
        content_editing: false,
        text: &quantick_drawings::UNMEASURED,
    };
    let points: Vec<Px> = fixture.points.iter().copied().map(px).collect();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(CHART),
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drawings-scene-golden"),
            ))
            .with_clip_rect(CHART);
        let fonts = TextFonts(ctx);
        let ctxt = DrawContext {
            appearance: quantick_appearance::ResolvedTheme::bundled_dark(),
            text: &fonts,
            ..ctxt
        };
        let mut scene = MarkScene::default();
        fixture
            .tool
            .paint_under(&mut scene, px_rect(CHART), style, &points, &ctxt);
        fixture
            .tool
            .paint(&mut scene, px_rect(CHART), style, &points, &ctxt, selected);
        paint_scene(&painter, &scene);
    })
    .shapes
}

fn describe(out: &mut String, depth: usize, shape: &egui::Shape) {
    let indent = "  ".repeat(depth);
    match shape {
        egui::Shape::Vec(shapes) => {
            let _ = writeln!(out, "{indent}Vec");
            for inner in shapes {
                describe(out, depth + 1, inner);
            }
        }
        egui::Shape::Text(text) => {
            let _ = writeln!(
                out,
                "{indent}Text pos={:?} rect={:?} fallback={:?} override={:?} opacity={:?} \
                 angle={:?} underline={:?} job={:?}",
                text.pos,
                text.galley.rect,
                text.fallback_color,
                text.override_text_color,
                text.opacity_factor,
                text.angle,
                text.underline,
                text.galley.job,
            );
        }
        other => {
            let _ = writeln!(out, "{indent}{other:?}");
        }
    }
}

fn record() -> String {
    let mut out = String::new();
    for fixture in fixtures() {
        for selected in [false, true] {
            let _ = writeln!(
                out,
                "## {} {}",
                fixture.label,
                if selected { "selected" } else { "plain" }
            );
            for clipped in paint(&fixture, selected) {
                let _ = writeln!(out, "clip={:?}", clipped.clip_rect);
                describe(&mut out, 1, &clipped.shape);
            }
        }
    }
    out
}

/// C6: every tool fixture, through the renderer, paints exactly the shapes
/// it painted before the move.
#[test]
fn drawings_scene_golden() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_FILE);
    let recorded = record();
    if std::env::var_os("QUANTICK_BLESS_DRAWINGS_GOLDEN").is_some() {
        std::fs::write(&path, &recorded).expect("write the drawings golden");
    }
    let golden = std::fs::read_to_string(&path)
        .expect("the drawings golden is recorded")
        .replace("\r\n", "\n");
    if golden != recorded {
        let first = golden
            .lines()
            .zip(recorded.lines())
            .position(|(left, right)| left != right)
            .unwrap_or_else(|| golden.lines().count().min(recorded.lines().count()));
        panic!(
            "drawing shapes differ from the golden at line {}:\n golden: {:?}\n painted: {:?}",
            first + 1,
            golden.lines().nth(first),
            recorded.lines().nth(first),
        );
    }
}

/// Where the theme backs labels, text keeps its authored ink and a local
/// backing supplies the missing contrast; where it paints none (Light's
/// paper), the text is bare and its hue is shaded until it reads.
#[cfg(test)]
#[test]
fn drawing_text_and_forms_remain_readable() {
    use quantick_appearance::{Appearance, PremultipliedRgba, contrast};
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for bookmap in [false, true] {
            let ctx = egui::Context::default();
            let mut appearance = Appearance::default();
            appearance.apply(source).unwrap();
            crate::theme::sync(&ctx, &appearance);
            let palette = if bookmap {
                crate::theme::bookmap(&ctx)
            } else {
                crate::theme::get(&ctx)
            };
            let _scope = crate::theme::scope(&ctx, palette.clone());
            for color in [
                palette.color(quantick_appearance::Token::DrawingDefault),
                egui::Color32::from_rgb(181, 131, 43),
                egui::Color32::from_rgb(20, 30, 45),
            ] {
                let mut scene = MarkScene::default();
                scene.shapes.push(quantick_drawings::MarkShape::Text {
                    at: Px::new(100.0, 100.0),
                    anchor: quantick_drawings::Align2([
                        quantick_drawings::Align::Min,
                        quantick_drawings::Align::Min,
                    ]),
                    text: "Light audit annotation".into(),
                    font: quantick_drawings::FontSpec {
                        family: quantick_drawings::FontFamily::Proportional,
                        size_px: 12.0,
                    },
                    color: PremultipliedRgba::from_premultiplied(color.to_array()),
                });
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    paint_scene(
                        &ctx.layer_painter(egui::LayerId::new(
                            egui::Order::Foreground,
                            egui::Id::new("annotation"),
                        )),
                        &scene,
                    );
                });
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            Some(text)
                        } else {
                            None
                        }
                    })
                    .expect("annotation text");
                let ink = text.galley.job.sections[0].format.color;
                let plates = palette.resolved.drawing_label_plates;
                if plates {
                    assert_eq!(ink, color, "authored ink is preserved");
                }
                let backing = output.shapes.iter().find_map(|shape| {
                    if let egui::Shape::Rect(rect) = &shape.shape {
                        Some(rect)
                    } else {
                        None
                    }
                });
                if palette.resolved.dark {
                    assert!(backing.is_none(), "Dark and Bookmap paint are preserved");
                } else if !plates {
                    assert!(backing.is_none(), "bare labels have no backing");
                    let rgba =
                        |c: egui::Color32| PremultipliedRgba::from_premultiplied(c.to_array());
                    assert!(
                        contrast::contrast_ratio(rgba(ink), rgba(palette.canvas())) >= 4.5,
                        "bare text reads on the canvas"
                    );
                } else {
                    let background = backing.map_or(palette.canvas(), |rect| {
                        assert!(
                            rect.rect
                                .contains_rect(text.galley.rect.translate(text.pos.to_vec2()))
                        );
                        rect.fill
                    });
                    let rgba =
                        |c: egui::Color32| PremultipliedRgba::from_premultiplied(c.to_array());
                    assert!(
                        contrast::contrast_ratio(rgba(color), rgba(background)) >= 4.5,
                        "text reads on its actual backing"
                    );
                }
            }
        }
    }
}
