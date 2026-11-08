//! Actual app paint preserves price coordinates while aligning inherited Light grids.
use super::*;
use quantick_appearance::{Inheritance, Token};

const SIDES: [PaneSide; 2] = [PaneSide::Time(0), PaneSide::Flow];

#[derive(Debug, PartialEq)]
struct GridPaint {
    horizontal: Vec<([egui::Pos2; 2], f32, egui::Color32)>,
    divider: Vec<[egui::Pos2; 2]>,
    time_rule: Vec<[egui::Pos2; 2]>,
    labels: Vec<(String, egui::Pos2, egui::Color32)>,
    raw_y: Vec<f32>,
    /// The time grid's lines, by x.
    vertical: Vec<f32>,
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
    let time_grid = style
        .canvas
        .time_grid_rgba()
        .map(|[r, g, b, a]| egui::Color32::from_rgba_unmultiplied(r, g, b, a));
    // Light rules and writes its axes in its own tokens; Dark in the grid's
    // colour and its muted text.
    let axes = crate::theme::axes(ctx);
    let (price_rule, time_rule, tick_ink) = if axes.resolved.dark {
        (expected_color, expected_color, axes.text_muted())
    } else {
        (
            *axes.get(Token::PriceAxisRule),
            *axes.get(Token::TimeAxisRule),
            *axes.get(Token::AxisInk),
        )
    };
    // Light rules its axes on whole device pixels, two of them at 1.5 as
    // the candle contour; Dark keeps its one-point strokes.
    let ppp = ctx.pixels_per_point();
    let crisp = quantick_chart::candle_pixels::crisp_line(0.0, 1.0, ppp).1;
    let rule_width = if axes.resolved.dark { 1.0 } else { crisp };
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
        raw_y: match axes.resolved.chart_axes.price_line_spacing {
            Some(spacing) => quantick_chart::price_grid::ticks(lo, hi, rect.height(), spacing),
            None => crate::chart::nice_ticks(lo, hi, 8),
        }
        .into_iter()
        .map(|tick| scale.y(tick))
        .collect(),
        vertical: Vec::new(),
    };
    for shape in &output.shapes {
        match &shape.shape {
            egui::Shape::LineSegment { points, stroke }
                if points[0].x == rect.left()
                    && points[1].x == rect.right()
                    && points[0].y == points[1].y
                    && (points[0].y - rect.bottom()).abs() <= 0.5001 =>
            {
                assert_eq!(stroke.color, egui::epaint::ColorMode::Solid(time_rule));
                assert_eq!(stroke.width, rule_width);
                paint.time_rule.push(*points);
            }
            egui::Shape::LineSegment { points, stroke }
                if points[0].x == rect.left()
                    && points[1].x == rect.right()
                    && points[0].y == points[1].y
                    && points[0].y > rect.top()
                    && points[0].y < rect.bottom() =>
            {
                let egui::epaint::ColorMode::Solid(color) = stroke.color else {
                    panic!("the grid uses a solid color");
                };
                assert_eq!(color, expected_color);
                assert!(stroke.width == 1.0 || stroke.width == crisp, "{stroke:?}");
                paint.horizontal.push((*points, stroke.width, color));
            }
            egui::Shape::LineSegment { points, stroke }
                if (points[0].x - gutter.left()).abs() <= 0.5001
                    && points[1].x == points[0].x
                    && points[0].y == rect.top()
                    && points[1].y == rect.bottom() =>
            {
                assert_eq!(stroke.color, egui::epaint::ColorMode::Solid(price_rule));
                assert_eq!(stroke.width, rule_width);
                paint.divider.push(*points);
            }
            // Other full-height marks (a seam, a day rule) wear their own
            // colours; the time grid is the one in the grid's.
            egui::Shape::LineSegment { points, stroke }
                if points[1].x == points[0].x
                    && points[0].x > rect.left()
                    && points[0].x < gutter.left() - 0.5001
                    && points[0].y == rect.top()
                    && points[1].y == rect.bottom()
                    && time_grid.is_some_and(|color| {
                        stroke.color == egui::epaint::ColorMode::Solid(color)
                    }) =>
            {
                paint.vertical.push(points[0].x);
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
            // Light opens with gridlines at its time labels as well; Dark,
            // and a Light-mode theme written before them, with none.
            let light_time_grid = source == quantick_appearance::LIGHT;
            assert_eq!(
                !inherited[0].vertical.is_empty(),
                light_time_grid,
                "{dpi}: {:?}",
                inherited[0].vertical
            );
            assert_eq!(inherited[0].vertical.len(), explicit[0].vertical.len());
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
                // On whole device pixels, as thick as the candle contour.
                assert_eq!(raw.1, 1.0, "an explicit grid keeps its stroke");
                assert_eq!(aligned.2, raw.2);
                let width = quantick_chart::candle_pixels::crisp_line(0.0, 1.0, dpi).1;
                assert_eq!(aligned.1, width);
                let top_edge = (aligned.0[0].y - width / 2.0) * dpi;
                assert!((top_edge - top_edge.round()).abs() < 0.001, "{top_edge}");
                let shift = (aligned.0[0].y - raw.0[0].y).abs();
                assert!(shift <= 0.5001 / dpi);
                displaced |= shift > 0.0001;
            }
            assert!(displaced, "fixture must exercise fractional grid positions");
        }
    }
}

/// The vertical grid and the time strip read one layout of the labels: a
/// Light frame lays them out once per pane, not once for each.
#[test]
fn the_time_labels_are_laid_out_once_per_pane_and_frame() {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(1.5);
    let mut app = fixture(&ctx, quantick_appearance::LIGHT);
    let runs = || crate::pane::TIME_MARK_RUNS.with(std::cell::Cell::get);
    let before = runs();
    let output = run_frame(&mut app, &ctx);
    let painted = SIDES
        .into_iter()
        .filter(|&side| app.active_tab().pane(side).frame.chart_rect.is_some())
        .count();
    let grids = SIDES
        .into_iter()
        .filter(|&side| !capture(&app, &ctx, &output, side).vertical.is_empty())
        .count();
    assert!(grids > 0, "Light draws its time grid");
    assert_eq!(runs() - before, painted, "one layout per painted pane");
}
