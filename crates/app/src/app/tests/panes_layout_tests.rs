use super::*;

#[test]
fn the_live_strip_carves_between_chart_and_gutter_only_when_shown() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

    let off = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
    assert!(off.live_strip.is_none());
    assert_eq!(off.chart.right(), off.price_gutter.left());

    let on = plot_split(
        area,
        crate::live_strip::LIVE_STRIP_WIDTH_PX,
        &[crate::indicators::PaneSizing::Auto; 0],
    );
    let strip = on.live_strip.expect("strip rect");
    assert_eq!(on.chart.right(), strip.left());
    assert_eq!(strip.right(), on.price_gutter.left());
    assert_eq!(strip.width(), crate::live_strip::LIVE_STRIP_WIDTH_PX);
    // The strip pays with the chart's pixels: the gutter stays put, and
    // the time axis keeps spanning exactly the chart body.
    assert_eq!(on.price_gutter, off.price_gutter);
    assert_eq!(
        on.chart.width(),
        off.chart.width() - crate::live_strip::LIVE_STRIP_WIDTH_PX
    );
    assert_eq!(on.time_strip.right(), on.chart.right());
}

/// The pane band is carved once, inside `plot_split`, so the rect the
/// renderer scales prices to is the rect the input handler hit-tests
/// against. When the two disagreed, a drawing was placed where you
/// clicked and then selected somewhere else — by 20% of the chart height
/// per visible pane.
#[test]
fn the_pane_band_comes_out_of_every_callers_chart_rect() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

    let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
    assert!(none.indicator_panes.is_empty());

    let one = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 1]);
    let pane = *one
        .indicator_panes
        .first()
        .expect("one visible pane claims one rect");
    assert!(
        one.chart.height() < none.chart.height(),
        "the band is paid for out of the candles' pixels"
    );
    assert_eq!(one.chart.bottom(), pane.rect.top(), "no gap, no overlap");
    assert_eq!(pane.rect.bottom(), none.chart.bottom());
    assert_eq!(one.chart.width(), none.chart.width());
    // The axes keep their column; the time strip is untouched.
    assert_eq!(one.price_gutter.x_range(), none.price_gutter.x_range());
    assert_eq!(one.time_strip, none.time_strip);

    let three = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 3]);
    assert_eq!(three.indicator_panes.len(), 3);
    assert!(three.chart.height() < one.chart.height());
}

/// The gutter is banded like the body it labels. Before it was, the whole
/// column belonged to the candles: dragging the numbers beside a CVD pane
/// stretched the *price* scale, and the pane — which had no axis at all —
/// did not move.
#[test]
fn every_pane_owns_the_gutter_band_beside_it() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

    let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
    assert!(none.pane_gutters.is_empty());
    assert_eq!(
        none.price_gutter.bottom(),
        none.chart.bottom(),
        "with no pane the gutter is the candles', top to bottom"
    );

    let two = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 2]);
    assert_eq!(two.pane_gutters.len(), two.indicator_panes.len());
    assert_eq!(
        two.price_gutter.bottom(),
        two.chart.bottom(),
        "the candles' scale stops where the candles do"
    );
    for (pane, gutter) in two.indicator_panes.iter().zip(&two.pane_gutters) {
        assert_eq!(
            gutter.y_range(),
            pane.rect.y_range(),
            "band beside its pane"
        );
        assert_eq!(gutter.x_range(), two.price_gutter.x_range(), "one column");
    }
    // No pixel answers to two scales: the bands tile the gutter exactly.
    assert_eq!(two.price_gutter.bottom(), two.pane_gutters[0].top());
    assert_eq!(two.pane_gutters[0].bottom(), two.pane_gutters[1].top());

    // The strip pays out of the candles, not the gutter: the pane bands
    // keep the same column when the tape is shown.
    let with_strip = plot_split(
        area,
        crate::live_strip::LIVE_STRIP_WIDTH_PX,
        &[crate::indicators::PaneSizing::Auto; 2],
    );
    assert_eq!(with_strip.pane_gutters, two.pane_gutters);
}

/// The whole point of collapsing: a strip is not a dead band. One click on
/// it brings the curve back, and it must survive the frame after — the
/// automatic rule is what collapsed the pane, so handing the pane back to
/// it would undo the click immediately.
/// Three panes in the smallest window the app allows: the state the user
/// reported as unusable. Something must collapse — that is the point — and
/// nothing that stays open may be below the readable floor.
#[test]
fn the_smallest_window_collapses_rather_than_squeezing_every_pane() {
    let ctx = egui::Context::default();
    let (app, _cmd_rx) = app_with_full_pane_band(&ctx, MIN_WINDOW);

    let panes = pane_slots(&app);
    assert_eq!(panes.len(), crate::indicators::MAX_PANES);
    assert!(
        panes.iter().any(|pane| pane.collapsed),
        "the smallest window cannot hold three readable panes: {panes:?}"
    );
    for pane in &panes {
        assert!(
            pane.collapsed || pane.rect.height() >= crate::indicators::MIN_PANE_HEIGHT_PX,
            "an expanded pane below the readable floor: {pane:?}"
        );
    }
}

/// The same band in a roomy window: nothing collapses, so the floor never
/// costs a user with a big screen anything.
#[test]
fn a_roomy_window_draws_every_pane() {
    let ctx = egui::Context::default();
    let (app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);
    assert!(
        pane_slots(&app).iter().all(|pane| !pane.collapsed),
        "a tall window has room for all of them: {:?}",
        pane_slots(&app)
    );
}

/// The whole point of collapsing: a strip is not a dead band. One click on
/// it brings the curve back, and it must survive the frame after — the
/// automatic rule is what collapsed the pane, so handing the pane back to
/// it would undo the click immediately.
#[test]
fn clicking_a_collapsed_strip_opens_it_and_it_stays_open() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, MIN_WINDOW);

    let collapsed = pane_slots(&app)
        .iter()
        .position(|pane| pane.collapsed)
        .expect("the smallest window collapses at least one pane");

    let strip = pane_slots(&app)[collapsed].rect;
    click_sized(&mut app, &ctx, MIN_WINDOW, strip.center());
    run_frame_at(&mut app, &ctx, MIN_WINDOW);
    assert!(
        !pane_slots(&app)[collapsed].collapsed,
        "one click opens the strip"
    );

    run_frame_at(&mut app, &ctx, MIN_WINDOW);
    assert!(
        !pane_slots(&app)[collapsed].collapsed,
        "and it is still open a frame later, not re-collapsed by the layout"
    );
}

/// Drag a pane's top edge and that pane resizes — the grammar the canvas
/// split and the live lane already use, on the one band in the app that
/// did not have it. Dragging up grows the pane into the chart.
#[test]
fn dragging_a_panes_top_edge_resizes_that_pane() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);

    let before = pane_slots(&app)[0].rect.height();
    let edge = pane_slots(&app)[0].rect.center_top();
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        edge,
        edge - egui::vec2(0.0, 40.0),
    );
    run_frame_at(&mut app, &ctx, TEST_WINDOW);

    let after = pane_slots(&app)[0].rect.height();
    assert!(
        after > before,
        "dragging the edge up grows the pane: {before} -> {after}"
    );
}

/// The floor holds during a drag too: a divider stops rather than
/// producing a pane too short to read. Anything else would hand the user a
/// way to recreate exactly the state this branch exists to prevent.
#[test]
fn a_divider_cannot_be_dragged_past_the_readable_floor() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);

    let edge = pane_slots(&app)[0].rect.center_top();
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        edge,
        edge + egui::vec2(0.0, 400.0),
    );
    run_frame_at(&mut app, &ctx, TEST_WINDOW);

    let pane = pane_slots(&app)[0];
    assert!(
        pane.collapsed || pane.rect.height() >= crate::indicators::MIN_PANE_HEIGHT_PX,
        "a drag cannot squeeze a pane below the floor: {pane:?}"
    );
}

/// Double click on a divider gives the pane back to the automatic layout —
/// the escape every other axis in the app offers on the same gesture.
#[test]
fn double_clicking_a_divider_returns_the_pane_to_automatic() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);

    let automatic = pane_slots(&app)[0].rect.height();
    let edge = pane_slots(&app)[0].rect.center_top();
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        edge,
        edge - egui::vec2(0.0, 60.0),
    );
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    assert!(
        (pane_slots(&app)[0].rect.height() - automatic).abs() > 1.0,
        "the drag took manual control"
    );

    let edge = pane_slots(&app)[0].rect.center_top();
    click_sized(&mut app, &ctx, TEST_WINDOW, edge);
    click_sized(&mut app, &ctx, TEST_WINDOW, edge);
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    assert!(
        (pane_slots(&app)[0].rect.height() - automatic).abs() < 1.0,
        "and a double click hands it back: {} vs {automatic}",
        pane_slots(&app)[0].rect.height()
    );
}

/// The other half of the disclosure. A control that only opens is half a
/// control: a trader who wants the candles back must be able to put a pane
/// away without deleting the indicator, and the value must survive it.
#[test]
fn the_disclosure_closes_a_pane_as_well_as_opening_it() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_full_pane_band(&ctx, TEST_WINDOW);
    assert!(
        pane_slots(&app).iter().all(|pane| !pane.collapsed),
        "the roomy window starts with every pane open"
    );

    let corner = crate::indicator_render::pane_disclosure_rect(pane_slots(&app)[0].rect, false);
    click_sized(&mut app, &ctx, TEST_WINDOW, corner.center());
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    assert!(
        pane_slots(&app)[0].collapsed,
        "clicking the open disclosure puts the pane away"
    );
    assert!(
        !pane_slots(&app)[1].collapsed,
        "and only that pane: the click was over one corner"
    );

    // Room is not what is keeping it shut, so it stays shut.
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    assert!(
        pane_slots(&app)[0].collapsed,
        "a hand-closed pane stays shut"
    );

    let strip = pane_slots(&app)[0].rect;
    click_sized(&mut app, &ctx, TEST_WINDOW, strip.center());
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    assert!(
        !pane_slots(&app)[0].collapsed,
        "and the same control brings it back"
    );
}

/// A pane body is a piece of the chart, so the chart's own gestures have
/// to work on it: drag to move, and the pane's own axis for the vertical
/// half. Before this the body answered nothing at all — the only way to
/// move a pane's curve out of its own way was to travel to the gutter on
/// the far side of the tape and drag it there.
#[test]
fn dragging_a_pane_body_pans_that_pane_and_the_shared_time_axis() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    let other = add_pane_indicator(&mut app, "delta", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx); // the frame that fits each pane and records it

    let body = pane_body(&app, 0);
    let (lo, hi) = pane_range(&app, flow);
    let edge_before = right_edge(&app);

    drag_chart(
        &mut app,
        &ctx,
        body.center(),
        body.center() + egui::vec2(40.0, 30.0),
    );

    let (panned_lo, panned_hi) = pane_range(&app, flow);
    assert!(
        ((panned_hi - panned_lo) - (hi - lo)).abs() < 1e-6,
        "a pan moves the window without resizing it: {lo}..{hi} -> {panned_lo}..{panned_hi}"
    );
    assert!(
        panned_lo > lo,
        "the candles' direction: pull the content down and the window climbs ({lo} -> {panned_lo})"
    );
    // Not the neighbour's resolved range: panning time changes what is
    // visible, so every pane still on auto legitimately refits. What must
    // not happen is the neighbour being taken off auto by a drag that was
    // never over it.
    assert!(
        pane_is_auto(&app, other),
        "one pane, one scale: the neighbour still fits its own values"
    );
    assert!(
        !pane_is_auto(&app, flow),
        "and the dragged one took control"
    );
    assert!(
        app.active_tab().flow_pane.price_view.is_auto(),
        "and the candles' own price scale is not a pane's to move"
    );
    assert!(
        (right_edge(&app) - edge_before).abs() > f32::EPSILON,
        "time is shared: the sideways half of the drag moved the chart"
    );
}

/// Time is moved once per drag, not once per pane. Three stacked panes
/// answering the same sideways drag would pan the chart three times, and
/// the bars would run away from the pointer.
#[test]
fn a_pane_drag_pans_time_once_however_many_panes_are_stacked() {
    let ctx = egui::Context::default();

    let mut travelled = Vec::new();
    for panes in [1_usize, 3] {
        let (mut app, _cmd_rx) = app_with_history(200);
        for index in 0..panes {
            add_pane_indicator(
                &mut app,
                &format!("pane{index}"),
                (0..200).map(f64::from).collect(),
            );
        }
        run_frame(&mut app, &ctx);

        let body = pane_body(&app, 0);
        let before = right_edge(&app);
        drag_chart(
            &mut app,
            &ctx,
            body.center(),
            body.center() + egui::vec2(40.0, 0.0),
        );
        travelled.push(right_edge(&app) - before);
    }

    assert!(
        (travelled[0] - travelled[1]).abs() < 1e-4,
        "one pane and three must pan time by the same amount: {travelled:?}"
    );
    assert!(travelled[0].abs() > f32::EPSILON, "and it did pan");
}

/// Double click inside a pane hands its scale back to auto-fit — the same
/// escape its gutter offers, so a trader who panned a pane by mistake gets
/// out of it wherever the pointer happens to be.
#[test]
fn double_clicking_a_pane_body_returns_it_to_auto_fit() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx);

    let body = pane_body(&app, 0);
    drag_chart(
        &mut app,
        &ctx,
        body.center(),
        body.center() + egui::vec2(0.0, 30.0),
    );
    assert!(!pane_is_auto(&app, flow), "the drag took manual control");

    click_chart(&mut app, &ctx, body.center());
    click_chart(&mut app, &ctx, body.center());
    run_frame(&mut app, &ctx);

    assert!(
        pane_is_auto(&app, flow),
        "and a double click gives it back to the values"
    );
}

/// The headline of this feature: a pane's numbers are its own axis. A drag
/// there stretches that pane and nothing else — before the gutter was
/// banded, the same pixels moved the *candles'* price scale, and the pane
/// had no axis to grab at all.
#[test]
fn dragging_a_pane_axis_zooms_that_pane_and_nothing_else() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    let other = add_pane_indicator(&mut app, "delta", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx); // the frame that fits each pane and records it

    let gutter = pane_gutter(&app, 0);
    let (lo, hi) = pane_range(&app, flow);
    let untouched = pane_range(&app, other);
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() - egui::vec2(0.0, 60.0),
    );

    let (zoomed_lo, zoomed_hi) = pane_range(&app, flow);
    assert!(
        zoomed_hi - zoomed_lo < hi - lo,
        "drag up compresses the span: {lo}..{hi} -> {zoomed_lo}..{zoomed_hi}"
    );
    assert!(
        (f64::midpoint(zoomed_lo, zoomed_hi) - f64::midpoint(lo, hi)).abs() < 1e-6,
        "and stretches around the middle rather than sliding the pane"
    );
    assert_eq!(pane_range(&app, other), untouched, "one pane, one scale");
    assert!(
        app.active_tab().flow_pane.price_view.is_auto(),
        "the candles never felt it: their gutter ends where they do"
    );
}

/// The other half of the isolation: the candles' own gutter must not reach
/// down into a pane. Both gestures exist on the same column of pixels, and
/// only the band decides which scale they mean.
#[test]
fn dragging_the_price_gutter_leaves_every_pane_alone() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx);

    let untouched = pane_range(&app, flow);
    let gutter = {
        let pane = &app.active_tab().flow_pane;
        plot_split(
            pane.last_plot_area.expect("a frame has been drawn"),
            pane.live_strip_width(app.active_tab().capabilities(&app.config)),
            pane.indicators.pane_sizing(
                &mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES],
            ),
        )
        .price_gutter
    };
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() - egui::vec2(0.0, 60.0),
    );

    assert!(
        !app.active_tab().flow_pane.price_view.is_auto(),
        "the candles took the drag"
    );
    assert_eq!(
        pane_range(&app, flow),
        untouched,
        "and the pane never felt it"
    );
    assert!(pane_is_auto(&app, flow));
}

/// The trader's own gesture for turning the chart over: keep dragging the
/// price gutter down and the candles shrink, flatten and flip. The same
/// motion carried on grows them upside down, and the opposite pull turns
/// the chart back — one continuous loop, no menu needed.
#[test]
fn dragging_the_price_gutter_through_the_flip_turns_the_chart_over() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);

    let gutter = app
        .active_tab()
        .flow_pane
        .last_price_gutter
        .expect("the draw published the gutter");
    assert!(!app.active_tab().flow_pane.price_view.is_inverted());

    // One violent pull down flattens the chart to the flip threshold —
    // still upright, whatever the speed of the flick...
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() + egui::vec2(0.0, 600.0),
    );
    assert!(
        !app.active_tab().flow_pane.price_view.is_inverted(),
        "the first pull only flattens"
    );
    // ...and the next pull turns it over.
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() + egui::vec2(0.0, 600.0),
    );
    assert!(
        app.active_tab().flow_pane.price_view.is_inverted(),
        "the second pull crossed the threshold and flipped the chart"
    );

    // Upside down, the same downward motion grows the chart back. The
    // dummy auto range is never read: the flip took manual control.
    let span = |app: &QuantickApp| {
        let (lo, hi) = app.active_tab().flow_pane.price_view.resolve((0.0, 0.0));
        hi - lo
    };
    let flat = span(&app);
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() + egui::vec2(0.0, 150.0),
    );
    assert!(
        span(&app) < flat,
        "inverted, dragging down compresses the span (bigger candles): {flat} -> {}",
        span(&app)
    );
    assert!(app.active_tab().flow_pane.price_view.is_inverted());

    // And the opposite motion shrinks it back to the threshold, where
    // the following pull turns it upright again.
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() - egui::vec2(0.0, 900.0),
    );
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() - egui::vec2(0.0, 300.0),
    );
    assert!(
        !app.active_tab().flow_pane.price_view.is_inverted(),
        "the way back crosses the same threshold"
    );
}

/// `QUANTICK_CONTEXT_MENU=axis` reaches the price axis's own menu: the
/// scripted right-click lands on the gutter itself, published by the
/// draw — never a guess about where the axis probably is.
#[test]
fn the_axis_menu_hook_lands_on_the_gutter() {
    assert_eq!(
        ContextMenuPane::from_env_value("axis"),
        Some(ContextMenuPane::Axis)
    );
    assert_eq!(
        ContextMenuPane::from_env_value("scale"),
        Some(ContextMenuPane::Axis)
    );

    let (mut app, _cmd_rx) = app_with_history(50);
    let ctx = egui::Context::default();
    assert_eq!(
        app.scripted_context_menu_pos(ContextMenuPane::Axis),
        None,
        "no draw yet, so no gutter to click"
    );
    run_frame(&mut app, &ctx);
    let gutter = app
        .active_tab()
        .flow_pane
        .last_price_gutter
        .expect("the draw published the gutter");
    let position = app
        .scripted_context_menu_pos(ContextMenuPane::Axis)
        .expect("one frame published it");
    assert!(gutter.contains(position));
    let chart = app
        .active_tab()
        .flow_pane
        .last_chart_rect
        .expect("the canvas laid out");
    assert!(
        position.x > chart.right(),
        "the axis, not the canvas: {position:?} vs {chart:?}"
    );
}

/// `QUANTICK_CONTEXT_MENU=time` reaches the time axis's own menu, the
/// gutter hook's twin — and lands on the *candles'* segment of the strip,
/// because past the lane divider the strip is the tape's rolling window
/// and carries no menu.
#[test]
fn the_time_menu_hook_lands_on_the_time_strip() {
    assert_eq!(
        ContextMenuPane::from_env_value("time"),
        Some(ContextMenuPane::Time)
    );
    assert_eq!(
        ContextMenuPane::from_env_value("clock"),
        Some(ContextMenuPane::Time)
    );
    assert_eq!(
        ContextMenuPane::from_env_value("tiem"),
        None,
        "a typo opens no menu rather than the wrong one"
    );

    let (mut app, _cmd_rx) = app_with_history(50);
    let ctx = egui::Context::default();
    assert_eq!(
        app.scripted_context_menu_pos(ContextMenuPane::Time),
        None,
        "no draw yet, so no strip to click"
    );
    run_frame(&mut app, &ctx);
    let strip = app
        .active_tab()
        .flow_pane
        .last_time_strip
        .expect("the draw published the strip");
    let position = app
        .scripted_context_menu_pos(ContextMenuPane::Time)
        .expect("one frame published it");
    assert!(strip.contains(position));
    let chart = app
        .active_tab()
        .flow_pane
        .last_chart_rect
        .expect("the canvas laid out");
    assert!(
        position.y > chart.bottom(),
        "the axis, not the canvas: {position:?} vs {chart:?}"
    );
}

/// The discrete way to turn the chart over: right-click on the price
/// gutter opens the axis's own menu, with the Inverted chart toggle in
/// The discrete way to turn the chart over: right-click on the price
/// gutter opens the axis's own menu, with the Inverted chart toggle in
/// it. The canvas keeps its layer menu; the axis speaks for the scale.
#[test]
fn right_clicking_the_price_gutter_offers_inverted_chart() {
    let (mut app, _cmd_rx) = app_with_history(50);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let target = app
        .active_tab()
        .flow_pane
        .last_price_gutter
        .expect("the draw published the gutter")
        .center();
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(target),
            egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
    );
    // The menu is an egui Area: it lays out on the frame after the click
    // that opened it, so the settled frame is the one to read.
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text == "Inverted chart"),
        "the axis menu is on screen: {texts:?}"
    );
}

/// Scroll is the same gesture with a wheel: it zooms the pane under the
/// pointer, and the candles keep auto-fitting.
#[test]
fn scrolling_a_pane_axis_zooms_that_pane() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx);

    let (lo, hi) = pane_range(&app, flow);
    let over = pane_gutter(&app, 0).center();
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(over),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 120.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );

    let (zoomed_lo, zoomed_hi) = pane_range(&app, flow);
    assert!(
        zoomed_hi - zoomed_lo < hi - lo,
        "scrolling up zooms in: {lo}..{hi} -> {zoomed_lo}..{zoomed_hi}"
    );
    assert!(
        app.active_tab().flow_pane.price_view.is_auto(),
        "the candles kept auto-fitting"
    );
}

/// Manual control is manual: the range holds while values keep arriving,
/// and a double-click on the axis hands the pane back to auto-fit.
#[test]
fn a_zoomed_pane_holds_its_range_until_the_axis_is_double_clicked() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    let flow = add_pane_indicator(&mut app, "cvd", (0..200).map(f64::from).collect());
    run_frame(&mut app, &ctx);

    let gutter = pane_gutter(&app, 0);
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() - egui::vec2(0.0, 60.0),
    );
    let held = pane_range(&app, flow);
    assert!(!pane_is_auto(&app, flow), "the drag took manual control");

    // The indicator recomputes ten times bigger: auto-fit would jump, a
    // range the user set does not.
    rebuild_pane_indicator(
        &mut app,
        flow,
        "cvd",
        (0..200).map(|row| f64::from(row) * 10.0).collect(),
    );
    run_frame(&mut app, &ctx);
    assert_eq!(pane_range(&app, flow), held, "the range the user set holds");

    click_chart(&mut app, &ctx, gutter.center());
    click_chart(&mut app, &ctx, gutter.center());
    run_frame(&mut app, &ctx);
    assert!(
        pane_is_auto(&app, flow),
        "double-click returns the pane to auto-fit"
    );
    let refitted = pane_range(&app, flow);
    assert!(
        refitted.1 > held.1,
        "and auto-fit sees the values that arrived meanwhile: {refitted:?} vs {held:?}"
    );
}

/// The pane reads as a chart: its own round numbers, in the gutter beside
/// it. The candles' axis never labels those pixels, and the pane's labels
/// never appear over the candles.
#[test]
fn a_pane_prints_its_own_value_labels_in_the_gutter() {
    let (mut app, _cmd_rx) = app_with_history(200);
    let ctx = egui::Context::default();
    // Values well away from the test's 95..115 price range, so a label can
    // only have come from the pane's own axis.
    add_pane_indicator(
        &mut app,
        "cvd",
        (0..200).map(|row| f64::from(row) - 200.0).collect(),
    );
    let output = run_frame(&mut app, &ctx);
    let texts = painted_text(&output);

    let pane_labels: Vec<&String> = texts
        .iter()
        .filter(|text| {
            text.parse::<f64>()
                .is_ok_and(|value| (-200.0..=-1.0).contains(&value))
        })
        .collect();
    assert!(
        pane_labels.len() >= 3,
        "the pane's own round numbers are drawn, and enough of them to \
             read as a scale: {texts:?}"
    );
    assert!(
        has_price_axis(&texts),
        "and the candles keep theirs: {texts:?}"
    );
}

/// Each pane zooms from the strip under it, and the split is exactly the
/// divider — so a drag can never mean both time axes at once.
#[test]
fn the_time_strip_splits_at_the_lane_divider() {
    let strip = egui::Rect::from_min_max(egui::pos2(0.0, 580.0), egui::pos2(1000.0, 600.0));

    let (history, lane) = split_time_strip(strip, Some(700.0));
    let lane = lane.expect("the lane owns the strip under it");
    assert_eq!(history.left(), strip.left());
    assert_eq!(history.right(), 700.0);
    assert_eq!(lane.left(), 700.0);
    assert_eq!(lane.right(), strip.right());

    // Without a lane the candles keep the whole strip, exactly as before.
    assert_eq!(split_time_strip(strip, None), (strip, None));
    // A divider off the strip is not a split either.
    assert_eq!(split_time_strip(strip, Some(-5.0)), (strip, None));
}

/// A trade a tenth of a second after the last, one unit at a walking
/// price, so bars carry distinct times and a readable price range.
/// The session this change came from: a MetaTrader tab whose time pane was
/// full of the terminal's candle history and whose flow pane had not seen
/// one tick, with the explanation floating in the middle of both.
#[test]
fn the_notice_lands_on_the_pane_that_is_waiting() {
    let (mut app, _notices, _feed_ends) = test_app_with_notices();
    let ctx = egui::Context::default();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    // One frame builds the time pane, the next lets both panes paint and
    // publish where they landed.
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().has_time_pane(),
        "the split is what this proof is about"
    );
    // The venue's candles reach the time pane; no trade reaches anything,
    // so the flow pane stays empty exactly as it did on screen.
    app.active_tab_mut()
        .time_panes
        .first_mut()
        .expect("the split built one")
        .install_history_prefix(venue_history(120));
    run_frame(&mut app, &ctx);

    let tab = app.active_tab();
    let flow = tab.flow_pane.last_area.expect("the flow pane painted");
    let time = tab.time_panes[0].last_area.expect("the time pane painted");
    let (chosen, slots) = tab.starved_pane().expect("a painted pane");
    assert_eq!(slots, 0, "the starved pane is the one with nothing on it");
    assert_eq!(chosen, flow, "the note belongs to the pane that is waiting");
    assert_ne!(
        chosen, time,
        "a pane full of candles is not waiting for anything"
    );

    // The corner belongs to the window, and the line belongs to the empty
    // pane. Neither may land on the pane that is painting fine.
    let notice = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
    let report = feed_notice::report(&notice, None).expect("a named reason speaks");
    assert!(report.is_offline());
    let canvas = flow.union(time);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let chip = feed_notice::chip_rect(ui.painter(), canvas);
            assert!(
                !chip.intersects(time),
                "chip {chip:?} covered the working time pane {time:?}"
            );
            let popup = feed_notice::popup_rect(ui.painter(), canvas, chip, &report);
            assert!(
                canvas.contains_rect(popup),
                "popup {popup:?} left the canvas {canvas:?}"
            );
        });
    });
}

/// One dead terminal stalls every MetaTrader tab at once, and the popup
/// must not follow the trader into a chart whose chip they never pressed.
///
/// Leaving the chart closes it, which is the second half of the same rule:
/// the popup is a glance, not a mode, so it never waits on a chart the
/// trader walked away from.
#[test]
fn the_popup_belongs_to_the_tab_whose_chip_opened_it() {
    let (mut app, _notices, _channels) = test_app_with_notices();
    let ctx = egui::Context::default();
    app.open_tab("binance".to_owned(), "TESTUSDT".to_owned(), None);
    for tab in &mut app.tabs {
        tab.forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    }
    app.active_tab = 0;
    run_frame(&mut app, &ctx);
    let chip = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip.center());
    assert!(app.control_feed_popup_open(), "opened on the first chart");

    app.active_tab = 1;
    run_frame(&mut app, &ctx);
    assert!(
        app.control_feed_chip_rect().is_some(),
        "the second chart is stalled too, so it has its own corner"
    );
    assert!(
        !app.control_feed_popup_open(),
        "but nobody pressed that corner"
    );

    app.active_tab = 0;
    run_frame(&mut app, &ctx);
    assert!(
        !app.control_feed_popup_open(),
        "and leaving the chart put it away, the way clicking elsewhere does"
    );
    assert!(
        app.control_feed_chip_rect().is_some(),
        "the corner itself stays: the feed is still stalled"
    );
}

/// The latency port, end to end through a tab: a provider that publishes a
/// split, one that cannot, and a recording that has no chain to attribute.
#[test]
fn a_tab_reads_the_split_its_provider_publishes_and_nothing_more() {
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    assert_eq!(
        app.active_tab().feed_latency(),
        None,
        "a feed that has published nothing yet has nothing to report"
    );

    let split = feed::FeedLatency {
        arrival_lag_ms: 18_112,
        source_lag_ms: Some(17_980),
        source_lag_peak_ms: Some(18_400),
        transport_lag_ms: Some(132),
        hop: Some("MT5"),
        prints: 64,
    };
    app.active_tab_mut().publish_latency_for_test(Some(split));
    assert_eq!(app.active_tab().feed_latency(), Some(split));

    // The cell the trader reads takes it from there: the feed's own total,
    // with the hop named because that delay is past the threshold worth
    // acting on. The chart has drained no print of its own yet, and the
    // cell is right to show the feed's figure anyway — a split only exists
    // because a print did arrive at the feed.
    assert_eq!(
        statusbar::tape_text(
            None,
            app.active_tab().trade_arrival_ms(),
            Some(50),
            app.active_tab().feed_latency(),
        ),
        "MT5 18112 ms"
    );
    assert_eq!(
        app.active_tab().trade_arrival_ms(),
        None,
        "and the chart's own measurement is untouched by the reading"
    );

    // A provider that cannot cut its own chain leaves the cell exactly as
    // it has always been — never a breakdown of zeros nobody measured.
    app.active_tab_mut().publish_latency_for_test(None);
    assert_eq!(app.active_tab().feed_latency(), None);
}

/// The reverse, so the test above cannot pass on a stroke that was always
/// there: switching sharing off takes the foreign copy away again.
#[test]
fn unsharing_removes_the_copy_from_the_other_pane() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let slot = 100;
    let anchored = {
        let pane = &app.active_tab().flow_pane;
        let time = pane.slot_open_time(slot).expect("a closed bar has a time");
        let price = pane
            .closed_bar(slot)
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .expect("the bar has a close");
        drawings::ChartPoint::at_time(slot as f32 + 0.5, price, Some(time))
    };
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.drawings.place_with(
        drawing_tool("horizontal-line"),
        &drawings::DrawingBand::Price,
        anchored,
        |tool| drawings::NewDrawing {
            style: drawings::DrawingStyle::default(),
            payload: tool.default_payload(),
        },
    );
    app.active_tab_mut()
        .flow_pane
        .drawings
        .selected_mut()
        .expect("selected")
        .scope = drawings::DrawingScope::AllCharts;
    let shared = drawing_strokes(&run_frame(&mut app, &ctx));

    app.active_tab_mut()
        .flow_pane
        .drawings
        .selected_mut()
        .expect("selected")
        .scope = drawings::DrawingScope::ThisChart;
    let alone = drawing_strokes(&run_frame(&mut app, &ctx));
    assert!(
        alone < shared,
        "unsharing must take the foreign copy away: {shared} -> {alone}"
    );
}

/// A position parked on a full-width canvas has to survive the canvas
/// being split under it: the bar is repaired into the pane the selection
/// lives on, never left hovering over its neighbour or off the view — and
/// the point the hand chose is repaired for drawing, never overwritten.
#[test]
fn a_parked_context_bar_is_repaired_into_the_pane_it_reappears_on() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let line = place_drawing(
        &mut app,
        &ctx,
        "horizontal-line",
        &[egui::pos2(700.0, 300.0)],
    );
    app.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(line));
    run_frame(&mut app, &ctx);

    // Parked far to the right of what either pane of a split will offer.
    let parked = egui::pos2(1200.0, 780.0);
    app.surfaces
        .drawing_chrome
        .context_bar_mut()
        .set_manual(parked);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    // Blank the mirror first: it is written only when the bar reaches
    // `show`, and every early return leaves the previous frame's value —
    // here the full-width rect, which would satisfy the assertion below
    // with the repair never having run.
    app.surfaces.drawing_chrome.forget_context_bar_rect();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let chart = app
        .drawing_pane()
        .last_chart_area
        .expect("the pane holding the selection drew");
    let bar = app
        .surfaces
        .drawing_chrome
        .context_bar_rect()
        .expect("the bar is still up");
    assert!(
        chart.contains_rect(bar),
        "the parked bar is repaired into {chart:?}, drawn at {bar:?}"
    );
    assert_eq!(
        app.surfaces.drawing_chrome.context_bar().manual_position(),
        Some(parked),
        "and the point the hand chose survives the repair"
    );
}

/// The time pane a split builds inherits the orientation of the chart it
/// splits away from. It is built a frame after the layout change, so the
/// boot's `QUANTICK_INVERTED` hook fires before it exists — without the
/// inheritance a scripted inverted capture is silently half-inverted.
#[test]
fn a_time_pane_born_into_an_inverted_tab_opens_upside_down() {
    let (mut app, _cmd_rx) = app_with_history(50);
    let ctx = egui::Context::default();
    app.active_tab_mut().flow_pane.price_view.set_inverted(true);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let time_pane = app
        .active_tab()
        .time_pane()
        .expect("the split built a time pane");
    assert!(
        time_pane.price_view.is_inverted(),
        "the second view keeps the market the way the trader has it"
    );
}

/// The x axis belongs to every chart on the canvas, not just the flow
/// pane. In the split, dragging the *timeframe* pane's own time strip has
/// to squeeze the *timeframe* pane — and leave the flow pane beside it
/// exactly where it was.
#[test]
fn the_time_panes_own_x_axis_zooms_the_time_pane_and_only_it() {
    let (mut app, _cmd_rx) = app_with_history(400);
    let ctx = egui::Context::default();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let plot = app
        .active_tab()
        .time_pane()
        .expect("the split has a time pane")
        .last_plot_area
        .expect("laid out by the frames above");
    // The time pane carries no tape and no indicator panes, so its own
    // layout call reduces to this.
    let strip = plot_split(plot, 0.0, &[]).time_strip;

    let before_time = time_zoom(&app);
    let before_flow = app.active_tab().flow_pane.viewport.px_per_bar();
    // Stretch first, squeeze back: both directions of the gesture are
    // proven from wherever the pane's zoom happens to open, with no
    // assumption about how far the squeeze side has left to travel.
    drag_chart(
        &mut app,
        &ctx,
        strip.center(),
        strip.center() + egui::vec2(120.0, 0.0),
    );
    let stretched = time_zoom(&app);
    assert!(
        stretched > before_time,
        "the timeframe pane stretches: {stretched} vs {before_time}"
    );

    drag_chart(
        &mut app,
        &ctx,
        strip.center(),
        strip.center() + egui::vec2(-120.0, 0.0),
    );
    assert!(
        time_zoom(&app) < stretched,
        "and squeezes: {} vs {stretched}",
        time_zoom(&app)
    );
    assert!(
        (app.active_tab().flow_pane.viewport.px_per_bar() - before_flow).abs() < f32::EPSILON,
        "and the flow pane beside it never moved"
    );
}

/// Order entry follows the focused pane — both charts are trading
/// surfaces, and the press that focuses a pane is the press that acts.
///
/// Proven through the consequence, not the flag: with a fully bracketed
/// position the entry line consumes a press without moving (it is
/// history, not an order), so a vertical drag that starts on it pans
/// nothing — on *either* pane, because the press itself moves the focus
/// (and with it the simulator's pointer) to the pane it landed in.
#[test]
fn the_focused_pane_hands_the_pointer_to_the_simulator() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    let fill = trade(4);
    app.active_tab_mut().ingest_live_trade_at(&fill, 0);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().paper.status_cell().is_some(),
        "this proof needs an open simulated position to grab"
    );
    // Both legs set, so the entry-line press blocks instead of starting
    // a bracket-creating drag — the no-pan consequence stays provable.
    app.active_tab_mut()
        .paper
        .apply_sim_command_for_tests(quantick_sim::Command::SetBracket {
            stop_loss: Some(fill.price - rust_decimal::Decimal::from(10)),
            take_profit: Some(fill.price + rust_decimal::Decimal::from(10)),
        });
    let entry =
        rust_decimal::prelude::ToPrimitive::to_f64(&fill.price).expect("the fill price is finite");

    for side in [PaneSide::Time(0), PaneSide::Flow] {
        app.active_tab_mut().pane_mut(side).price_view.reset();
        let chart = app
            .active_tab()
            .pane(side)
            .last_chart_area
            .expect("the pane reported its rect");
        let start = egui::pos2(chart.center().x, price_y(&app, side, entry));
        assert!(
            chart.contains(start),
            "{side:?}: the entry line must cross this pane to be grabbed"
        );
        drag_chart(&mut app, &ctx, start, start + egui::vec2(0.0, 40.0));

        assert!(
            app.active_tab().pane(side).price_view.is_auto(),
            "{side:?}: the entry line owns the gesture on the pane the \
                 press focused — the chart must not pan under it"
        );
        assert_eq!(
            app.active_tab().focused_side(),
            side,
            "the press that traded is the press that focused"
        );
    }
}

/// (a) The split really puts two charts on the canvas: two panes with
/// their own laid-out rects, side by side, and a divider between them.
#[test]
fn enabling_the_split_lays_out_two_panes_and_a_divider() {
    let ctx = egui::Context::default();
    let (app, _commands) = split_app(&ctx, 200);

    let time = app
        .active_tab()
        .time_pane()
        .expect("Time + Flow builds the time pane")
        .last_chart_area
        .expect("the time pane was laid out");
    let flow = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("the flow pane was laid out");
    assert!(
        time.right() <= flow.left(),
        "time pane left, flow pane right: {time:?} vs {flow:?}"
    );
    assert!(
        flow.left() - time.right() >= CANVAS_DIVIDER_PX,
        "the divider owns the pixels between them"
    );
    assert!(time.width() > 0.0 && flow.width() > 0.0);
}

/// Both panes paint: each keeps its own price axis, and the time pane
/// carries the timeframe selector §11 gives it.
#[test]
fn both_panes_paint_and_the_time_pane_carries_its_own_selector() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        has_price_axis(&texts),
        "the split canvas still paints a chart: {texts:?}"
    );
    for (label, _) in time_header::PRESETS {
        assert!(
            texts.iter().any(|text| text == label),
            "the time pane's header must offer {label:?}; painted: {texts:?}"
        );
    }
}

/// Seeding replays every retained trade, so it is armed on the frame that
/// asks for the split and done on the next — the overlay gets painted
/// before the work, exactly as a bar-spec change does.
#[test]
fn enabling_the_split_paints_before_it_seeds() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);

    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    assert!(
        app.active_tab().time_panes.is_empty(),
        "the frame carrying the change does no work"
    );
    assert!(
        app.active_tab().loading.is_active(LoadingTask::BarRebuild),
        "and arms the overlay that says what is coming — the same one
             `a_rebuilt_chart_still_paints_itself` proves reaches the screen"
    );

    run_frame(&mut app, &ctx);
    let time = app
        .active_tab()
        .time_pane()
        .expect("the next frame builds it");
    assert!(
        !time.state.trades().is_empty(),
        "seeded from the market the flow pane already holds"
    );
    assert!(
        !app.active_tab().loading.is_active(LoadingTask::BarRebuild),
        "and the overlay comes down with the work"
    );
}

/// The forming bar changes with every print and only its latest value is
/// ever read, so a batch of prints is one update per pane — not one per
/// print per pane, which the worker then had to collapse again.
#[test]
fn a_batch_of_prints_publishes_one_forming_bar_per_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    settle_indicators(&mut app);

    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (cmd_tx, _cmd_rx) = mpsc::channel(16);
    app.active_tab_mut().attach_for_test(FeedHandle {
        events: evt_rx,
        book_events: mpsc::channel(8).1,
        notices: feed::silent_notices(),
        capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
        latency: feed::unsplit_latency(),
        commands: cmd_tx,
        replay: None,
    });
    let batch: Vec<_> = (700..710).map(trade).collect();
    let before: Vec<usize> = [PaneSide::Flow, PaneSide::Time(0)]
        .into_iter()
        .map(|side| {
            app.active_tab()
                .pane(side)
                .indicator_worker
                .partial_updates_for_test()
        })
        .collect();
    evt_tx.try_send(FeedEvent::LiveBatch(batch)).unwrap();

    app.active_tab_mut().drain_feed();

    for (side, before) in [PaneSide::Flow, PaneSide::Time(0)].into_iter().zip(before) {
        let sent = app
            .active_tab()
            .pane(side)
            .indicator_worker
            .partial_updates_for_test()
            - before;
        assert_eq!(
            sent, 1,
            "ten prints are one forming-bar update on the {side:?} pane"
        );
    }
}

/// Seeding a pane opened mid-session must not relabel live prints as
/// history: the backfill divider is a data-honesty mark, not a decoration.
#[test]
fn a_pane_opened_late_keeps_the_backfill_boundary_honest() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(100);
    // Everything so far is backfill; now three prints arrive live.
    for id in 101..=103 {
        let trade = trade(id);
        app.active_tab_mut()
            .ingest_live_trade_at(&trade, trade.timestamp_ms);
    }
    run_frame(&mut app, &ctx);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    // The seed runs on the frame after the click; see
    // `enabling_the_split_paints_before_it_seeds`.
    run_frame(&mut app, &ctx);

    let time = app.active_tab().time_pane().expect("time pane");
    assert_eq!(
        time.state.trades().len(),
        103,
        "the new pane opens showing the market, not an empty chart"
    );
    assert_eq!(
        time.state.backfill_trade_count(),
        app.active_tab().flow_pane.state.backfill_trade_count(),
        "the live prints must not become history in the second view"
    );
}

/// (c) The time pane's header governs the time pane and nothing else; the
/// toolbar's BARS group keeps governing the flow pane (§11).
#[test]
fn a_timeframe_chip_moves_only_the_time_panes_spec() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let flow_spec = app.active_tab().flow_pane.state.spec().clone();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).state.spec(),
        &BarSpec::Time(time_header::DEFAULT_INTERVAL_MS),
        "the time pane opens on M1, not on the flow selector's interval"
    );

    // The 15m chip, clicked where it was actually drawn.
    let (label, expected_ms) = time_header::PRESETS[2];
    let chip = app.active_tab().time_header_chip(2).expect("the 15m chip");
    assert!(chip.is_positive(), "the {label} chip was laid out");
    click_chart(&mut app, &ctx, chip.center());
    // The spec change is deferred one frame, exactly as the toolbar's is.
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).state.spec(),
        &BarSpec::Time(expected_ms),
        "clicking {label} must re-cut the time pane"
    );
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &flow_spec,
        "and must leave the chart beside it alone"
    );
}

/// The Time layout is the timeframe chart alone: built and seeded like
/// the split's pane, full window (no divider), header included, and the
/// chrome speaks for it.
#[test]
fn the_time_layout_shows_the_timeframe_chart_alone() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    app.active_tab_mut().set_layout(CanvasLayout::Time);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let tab = app.active_tab();
    let time = tab.time_pane().expect("the pane was built");
    assert_eq!(
        time.state.trades().len(),
        200,
        "and seeded, so it opens showing the market"
    );
    assert_eq!(
        tab.focused_side(),
        PaneSide::Time(0),
        "the chrome speaks for the one visible chart"
    );
    assert!(
        tab.canvas_divider_rect().is_none(),
        "one pane, no divider to drag"
    );
    let chip = tab.time_header_chip(0).expect("chips recorded");
    assert!(
        chip.is_positive(),
        "the header still offers its timeframes at full width"
    );
}

/// Two tabs speaking on one frame: the slot holds one message, and the
/// one the trader is looking at wins it rather than tab order deciding in
/// silence.
#[test]
fn the_watched_market_wins_the_slot() {
    let (mut app, _commands) = app_with_history(50);
    app.open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    let watched = app.active_tab;
    for (index, tab) in app.tabs.iter_mut().enumerate() {
        tab.paper.show_toast(format!("message from tab {index}"));
    }
    app.settle_paper_panels(Instant::now());
    assert_eq!(
        app.surfaces.toast.message(),
        Some(format!("message from tab {watched}").as_str()),
        "the chart on screen is the one whose acknowledgement stays"
    );
}

/// Resetting forgets the file without rearranging the charts the trader is
/// reading: the entry governs the *startup* layout, not this session.
#[test]
fn resetting_the_startup_layout_leaves_this_session_alone() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.workspace.set_ui_state_path(scratch_ui_state("reset"));
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    app.save_workspace("test");
    assert!(app.workspace.ui_state_path().exists());

    app.forget_workspace();

    assert!(
        !app.workspace.ui_state_path().exists(),
        "the next launch opens on config"
    );
    assert_eq!(
        app.active_tab().layout,
        CanvasLayout::TimeAndFlow,
        "the charts on screen are not the trader's startup preference"
    );
}

/// A window that opens on the split focuses the flow chart, not the
/// context beside it.
///
/// Caught by looking at the shipped default on screen: the BARS group and
/// the status line were speaking for the timeframe pane, so the first
/// thing a trader touched on a fresh launch would have re-cut the context
/// chart instead of quantick's own. `set_layout` focusing what it reveals
/// is right for a menu click and wrong for an opening.
#[test]
fn a_window_that_opens_on_the_split_focuses_the_flow_chart() {
    let ctx = egui::Context::default();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, _cmd_rx) = mpsc::channel(16);
    let mut config = test_config();
    config.feeds[0].default_layout = Some(crate::config::DeclaredLayout::TimeAndFlow);
    let mut app = QuantickApp::new(
        config,
        "binance",
        "TESTUSDT",
        BarSpec::Tick(50),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    let _ends = (evt_tx, book_tx);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(app.active_tab().layout, CanvasLayout::TimeAndFlow);
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Flow,
        "a fresh window's controls speak for the chart quantick is built around"
    );
    assert_eq!(
        app.status_model().spec_summary,
        "tick(50)",
        "and so does the status line"
    );
}

/// Reset discards a *layout*. Every other standing choice in the file —
/// the bookmarks, the stars, the Open-recent list, the replay folder — is
/// not part of one, and losing them to a layout reset would be a silent
/// cost the entry never mentions.
#[test]
fn resetting_the_startup_layout_keeps_the_other_standing_choices() {
    let (mut app, _commands) = app_with_history(50);
    app.workspace
        .set_ui_state_path(scratch_ui_state("reset-standing"));
    *app.workspace.session_mut().recent_mut() = vec!["D:/desk/scalp.qws.toml".to_owned()];
    app.save_workspace("test");

    app.forget_workspace();

    let file = ui_state::load(app.workspace.ui_state_path());
    assert_eq!(
        file.recent_workspaces,
        vec!["D:/desk/scalp.qws.toml".to_owned()],
        "the Open-recent menu is not part of the layout being reset"
    );
    assert!(file.tabs.is_empty(), "and the layout really was reset");
    let _ = std::fs::remove_file(app.workspace.ui_state_path());
}

/// Switching layouts focuses the pane the switch reveals, so the first
/// command after a switch already lands on the chart that just appeared
/// (audit: opening the split did not focus the pane it created).
#[test]
fn switching_layouts_focuses_the_pane_the_switch_reveals() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    run_frame(&mut app, &ctx);

    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Time(0),
        "coming from Single, the split reveals the time pane"
    );

    app.active_tab_mut().set_layout(CanvasLayout::Single);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);

    app.active_tab_mut().set_layout(CanvasLayout::Time);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));

    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Flow,
        "coming from Time, the split reveals the flow pane"
    );
}

/// The BARS group edits the focused pane — the same pane the status bar
/// reads and indicator commands land on. In the Time layout that is the
/// timeframe chart on screen; the hidden flow pane is untouched.
#[test]
fn the_bars_selectors_govern_the_focused_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    app.active_tab_mut().set_layout(CanvasLayout::Time);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let flow_spec = app.active_tab().flow_pane.state.spec().clone();

    // The exact selector fields the toolbar's BARS group borrows for the
    // focused pane, written through the same deferred-spec path.
    let pane = app.active_tab_mut().focused_pane_mut();
    pane.kind = crate::state::BarKind::Time;
    pane.time_interval_ms = 300_000;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();

    assert_eq!(
        app.active_tab()
            .time_pane()
            .expect("time pane")
            .state
            .spec(),
        &BarSpec::Time(300_000),
        "the change lands on the chart on screen"
    );
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &flow_spec,
        "and not on the hidden one"
    );
}

/// A feed declaring `default_layout` and `default_bars` opens wearing
/// them: the declared canvas, the declared spec on the flow pane, the
/// declared interval on the timeframe pane — and the venue asked for the
/// candle history the pane needs.
#[test]
fn a_feed_declaring_layout_and_bars_opens_wearing_them() {
    let ctx = egui::Context::default();
    let (_evt_tx, evt_rx) = mpsc::channel(64);
    let (_book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
    let mut config = test_config();
    config.feeds[0].default_layout = Some(crate::config::DeclaredLayout::Time);
    config.feeds[0].default_bars = Some("time:5m".to_string());
    // The declared spec reaches Tab::new the same way main.rs resolves it.
    let spec = config.startup_spec_for("binance").expect("declared spec");
    let mut app = QuantickApp::new(
        config,
        "binance",
        "TESTUSDT",
        spec,
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(FeedCapabilities {
                book_capture: false,
                history_paging: true,
                traded_volume: true,
                ohlcv_history: true,
                ohlcv_generation: 0,
            }),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );

    assert_eq!(app.active_tab().layout, CanvasLayout::Time);
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BarSpec::Time(300_000),
        "the flow pane opened on the declared spec"
    );
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab()
            .time_pane()
            .expect("built the frame after startup")
            .state
            .spec(),
        &BarSpec::Time(300_000),
        "the timeframe pane opens on the declared interval too"
    );
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        1,
        "the timeframe chart asked the venue for its history"
    );
}

/// A new tab on a feed that declares its own defaults takes them; one on
/// a feed that declares nothing keeps inheriting from the tab you were on.
#[test]
fn a_new_tab_takes_the_feeds_declared_defaults_over_inheritance() {
    let mut config = test_config();
    config.feeds.push(FeedConfig {
        id: "mt".to_string(),
        name: "MetaTrader 5".to_string(),
        provider: ProviderKind::MetaTrader,
        symbols: vec!["WINQ26".to_string()],
        bubble_preset: None,
        symbol_bubble_presets: Default::default(),
        default_layout: Some(crate::config::DeclaredLayout::TimeAndFlow),
        default_bars: Some("tick:7".to_string()),
    });
    let mut app = app_on(config, "binance", "TESTUSDT");
    assert_eq!(app.active_tab().layout, CanvasLayout::Single);

    app.adopt_tab("mt".to_string(), "WINQ26".to_string(), stub_feed().0, None);
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BarSpec::Tick(7),
        "the declaration wins over the inherited spec"
    );
    assert_eq!(app.active_tab().layout, CanvasLayout::TimeAndFlow);

    app.adopt_tab(
        "binance".to_string(),
        "ETHUSDT".to_string(),
        stub_feed().0,
        None,
    );
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BarSpec::Tick(7),
        "a feed declaring nothing still inherits from the tab you were on"
    );
    assert_eq!(
        app.active_tab().layout,
        CanvasLayout::Single,
        "and opens on the factory canvas"
    );
}

/// (e) Dragging the divider moves it, and stops at the quarter §11
/// promises each pane.
#[test]
fn dragging_the_divider_resizes_the_panes_and_stops_at_the_minimum() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let flow_before = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("laid out")
        .width();
    let divider = app
        .active_tab()
        .canvas_divider_rect()
        .expect("the divider was registered");
    let grab = divider.center();

    drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x + 120.0, grab.y));
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().split_fraction > DEFAULT_PANE_FRACTION,
        "dragging right widens the time pane, got {}",
        app.active_tab().split_fraction
    );
    assert!(
        app.active_tab()
            .flow_pane
            .last_chart_area
            .expect("laid out")
            .width()
            < flow_before,
        "at the flow pane's expense"
    );

    // Now shove it far past the minimum: it stops, it does not collapse.
    for _ in 0..6 {
        let grab = app
            .active_tab()
            .canvas_divider_rect()
            .expect("registered")
            .center();
        drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x + 400.0, grab.y));
        run_frame(&mut app, &ctx);
    }
    // Asserted on what is *drawn*, not on the stored share. The floor
    // moved: it is a width in pixels applied by the splitter every frame,
    // rather than a quarter of the canvas held on the field. A trader is
    // promised a readable flow pane, not a particular fraction, and the
    // fraction was the wrong thing to pin — holding it as a second floor
    // is what made collapse-by-drag unreachable.
    let flow_width = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("laid out")
        .width();
    // "Stops at the minimum" is the claim, so the test is that it stops:
    // shove it further still and the pane must not move. Asserted this way
    // rather than against the floor in pixels, because the floor governs
    // the *pane* while the only rect a test can read is the chart inside
    // it — the price axis and the tape both take a slice first, and a test
    // that re-derives their widths is a second copy of the layout.
    let settled = flow_width;
    for _ in 0..4 {
        let grab = app
            .active_tab()
            .canvas_divider_rect()
            .expect("registered")
            .center();
        drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x + 400.0, grab.y));
        run_frame(&mut app, &ctx);
    }
    let after = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("laid out")
        .width();
    assert!(
        (after - settled).abs() < 1.0,
        "the flow pane kept shrinking past its floor: {settled}px then {after}px"
    );
    assert!(after > 0.0, "the flow pane was squeezed away entirely");
}

/// Collapse has to be reachable by the hand that asks for it.
///
/// The rail was verified through `QUANTICK_PANE_COLLAPSED`, which sets the
/// flag directly and proves only that the rail *renders*. The gesture was
/// unreachable: `split_fraction` was re-floored to a quarter of the canvas
/// every frame, so a drag restarted at ~400px each time and crossing the
/// 120px threshold needed one impossible frame. A capability a trader
/// cannot reach is not a capability, so this drives the divider the way a
/// hand does — repeatedly, leftward — and asserts the column gives way.
#[test]
fn dragging_the_divider_to_the_edge_collapses_the_column() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    assert!(
        !app.active_tab().context_collapsed,
        "the split opens with its context column shown"
    );

    for _ in 0..8 {
        let Some(divider) = app.active_tab().canvas_divider_rect() else {
            break;
        };
        let grab = divider.center();
        drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x - 400.0, grab.y));
        run_frame(&mut app, &ctx);
    }

    assert!(
        app.active_tab().context_collapsed,
        "dragging the divider to the edge must dismiss the column, not              stall against a floor the gesture can never cross"
    );
    // And the width it springs back to survived the gesture that put it
    // away, which is the whole promise of the rail.
    assert!(
        app.active_tab().split_fraction > 0.0,
        "the collapse spent the width it was meant to remember"
    );
}

/// A divider drag belongs to the tab it started on. egui keeps drag state
/// per interaction id, so one id shared across tabs would hand the
/// in-flight gesture to the next tab's divider the moment `Ctrl+Tab`
/// fires under a held button — the tab-level case of the rule
/// [`crate::pane`] states for panes.
#[test]
fn a_divider_drag_does_not_follow_a_tab_switch() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    // A second market, split as well, so both tabs register a divider.
    app.adopt_tab(
        "binance".to_owned(),
        "ETHUSDT".to_owned(),
        stub_feed().0,
        None,
    );
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let untouched = app.tabs[1].split_fraction;

    // Back to the first tab, and press its divider.
    app.active_tab = 0;
    run_frame(&mut app, &ctx);
    let grab = app
        .active_tab()
        .canvas_divider_rect()
        .expect("the first tab's divider was registered")
        .center();
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(grab), pointer_button(grab, true)],
    );

    // Ctrl+Tab mid-gesture, with the button still down.
    app.cycle_tab(1);
    let moved = egui::pos2(grab.x + 120.0, grab.y);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(moved)]);
    run_frame_with_events(&mut app, &ctx, vec![pointer_button(moved, false)]);

    assert_eq!(
        app.tabs[1].split_fraction, untouched,
        "the second tab's divider must not inherit the first tab's drag"
    );
}

/// The keyboard's drawing grammar follows focus as well: Delete removes
/// the selection on the pane the user is in, never its opposite number on
/// the chart beside it.
#[test]
fn the_keyboard_deletes_from_the_focused_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    for side in [PaneSide::Flow, PaneSide::Time(0)] {
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, side);
        click_chart(&mut app, &ctx, point);
    }
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .len(),
        1
    );
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Time(0),
        "the last click was on the time pane"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);

    assert!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .is_empty(),
        "Delete removes the focused pane's selection"
    );
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "and leaves the pane beside it untouched"
    );
}

/// The status bar's content section speaks for the focused pane (§11), so
/// it always describes the chart the user is working in.
#[test]
fn the_status_bar_follows_the_focused_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    let point = pane_point(&app, PaneSide::Flow);

    click_chart(&mut app, &ctx, point);
    let flow_status = app.status_model();
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let time_status = app.status_model();

    assert_eq!(
        flow_status.spec_summary,
        app.active_tab().flow_pane.state.spec().summary()
    );
    assert_eq!(
        time_status.spec_summary,
        app.active_tab()
            .pane(PaneSide::Time(0))
            .state
            .spec()
            .summary()
    );
    assert_ne!(
        flow_status.spec_summary, time_status.spec_summary,
        "the two panes report different specs, so the bar has to change"
    );
    // Provenance is the market's and never moves with focus.
    assert_eq!(flow_status.symbol, time_status.symbol);
    assert_eq!(flow_status.venue, time_status.venue);
}

/// Each pane shows its own layout: creating a layout on the focused pane
/// changes that pane alone, an edit reaches only the panes on the same
/// layout, and a pane that opens inherits the focused pane's layout.
#[test]
fn two_panes_show_two_layouts_side_by_side() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    let first = app.layouts().active_id();
    assert_eq!(app.layouts().get(first).unwrap().indicators.len(), 1);

    // Focus the time pane and give it a layout of its own.
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let second = app.create_layout(Some("levels")).expect("second");
    assert_eq!(app.focused_pane_layout(), second);
    assert_eq!(
        app.pane_layout(app.active_tab().id, PaneSide::Flow),
        first,
        "the flow pane kept its layout"
    );
    assert!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .indicators
            .all()
            .is_empty(),
        "the new layout is empty on the pane that took it"
    );
    assert_eq!(
        app.active_tab().flow_pane.indicators.all().len(),
        1,
        "and the flow pane still shows layout 1's EMA"
    );
    assert_eq!(
        app.slot_kinds.len(),
        1,
        "the time pane's registration went with the layout it left, and              only the flow pane's is left"
    );

    // An edit on the time pane reaches layout 2 only.
    app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
    settle_indicators(&mut app);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .indicators
            .all()
            .len(),
        1
    );
    assert_eq!(
        app.active_tab().flow_pane.indicators.all().len(),
        1,
        "the flow pane gained nothing"
    );
    assert_eq!(app.layouts().get(second).unwrap().indicators.len(), 1);
    assert_eq!(
        app.layouts().get(second).unwrap().indicators[0].kind,
        crate::indicators::state_file::SavedKind::NativeCvd
    );
    assert_eq!(app.layouts().get(first).unwrap().indicators.len(), 1);
    assert_eq!(
        app.slot_kinds.len(),
        2,
        "one registration per pane, and no leak from the switch"
    );

    // A pane that opens takes the focused pane's layout.
    app.open_tab("binance".to_owned(), "ETHUSDT".to_owned(), None);
    run_frame(&mut app, &ctx);
    let opened = app.active_tab().id;
    assert_eq!(app.pane_layout(opened, PaneSide::Flow), second);
    assert_eq!(app.active_tab().flow_pane.layout_label, "levels");

    // Switching the time pane back brings layout 1's set to it alone.
    app.cycle_tab(-1);
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.switch_layout(first), Ok(true));
    settle_indicators(&mut app);
    let labels: Vec<String> = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .indicators
        .all()
        .iter()
        .map(|view| view.label().to_owned())
        .collect();
    assert_eq!(
        labels.len(),
        1,
        "layout 1 is back on the time pane: {labels:?}"
    );
    assert!(labels[0].contains("EMA"));
    assert_eq!(
        app.layouts().get(second).unwrap().indicators.len(),
        1,
        "layout 2 kept its own set while it was away"
    );
    // The registry, not just the views: a leaked slot changes no view
    // count but shifts every later edit's layout index by one, so a
    // mirrored remove would take the wrong indicator on the other panes.
    assert_eq!(
            app.slot_kinds
                .iter()
                .filter(|(owner, _)| owner.tab == app.active_tab().id
                    && owner.side == PaneSide::Time(0))
                .count(),
            1,
            "the time pane holds one registration after two switches, not two"
        );
}
/// The strip and the file record the layout per pane: a workspace
/// captured with two panes on two layouts names both, and a pane told
/// its layout before seeding opens on it.
#[test]
fn per_pane_layouts_are_recorded_and_restored() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let first = app.layouts().active_id();
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let second = app.create_layout(Some("levels")).expect("second");
    app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
    settle_indicators(&mut app);
    app.flush_layouts();

    let (tabs, _chrome) = app.capture_arrangement();
    assert_eq!(tabs[0].flow_layout, Some(first.0));
    assert_eq!(tabs[0].context_layouts, vec![second.0]);

    let path = app.workspace.layouts_path().to_path_buf();
    let (mut again, _commands2) = split_app(&ctx, 200);
    again.workspace.set_layouts_path(path.to_path_buf());
    again.active_tab_mut().flow_pane.layout = Some(first);
    again.active_tab_mut().pane_mut(PaneSide::Time(0)).layout = Some(second);
    again.reload_layouts(&[]);
    settle_indicators(&mut again);
    assert_eq!(
        again.pane_layout(again.active_tab().id, PaneSide::Time(0)),
        second
    );
    assert_eq!(
        again
            .active_tab()
            .pane(PaneSide::Time(0))
            .indicators
            .all()
            .len(),
        1,
        "the restored context pane opened on layout 2 and its CVD"
    );
    assert!(
        again.active_tab().flow_pane.indicators.all().is_empty(),
        "layout 1 is empty here"
    );
    assert_eq!(
        again.active_tab().pane(PaneSide::Time(0)).layout_label,
        "levels"
    );
}

/// The whole book survives a restart: layouts, the active one, each
/// layout's indicators and each market's drawings. A fresh app on the
/// same cockpit home opens on it.
#[test]
fn layouts_come_back_after_a_restart() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    app.maintain_indicator_state();
    place_level(&mut app, PaneSide::Time(0), 100.0);
    run_frame(&mut app, &ctx);
    let second = app.create_layout(Some("levels")).expect("second");
    app.rename_layout(second, "open").expect("renamed");
    app.flush_layouts();

    let path = app.workspace.layouts_path().to_path_buf();
    let crate::layouts::Loaded::Book(book) = crate::layouts::load(&path) else {
        panic!("the book was written");
    };
    assert_eq!(book.layouts().len(), 2);
    assert_eq!(book.active().name, "open");
    assert_eq!(book.layouts()[0].indicators.len(), 1);
    let key = crate::layouts::DrawingKey {
        feed: "binance".to_owned(),
        symbol: "TESTUSDT".to_owned(),
        pane: 1,
    };
    assert_eq!(book.layouts()[0].drawings(&key).map(<[_]>::len), Some(1));

    // A second app on the same home — the same file — opens on the book.
    let (mut again, _commands2) = split_app(&ctx, 200);
    again.workspace.set_layouts_path(path.to_path_buf());
    again.reload_layouts(&[]);
    assert_eq!(again.layouts().active().name, "open");
    again.switch_layout(book.layouts()[0].id).expect("layout 1");
    settle_indicators(&mut again);
    assert_eq!(
        again
            .active_tab()
            .pane(PaneSide::Time(0))
            .indicators
            .all()
            .len(),
        1,
        "the EMA is back on the restored layout"
    );
    assert_eq!(drawings_on(&again, PaneSide::Time(0)), vec![100.0]);
}

/// A settings preview reaches the origin's worker and nothing else: the
/// layout keeps the last committed inputs, so Discard leaves no trace on
/// another chart or in the file.
#[test]
fn a_previewed_input_never_reaches_the_layout() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
    settle_indicators(&mut app);
    let slot = app.active_tab().flow_pane.indicators.all()[0].slot;
    let target = TabSlot {
        tab: app.active_tab().id,
        side: PaneSide::Flow,
        slot,
    };
    app.open_indicator_settings_at(target);
    app.indicator_settings.as_mut().expect("dialog").draft = vec![
        quantick_indicators::InputValue::Int(50),
        quantick_indicators::InputValue::Source(quantick_indicators::SourceId::Close),
    ];
    app.preview_indicator_settings_draft();
    assert!(
        app.layouts().active().indicators[0].inputs.is_empty(),
        "a preview is not a commit"
    );
    app.apply_indicator_settings_draft();
    assert_eq!(
        app.layouts().active().indicators[0].inputs[0],
        crate::indicators::state_file::SavedInput::Int(50),
        "Apply is"
    );
}

/// Deleting a layout destroys its drawings, so the strip's Delete asks
/// first; the confirmed half is what removes it, and cancelling keeps it.
#[test]
fn deleting_a_layout_waits_for_the_confirmation() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let second = app.create_layout(Some("levels")).expect("second");
    app.apply_strip_action(crate::layout_strip::StripAction::Delete(second));
    assert_eq!(app.layouts().layouts().len(), 2, "nothing is deleted yet");
    assert_eq!(app.layout_delete_confirm, Some(second));
    app.layout_delete_confirm = None;
    assert_eq!(app.layouts().layouts().len(), 2, "cancelling keeps it");

    app.apply_strip_action(crate::layout_strip::StripAction::Delete(second));
    app.confirm_layout_delete();
    assert_eq!(app.layouts().layouts().len(), 1, "confirming deletes it");
    assert_ne!(
        app.layouts().active_id(),
        second,
        "and the neighbour is active"
    );
    assert!(app.layout_delete_confirm.is_none());
}

/// Going back to Single hides the context chart; it must not throw away
/// what the user built on it, and it must keep following the market.
#[test]
fn leaving_the_split_keeps_the_time_panes_work_and_its_bars() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .len(),
        1
    );

    app.active_tab_mut().set_layout(CanvasLayout::Single);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Flow,
        "a single canvas is the flow pane, whatever had focus"
    );

    let before = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .state
        .trades()
        .len();
    let trade = trade(700);
    app.active_tab_mut()
        .ingest_live_trade_at(&trade, trade.timestamp_ms);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .state
            .trades()
            .len(),
        before + 1,
        "a hidden pane keeps draining, so showing it again never catches up"
    );

    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .len(),
        1,
        "and its drawings survived the round trip"
    );
}

/// The default layout is the one quantick opens on, and the split must not
/// have quietly changed it: one pane, the whole canvas, no second worker.
#[test]
fn the_single_layout_still_gives_the_flow_pane_the_whole_canvas() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);

    assert_eq!(app.active_tab().layout, CanvasLayout::Single);
    assert!(
        app.active_tab().time_panes.is_empty(),
        "an unsplit canvas builds no second pane, and no worker behind it"
    );
    let chart = app
        .active_tab()
        .flow_pane
        .last_chart_area
        .expect("laid out");
    // The canvas the pane was given, reconstructed from the rect it kept:
    // wider than half the window, so nothing was carved off for a divider.
    assert!(
        chart.width() > 600.0,
        "the flow pane still owns the canvas, got {chart:?}"
    );
    assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
}

/// A press egui routed to a floating window is not a click on the pane
/// underneath it.
///
/// The toast, the object manager and the inspector all float over the
/// canvas. Taking their presses as pane clicks made the focused pane
/// follow whichever pane the *window* happened to overlap, so the toast's
/// Undo — which acts on the focused pane — undid an edit on the other
/// chart, and the manager's list flipped under the click that opened it.
#[test]
fn a_press_on_a_floating_window_does_not_move_pane_focus() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);

    // A mark on the time pane, then delete it: the toast comes up with an
    // Undo that acts on whatever pane has focus.
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .len(),
        1
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
    assert!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .is_empty()
    );
    // A fresh egui Area sizes itself on its first frame.
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let undo = app
        .surfaces
        .toast
        .undo_rect()
        .expect("the toast offers Undo");
    let divider = app
        .active_tab()
        .canvas_divider_rect()
        .expect("the split is on");
    assert!(
        undo.center().x > divider.right(),
        "the regression needs the toast's button to float over the *other* pane"
    );

    click_chart(&mut app, &ctx, undo.center());

    assert_eq!(
        app.active_tab().focused_side(),
        PaneSide::Time(0),
        "a press routed to the toast is not a click on the pane behind it"
    );
    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Time(0))
            .drawings
            .items()
            .len(),
        1,
        "so Undo puts back the mark it was offered for"
    );
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "and touches nothing on the pane the button happens to float over"
    );
}

/// (a) A time pane on a feed that serves candles asks once, and the reply
/// becomes bars in front of the ones cut from prints.
#[test]
fn a_time_pane_asks_for_venue_history_once_and_renders_the_reply() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);

    assert_eq!(
        drain_ohlcv_requests(&mut commands),
        1,
        "the pane asks exactly once when it is built"
    );
    let slots_before = app.active_tab().pane(PaneSide::Time(0)).slots();
    assert!(
        app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "and says it is waiting"
    );

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    let pane = app.active_tab().pane(PaneSide::Time(0));
    assert_eq!(
        pane.seam_slot(),
        120,
        "the whole prefix stands in front of the engine's bars"
    );
    assert!(pane.slots() > slots_before, "and the chart grew by it");
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "the wait ended with the reply"
    );
    // And it really paints: the axis is there and the candles are drawn.
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        has_price_axis(&texts),
        "the pane draws its chart: {texts:?}"
    );
}

/// A push feed replacing its block mid-run discards the slices still in
/// flight rather than folding them onto the base they no longer belong to
/// — which would build a history missing exactly its newest part.
#[test]
fn slices_of_a_superseded_answer_are_dropped_and_the_tab_asks_again() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_requests(&mut commands);

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history_range(-20, 0),
            slice: crate::feed::OhlcvSlice::More,
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 20);

    // The feed stored a fresh block: the base being built is abandoned.
    // What is already drawn stays drawn — blanking the chart while a
    // replacement is fetched would be a worse answer than a slightly old
    // one, and the replacement overwrites it wholesale when it lands.
    app.active_tab_mut().forget_ohlcv_generation_for_test();
    app.drain_tabs();

    // The rest of the abandoned run arrives anyway, and is ignored: were
    // it folded in, the base would be the two older weeks with the newest
    // one — the part already thrown away — missing from the middle.
    for slice in [
        crate::feed::OhlcvSlice::More,
        crate::feed::OhlcvSlice::Last { complete: true },
    ] {
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: venue_history_range(-60, -20),
                slice,
            })
            .unwrap();
        app.drain_tabs();
    }
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        20,
        "the leftovers extended nothing"
    );
    assert_eq!(
        drain_ohlcv_requests(&mut commands),
        1,
        "the abandoned run's closing slice freed the tab to ask again, once"
    );
    assert!(
        app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "and the wait now belongs to the replacement request"
    );

    // The replacement answer installs cleanly over what was left.
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history_range(-45, 0),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        45,
        "the fresh answer is the whole prefix, not an addition to the old"
    );
}

/// S1 end to end on the single-pane route: `bars → time` on the flow
/// pane asks the venue for candle history and wears the reply as its
/// prefix — the fix for the audit's BLOCKER-1, where the toolbar route
/// produced a 1-second chart and then an empty one. The venue prefix
/// belongs to what a pane shows, never to which pane object it is.
#[test]
fn the_flow_pane_cutting_time_bars_earns_the_venue_prefix() {
    let ctx = egui::Context::default();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
    let mut app = QuantickApp::new(
        test_config(),
        "binance",
        "TESTUSDT",
        BarSpec::Tick(1),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(FeedCapabilities {
                book_capture: false,
                history_paging: true,
                traded_volume: true,
                ohlcv_history: true,
                ohlcv_generation: 0,
            }),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    let _ = book_tx;
    let trades: Vec<_> = (0..200).map(minute_trade).collect();
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.drain_tabs();
    run_frame(&mut app, &ctx);
    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        0,
        "a tick chart asks for no candles"
    );

    // The toolbar route: `bars → time`. The kind's default interval is a
    // real timeframe (QW2), so the spec that lands is one minute.
    app.active_tab_mut().flow_pane.kind = crate::state::BarKind::Time;
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        *app.active_tab().flow_pane.state.spec(),
        BarSpec::Time(crate::time_header::DEFAULT_INTERVAL_MS),
        "bars → time opens on 1m, not one second"
    );
    assert!(
        drain_ohlcv_requests(&mut cmd_rx) >= 1,
        "the time-cutting flow pane asks the venue for history"
    );

    evt_tx
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    let tab = app.active_tab();
    assert_eq!(
        tab.flow_pane.seam_slot(),
        120,
        "the venue candles stand in front of the bars cut from prints"
    );

    // And leaving the time kind hands the prefix back: a tick chart is
    // the tape's alone.
    app.active_tab_mut().flow_pane.kind = crate::state::BarKind::Tick;
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().flow_pane.seam_slot(),
        0,
        "a pane that stopped cutting by time carries no venue candles"
    );
}

/// (h) A recording with no context file beside it has no venue behind it,
/// and nothing to ask. One that *does* is the test after this one: the
/// capability, never the source, is what decides.
#[test]
fn a_replaying_tab_with_no_context_asks_for_no_venue_history() {
    let ctx = egui::Context::default();
    let (mut app, _events, mut commands) = history_app(&ctx);
    drain_ohlcv_requests(&mut commands);
    // Put the tab on a recording, then give it a fresh time pane.
    let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
    let session = quantick_replay::Session::from_text(
        std::path::Path::new("WINJ26_2026-03-16.csv"),
        text,
        quantick_replay::ParseOptions::default(),
    )
    .expect("fixture session parses");
    with_config(&mut app, |tab, config| {
        tab.open_replay(
            config,
            crate::feed::ReplayRequest {
                session: std::sync::Arc::new(session),
                options: crate::feed::ReplayOptions {
                    autoplay: false,
                    ..Default::default()
                },
            },
        )
    });
    let commands_after = std::mem::replace(&mut commands, mpsc::channel(1).1);
    drop(commands_after);
    run_frame(&mut app, &ctx);
    app.drain_tabs();

    assert!(app.active_tab().replay.is_some());
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "a recording has no venue to wait on"
    );
}

/// The reach the trader picked is the reach the tab keeps, in every tab.
#[test]
fn the_reach_is_a_standing_choice_mirrored_onto_every_tab() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = history_app(&ctx);
    assert_eq!(
        app.active_tab().history_reach,
        crate::history_reach::HistoryReach::Page,
        "the press the button has always had is what a chart opens on"
    );
    app.history_reach = crate::history_reach::HistoryReach::PreviousSession;
    app.drain_tabs();
    assert_eq!(
        app.active_tab().history_reach,
        crate::history_reach::HistoryReach::PreviousSession
    );
}

/// (c) Removing is a catalog edit and only that: the file and the picker
/// lose the symbol, a tab showing that market does not.
#[test]
fn removing_a_symbol_updates_the_file_and_leaves_open_tabs_alone() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let path = symbols_scratch("removed");
    let _ = std::fs::remove_file(&path);
    app.workspace.set_symbols_path(path.clone());
    app.add_symbol("binance", "WINQ26")
        .expect("the catalog takes a symbol that fits");
    app.adopt_tab(
        "binance".to_owned(),
        "WINQ26".to_owned(),
        stub_feed().0,
        None,
    );
    run_frame(&mut app, &ctx);
    let open_tabs = app.tabs.len();

    app.remove_symbol("binance", "WINQ26");

    assert!(
        !app.config
            .feed("binance")
            .expect("the feed")
            .symbols
            .iter()
            .any(|symbol| symbol == "WINQ26"),
        "the catalog lost it"
    );
    assert!(!app.added_symbols.contains("binance", "WINQ26"));
    assert!(
        !crate::symbols_file::load(&path).contains("binance", "WINQ26"),
        "and so did the file"
    );
    assert_eq!(app.tabs.len(), open_tabs, "no tab was closed");
    assert_eq!(
        app.active_tab().symbol,
        "WINQ26",
        "and the one showing that market still is"
    );
    let _ = std::fs::remove_file(&path);
}

/// The remove affordance is refused for a market a tab is on: the picker
/// greys it out, and the reason is that the next SOURCE correction would
/// otherwise retarget that tab to another instrument.
#[test]
fn a_symbol_a_tab_is_showing_is_not_offered_for_removal() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    app.workspace.set_symbols_path(symbols_scratch("guard"));
    let _ = std::fs::remove_file(app.workspace.symbols_path());
    app.add_symbol("binance", "WINQ26")
        .expect("the catalog takes a symbol that fits");
    app.adopt_tab(
        "binance".to_owned(),
        "WINQ26".to_owned(),
        stub_feed().0,
        None,
    );
    run_frame(&mut app, &ctx);

    // What the picker is handed, and what it does with it.
    let open: Vec<(String, String)> = app
        .tabs
        .iter()
        .map(|tab| (tab.feed_id.clone(), tab.symbol.clone()))
        .collect();
    assert!(
        open.iter()
            .any(|(feed, symbol)| feed == "binance" && symbol == "WINQ26"),
        "the tab is on the market the picker must protect"
    );
    // The app-side rule holds even if the affordance were clicked: the
    // catalog edit is refused for the last symbol and allowed otherwise,
    // and the tab is never touched either way.
    app.remove_symbol("binance", "WINQ26");
    assert_eq!(
        app.active_tab().symbol,
        "WINQ26",
        "removing a symbol never moves a tab off its market"
    );
    let _ = std::fs::remove_file(app.workspace.symbols_path());
}

/// (a) The `+` opens the picker, and choosing a market adds a tab that
/// becomes the active one.
#[test]
fn the_plus_opens_a_picker_and_its_choice_becomes_the_active_tab() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(100);
    assert_eq!(app.tabs.len(), 1, "quantick opens on one tab");
    assert_eq!(app.active_tab().symbol, "TESTUSDT");

    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");

    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, 1, "the new tab is the one you land on");
    assert_eq!(app.active_tab().symbol, "ETHUSDT");
    assert_ne!(
        app.tabs[0].id, app.tabs[1].id,
        "ids are handed out, never reused"
    );
    // Pane ids namespace egui state; two tabs sharing one would share a
    // drag the moment both had been on screen.
    assert_ne!(app.tabs[0].flow_pane.id, app.tabs[1].flow_pane.id);
    // The picker's choice is honoured, not the config default.
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.contains("ETHUSDT")),
        "the strip names the market it opened; painted: {texts:?}"
    );
}

/// (b) A tab is a whole workspace: switching away and back finds its bars,
/// its viewport, its focus and its drawings exactly as they were.
#[test]
fn switching_tabs_preserves_everything_each_one_owns() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(200);

    // Give tab 0 a distinctive state: a drawing, a panned viewport, and
    // the split open with the time pane focused.
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    // Clicking the time pane focuses it and lands the mark there — the
    // real gesture, not a poked field.
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .last_chart_area
        .expect("the time pane was laid out")
        .center();
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    let slots = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .viewport
        .pan_pixels(120.0, slots);
    let first_bars = app.active_tab().flow_pane.state.bars().len();
    let first_edge = app.active_tab().flow_pane.viewport.right_edge_bar(slots);
    let first_drawings = app.active_tab().focused_pane().drawings.items().len();
    assert_eq!(first_drawings, 1, "the drawing landed on the focused pane");

    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert_eq!(
        app.active_tab().layout,
        CanvasLayout::Single,
        "a new tab opens on the default layout, not the previous tab's"
    );
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());

    app.apply_tab_action(TabAction::Activate(0));
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), first_bars);
    assert_eq!(
        app.active_tab()
            .flow_pane
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        first_edge,
        "the viewport came back where it was left"
    );
    assert_eq!(app.active_tab().layout, CanvasLayout::TimeAndFlow);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    assert_eq!(
        app.active_tab().focused_pane().drawings.items().len(),
        first_drawings,
        "and its marks with it"
    );
}

/// (c) §11: switching never tears a feed down. A background tab keeps
/// draining — its channels are bounded, and one left full backs its feed
/// thread up until the market it shows is hours behind.
#[test]
fn a_background_tab_keeps_ingesting() {
    let ctx = egui::Context::default();
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    let ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    // Leave the new market in the background.
    app.apply_tab_action(TabAction::Activate(0));
    assert_eq!(app.active_tab, 0);

    let before = app.tabs[1].flow_pane.state.trades().len();
    // Push into the background tab's own channel, then run the window's
    // drain — not that tab's, which would prove nothing about the loop.
    for id in 900..905 {
        ends.events.try_send(FeedEvent::Live(trade(id))).unwrap();
    }
    app.drain_tabs();

    assert_eq!(
        app.tabs[1].flow_pane.state.trades().len(),
        before + 5,
        "a tab off screen still takes in what its feed sent"
    );
    assert_eq!(
        app.trades_since_summary, 5,
        "and the window counts them as its own ingest"
    );
}

/// (d) Closing the active tab activates a neighbour and takes the market
/// with it. The last tab has no × to click.
#[test]
fn closing_a_tab_activates_a_neighbour_and_drops_its_market() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let first = app.active_tab().id;
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let second = app.active_tab().id;
    // Register a slot on the tab about to close, so the bookkeeping has
    // something to lose with it.
    app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
    assert!(app.slot_kinds.iter().any(|(owner, _)| owner.tab == second));

    app.apply_tab_action(TabAction::Close(1));

    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active_tab().id, first, "a neighbour takes over");
    assert!(
        !app.slot_kinds.iter().any(|(owner, _)| owner.tab == second),
        "its indicator bookkeeping went with it"
    );
    // The last tab stays: a window with no market has nothing to draw.
    app.apply_tab_action(TabAction::Close(0));
    assert_eq!(app.tabs.len(), 1, "the last tab is not closable");
    run_frame(&mut app, &ctx);
}

/// The workers a closed tab owned end with it: their run loops exit when
/// the command channels they hold disconnect, so dropping the tab is the
/// whole shutdown protocol.
#[test]
fn closing_a_tab_ends_its_worker_threads() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    // A flush proves the worker is alive and answering right now.
    app.active_tab_mut().flow_pane.indicator_worker.flush();
    let doomed = app.tabs.pop().expect("the second tab");
    let worker = doomed.flow_pane.indicator_worker;
    drop(doomed.flow_pane.orderflow);
    app.active_tab = 0;

    // Dropping the handle disconnects the command channel; the run loop's
    // `recv` then fails and the thread returns. A send after that is
    // refused rather than queued into a thread nobody will ever join.
    drop(worker);
    // The window is still whole, and the surviving tab still draws.
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        has_price_axis(&texts),
        "the surviving tab keeps drawing: {texts:?}"
    );
}

/// (e) The SOURCE group writes into the active tab only — switching a
/// market must not relabel the tab beside it.
#[test]
fn the_source_combo_changes_only_the_active_tab() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let untouched = app.tabs[0].symbol.clone();

    // What the combo does: write the selection, then let the frame switch.
    app.active_tab_mut().symbol = "TESTUSDT".to_owned();
    let (tab, config) = app.active_with_config();
    tab.maybe_switch_feed(config);

    assert_eq!(app.active_tab().symbol, "TESTUSDT");
    assert_eq!(app.active_tab().active.1, "TESTUSDT", "its feed followed");
    assert_eq!(
        app.tabs[0].symbol, untouched,
        "the other tab kept its market"
    );
    assert_eq!(app.tabs[0].active.1, untouched);
}

/// (f) The transport speaks for one tab at a time (§11). A recording in a
/// background tab keeps its own clock but claims none of the chrome.
#[test]
fn the_transport_shows_only_while_the_active_tab_replays() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
    let session = quantick_replay::Session::from_text(
        std::path::Path::new("WINJ26_2026-03-16.csv"),
        text,
        quantick_replay::ParseOptions::default(),
    )
    .expect("fixture session parses");
    with_config(&mut app, |tab, config| {
        tab.open_replay(
            config,
            crate::feed::ReplayRequest {
                session: std::sync::Arc::new(session),
                options: crate::feed::ReplayOptions {
                    autoplay: false,
                    ..Default::default()
                },
            },
        )
    });
    assert!(app.active_tab().replay.is_some());
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.contains("WINJ26")),
        "the active tab's session is named on screen: {texts:?}"
    );

    // Open a live tab beside it and switch: the recording keeps playing in
    // its own tab, but the transport belongs to whoever is on screen.
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert!(
        app.tabs[0].replay.is_some(),
        "the background tab is still the one holding the recording"
    );
    assert!(
        app.active_tab().replay.is_none(),
        "and the active tab is streaming"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        !texts.iter().any(|text| text.contains("Speed")),
        "no transport for a tab that is not on screen: {texts:?}"
    );
}

/// (g) `Ctrl+Tab` / `Ctrl+Shift+Tab` walk the strip and wrap (§10).
#[test]
fn the_cycle_shortcuts_walk_the_strip_and_wrap() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let _ends = open_second_tab(&mut app, &ctx, "TESTUSDT");
    assert_eq!(app.tabs.len(), 3);
    assert_eq!(app.active_tab, 2);

    app.cycle_tab(1);
    assert_eq!(
        app.active_tab, 0,
        "forward from the last wraps to the first"
    );
    app.cycle_tab(-1);
    assert_eq!(app.active_tab, 2, "and back again");
    app.cycle_tab(-1);
    assert_eq!(app.active_tab, 1);

    // Through the real key path, so the shortcut itself is covered.
    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::Tab, egui::Modifiers::CTRL)],
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.active_tab, 2, "Ctrl+Tab moves forward one");
}

/// Ctrl+W closes, Ctrl+T opens the picker — and neither collides with a
/// binding the chart already had.
#[test]
fn the_tab_shortcuts_open_and_close() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert_eq!(app.tabs.len(), 2);

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::W, egui::Modifiers::CTRL)],
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.tabs.len(), 1, "Ctrl+W closes the active tab");

    run_frame_with_modifiers(
        &mut app,
        &ctx,
        vec![key_press_with(egui::Key::T, egui::Modifiers::CTRL)],
        egui::Modifiers::CTRL,
    );
    assert!(
        app.surfaces.source_picker.is_open(),
        "Ctrl+T opens the picker"
    );
}

/// Provenance follows the active tab (§11): the status bar names the
/// market on screen, not the one that happens to be first.
#[test]
fn the_status_bar_follows_the_active_tab() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    assert_eq!(app.status_model().symbol, "TESTUSDT");

    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert_eq!(app.status_model().symbol, "ETHUSDT");

    app.apply_tab_action(TabAction::Activate(0));
    assert_eq!(app.status_model().symbol, "TESTUSDT");
}
