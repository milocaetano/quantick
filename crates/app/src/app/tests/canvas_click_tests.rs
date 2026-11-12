//! The canvas's double and triple click as a hand makes them: real pointer
//! presses and releases, frame by frame, on the toolkit's own click clock
//! (`predicted_dt` a frame), through the whole app frame — the gesture, then
//! the draw that centres the price.
use super::*;
use quantick_chart_interaction::pane::CanvasHit;

const PRIMARY: egui::PointerButton = egui::PointerButton::Primary;

/// Frames of the toolkit's clock covering `delays` double-click delays.
fn frames_for(ctx: &egui::Context, delays: f64) -> usize {
    let seconds = ctx.options(|options| delays * options.input_options.max_double_click_delay);
    let dt = f64::from(ctx.input(|input| input.predicted_dt));
    (seconds / dt).ceil() as usize
}

/// Idle frames past twice the double-click delay: the toolkit counts a third
/// click inside it, so the next click starts a new run.
fn quiet(app: &mut QuantickApp, ctx: &egui::Context) {
    for _ in 0..frames_for(ctx, 2.0) + 2 {
        run_frame(app, ctx);
    }
}

fn click(app: &mut QuantickApp, ctx: &egui::Context, at: egui::Pos2) {
    click_sized(app, ctx, TEST_WINDOW, at);
}

fn hit(app: &QuantickApp, at: egui::Pos2) -> Option<CanvasHit> {
    app.active_tab().flow_pane.hit_test().canvas_hit(at)
}

/// The candles back `40` bars into history at `px_per_bar`; with
/// `far_prices`, under a manual range far above every bar, so the whole
/// canvas is open.
fn into_history(app: &mut QuantickApp, ctx: &egui::Context, px_per_bar: f32, far_prices: bool) {
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.model.viewport.set_px_per_bar(px_per_bar);
    pane.model
        .viewport
        .pan_pixels(40.0 * px_per_bar, pane.slots());
    if far_prices {
        assert!(pane.price_view.set_manual_range(10_000.0, 20_000.0));
    }
    run_frame(app, ctx);
    assert!(!app.active_tab().flow_pane.model.viewport.follows_live());
}

/// The candles alone, in history at 20 px a bar.
fn candles_app(ctx: &egui::Context, far_prices: bool) -> QuantickApp {
    let (mut app, _commands) = app_with_history(200);
    switch_layer(&mut app, ChartLayer::TapeChart, false);
    run_frame(&mut app, ctx);
    into_history(&mut app, ctx, 20.0, far_prices);
    app
}

/// The canvas's middle, open under the far price range.
fn open_canvas(app: &QuantickApp) -> egui::Pos2 {
    let at = app
        .active_tab()
        .flow_pane
        .frame
        .chart_rect
        .unwrap()
        .center();
    assert_eq!(hit(app, at), Some(CanvasHit::Empty));
    at
}

/// A point on a candle `back` bars left of the view's right edge — clear
/// of the lane handle there.
fn a_candle(app: &QuantickApp, back: usize) -> egui::Pos2 {
    let pane = &app.active_tab().flow_pane;
    let (area, right, total, _) = pane.hit_test().last_projection().expect("drawn");
    let viewport = &pane.model.viewport;
    let x = viewport.x_center(viewport.right_edge_bar(total) as usize - back, right, total);
    (0..400)
        .map(|step| egui::pos2(x, area.top() + area.height() * step as f32 / 400.0))
        .find(|at| hit(app, *at) == Some(CanvasHit::Candle))
        .expect("a candle in view")
}

/// Open canvas above the candles at the chart's middle, under auto-fit.
fn open_above_the_candles(app: &QuantickApp) -> egui::Pos2 {
    let chart = app.active_tab().flow_pane.frame.chart_rect.unwrap();
    (0..400)
        .map(|step| egui::pos2(chart.center().x, chart.top() + 2.0 + step as f32))
        .find(|at| hit(app, *at) == Some(CanvasHit::Empty))
        .expect("open canvas above the candles")
}

#[test]
fn a_real_double_click_on_open_canvas_centres_the_live_price_at_its_spacing() {
    let ctx = egui::Context::default();
    let mut app = candles_app(&ctx, true);
    let at = open_canvas(&app);
    click(&mut app, &ctx, at);
    click(&mut app, &ctx, at);
    assert!(ctx.input(|input| input.pointer.button_double_clicked(PRIMARY)));
    let pane = &app.active_tab().flow_pane;
    assert!(pane.model.viewport.follows_live());
    assert_eq!(pane.model.viewport.px_per_bar(), 20.0, "spacing kept");
    assert_last_price_centred(pane);
    assert!(!app.surfaces.chart_settings.model_state.is_open());
}

/// The third click zooms where its double centred, even though the view
/// that double moved puts a live candle under the pointer: the triple
/// is not hit-tested again.
#[test]
fn a_real_triple_click_doubles_the_spacing_though_its_double_moved_a_candle_under_it() {
    let ctx = egui::Context::default();
    let mut app = candles_app(&ctx, true);
    let open = open_canvas(&app);
    click(&mut app, &ctx, open);
    click(&mut app, &ctx, open);
    let live_candle = a_candle(&app, 4);
    quiet(&mut app, &ctx);
    into_history(&mut app, &ctx, 20.0, true);
    assert_eq!(
        hit(&app, live_candle),
        Some(CanvasHit::Empty),
        "open in history"
    );

    click(&mut app, &ctx, live_candle);
    click(&mut app, &ctx, live_candle);
    assert_eq!(
        hit(&app, live_candle),
        Some(CanvasHit::Candle),
        "the double's frame put a live candle under the pointer"
    );
    click(&mut app, &ctx, live_candle);
    assert!(ctx.input(|input| input.pointer.button_triple_clicked(PRIMARY)));
    let pane = &app.active_tab().flow_pane;
    assert!(pane.model.viewport.follows_live());
    assert_eq!(pane.model.viewport.px_per_bar(), 40.0, "the triple doubles");
    assert_last_price_centred(pane);
    run_frame(&mut app, &ctx);
    assert!(
        !app.surfaces.chart_settings.model_state.is_open(),
        "no candle settings"
    );
}

/// A candle's double click opens the Candles settings over a view left
/// where it was; the click after it, on open canvas, is no triple.
#[test]
fn a_real_double_click_on_a_candle_opens_its_settings_and_the_next_click_zooms_nothing() {
    let ctx = egui::Context::default();
    let mut app = candles_app(&ctx, false);
    let candle = a_candle(&app, 3);
    let open = open_above_the_candles(&app);
    let before = candles_view(&app);
    click(&mut app, &ctx, candle);
    click(&mut app, &ctx, candle);
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces.chart_settings.model_state.is_open(),
        "the Candles settings"
    );
    assert_eq!(candles_view(&app), before, "the view kept");
    // Closed so the window cannot take the next click off the canvas.
    app.surfaces.chart_settings.model_state.close();
    click(&mut app, &ctx, open);
    assert!(ctx.input(|input| input.pointer.button_triple_clicked(PRIMARY)));
    let pane = &app.active_tab().flow_pane;
    assert_eq!(candles_view(&app), before, "no zoom, no return to live");
    assert!(!pane.model.viewport.follows_live());
    assert!(pane.price_view.is_auto(), "no centred price");
}

/// Over the tape the double click returns the tape to live and leaves the
/// candles; a click on the candles' canvas right after it is no triple.
#[test]
fn a_real_double_click_on_the_tape_returns_it_to_live_and_the_next_click_zooms_nothing() {
    let ctx = egui::Context::default();
    let (mut app, tape, candles) = split_app(&ctx);
    let manual = (90.455, 111.545);
    let pane = &mut app.active_tab_mut().flow_pane;
    assert!(pane.price_view.set_manual_range(manual.0, manual.1));
    pane.model.viewport.pan_pixels(64.0, pane.slots());
    app.active_tab_mut()
        .tape_mut()
        .set_tape_end(TapeEnd::Past { end_ms: 30_000 });
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(&mut app, &ctx);
    assert!(!app.active_tab().tape().tape_end().is_live());
    let before = candles_view(&app);
    click(&mut app, &ctx, tape);
    click(&mut app, &ctx, tape);
    assert!(
        app.active_tab().tape().tape_end().is_live(),
        "the tape live"
    );
    click(&mut app, &ctx, candles);
    assert!(ctx.input(|input| input.pointer.button_triple_clicked(PRIMARY)));
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert_eq!(candles_view(&app), before, "the candles never moved");
    assert!(!pane.model.viewport.follows_live());
    assert_eq!(
        pane.price_view.manual_range(),
        Some(manual),
        "nor the price"
    );
    assert!(!app.surfaces.chart_settings.model_state.is_open());
}

/// A click, then a real double click past one double-click delay but inside
/// two: the toolkit counts the pair's second click as the run's third. It is
/// still the double click: back to live, the price centred, no zoom.
#[test]
fn a_single_click_then_a_quick_real_double_click_still_centres() {
    let ctx = egui::Context::default();
    let mut app = candles_app(&ctx, true);
    let at = open_canvas(&app);
    click(&mut app, &ctx, at);
    for _ in 0..=frames_for(&ctx, 1.0) {
        run_frame(&mut app, &ctx);
    }
    click(&mut app, &ctx, at);
    assert!(!ctx.input(|input| input.pointer.button_double_clicked(PRIMARY)));
    assert!(!app.active_tab().flow_pane.model.viewport.follows_live());
    click(&mut app, &ctx, at);
    assert!(
        ctx.input(|input| input.pointer.button_triple_clicked(PRIMARY)),
        "the toolkit counted three"
    );
    let pane = &app.active_tab().flow_pane;
    assert!(
        pane.model.viewport.follows_live(),
        "the double click's return"
    );
    assert_eq!(pane.model.viewport.px_per_bar(), 20.0, "no zoom");
    assert_last_price_centred(pane);
}
