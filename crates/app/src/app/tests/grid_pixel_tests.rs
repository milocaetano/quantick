//! Actual app paint preserves price coordinates while aligning inherited Light grids.
use super::*;
use quantick_appearance::Inheritance;

const SIDES: [PaneSide; 2] = [PaneSide::Time(0), PaneSide::Flow];

#[derive(Debug, PartialEq)]
struct GridPaint {
    horizontal: Vec<([egui::Pos2; 2], f32, egui::Color32)>,
    divider: Vec<[egui::Pos2; 2]>,
    time_rule: Vec<[egui::Pos2; 2]>,
    labels: Vec<(String, egui::Pos2, egui::Color32)>,
    raw_y: Vec<f32>,
}

fn fixture(ctx: &egui::Context, source: &str) -> QuantickApp {
    let (mut app, events, _commands, _book) = test_app();
    app.runtime
        .appearance
        .apply_candidate(source, false)
        .unwrap();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    for side in SIDES {
        app.active_tab_mut()
            .set_pane_bar_spec(side.index(), crate::state::BarSpec::Tick(1))
            .unwrap();
        let pane = app.active_tab_mut().pane_mut(side);
        assert!(pane.price_view.set_manual_range(92.345, 113.28));
        for layer in [
            ChartLayer::Footprint,
            ChartLayer::LiveStrip,
            ChartLayer::TapeChart,
        ] {
            pane.set_layer_visible(layer, false, &mut Default::default());
        }
    }
    events
        .try_send(FeedEvent::Backfilled((1..=200).map(trade).collect()))
        .unwrap();
    let tab_id = app.runtime.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    settled_frame(&mut app, ctx);
    app
}

fn capture(
    app: &QuantickApp,
    ctx: &egui::Context,
    output: &egui::FullOutput,
    side: PaneSide,
) -> GridPaint {
    let pane = app.active_tab().pane(side);
    let rect = pane.frame.chart_rect.expect("the populated pane painted");
    let gutter = pane.frame.price_gutter.unwrap();
    let style = pane.presentation.candles.resolve_surface(
        &app.runtime.style,
        app.runtime.appearance.resolved(),
        pane.orderflow.is_some(),
    );
    let expected_color = crate::pane::grid_color(&style);
    let tick_ink = crate::theme::axes(ctx).text_muted();
    let mut tick_labels = 0;
    let scale = pane
        .price_view
        .scale(pane.frame.auto_range.unwrap(), rect.top(), rect.bottom());
    let (lo, hi) = scale.range();
    let mut paint = GridPaint {
        horizontal: Vec::new(),
        divider: Vec::new(),
        time_rule: Vec::new(),
        labels: Vec::new(),
        raw_y: crate::chart::nice_ticks(lo, hi, 8)
            .into_iter()
            .map(|tick| scale.y(tick))
            .collect(),
    };
    for shape in &output.shapes {
        match &shape.shape {
            egui::Shape::LineSegment { points, stroke }
                if points[0].x == rect.left()
                    && points[1].x == rect.right()
                    && points[0].y == points[1].y
                    && points[0].y > rect.top()
                    && points[0].y <= rect.bottom() =>
            {
                let egui::epaint::ColorMode::Solid(color) = stroke.color else {
                    panic!("the grid uses a solid color");
                };
                assert_eq!(color, expected_color);
                assert_eq!(stroke.width, 1.0);
                if points[0].y == rect.bottom() {
                    paint.time_rule.push(*points);
                } else {
                    paint.horizontal.push((*points, stroke.width, color));
                }
            }
            egui::Shape::LineSegment { points, stroke }
                if points[0] == egui::pos2(gutter.left(), rect.top())
                    && points[1] == egui::pos2(gutter.left(), rect.bottom()) =>
            {
                assert_eq!(stroke.color, egui::epaint::ColorMode::Solid(expected_color));
                assert_eq!(stroke.width, 1.0);
                paint.divider.push(*points);
            }
            egui::Shape::Text(text)
                if text.pos.x == gutter.left() + crate::chart::AXIS_LABEL_GAP_PX
                    && text.galley.job.text.parse::<f64>().is_ok() =>
            {
                tick_labels += usize::from(text.fallback_color == tick_ink);
                paint
                    .labels
                    .push((text.galley.job.text.clone(), text.pos, text.fallback_color));
            }
            _ => {}
        }
    }
    assert_eq!(
        paint.horizontal.len(),
        paint.raw_y.len(),
        "{side:?}: {paint:?}"
    );
    assert_eq!(paint.divider.len(), 1);
    assert_eq!(paint.time_rule.len(), 1);
    assert!(paint.labels.len() >= 2);
    assert_eq!(
        tick_labels,
        paint.horizontal.len() - 1,
        "the last-price chip replaces one muted tick label: {side:?}: {paint:?}"
    );
    paint
}

#[test]
fn inherited_light_time_grid_aligns_at_each_dpi_without_moving_prices_or_excluded_grids() {
    let authored_dark = "version=1\nname='White dark-mode chart'\ndark=true\n[colors]\ncanvas='#FFFFFF'\nchart_grid='#BABBBB'\n[bookmap.colors]\ncanvas='#FFFFFF'\nchart_grid='#BABBBB'";
    let authored_light = "version=1\nname='Daytime custom grid'\ndark=false\n[colors]\ncanvas='#F0F0F0'\nchart_grid='#123456'";
    for dpi in [1.0, 1.5, 2.0] {
        for (source, align_time) in [
            (quantick_appearance::LIGHT, true),
            (quantick_appearance::DARK, false),
            (authored_dark, false),
            (authored_light, true),
        ] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(dpi);
            let mut app = fixture(&ctx, source);
            let inherited_output = settled_frame(&mut app, &ctx);
            assert_eq!(ctx.pixels_per_point(), dpi);
            let inherited = SIDES.map(|side| capture(&app, &ctx, &inherited_output, side));
            for side in SIDES {
                let pane = app.active_tab().pane(side);
                let mut style = pane.presentation.candles.resolve_surface(
                    &app.runtime.style,
                    app.runtime.appearance.resolved(),
                    pane.orderflow.is_some(),
                );
                style.ownership.grid = Inheritance::Explicit;
                app.active_tab_mut()
                    .pane_mut(side)
                    .presentation
                    .candles
                    .set(style);
            }
            let explicit_output = settled_frame(&mut app, &ctx);
            let explicit = SIDES.map(|side| capture(&app, &ctx, &explicit_output, side));
            assert_eq!(
                inherited[1], explicit[1],
                "Flow keeps every painted coordinate"
            );
            for (painted, raw) in explicit[0].horizontal.iter().zip(&explicit[0].raw_y) {
                assert_eq!(
                    painted.0[0].y, *raw,
                    "explicit grid retains its raw price coordinate"
                );
            }
            if !align_time {
                assert_eq!(
                    inherited[0], explicit[0],
                    "Dark, including white authored Dark, stays exact"
                );
                continue;
            }
            assert_eq!(inherited[0].labels, explicit[0].labels);
            assert_eq!(inherited[0].divider, explicit[0].divider);
            assert_eq!(inherited[0].time_rule, explicit[0].time_rule);
            assert_eq!(inherited[0].raw_y, explicit[0].raw_y);
            let mut displaced = false;
            for (aligned, raw) in inherited[0].horizontal.iter().zip(&explicit[0].horizontal) {
                assert_eq!((aligned.1, aligned.2), (raw.1, raw.2));
                let physical_y = aligned.0[0].y * dpi;
                assert!((physical_y.fract() - 0.5).abs() < 0.0001);
                let shift = (aligned.0[0].y - raw.0[0].y).abs();
                assert!(shift <= 0.5001 / dpi);
                displaced |= shift > 0.0001;
            }
            assert!(displaced, "fixture must exercise fractional grid positions");
        }
    }
}
