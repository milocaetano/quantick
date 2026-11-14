use super::*;
use quantick_chart::flow_execution::FlowQuantityLabel;

#[test]
fn configured_quantity_font_drives_measurement_and_painting_from_one_snapshot() {
    let ctx = egui::Context::default();
    let mut state = quantick_appearance::Appearance::default();
    state.apply("version = 1\nname = 'Quantities'\n[typography.chart_label]\nsize = 22\nfamily = 'monospace'\n[colors]\ntext_primary = '#123456'").unwrap();
    crate::theme::sync(&ctx, &state);
    let appearance = crate::theme::get(&ctx);
    let label = FlowQuantityLabel {
        center: [100.0, 90.0],
        radius: 60.0,
        text: "123.4".into(),
    };
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let galley = quantity_galley(&painter, &label, &appearance);
        let font = &galley.job.sections[0].format.font_id;
        assert_eq!(font.family, egui::FontFamily::Monospace);
        assert_eq!(font.size, 2.0 * super::super::constants::LABEL_MAX_FONT_PX);
        assert_eq!(
            galley.job.sections[0].format.color,
            appearance.text_primary()
        );
        let placed = flow::place_quantity_labels(
            vec![label.clone()],
            [[0.0, 0.0], [240.0, 180.0]],
            [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE],
            |label| quantity_galley(&painter, label, &appearance).size().into(),
        );
        assert_eq!(placed.len(), 1);
        let pos = egui::Pos2::from(placed[0].0);
        assert_eq!(pos + galley.size() / 2.0, egui::Pos2::from(label.center));
        paint_quantity_labels(&painter, painter.clip_rect(), &placed, &appearance);
    });
}

/// The retest's case: the top mass of 86,307 contracts on a 15.1 px peak
/// sphere carries its compact quantity, measured with the real font.
#[test]
fn a_fifteen_pixel_peak_sphere_carries_its_compact_quantity() {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let appearance = crate::theme::get(ctx);
        let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
        let measure =
            |label: &FlowQuantityLabel| quantity_galley(&painter, label, &appearance).size().into();
        let quantity = rust_decimal::Decimal::from(86_307);
        let text = quantick_orderflow::config::labels::format_quantity_compact(quantity);
        let label = FlowQuantityLabel {
            center: [200.0, 150.0],
            radius: 15.1,
            text,
        };
        let area = [[0.0, 0.0], [400.0, 300.0]];
        let placed = flow::place_quantity_labels_until_miss(vec![label], area, fit, measure, true);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].1.text, "86K");
    });
}

/// Eligible against labelled peak masses on a recorded WIN session, with
/// the real font, at several candle widths: the last 1,200 px of
/// tick:2000 candles, 660 px tall, the mini index look. Reads the
/// `quantick-replay 1` CSV named by `PEAK_SPHERES_REPLAY_CSV`; run by name
/// with `--ignored --nocapture`.
#[test]
#[ignore = "reads a local replay recording; run by name"]
fn a_recorded_session_labels_its_largest_peak_masses() {
    use quantick_engine::{Side, Trade};
    use quantick_orderflow::projection::{PriceWindow, flow_tape as tape};
    use rust_decimal::{Decimal, prelude::ToPrimitive as _};
    const TICKS: usize = 2_000;
    const HEIGHT: f32 = 660.0;
    let path = std::env::var("PEAK_SPHERES_REPLAY_CSV").expect("PEAK_SPHERES_REPLAY_CSV");
    let text = std::fs::read_to_string(path).expect("a readable recording");
    let all: Vec<Trade> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("Date"))
        .enumerate()
        .map(|(n, line)| {
            let fields: Vec<&str> = line.split(',').collect();
            Trade {
                agg_id: n as u64,
                timestamp_ms: n as i64,
                price: fields[2].parse().unwrap(),
                quantity: fields[5].parse().unwrap(),
                side: if fields[6] == "B" {
                    Side::Buy
                } else {
                    Side::Sell
                },
            }
        })
        .collect();
    let style = quantick_orderflow::TickBubbleStyle {
        reference_quantity: 1.0,
        reference_mode: quantick_orderflow::TickReferenceMode::VisibleMax,
        viewport_scale: None,
        zoom_regions: true,
        peak_spheres: Some(true),
        max_radius: 18.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    };
    let calibration = quantick_orderflow::ViewportBubbleScale {
        x_units_per_pixel: 1.0 / 22.0,
        price_units_per_pixel: 1.0,
    };
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let appearance = crate::theme::get(ctx);
            for px in [20.0_f32, 12.0, 8.0, 5.0, 3.0] {
                let candles = (1_200.0 / px) as usize;
                let whole = all.len() / TICKS;
                let trades = &all[(whole - candles) * TICKS..whole * TICKS];
                let low = trades.iter().map(|trade| trade.price).min().unwrap() - Decimal::from(40);
                let high = trades.iter().map(|trade| trade.price).max().unwrap() + Decimal::from(40);
                let px_per_price = f64::from(HEIGHT) / (high - low).to_f64().unwrap();
                let gain = calibration.radius_gain(f64::from(px), px_per_price);
                let rung = tape::FlowRegionSpan::held(None, f64::from(px));
                let frame = tape::project_flow_tape(
                    trades.iter().enumerate().map(|(ordinal, trade)| tape::FlowExecution {
                        ordinal,
                        slot: ordinal / TICKS,
                        accepted_ordinal: ordinal % TICKS,
                        ticks_per_bar: TICKS.into(),
                        trade,
                        opening: false,
                    }),
                    1,
                    trades.len(),
                    trades.len(),
                    tape::FlowTapeView {
                        first_slot: 0,
                        end_slot: candles,
                        clip_left: Decimal::ZERO,
                        clip_right: Decimal::from(candles),
                        width_px: candles as f32 * px,
                        height_px: HEIGHT,
                        prices: PriceWindow::new(low, high).unwrap(),
                        reference: tape::FlowReference::VisibleRegionsAtLeast(Decimal::ONE),
                        radius_limit: 18.0 * gain,
                        merge_support_radius: 6.0,
                        region: tape::FlowRegionSpan::Peaks(rung),
                        exclude_opening: false,
                    },
                );
                let history = [[0.0, 0.0], [candles as f32 * px, HEIGHT]];
                let plan = flow::FlowPresentation::styled(&frame, history, Some(style), |dot| {
                    let x = dot.candle_position.to_f32()? * px;
                    let y = HEIGHT - ((dot.mark.price - low).to_f64()? * px_per_price) as f32;
                    Some([x, y])
                });
                let labels = plan.quantity_labels(&frame, Some(style));
                let eligible = labels.len();
                let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
                let measure = |label: &FlowQuantityLabel| {
                    quantity_galley(&painter, label, &appearance).size().into()
                };
                let placed = flow::place_quantity_labels_until_miss(labels, history, fit, measure, true);
                let texts: Vec<_> = placed.iter().map(|(_, label)| label.text.clone()).collect();
                println!(
                    "{px} px/bar, regions of {rung}: {} masses, {eligible} eligible, {} labelled {texts:?}",
                    frame.dots.len(),
                    placed.len()
                );
            }
        });
}

#[test]
fn tick_quantities_use_fitted_nonoverlapping_galleys() {
    let ctx = egui::Context::default();
    let history = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 180.0));
    let labels = [
        FlowQuantityLabel {
            center: [100.0, 90.0],
            radius: 30.0,
            text: "28.1".into(),
        },
        FlowQuantityLabel {
            center: [105.0, 90.0],
            radius: 20.0,
            text: "14.7".into(),
        },
        FlowQuantityLabel {
            center: [180.0, 90.0],
            radius: 15.0,
            text: "7.2".into(),
        },
        FlowQuantityLabel {
            center: [0.0, 90.0],
            radius: 15.0,
            text: "7.2".into(),
        },
    ];
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let appearance = crate::theme::get(ctx);
        let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
        let measure =
            |label: &FlowQuantityLabel| quantity_galley(&painter, label, &appearance).size().into();
        let area = [history.min.into(), history.max.into()];
        let placed = flow::place_quantity_labels(labels.to_vec(), area, fit, measure);
        paint_quantity_labels(&painter, history, &placed, &appearance);
    });
    let bounds: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.override_text_color.is_none() => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        bounds.len(),
        2,
        "the overlapping and clipped labels are omitted"
    );
    assert!(!bounds[0].intersects(bounds[1]));
    assert!(bounds.iter().all(|bounds| history.contains_rect(*bounds)));
}
