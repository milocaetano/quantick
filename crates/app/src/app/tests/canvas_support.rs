use super::*;

/// The slot a stored fractional anchor sits on, asked of the one owner.
///
/// A test that recomputes it — `bar.floor()`, as five of them used to —
/// is a second copy of the projection rule, free to disagree with the
/// production one and to keep passing while it does. See
/// [`Viewport::slot_of`].
pub(super) fn slot_of(bar: f32) -> usize {
    Viewport::slot_of(bar).expect("a placed anchor sits on a slot")
}

/// The bar the viewport has at its right edge — how far the chart is
/// panned along time.
pub(super) fn right_edge(app: &QuantickApp) -> f32 {
    let pane = &app.active_tab().flow_pane;
    pane.model.viewport.right_edge_bar(pane.slots())
}

/// How many switches each pane's menu lays out at its top level. The two
/// menus split one list ([`ChartLayer::on_tape`]), so counting either from
/// the list is what keeps a new layer from silently landing on the wrong
/// canvas's menu.
pub(super) fn chart_menu_entries() -> usize {
    ChartLayer::ALL
        .into_iter()
        .filter(|layer| !layer.on_tape())
        .count()
}

pub(super) fn tape_menu_entries() -> usize {
    ChartLayer::ALL
        .into_iter()
        .filter(|layer| layer.on_tape())
        .count()
}

/// The active tab's flow pane beside the chrome a canvas frame hands it.
pub(super) fn with_flow_pane<R>(
    app: &mut QuantickApp,
    body: impl FnOnce(&mut ChartPane, &mut pane::PaneChrome<'_>) -> R,
) -> R {
    let capabilities = app.active_tab().capabilities(&app.config);
    let side_inferred = app.active_tab().side_note(&app.config).is_some();
    let mut begin_text_edit = false;
    let QuantickApp {
        runtime:
            workspace_runtime::WorkspaceRuntime {
                tabs,
                style,
                tz,
                workspace,
                footprint_config,
                ..
            },
        drawings,
        surfaces,
        ..
    } = app;
    let tab_id = tabs.id_at(tabs.active_index());
    let tab = tabs.runtime_mut(tabs.active_index());
    let (market, layers) = workspace.canvas_settings();
    let mut chrome = pane::PaneChrome {
        market,
        tab: tab_id,
        side: pane::PaneSide::Flow,
        toolrail: &mut surfaces.toolrail,
        presets: &drawings.presets,
        drawing_chrome: &mut surfaces.drawing_chrome,
        begin_text_edit: &mut begin_text_edit,
        style,
        tz: *tz,
        feed_gaps: &[],
        paper: &mut tab.paper,
        strategies: &mut tab.market.strategies,
        paper_takes_input: true,
        paper_hud_here: true,
        // One pane in hand and no tab around it: there is no other pane
        // whose shared marks could be under the pointer.
        shared_pick: None,
        shared: pane::SharedInteraction::default(),
        capabilities,
        side_inferred,
        price_tag: Default::default(),
        footprint: footprint_config,
        layers,
    };
    body(&mut tab.flow_pane, &mut chrome)
}

/// Switch a layer exactly as the menu does, including the hop the app makes
/// for the ones a pane does not own.
pub(super) fn switch_layer(app: &mut QuantickApp, layer: ChartLayer, visible: bool) {
    with_flow_pane(app, |pane, chrome| {
        pane.set_layer_visible(layer, visible, chrome.layers);
    });
    app.layer_wiring().apply_actions();
}

/// Whether the active tab's flow pane is painting `layer`.
pub(super) fn layer_on(app: &QuantickApp, layer: ChartLayer) -> bool {
    app.active_tab()
        .flow_pane
        .layer_visible(layer, &app.runtime.style)
}

/// The frame's rectangles, so assertions can identify the layer they test.
pub(super) fn painted_rects(output: &egui::FullOutput) -> Vec<&egui::epaint::RectShape> {
    fn walk<'a>(shape: &'a egui::Shape, found: &mut Vec<&'a egui::epaint::RectShape>) {
        match shape {
            egui::Shape::Rect(rect) => found.push(rect),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

/// Every string the frame painted, panels and chart alike.
pub(super) fn painted_text(output: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &egui::Shape, found: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => found.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

/// The visual centre of one exact painted label, for tests that operate the
/// same egui popup or submenu a trader clicks.
pub(super) fn painted_text_center(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
    fn walk(shape: &egui::Shape, wanted: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                Some(text.visual_bounding_rect().center())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| walk(shape, wanted)),
            _ => None,
        }
    }

    output
        .shapes
        .iter()
        .find_map(|clipped| walk(&clipped.shape, wanted))
}

/// Whether the frame drew the price axis over the test's price range —
/// the labels only exist once the chart really scaled and painted itself.
pub(super) fn has_price_axis(texts: &[String]) -> bool {
    texts.iter().any(|text| {
        text.parse::<f64>()
            .is_ok_and(|price| (95.0..=115.0).contains(&price))
    })
}

pub(super) fn run_frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    run_frame_with_events(app, ctx, Vec::new())
}

pub(super) fn run_frame_with_events(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_frame_with_modifiers(app, ctx, events, egui::Modifiers::NONE)
}

/// The window every frame-driving test gets unless it asks for another:
/// roomy, so a test about something else is never accidentally a test
/// about a cramped layout.
pub(super) const TEST_WINDOW: egui::Vec2 = egui::vec2(1400.0, 900.0);

/// The smallest window whose *layout* is still promised: the chrome
/// collapses down to here and clips below it (the drawing rail's Minimal
/// stage, docs/drawing-toolbar-ux.md §2.8). The window itself has no
/// minimum any more — see [`A_DEGENERATE_WINDOW`] — but this is still
/// where the pane band is under real pressure while everything is
/// expected to read, so the layout tests stay aimed at it.
pub(super) const MIN_WINDOW: egui::Vec2 = egui::vec2(900.0, 560.0);

/// A window dragged down to nothing. `main.rs` sets no
/// `with_min_inner_size`, so this is reachable by a trader with a mouse;
/// nothing is promised about what it looks like, only that the app is
/// still there when the window is dragged back out.
pub(super) const A_DEGENERATE_WINDOW: egui::Vec2 = egui::vec2(1.0, 1.0);

pub(super) fn run_frame_with_modifiers(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    run_frame_sized(app, ctx, TEST_WINDOW, events, modifiers)
}

/// A frame at a chosen window size — how a test reaches the layout a
/// smaller window produces without resizing anything global.
pub(super) fn run_frame_sized(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
        events,
        modifiers,
        ..Default::default()
    };
    let now = frame_instant(ctx);
    ctx.run(input, |ctx| app.draw_frame(ctx, now))
}

/// A press-drag-release in a window of `size`.
pub(super) fn drag_sized(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    start: egui::Pos2,
    end: egui::Pos2,
) {
    drag_sized_from_press(app, ctx, size, start, end);
}

/// [`drag_sized`], returning the press frame's output so a test can check
/// what the press itself landed on.
pub(super) fn drag_sized_from_press(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    start: egui::Pos2,
    end: egui::Pos2,
) -> egui::FullOutput {
    let press = run_frame_sized(
        app,
        ctx,
        size,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
        egui::Modifiers::NONE,
    );
    run_frame_sized(
        app,
        ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        egui::Modifiers::NONE,
    );
    run_frame_sized(
        app,
        ctx,
        size,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
        egui::Modifiers::NONE,
    );
    press
}

pub(super) fn run_frame_at(app: &mut QuantickApp, ctx: &egui::Context, size: egui::Vec2) {
    run_frame_sized(app, ctx, size, Vec::new(), egui::Modifiers::NONE);
}

pub(super) fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(in crate::app) fn click_chart(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    position: egui::Pos2,
) {
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
    );
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, false),
        ],
    );
}

/// A click with a modifier held, and the move that carries the pointer
/// there — the click-move-click half of a placement, as opposed to a drag.
pub(super) fn click_chart_with(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    position: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    for pressed in [true, false] {
        run_frame_sized(
            app,
            ctx,
            TEST_WINDOW,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, pressed),
            ],
            modifiers,
        );
    }
}

/// Move the pointer without pressing anything, and answer what got
/// painted — the frames between two clicks, which is where a placement
/// gesture does all of its talking.
pub(super) fn move_chart_with(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    position: egui::Pos2,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    run_frame_sized(
        app,
        ctx,
        TEST_WINDOW,
        vec![egui::Event::PointerMoved(position)],
        modifiers,
    )
}

pub(super) fn drag_chart(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    start: egui::Pos2,
    end: egui::Pos2,
) {
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(end)]);
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    );
}

/// A press-drag-release with a modifier held for the whole gesture.
pub(super) fn drag_chart_with(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    start: egui::Pos2,
    end: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    for events in [
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    ] {
        run_frame_sized(app, ctx, TEST_WINDOW, events, modifiers);
    }
}

/// The time pane's chart rect and price scale, as it last drew them.
pub(super) fn time_pane_projection(app: &QuantickApp) -> (egui::Rect, PriceScale) {
    let time_pane = app
        .active_tab()
        .time_pane()
        .expect("the split built a time pane");
    let chart = time_pane.frame.chart_area.expect("the time pane drew");
    let (lo, hi) = time_pane
        .price_view
        .resolve(time_pane.frame.auto_range.expect("the pane has a range"));
    let scale = PriceScale::from_range(
        lo,
        hi,
        time_pane.frame.chart_top,
        time_pane.frame.chart_top + time_pane.frame.chart_height,
    );
    (chart, scale)
}

pub(super) fn run_sized_frame(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_frame_sized(app, ctx, size, events, egui::Modifiers::NONE)
}

pub(super) fn click_sized(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    position: egui::Pos2,
) {
    run_sized_frame(
        app,
        ctx,
        size,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
    );
    run_sized_frame(
        app,
        ctx,
        size,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, false),
        ],
    );
}

pub(super) fn key_press(key: egui::Key) -> egui::Event {
    key_press_with(key, egui::Modifiers::NONE)
}

pub(super) fn key_press_with(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// The release half of a chord — the only half the platform is guaranteed
/// to deliver for a paste shortcut.
pub(super) fn key_release_with(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    }
}

/// Whether any painted line segment uses `color` — proof a stroke of that
/// colour reached the screen (or, negated, that a hidden object did not).
pub(super) fn painted_line_with_color(output: &egui::FullOutput, color: egui::Color32) -> bool {
    output.shapes.iter().any(|clipped| match &clipped.shape {
        egui::Shape::LineSegment { stroke, .. } => {
            stroke.color == egui::epaint::ColorMode::Solid(color)
        }
        _ => false,
    })
}

pub(super) fn time_zoom(app: &QuantickApp) -> f32 {
    let pane = app.active_tab().time_pane().expect("time pane");
    pane.model.viewport.px_per_bar()
}

/// An app with `count` trades of history, split, and laid out by two real
/// frames so both panes have reported their rects.
pub(in crate::app) fn split_app(
    ctx: &egui::Context,
    count: u64,
) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, commands) = app_with_history(count);
    run_frame(&mut app, ctx);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    (app, commands)
}

/// A point inside the pane on `side`, for a click that focuses it.
pub(in crate::app) fn pane_point(app: &QuantickApp, side: PaneSide) -> egui::Pos2 {
    app.active_tab()
        .pane(side)
        .frame
        .chart_area
        .expect("the pane reported its rect")
        .center()
}

/// Where `price` sits on `side`'s axis, computed the way that pane's own
/// frame computes it.
pub(super) fn price_y(app: &QuantickApp, side: PaneSide, price: f64) -> f32 {
    let pane = app.active_tab().pane(side);
    let chart = pane.frame.chart_area.expect("the pane reported its rect");
    let auto = pane.frame.auto_range.expect("the pane fitted a range");
    let (lo, hi) = pane.price_view.resolve(auto);
    PriceScale::from_range(lo, hi, chart.top(), chart.bottom()).y(price)
}

/// A frame carrying the window's close request, which is the only signal
/// the exit save has to work from.
pub(super) fn close_requested_frame(app: &mut QuantickApp, ctx: &egui::Context) {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    let now = frame_instant(ctx);
    let _ = ctx.run(input, |ctx| app.draw_frame(ctx, now));
}

/// Put the pointer over `slot` of the flow pane, the way the cursor tests
/// do, so a mark has a bar to resolve.
pub(super) fn hover_bar(app: &mut QuantickApp, ctx: &egui::Context, slot: usize) {
    run_frame(app, ctx);
    let position = {
        let pane = &app.active_tab().flow_pane;
        let chart = pane.frame.chart_area.expect("the pane reported its rect");
        let right = pane.frame.candles_right.unwrap_or_else(|| chart.right());
        egui::pos2(
            pane.model.viewport.x_center(slot, right, pane.slots()),
            chart.center().y,
        )
    };
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(position)]);
}
