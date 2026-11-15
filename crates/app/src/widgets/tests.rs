use super::*;

/// Every string one frame painted, so a test can ask what reached the
/// screen rather than what the builder was handed.
fn painted_text(output: &egui::FullOutput) -> Vec<String> {
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

/// The letter has to reach the screen, not just the builder. A vector
/// icon takes the *strokes* branch of the paint, which is exactly the
/// branch that used to have no way to say a word — so a regression here
/// would be silent: the lines would still be there and the button would
/// still work, with the one mark that says which Fibonacci it is gone.
#[test]
fn light_controls_use_resolved_ink_and_keep_explicit_accents() {
    let ctx = egui::Context::default();
    let mut state = quantick_appearance::Appearance::default();
    state.apply(quantick_appearance::LIGHT).unwrap();
    theme::sync(&ctx, &state);
    let palette = theme::get(&ctx);
    let idle = icon_paint(&palette, true, false, false, false, palette.accent());
    assert_eq!(idle.glyph, palette.text_muted());
    assert_ne!(idle.glyph, theme::TEXT_MUTED);
    let explicit = theme::ACCENT;
    let active = icon_paint(&palette, true, false, true, false, explicit);
    assert_eq!(active.glyph, explicit);
    assert_eq!(active.fill, Some(theme::active_tint(explicit)));
    assert_eq!(palette.ink(quantick_engine::Side::Buy), palette.buy_ink());
    assert_eq!(palette.drawing_swatches()[0], palette.accent());
}

#[test]
fn a_vector_icon_paints_the_letter_it_was_given() {
    let strokes: crate::drawings::IconStrokes = &[&[(0.4, 0.9), (0.9, 0.2)]];
    let letter = crate::drawings::IconLetter {
        text: "R",
        at: (0.15, 0.26),
        height: 0.46,
    };
    let ctx = egui::Context::default();
    let draw = |with_letter: Option<crate::drawings::IconLetter>| {
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                IconButton::new("", TOOLRAIL_ICON)
                    .vector_icon(strokes, &[], with_letter)
                    .show(ui);
            });
        })
    };
    // Twice: egui lays a frame out against the previous one, and the
    // first frame of a fresh context has no sizes to paint into yet.
    let _ = draw(Some(letter));
    assert!(
        painted_text(&draw(Some(letter)))
            .iter()
            .any(|text| text == "R"),
        "a lettered vector icon must paint its letter"
    );
    let _ = draw(None);
    assert!(
        !painted_text(&draw(None)).iter().any(|text| text == "R"),
        "an icon given no letter must paint none"
    );
}

#[test]
fn idle_is_muted_on_bare_chrome() {
    let paint = icon_paint(
        &theme::get(&egui::Context::default()),
        true,
        false,
        false,
        false,
        theme::ACCENT,
    );
    assert_eq!(paint.fill, None);
    assert_eq!(paint.glyph, theme::TEXT_MUTED);
}

#[test]
fn hover_lifts_the_glyph_on_a_border_fill() {
    let paint = icon_paint(
        &theme::get(&egui::Context::default()),
        true,
        false,
        false,
        true,
        theme::ACCENT,
    );
    assert_eq!(paint.fill, Some(theme::BORDER));
    assert_eq!(paint.glyph, theme::TEXT_PRIMARY);
}

#[test]
fn active_tints_in_the_layer_accent_even_under_hover() {
    for hovered in [false, true] {
        let paint = icon_paint(
            &theme::get(&egui::Context::default()),
            true,
            false,
            true,
            hovered,
            theme::BUY,
        );
        assert_eq!(paint.fill, Some(theme::active_tint(theme::BUY)));
        assert_eq!(paint.glyph, theme::BUY);
    }
}

#[test]
fn precedence_is_disabled_pressed_active_hover_idle() {
    // The full boolean cross-product, so no state combination can drift.
    for pressed in [false, true] {
        for active in [false, true] {
            for hovered in [false, true] {
                let disabled = icon_paint(
                    &theme::get(&egui::Context::default()),
                    false,
                    pressed,
                    active,
                    hovered,
                    theme::ACCENT,
                );
                assert_eq!(disabled.fill, None, "disabled beats everything");
                assert!(
                    disabled.glyph.a() < theme::TEXT_MUTED.a(),
                    "disabled glyph must be dimmer than idle"
                );

                let paint = icon_paint(
                    &theme::get(&egui::Context::default()),
                    true,
                    pressed,
                    active,
                    hovered,
                    theme::ACCENT,
                );
                let expected_fill = if pressed {
                    Some(theme::press_tint(theme::ACCENT))
                } else if active {
                    Some(theme::active_tint(theme::ACCENT))
                } else if hovered {
                    Some(theme::BORDER)
                } else {
                    None
                };
                assert_eq!(paint.fill, expected_fill);
            }
        }
    }
}

#[test]
fn icon_geometries_match_the_model() {
    assert_eq!(TOOLBAR_ICON.glyph, 16.0);
    assert_eq!(TOOLBAR_ICON.hit, 28.0);
    assert_eq!(RAIL_ICON.glyph, 20.0);
    assert_eq!(RAIL_ICON.hit, 30.0);
    assert_eq!(TOOLRAIL_ICON.glyph, 18.0);
    assert_eq!(TOOLRAIL_ICON.hit, 32.0);
}

#[test]
fn marker_bars_hug_their_edge_inset_from_both_ends() {
    let rect = egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(32.0, 32.0));
    let left = MarkerEdge::Left.bar(rect);
    assert_eq!(left.left(), rect.left());
    assert_eq!(left.width(), ACTIVE_MARKER_WIDTH_PX);
    assert_eq!(left.top(), rect.top() + ACTIVE_MARKER_INSET_PX);
    assert_eq!(left.bottom(), rect.bottom() - ACTIVE_MARKER_INSET_PX);
    let top = MarkerEdge::Top.bar(rect);
    assert_eq!(top.top(), rect.top());
    assert_eq!(top.height(), ACTIVE_MARKER_WIDTH_PX);
    let bottom = MarkerEdge::Bottom.bar(rect);
    assert_eq!(bottom.bottom(), rect.bottom());
}

/// The close prompt over a selected drawing's context bar (a Foreground
/// area): the prompt is the top layer where it stands, the scrim takes
/// the click aimed at the bar, and the buttons stand right, Enter's last.
#[test]
fn a_prompt_paints_above_and_blocks_a_foreground_overlay() {
    let ctx = egui::Context::default();
    let prompt = quantick_session::prompt::Prompt {
        id: "close".to_owned(),
        title: "Close WINZ26".to_owned(),
        body: "This tab changed its layout since Scalp was last saved (RSI, CVD, 3 drawings)."
            .to_owned(),
        note: None,
        buttons: ["Cancel", "Close without saving", "Save to Scalp"]
            .map(str::to_owned)
            .to_vec(),
        enter: 2,
        escape: 0,
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let corner = egui::pos2(20.0, 20.0);
    let frame = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let mut clicked = false;
        let output = ctx.run(input, |ctx| {
            let bar = egui::Area::new(egui::Id::new("context_bar"));
            bar.order(egui::Order::Foreground)
                .fixed_pos(egui::Pos2::ZERO)
                .show(ctx, |ui| {
                    let button = egui::Button::new("bar").min_size(screen.size());
                    clicked = ui.add(button).clicked();
                });
            let _ = ask(ctx, &prompt);
        });
        (clicked, output)
    };
    let _ = frame(Vec::new());
    let (_, output) = frame(Vec::new());
    let center = ctx.layer_id_at(screen.center()).expect("a layer");
    assert_eq!(center.order, egui::Order::Tooltip, "the prompt is on top");
    assert_eq!(center.id, egui::Id::new("close"));
    let mut texts = Vec::new();
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            texts.push((text.galley.text().to_owned(), rect));
        }
    }
    let at = |word: &str| {
        let found = texts.iter().find(|(text, _)| text.starts_with(word));
        found.map(|(_, rect)| *rect).expect(word)
    };
    let (body, save) = (at("This tab"), at("Save to"));
    assert!(at("Cancel").max.x < at("Close without").min.x, "{texts:?}");
    assert!(
        at("Close without").max.x < save.min.x,
        "Enter's answer last"
    );
    let right = (save.max.x - body.max.x).abs();
    assert!(right < 12.0, "the row ends where the body does: {texts:?}");
    let press = |pressed| egui::Event::PointerButton {
        pos: corner,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = frame(vec![egui::Event::PointerMoved(corner), press(true)]);
    let (clicked, _) = frame(vec![press(false)]);
    assert!(!clicked, "the scrim took the click");
    let _ = frame(Vec::new());
    let center = ctx.layer_id_at(screen.center()).expect("a layer");
    assert_eq!(
        center.id,
        egui::Id::new("close"),
        "and the prompt stays on top"
    );
}
