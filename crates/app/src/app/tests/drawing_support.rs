use super::*;

/// What the gear on the context bar does, in one line.
///
/// Selecting a drawing raises the bar, not the panel, so a test that is
/// about the panel's *contents* has to open it first — the same way the
/// trader does. `the_gear_on_the_context_bar_opens_the_inspector` is the
/// test that proves this shortcut matches the real button.
pub(super) fn open_inspector(app: &mut QuantickApp, ctx: &egui::Context) {
    app.surfaces.drawing_chrome.set_inspector_open(true);
    // Two frames: the first opens the window, the second lets it settle
    // its size and automatic placement before anything reads its rect.
    run_frame(app, ctx);
    run_frame(app, ctx);
}

pub(super) fn drawing_tool(id: &str) -> drawings::DrawingTool {
    drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("drawing tool is registered")
}

pub(super) fn arm_drawing_from_toolbox(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    id: &str,
) -> drawings::DrawingTool {
    let drawing = drawing_tool(id);
    let tool = Tool::Drawing(drawing);
    if let Some(button) = app.surfaces.toolrail.button_rect(tool) {
        click_chart(app, ctx, button.center());
    } else {
        // A folded family member has no direct slot: open the family
        // flyout through the slot's caret zone and arm it by its row —
        // the same pointer path a user takes.
        let family = drawing
            .family()
            .expect("only family members lack a direct slot");
        let slot = drawings::DRAWING_TOOLS
            .into_iter()
            .filter(|candidate| {
                candidate
                    .family()
                    .is_some_and(|candidate_family| candidate_family.id == family.id)
            })
            .find_map(|candidate| app.surfaces.toolrail.button_rect(Tool::Drawing(candidate)))
            .expect("the family slot was rendered");
        click_chart(app, ctx, slot.max - egui::vec2(4.0, 4.0));
        run_frame(app, ctx);
        let row = app
            .surfaces
            .toolrail
            .flyout_row_rect(drawing)
            .expect("the flyout lists the folded member");
        click_chart(app, ctx, row.center());
    }
    assert_eq!(
        app.surfaces.toolrail.tool(),
        tool,
        "arming {id} through the rail must land"
    );
    drawing
}

/// Count the line segments painted in the drawing colour — how many
/// strokes of *this* object are on screen, across every pane.
pub(super) fn drawing_strokes(output: &egui::FullOutput) -> usize {
    let color = egui::epaint::ColorMode::Solid(crate::drawings::to_color32(
        crate::drawings::DEFAULT_DRAWING_COLOR,
    ));
    output
        .shapes
        .iter()
        // A lit glyph shares the drawing default's accent; its strip's or
        // capsule's clip is what tells it from the canvas.
        .filter(|clipped| clipped.clip_rect.height() > 2.0 * crate::time_header::HEIGHT_PX)
        .filter(|clipped| match &clipped.shape {
            egui::Shape::LineSegment { stroke, .. } => stroke.color == color,
            _ => false,
        })
        .count()
}

/// Twice the area of the triangle the three anchors make, in chart space.
/// Zero exactly when they are collinear — which for a channel means a
/// corridor of no width, and for a triangle no triangle.
pub(super) fn anchor_cross(points: &[drawings::ChartPoint]) -> f64 {
    let [a, b, c] = points else {
        panic!("a three-anchor object, got {}", points.len());
    };
    (f64::from(b.bar) - f64::from(a.bar)) * (c.price - a.price)
        - (f64::from(c.bar) - f64::from(a.bar)) * (b.price - a.price)
}

/// The price the trend line holds at `bar` — the height the width anchor
/// is measured above or below.
pub(super) fn trend_price_at(points: &[drawings::ChartPoint], bar: f32) -> f64 {
    let (a, b) = (&points[0], &points[1]);
    let span = f64::from(b.bar - a.bar);
    if span.abs() < 1e-9 {
        return a.price;
    }
    a.price + (b.price - a.price) * f64::from(bar - a.bar) / span
}

/// A tab split in two, with one shared horizontal line drawn on the flow
/// pane, and the screen position that line occupies on the *time* pane.
///
/// The mark is anchored on a real flow bar (so it carries a real market
/// instant) at the price sitting in the middle of the time pane's window
/// (so a drag has room to move in either direction without leaving the
/// chart). Its y is computed through the time pane's own price scale,
/// which is the whole point: the two panes agree on the price and on
/// nothing else.
pub(super) fn split_with_a_shared_line(
    ctx: &egui::Context,
) -> (QuantickApp, mpsc::Receiver<FeedCommand>, egui::Pos2) {
    let (mut app, commands) = app_with_history(200);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    // The layout is deferred a frame, so the time pane does not exist yet
    // on the line above — hence these frames before it is configured.
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    // One-second bars, so this fixture's 20 seconds of tape is 20 bars on
    // the time pane against 200 on the flow pane. That difference is what
    // §D7 is about — the two panes agree on market time and on nothing
    // else — and without it the time pane holds a single bar, most of its
    // chart is empty space no instant can be named in, and every gesture
    // below silently does nothing.
    let pane = app
        .active_tab_mut()
        .time_pane_mut()
        .expect("two frames is enough for the deferred layout to build it");
    pane.spec
        .update(
            quantick_engine::bar_selection::SelectionCommand::Select(
                crate::state::BarKind::Time.label(),
            ),
            quantick_engine::bar_selection::BarInputAvailability::ALL,
        )
        .unwrap();
    pane.spec.retain(crate::state::BarSpec::Time(1_000));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    assert!(
        app.active_tab()
            .time_pane()
            .is_some_and(|pane| pane.slots() > 5),
        "the time pane must hold a real series, or these tests pass on a              gesture that never reached a bar"
    );

    let (chart, scale) = time_pane_projection(&app);
    // Mid-window price, and an x near the newest bar: both panes can name
    // an instant there, and a drag has room above and below.
    let price = scale.price_at(chart.center().y);
    let slot = 100;
    let time = app
        .active_tab()
        .flow_pane
        .slot_open_time(slot)
        .expect("a closed bar has a time");
    app.active_tab_mut().flow_pane.drawings.place_with(
        drawing_tool("horizontal-line"),
        &drawings::DrawingBand::Price,
        drawings::ChartPoint::at_time(slot as f32 + 0.5, price, Some(time)),
        |tool| drawings::NewDrawing {
            style: drawings::DrawingStyle::default(),
            payload: tool.default_payload(),
            look: Default::default(),
        },
    );
    app.active_tab_mut()
        .flow_pane
        .drawings
        .selected_mut()
        .expect("placement selects what it completed")
        .scope = DrawingScope::AllCharts;
    // Nothing selected to start with, so the assertions cannot pass on the
    // selection placement left behind.
    app.active_tab_mut().flow_pane.drawings.select(None);
    run_frame(&mut app, ctx);

    let (chart, scale) = time_pane_projection(&app);
    (
        app,
        commands,
        egui::pos2(chart.right() - 30.0, scale.y(price)),
    )
}

/// A canvas point at price-line `y` that is provably clear of the open
/// inspector.
///
/// These proofs are about the canvas gesture, not about where the
/// placement rule currently puts the panel — so they ask the panel where
/// it is instead of encoding an answer that changes whenever the Style
/// tab grows a row.
pub(super) fn canvas_point_clear_of_inspector(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    y: f32,
) -> egui::Pos2 {
    run_frame(app, ctx);
    let clear = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .filter(|rect| rect.y_range().contains(y))
        .map_or(400.0, |rect| rect.right() + 40.0);
    egui::pos2(clear, y)
}

/// A chrome section that says nothing except where the popup was parked —
/// every other field at what a fresh cockpit has, so a test that restores
/// one is testing the position and not the dock.
pub(super) fn chrome_with_popup_at(position: Option<[f32; 2]>) -> ui_state::SavedChrome {
    ui_state::SavedChrome {
        timezone_minutes: 0,
        dock_visible: true,
        dock_tab: None,
        rail_visible: true,
        rail_dock: ui_state::SavedRailDock::Left,
        perf_readings: false,
        legacy_favorite_tools: Vec::new(),
        progressive_history: true,
        history_reach: None,
        history_reach_span_minutes: None,
        venue_lead_in: false,
        record_deals: None,
        inspector_position: position,
        orderflow_folds: Default::default(),
    }
}

/// Put a horizontal line on the chart at that height and leave it there.
/// Two of them is the setup every "the next drawing too" claim needs.
pub(super) fn draw_horizontal_line(app: &mut QuantickApp, ctx: &egui::Context, price_y: f32) {
    app.surfaces
        .toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(app, ctx, egui::pos2(700.0, price_y));
    run_frame(app, ctx);
}

/// Open the popup and drag its title bar by `delta`, returning where it
/// ended up. The gesture the whole feature is about.
pub(super) fn park_the_popup(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    delta: egui::Vec2,
) -> egui::Pos2 {
    open_inspector(app, ctx);
    let popup = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the properties popup is open");
    // Left of the trailing icons, which is the grip a trader gets.
    let grip = egui::pos2(popup.left() + 60.0, popup.top() + 14.0);
    drag_chart(app, ctx, grip, grip + delta);
    // The write is queued during the release frame and flushed at the top
    // of the next one, where every other workspace write lives.
    run_frame(app, ctx);
    app.surfaces
        .drawing_chrome
        .inspector_pos()
        .expect("the drag records a position")
}

/// Place a tool by its registry id through the pointer, anchor by anchor,
/// and hand back its index. The mission's tools need one click and two
/// respectively, so the helper takes the anchors as a list rather than
/// assuming either.
pub(super) fn place_drawing(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    id: &str,
    anchors: &[egui::Pos2],
) -> usize {
    app.surfaces.toolrail.arm(Tool::Drawing(drawing_tool(id)));
    for anchor in anchors {
        click_chart(app, ctx, *anchor);
    }
    run_frame(app, ctx);
    app.active_tab()
        .drawing_pane()
        .drawings
        .items()
        .iter()
        .rposition(|drawing| drawing.tool.id() == id)
        .unwrap_or_else(|| panic!("{id} was placed"))
}

/// Select `index` the way a click on the object does, raise the properties
/// popup through the gear's own door, and report where it opened.
pub(super) fn select_and_open_popup(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    index: usize,
) -> egui::Pos2 {
    app.drawing_pane_mut().drawings.select(Some(index));
    run_frame(app, ctx);
    open_inspector(app, ctx);
    // egui keeps an Area's last rect after the Area stops being shown, so
    // `area_rect` alone cannot tell "open here" from "closed, and here is
    // where it used to be" — and the last place it was is exactly the
    // value that would make a position assertion pass on a popup that is
    // no longer floating. Ask the app what it drew before reading egui.
    assert!(
        app.surfaces.drawing_chrome.inspector_open(),
        "the gear's door is open"
    );
    assert!(
        !app.surfaces.drawing_chrome.inspector_pinned(),
        "and the popup is floating, not docked"
    );
    assert_eq!(
        app.active_tab().drawing_pane().drawings.selected(),
        Some(index),
        "on the object this call selected"
    );
    ctx.memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the properties popup is open")
        .min
}

/// A tool the tests can star without caring which one it is.
pub(super) fn starrable_tool() -> crate::drawings::DrawingTool {
    crate::drawings::DrawingTool::by_id("measure").expect("a registered drawing tool")
}

/// A level for the layout tests: one anchor, in market time, on the pane.
pub(super) fn place_level(app: &mut QuantickApp, side: PaneSide, price: f64) {
    let tool = crate::drawings::DrawingTool::by_id("horizontal-line").expect("the tool");
    let time = app
        .active_tab()
        .pane(side)
        .slot_open_time(0)
        .expect("bars to anchor on");
    let placed = app.active_tab_mut().pane_mut(side).drawings.place(
        tool,
        crate::drawings::ChartPoint {
            bar: 0.5,
            price,
            time_ms: Some(time),
        },
    );
    assert!(placed, "a horizontal line places on one anchor");
}

/// Set the scope of every mark on `side` as the context bar's sharing button
/// does: one recorded edit each, so the next frame writes it.
pub(super) fn set_scope_on(app: &mut QuantickApp, side: PaneSide, scope: DrawingScope) {
    let store = &mut app.active_tab_mut().pane_mut(side).drawings;
    for index in 0..store.items().len() {
        let before = store.items()[index].clone();
        store.items_mut()[index].scope = scope;
        store.record_edit_of(index, before);
    }
}

pub(super) fn drawings_on(app: &QuantickApp, side: PaneSide) -> Vec<f64> {
    app.active_tab()
        .pane(side)
        .drawings
        .items()
        .iter()
        .map(|drawing| drawing.points[0].price)
        .collect()
}

/// A fixed-range profile placed on the flow pane with the footprint layer
/// switched off — the state the trader reported the jagged profile from,
/// and the one where nothing but the profile itself wants the ladders.
pub(super) fn place_range_profile_with_the_layer_off(app: &mut QuantickApp) {
    let frvp = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .expect("frvp is registered");
    let pane = app.active_tab_mut().pane_mut(PaneSide::Flow);
    pane.set_layer_visible(
        ChartLayer::Footprint,
        false,
        &mut quantick_layers::LayerActions::default(),
    );
    assert!(
        !pane
            .drawings
            .place(frvp, crate::drawings::ChartPoint::at(1.0, 100.0))
    );
    assert!(
        pane.drawings
            .place(frvp, crate::drawings::ChartPoint::at(20.0, 105.0))
    );
}

/// The grouping the flow pane's profile actually folded at, read off the
/// drawing rather than off the chart state — the ladder's row width and
/// the profile's are meant to be one number, and a test that read only the
/// state could not tell whether they had come apart.
pub(super) fn folded_profile_group(app: &QuantickApp) -> Decimal {
    app.active_tab().pane(PaneSide::Flow).drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<crate::drawings::FrvpPayload>()
        .expect("frvp payload")
        .cache
        .as_ref()
        .expect("a placed range over a live tape folded")
        .output()
        .profile
        .as_ref()
        .expect("the range covers bars that have tape")
        .0
        .group()
}

/// One anchor at the newest bar's open time and its close price — what an
/// agent reads from `chart.window.read` before it annotates.
pub(super) fn newest_anchor(app: &QuantickApp) -> serde_json::Value {
    let pane = app.active_tab().drawing_pane();
    let slot = pane.slots().saturating_sub(1);
    anchor_at_slot(app, slot)
}

pub(super) fn anchor_at_slot(app: &QuantickApp, slot: usize) -> serde_json::Value {
    let pane = app.active_tab().drawing_pane();
    let time = pane
        .slot_open_time(slot)
        .expect("the selected bar has a time");
    let price = pane
        .closed_bar(slot)
        .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
        .unwrap_or(1.0);
    serde_json::json!({
        "time_unix_ms": time,
        "price": format!("{price}"),
    })
}

/// Say that these objects were placed by an assistant, as the gateway's
/// own actor does when an agent calls the same action. Named by id, so a
/// test can never accidentally relabel the trader's own drawing.
pub(super) fn stamp_agent_author(app: &mut QuantickApp, ids: &[u64]) {
    let pane = app.active_tab_mut().drawing_pane_mut();
    let count = pane.drawings.items().len();
    for index in 0..count {
        if ids.contains(&pane.drawings.items()[index].id.0) {
            pane.drawings.set_author_at(
                index,
                Some(crate::drawings::DrawingAuthor {
                    actor_kind: "agent".to_owned(),
                    client_name: "quantick integration test".to_owned(),
                }),
            );
        }
    }
}
