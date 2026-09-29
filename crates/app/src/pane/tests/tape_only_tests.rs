//! A pane in tape-only mode fits its price axis to the tape, never to the
//! candles it no longer draws.

use eframe::egui;
use quantick_engine::Bar;
use rust_decimal::Decimal;

use crate::orderflow_view::LiveLane;
use crate::pane::ChartPane;
use crate::pane::frame_layout::{FrameLayout, Series};
use crate::price_view::PriceView;
use crate::state::BarSpec;

fn bar(low: i64, high: i64, close: i64) -> Bar {
    Bar {
        open_time: 0,
        close_time: 0,
        open: Decimal::from(close),
        high: Decimal::from(high),
        low: Decimal::from(low),
        close: Decimal::from(close),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 1,
    }
}

/// The candles span 400..1600; the tape traded 995..1015 and last printed
/// at 1005. Tape only, the axis is the tape's range plus the fit's margin;
/// otherwise the candles still decide it.
#[test]
fn a_tape_only_pane_fits_its_price_axis_to_the_tape_alone() {
    let pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1_000.0, 600.0));
    let areas = pane.plot_areas(rect, crate::config::FeedCapabilities::none());
    let chart = areas.chart;
    let bars = [bar(400, 1_600, 1_000), bar(990, 1_010, 1_005)];
    let layout = |tape_only: bool| FrameLayout {
        areas: pane.plot_areas(rect, crate::config::FeedCapabilities::none()),
        chart_rect: chart,
        history_rect: if tape_only {
            egui::Rect::from_min_max(chart.min, egui::pos2(chart.left(), chart.bottom()))
        } else {
            egui::Rect::from_min_max(chart.min, egui::pos2(chart.right() - 350.0, chart.bottom()))
        },
        live_lane: Some(LiveLane {
            width_px: if tape_only { chart.width() } else { 350.0 },
            end_ms: 0,
        }),
        total: bars.len(),
        closed_total: bars.len(),
        start: 0,
        end: bars.len(),
        cw: 8.0,
        indicator_guide_x: None,
        tape_only,
        native_tape: tape_only,
        tape_padding_px: 0.0,
    };
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(ctx, egui::LayerId::background(), rect);
    let fitted = |tape_only: bool, price_view: &PriceView, tape_range| {
        let layout = layout(tape_only);
        layout
            .resolve(
                &painter,
                Series {
                    prefix: &[],
                    closed: &bars,
                    partial: None,
                },
                tape_range,
                None,
                price_view,
                egui::Color32::BLACK,
            )
            .expect("a scale")
            .0
            .scale
    };
    let tape_range = Some((995.0, 1_015.0));
    let (lo, hi) = fitted(true, &PriceView::new(), tape_range).range();
    assert!((lo - 994.0).abs() < 1e-9, "{lo}");
    assert!((hi - 1_016.0).abs() < 1e-9, "{hi}");
    let (lo, hi) = fitted(false, &PriceView::new(), tape_range).range();
    assert!(
        lo < 400.0 && hi > 1_600.0,
        "the candles still decide: {lo} {hi}"
    );
    assert_eq!(
        fitted(false, &PriceView::new(), Some((-10_000.0, 20_000.0))).range(),
        (lo, hi),
        "the ordinary candle axis ignores tape prices outside the candles"
    );

    let mut candle_price_view = PriceView::new();
    candle_price_view.pan(10_000.0, (400.0, 1_600.0));
    candle_price_view.set_inverted(true);
    let tape_scale = fitted(true, &candle_price_view, tape_range);
    assert_eq!(
        tape_scale.range(),
        (10_400.0, 11_600.0),
        "an explicit manual tape range is honored after mode entry"
    );
    assert!(tape_scale.y(995.0) < tape_scale.y(1_015.0));
    assert_eq!(
        fitted(false, &candle_price_view, tape_range).range(),
        (10_400.0, 11_600.0),
        "ordinary candle panes retain their manual price view"
    );
    assert_eq!(
        fitted(true, &candle_price_view, Some((900.0, 1_200.0))).range(),
        tape_scale.range(),
        "new prints cannot replace an explicit manual tape range"
    );
    candle_price_view.reset();
    assert_eq!(
        fitted(true, &candle_price_view, tape_range).range(),
        (994.0, 1_016.0)
    );
}

#[test]
fn tape_only_reserves_no_forming_candle_volume_profile_strip() {
    use quantick_layers::{ChartLayer, LayerActions};

    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let capabilities = crate::config::FeedCapabilities {
        traded_volume: true,
        ..crate::config::FeedCapabilities::none()
    };
    let mut effects = LayerActions::default();
    pane.set_layer_visible(ChartLayer::LiveStrip, true, &mut effects);
    let ordinary_width = pane.live_strip_width(capabilities);
    assert!(
        ordinary_width > 0.0,
        "ordinary charts retain their live profile"
    );
    pane.set_layer_visible(ChartLayer::TapeOnly, true, &mut effects);
    assert_eq!(pane.live_strip_width(capabilities), 0.0);
    let rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1_000.0, 600.0));
    assert!(pane.plot_areas(rect, capabilities).live_strip.is_none());
    assert!(
        pane.layers.requested(ChartLayer::LiveStrip),
        "the user's ordinary setting is retained"
    );
    pane.set_layer_visible(ChartLayer::TapeOnly, false, &mut effects);
    assert_eq!(pane.live_strip_width(capabilities), ordinary_width);
}

#[test]
fn the_tape_switch_and_key_share_the_header_without_covering_the_live_edge() {
    use crate::orderflow_render::{
        OrderflowRenderStyle, ProjectedLayout, RenderContext, draw_compact_legend,
    };
    use quantick_orderflow::{DisplayGrouping, EffectiveGrouping, HeatmapProjection};

    let pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let projection = HeatmapProjection::empty(
        true,
        EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, Decimal::from(100)),
    );
    let area = egui::Rect::from_min_size(egui::pos2(30.0, 50.0), egui::vec2(281.0, 300.0));
    let plot = crate::plot_area::plot_split(area, 0.0, &[]).chart;
    assert_eq!(plot.width(), 185.0);
    let header = egui::Rect::from_min_max(egui::pos2(plot.left(), area.top()), plot.right_top());
    let mut config = quantick_orderflow::HeatmapConfig::default();
    config.live_lane.tape_only = true;
    config.live_lane.show_aggressions = true;
    config.live_lane.show_depth = false;
    let style = OrderflowRenderStyle::from_config(&config, egui::Color32::BLACK.to_array());
    let layout = ProjectedLayout::new(header, &pane.viewport, 2, 0, 2, header.width());
    for tape_only in [false, true] {
        let ctx = egui::Context::default();
        let mut key = None;
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            if tape_only {
                key =
                    draw_compact_legend(&painter, &RenderContext::new(&projection, layout, &style));
            }
            pane.layer_renderers
                .canvas(&mut crate::pane::render_registry::CanvasPass {
                    painter: &painter,
                    rect: plot,
                    tape_on: Some(true),
                    tape_hovered: true,
                    state: &pane.layers,
                    facts: quantick_layers::LayerFacts {
                        flow_pane: true,
                        tape_on: true,
                        tape_only,
                        ..Default::default()
                    },
                });
        });
        let chip = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(rect) if rect.fill != egui::Color32::TRANSPARENT => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .expect("the tape control remains visible");
        if tape_only {
            assert!(
                header.contains_rect(chip),
                "the chip is outside the tape: {chip:?}"
            );
            assert!(
                key.expect("the buy/sell key remains visible").right() < chip.left(),
                "both controls fit at the narrow split without hiding one another"
            );
            for shape in &output.shapes {
                let ink = shape
                    .shape
                    .visual_bounding_rect()
                    .intersect(shape.clip_rect);
                if ink.is_positive() {
                    assert!(
                        ink.bottom() <= plot.top(),
                        "no header ink can hide the high forming dot"
                    );
                }
            }
        } else {
            assert_eq!(
                chip,
                crate::pane::tape_switch_rect(plot, false),
                "ordinary/BTC placement is unchanged"
            );
        }
    }
}
