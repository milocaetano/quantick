use super::*;

#[test]
fn drawing_toolbar_avoids_manager() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let index = place_drawing(
        &mut app,
        &ctx,
        "trend-line",
        &[egui::pos2(400.0, 300.0), egui::pos2(600.0, 400.0)],
    );
    app.surfaces.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(index));
    for parked in [false, true] {
        if parked {
            app.surfaces
                .drawing_chrome
                .context_bar_mut()
                .set_manual(egui::pos2(55.0, 190.0));
        }
        for (locked, hidden) in [(false, false), (true, false), (false, true)] {
            app.drawing_pane_mut().drawings.set_locked_at(index, locked);
            app.drawing_pane_mut().drawings.set_hidden_at(index, hidden);
            app.surfaces.drawing_chrome.set_manager_open(true);
            app.surfaces.drawing_chrome.forget_context_bar_rect();
            for _ in 0..3 {
                run_frame(&mut app, &ctx);
            }
            assert!(
                app.surfaces.drawing_chrome.context_bar_rect().is_none(),
                "the manager owns object actions while open"
            );
            assert!(
                !app.surfaces
                    .drawing_chrome
                    .manager_action_rects()
                    .is_empty()
            );
            app.surfaces.drawing_chrome.set_manager_open(false);
            for _ in 0..3 {
                run_frame(&mut app, &ctx);
            }
            if !hidden {
                assert!(
                    app.surfaces.drawing_chrome.context_bar_rect().is_some(),
                    "closing restores the selected object's toolbar"
                );
            }
        }
    }
}

#[test]
fn layout_readability_combined() {
    for theme in [
        quantick_appearance::Builtin::Light,
        quantick_appearance::Builtin::Dark,
    ] {
        for width in [1000.0, 1280.0] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let ctx = egui::Context::default();
                let (mut app, _commands) = app_with_history(100);
                app.runtime.appearance.apply_builtin(theme, false).unwrap();
                app.surfaces
                    .toolrail
                    .set_dock(crate::toolrail::ToolboxDock::Top);
                app.surfaces.dock.open_tab(crate::dock::DockTab::Trading);
                let paper = &mut app.active_tab_mut().paper;
                paper.seed(&trade(0));
                paper.market(quantick_engine::Side::Buy);
                paper.on_trade(&trade(1));
                let pnl = paper
                    .account()
                    .open_pnl_text()
                    .expect("simulated open position");
                app.surfaces.market_palette.update(
                    quantick_ui_model::market_palette::Intent::Open {
                        query: "no-market-match-with-a-long-symbol-name".into(),
                        switching: None,
                    },
                );
                let mut output = None;
                for _ in 0..4 {
                    let mut input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 780.0),
                        )),
                        ..Default::default()
                    };
                    input
                        .viewports
                        .entry(egui::ViewportId::ROOT)
                        .or_default()
                        .native_pixels_per_point = Some(scale);
                    let now = frame_instant(&ctx);
                    output = Some(ctx.run(input, |ctx| app.draw_frame(ctx, now)));
                }
                let output = output.unwrap();
                let pnl_shape = output
                    .shapes
                    .iter()
                    .find_map(|clipped| {
                        if let egui::Shape::Text(text) = &clipped.shape {
                            (text.galley.text() == pnl).then_some((clipped, text))
                        } else {
                            None
                        }
                    })
                    .expect("the full ticket calculation is painted beside the market palette");
                assert!(
                    pnl_shape
                        .0
                        .clip_rect
                        .expand(1.0)
                        .contains_rect(pnl_shape.1.visual_bounding_rect())
                );
                let footer: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            let words = text.galley.text();
                            if words == "BARS"
                                || words == "WORKSPACE"
                                || words.contains("background")
                                || words.starts_with("Add no-market")
                            {
                                let rect = text.visual_bounding_rect();
                                assert!(
                                    shape.clip_rect.contains_rect(rect),
                                    "{words} clipped at {width}/{scale}"
                                );
                                return Some((words, rect));
                            }
                        }
                        None
                    })
                    .collect();
                assert!(footer.len() >= 3, "the integrated market footer is present");
                for (index, (words, rect)) in footer.iter().enumerate() {
                    for (other, next) in &footer[index + 1..] {
                        assert!(
                            !rect.shrink(1.0).intersects(*next),
                            "{words} overlaps {other}"
                        );
                    }
                }
                let palette = crate::theme::get(&ctx);
                let rgb = |c: egui::Color32| {
                    quantick_appearance::PremultipliedRgba::from_premultiplied(c.to_array())
                };
                if theme == quantick_appearance::Builtin::Light {
                    assert!(
                        quantick_appearance::contrast::contrast_ratio(
                            rgb(palette.text_support()),
                            rgb(palette.chrome())
                        ) >= 4.5
                    );
                    // The list door and the capsule leave TRADE inline from
                    // 1280 points; narrower, §6 folds it into the overflow.
                    let buy = output.shapes.iter().find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "BUY 1" => Some(text),
                        _ => None,
                    });
                    assert!(
                        buy.is_some() || width / scale < 1280.0,
                        "actual toolbar BUY label at {width}/{scale}"
                    );
                    if let Some(buy) = buy {
                        let ink = buy
                            .override_text_color
                            .unwrap_or_else(|| buy.galley.rows[0].visuals.mesh.vertices[0].color);
                        assert!(
                            quantick_appearance::contrast::contrast_ratio(
                                rgb(ink),
                                rgb(palette.buy())
                            ) >= 4.5
                        );
                    }
                }
                let reference = egui::Context::default();
                let mut appearance = quantick_appearance::Appearance::default();
                appearance.apply(theme.source()).unwrap();
                crate::theme::sync(&reference, &appearance);
                assert_eq!(
                    crate::theme::bookmap(&ctx).canvas(),
                    crate::theme::bookmap(&reference).canvas(),
                    "each theme preserves its configured Bookmap canvas"
                );
            }
        }
    }
}

#[test]
fn top_rail_has_no_unpainted_gutter_after_restore() {
    for theme in [
        quantick_appearance::Builtin::Light,
        quantick_appearance::Builtin::Dark,
    ] {
        let mut chrome = chrome_with_popup_at(None);
        chrome.rail_dock = ui_state::SavedRailDock::Top;
        let mut workspace = ui_state::Workspace::default();
        workspace.chrome = Some(chrome);
        let (mut app, events, _commands, _book) =
            test_app_with_workspace_and_launch(workspace, AppLaunch::default());
        app.runtime.appearance.apply_builtin(theme, false).unwrap();
        events
            .try_send(FeedEvent::Backfilled((1..=200).map(trade).collect()))
            .unwrap();
        let tab = app.runtime.tabs.active_id();
        app.active_tab_mut().drain_feed(tab);
        let ctx = egui::Context::default();
        for width in [1000.0, 1280.0] {
            let mut output = run_frame_sized(
                &mut app,
                &ctx,
                egui::vec2(width, 780.0),
                Vec::new(),
                egui::Modifiers::NONE,
            );
            for _ in 0..3 {
                output = run_frame_sized(
                    &mut app,
                    &ctx,
                    egui::vec2(width, 780.0),
                    Vec::new(),
                    egui::Modifiers::NONE,
                );
            }
            assert_eq!(
                app.surfaces.toolrail.dock(),
                crate::toolrail::ToolboxDock::Top
            );
            let objects = app
                .surfaces
                .toolrail
                .objects_button_rect()
                .expect("restored rail actions are reachable");
            let header = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        (text.galley.text() == "ORDER FLOW").then(|| text.visual_bounding_rect())
                    } else {
                        None
                    }
                })
                .expect("chart header");
            assert!(
                header.top() >= objects.bottom() && header.top() - objects.bottom() < 20.0,
                "the chart header immediately follows the single rail band: {header:?}/{objects:?}"
            );
        }
    }
}

/// The actual locked-coordinate and preset forms share the cross-theme matrix.
#[test]
fn drawing_text_and_forms_remain_readable_inspector() {
    for theme in [
        quantick_appearance::Builtin::Light,
        quantick_appearance::Builtin::Dark,
    ] {
        for width in [1000.0, 1280.0] {
            for id in [
                "trend-line",
                "fib-retracement",
                "anchored-vwap",
                "fixed-range-profile",
            ] {
                let ctx = egui::Context::default();
                let (mut app, _commands) = app_with_history(200);
                app.runtime.appearance.apply_builtin(theme, false).unwrap();
                run_frame(&mut app, &ctx);
                let anchors = [egui::pos2(400.0, 300.0), egui::pos2(600.0, 400.0)];
                let count = if id == "anchored-vwap" { 1 } else { 2 };
                let index = place_drawing(&mut app, &ctx, id, &anchors[..count]);
                app.surfaces.toolrail.arm(Tool::Pointer);
                app.drawing_pane_mut().drawings.select(Some(index));
                let locked = id == "trend-line";
                app.drawing_pane_mut().drawings.set_locked_at(index, locked);
                app.surfaces.drawing_chrome.set_inspector_pinned(true);
                app.surfaces.drawing_chrome.set_inspector_open(true);
                app.surfaces.drawing_chrome.set_inspector_tab(if locked {
                    InspectorTab::Coordinates
                } else {
                    InspectorTab::Extra
                });
                let needle = if locked {
                    "Unlock the drawing to edit its coordinates."
                } else {
                    "Preset name"
                };
                for scale in [1.0, 1.25, 1.5, 2.0] {
                    let mut output = None;
                    for _ in 0..4 {
                        let mut input = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 780.0),
                            )),
                            ..Default::default()
                        };
                        input
                            .viewports
                            .entry(egui::ViewportId::ROOT)
                            .or_default()
                            .native_pixels_per_point = Some(scale);
                        let now = frame_instant(&ctx);
                        output = Some(ctx.run(input, |ctx| app.draw_frame(ctx, now)));
                    }
                    let output = output.unwrap();
                    let (clipped, text) = output
                        .shapes
                        .iter()
                        .find_map(|clipped| {
                            if let egui::Shape::Text(text) = &clipped.shape {
                                (text.galley.text() == needle).then_some((clipped, text))
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| panic!("{id}: {needle} is visible"));
                    let rect = text.visual_bounding_rect();
                    assert!(
                        clipped.clip_rect.expand(1.0).contains_rect(rect),
                        "{id}: {needle} fits at {width}/{scale}"
                    );
                    if locked {
                        assert!(text.galley.job.sections[0].format.font_id.size >= 12.0);
                    } else if theme == quantick_appearance::Builtin::Light {
                        let ink = crate::theme::get(&ctx).text_support();
                        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(border) if border.rect.contains_rect(rect) && border.stroke.color == ink)), "{id}: editable preset has a visible boundary");
                    }
                }
            }
        }
    }
}

/// Exercise the real tool adapter and on-chart editor, including legacy foreground resolution.
#[test]
fn drawing_text_and_forms_remain_readable_authored_notes() {
    for theme in [
        quantick_appearance::Builtin::Light,
        quantick_appearance::Builtin::Dark,
    ] {
        for layout in [CanvasLayout::Time, CanvasLayout::Single] {
            let ctx = egui::Context::default();
            let (mut app, _commands) = app_with_history(200);
            app.runtime.appearance.apply_builtin(theme, false).unwrap();
            app.active_tab_mut().set_layout(layout);
            for _ in 0..3 {
                run_frame(&mut app, &ctx);
            }
            let index = place_drawing(&mut app, &ctx, "text", &[egui::pos2(400.0, 300.0)]);
            app.surfaces.drawing_chrome.end_inline_text_edit();
            app.surfaces.toolrail.arm(Tool::Pointer);
            for color in [
                egui::Color32::from_rgb(181, 131, 43),
                egui::Color32::from_rgb(250, 250, 250),
                egui::Color32::from_rgba_unmultiplied(181, 131, 43, 150),
            ] {
                let authored =
                    quantick_appearance::PremultipliedRgba::from_premultiplied(color.to_array());
                app.drawing_pane_mut().drawings.select(Some(index));
                let drawing = app.drawing_pane_mut().drawings.selected_mut().unwrap();
                drawing.style.color = authored;
                drawing
                    .tool
                    .set_inline_text(drawing.payload.as_mut(), "Authored annotation".into());
                for (selected, locked) in [(false, false), (true, false), (true, true)] {
                    app.drawing_pane_mut().drawings.set_locked_at(index, locked);
                    app.drawing_pane_mut()
                        .drawings
                        .select(selected.then_some(index));
                    let mut output = run_frame(&mut app, &ctx);
                    for _ in 0..3 {
                        output = run_frame(&mut app, &ctx);
                    }
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text)
                                if text.galley.text() == "Authored annotation" =>
                            {
                                assert!(
                                    shape
                                        .clip_rect
                                        .expand(1.0)
                                        .contains_rect(text.visual_bounding_rect())
                                );
                                Some(text)
                            }
                            _ => None,
                        })
                        .expect("actual drawing tool paints the annotation");
                    let palette = app.active_tab().drawing_pane().palette(&ctx);
                    let resolved = palette.resolved.legible_drawing(authored);
                    assert_eq!(
                        text.galley.job.sections[0].format.color.to_array(),
                        resolved.to_array(),
                        "existing canvas foreground resolution is preserved"
                    );
                    assert_eq!(
                        app.active_tab().drawing_pane().drawings.items()[index]
                            .style
                            .color,
                        authored
                    );
                }
                app.drawing_pane_mut().drawings.set_locked_at(index, false);
                app.drawing_pane_mut().drawings.select(Some(index));
                let drawing = app.active_tab().drawing_pane().drawings.items()[index].clone();
                let tab = app.runtime.tabs.active_id();
                let side = app.active_tab().focused_side();
                assert!(
                    app.surfaces
                        .drawing_chrome
                        .begin_inline_text_edit(tab, side, index, &drawing)
                );
                let mut output = run_frame(&mut app, &ctx);
                for _ in 0..3 {
                    output = run_frame(&mut app, &ctx);
                }
                // Let the foreground Area's opening fade settle before measuring its ink.
                for _ in 0..16 {
                    output = run_frame(&mut app, &ctx);
                }
                assert_eq!(
                    app.surfaces.drawing_chrome.inline_text_editing(),
                    Some(index)
                );
                let palette = crate::theme::get(&ctx);
                let resolved = palette.resolved.legible_drawing(authored);
                let text = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "Authored annotation" => {
                            assert!(
                                shape
                                    .clip_rect
                                    .expand(1.0)
                                    .contains_rect(text.visual_bounding_rect())
                            );
                            Some(text)
                        }
                        _ => None,
                    })
                    .expect("inline editor paints the authored content");
                if theme == quantick_appearance::Builtin::Light && color.a() == 255 {
                    let backing = output
                        .shapes
                        .iter()
                        .rev()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Rect(rect)
                                if rect.fill.a() == 255
                                    && rect.rect.contains_rect(text.visual_bounding_rect()) =>
                            {
                                Some(rect.fill)
                            }
                            _ => None,
                        })
                        .expect("opaque editor backing");
                    let background = quantick_appearance::PremultipliedRgba::from_premultiplied(
                        backing.to_array(),
                    );
                    assert!(
                        quantick_appearance::contrast::contrast_ratio(resolved, background) >= 4.5,
                        "inline text pair: theme={theme:?} layout={layout:?} authored={authored:?} resolved={resolved:?} background={background:?} text={:?}",
                        text.visual_bounding_rect()
                    );
                }
                assert_eq!(
                    text.galley.job.sections[0].format.color.to_array(),
                    resolved.to_array()
                );
                assert_eq!(
                    app.active_tab().drawing_pane().drawings.items()[index]
                        .style
                        .color,
                    authored
                );
                app.surfaces.drawing_chrome.end_inline_text_edit();
            }
        }
    }
}
