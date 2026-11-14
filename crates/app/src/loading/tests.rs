use super::*;

fn words(reconnect: bool) -> OverlayWords {
    OverlayWords {
        verb: "No connection to",
        meta: "Replay · never connected".to_owned(),
        reconnect,
    }
}

/// The named stack, read off what egui actually paints: the chart first,
/// then the overlay and the slot, then a menu on top of them all.
#[test]
fn the_chart_loading_and_a_menu_paint_in_the_named_order() {
    const CHART: egui::Color32 = egui::Color32::from_rgb(1, 2, 3);
    const MENU: egui::Color32 = egui::Color32::from_rgb(4, 5, 6);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.painter().rect_filled(screen, 0.0, CHART);
                let _ = draw_overlay(ui, screen, "X", &words(false), (0, 1));
                let slot = Slot {
                    job: false,
                    text: "the slot",
                };
                draw_slot(ui, screen.right(), screen, &slot, None);
            });
            let menu = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("menu"));
            ctx.layer_painter(menu).rect_filled(screen, 0.0, MENU);
        },
    );
    let at = |wanted: &dyn Fn(&egui::Shape) -> bool| {
        output
            .shapes
            .iter()
            .position(|clipped| wanted(&clipped.shape))
            .expect("painted")
    };
    let filled = |fill| move |shape: &egui::Shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == fill);
    let chart = at(&filled(CHART));
    let menu = at(&filled(MENU));
    let overlay = at(&|shape| matches!(shape, egui::Shape::Mesh(_)));
    let slot =
        at(&|shape| matches!(shape, egui::Shape::Text(text) if text.galley.job.text == "the slot"));
    assert!(
        chart < overlay && overlay < menu,
        "{chart} {overlay} {menu}"
    );
    assert!(chart < slot && slot < menu, "{chart} {slot} {menu}");
}

/// The slot paints its words in the ink it is given, and the dots that pad
/// a walking step stay clear, so the dots still walk on the band.
#[test]
fn the_slot_keeps_its_padding_dots_clear_in_any_ink() {
    const INK: egui::Color32 = egui::Color32::from_rgb(242, 244, 247);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let slot = Slot {
                job: true,
                text: "loading.",
            };
            draw_slot(ui, screen.right(), screen, &slot, Some(INK));
        });
    });
    let words = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with("loading") => {
                Some(text.clone())
            }
            _ => None,
        })
        .expect("the slot's words");
    assert_eq!(words.override_text_color, None, "no blanket recolour");
    let colours: Vec<_> = words
        .galley
        .job
        .sections
        .iter()
        .map(|section| section.format.color)
        .collect();
    assert_eq!(colours, [INK, egui::Color32::TRANSPARENT], "{colours:?}");
}

/// The chip paints in grades of the ink it is given, the band's light ink,
/// never the loading palette tuned for a pale surface: the border and the
/// words in their shares, an ok dot whole and a waiting one faint.
#[test]
fn the_chip_grades_the_ink_it_is_given() {
    const INK: egui::Color32 = egui::Color32::from_rgb(242, 244, 247);
    let ctx = egui::Context::default();
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply(quantick_appearance::LIGHT).unwrap();
    crate::theme::sync(&ctx, &appearance);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let dot = |name, look| Dot {
        name,
        look,
        hover: "",
    };
    let dots = [
        dot("feed", DotLook::Solid),
        dot("book", DotLook::Faint),
        dot("tape", DotLook::Solid),
    ];
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            draw_chip(ui, screen.right(), screen, &dots, true, INK);
        });
    });
    let (mut fills, mut borders, mut words) = (Vec::new(), Vec::new(), Vec::new());
    for clipped in &output.shapes {
        match &clipped.shape {
            egui::Shape::Circle(circle) => fills.push(circle.fill),
            egui::Shape::Rect(rect) if rect.stroke.width > 0.0 => {
                borders.push(rect.stroke.color);
            }
            egui::Shape::Text(text) => words.push(text.galley.job.sections[0].format.color),
            _ => {}
        }
    }
    let faint = INK.gamma_multiply(CHIP_FAINT_SHARE);
    assert_eq!(fills, [INK, faint, INK]);
    assert_eq!(borders, [INK.gamma_multiply(CHIP_BORDER_SHARE)]);
    assert_eq!(words, [INK.gamma_multiply(CHIP_WORD_SHARE); 3]);
    let palette = crate::theme::get(&ctx);
    assert!(!fills.contains(&palette.loading_title()));
    assert_ne!(borders[0], palette.loading_border());
}

/// A control that would leave the rect is not drawn, nor answers a press,
/// and the group recentres without it.
#[test]
fn a_control_with_no_room_is_not_offered() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let roomy =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 400.0));
                assert!(reconnect_fits(ctx, roomy, "X", &words(true)));
                let (control, _) = draw_overlay(ui, roomy, "X", &words(true), (0, 1));
                assert!(roomy.contains_rect(control.expect("it fits")));

                let short =
                    egui::Rect::from_min_size(egui::pos2(0.0, 500.0), egui::vec2(800.0, 40.0));
                assert!(!reconnect_fits(ctx, short, "X", &words(true)));
                let (control, pressed) = draw_overlay(ui, short, "X", &words(true), (0, 1));
                assert!(control.is_none() && !pressed);
            });
        },
    );
}

/// The asset has no plate: the logo is cells, not a filled square.
#[test]
fn the_logo_samples_a_plate_less_asset() {
    let (alpha, side) = logo_alpha();
    let shatter = quantick_chart::logo_shatter::Shatter::new(800.0, 600.0, alpha, *side, 1);
    let grid = (600.0 * 0.42 / shatter.cell_px()).ceil() as usize;
    assert!(!shatter.is_empty());
    assert!(
        shatter.len() < grid * grid / 2,
        "a plate would fill the square: {} cells of {}",
        shatter.len(),
        grid * grid
    );
}

/// A press on the control is an answer; the same press beside it is not.
#[test]
fn the_overlay_control_answers_a_real_press() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let press_at = |source: &str, at: Option<egui::Pos2>| {
        let ctx = egui::Context::default();
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(source).unwrap();
        crate::theme::sync(&ctx, &appearance);
        let frame = |events: Vec<egui::Event>| {
            let mut answer = (None, false);
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        answer = draw_overlay(ui, ui.max_rect(), "X", &words(true), (0, 1));
                    });
                },
            );
            answer
        };
        let control = frame(Vec::new()).0.expect("laid out");
        let at = at.unwrap_or(control.center());
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = frame(vec![egui::Event::PointerMoved(at)]);
        let _ = frame(vec![button(true)]);
        let result = frame(vec![button(false)]);
        assert_eq!(
            result.0,
            Some(control),
            "painting never moves the hit region"
        );
        (control, result.1)
    };
    let dark = press_at(quantick_appearance::DARK, None);
    let light = press_at(quantick_appearance::LIGHT, None);
    assert!(dark.1 && light.1);
    assert_eq!(
        dark.0, light.0,
        "Light and Dark offer the same reconnect target"
    );
    for source in [quantick_appearance::DARK, quantick_appearance::LIGHT] {
        assert!(!press_at(source, Some(egui::pos2(20.0, 20.0))).1);
    }
}

/// Reconnect's ink is chosen at paint time, so a hover reaches the pixels:
/// the tessellated words change colour, resting in `connection_action`
/// and lit in `loading_title`, in both themes.
#[test]
fn the_reconnect_hover_ink_reaches_the_pixels() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        let ctx = egui::Context::default();
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(source).unwrap();
        crate::theme::sync(&ctx, &appearance);
        let frame = |events: Vec<egui::Event>| {
            let mut control = None;
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let output = ctx.run(raw, |ctx| {
                let panel = egui::CentralPanel::default().frame(egui::Frame::none());
                panel.show(ctx, |ui| {
                    control = draw_overlay(ui, screen, "X", &words(true), (0, 1)).0;
                });
            });
            let words: Vec<_> = output
                .shapes
                .into_iter()
                .filter(|clipped| {
                    matches!(&clipped.shape,
                        egui::Shape::Text(text) if text.galley.text() == RECONNECT_LABEL)
                })
                .collect();
            assert!(!words.is_empty(), "Reconnect is painted");
            let control = control.expect("laid out");
            let mut inks = std::collections::BTreeSet::new();
            for primitive in ctx.tessellate(words, 1.0) {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    inks.extend(mesh.vertices.iter().map(|vertex| vertex.color.to_array()));
                }
            }
            (inks, control)
        };
        let (rest, control) = frame(Vec::new());
        let _ = frame(vec![egui::Event::PointerMoved(control.center())]);
        let (lit, _) = frame(Vec::new());
        let palette = crate::theme::get(&ctx);
        let only = |color: egui::Color32| std::collections::BTreeSet::from([color.to_array()]);
        assert_eq!(
            rest,
            only(palette.connection_action()),
            "{}",
            palette.resolved.name
        );
        assert_eq!(
            lit,
            only(palette.loading_title()),
            "{}",
            palette.resolved.name
        );
        assert_ne!(rest, lit);
    }
}

/// The overlay paints a drag through: only its one control takes a press,
/// and without the control nothing does.
#[test]
fn the_overlay_takes_no_input_but_its_one_control() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let mut control = None;
    for _ in 0..3 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    control = draw_overlay(ui, ui.max_rect(), "DOGEUSDT", &words(true), (0, 1)).0;
                });
            },
        );
    }
    let control = control.expect("the control was laid out");
    assert!(control.width() < 200.0, "a word, not a band: {control:?}");
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let (none, pressed) = draw_overlay(ui, ui.max_rect(), "X", &words(false), (0, 1));
                assert!(none.is_none() && !pressed, "no control, no region");
            });
        },
    );
}

/// No plate stands behind the words in either theme, and the logo paints
/// after them, so its flying cells cross over the text.
#[test]
fn the_logo_paints_over_bare_words() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        let ctx = egui::Context::default();
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(source).unwrap();
        crate::theme::sync(&ctx, &appearance);
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let output = ctx.run(raw, |ctx| {
            let panel = egui::CentralPanel::default().frame(egui::Frame::none());
            panel.show(ctx, |ui| {
                draw_overlay(ui, screen, "X", &words(true), (0, 1))
            });
        });
        let shapes: Vec<_> = output.shapes.iter().map(|clipped| &clipped.shape).collect();
        let logo = shapes
            .iter()
            .rposition(|shape| matches!(shape, egui::Shape::Mesh(_)))
            .expect("the logo");
        let mut text: Vec<_> = (0..shapes.len())
            .filter(|&index| matches!(shapes[index], egui::Shape::Text(_)))
            .collect();
        // A bold action is struck twice; the strike is one word.
        text.dedup_by(|a, b| match (shapes[*a], shapes[*b]) {
            (egui::Shape::Text(a), egui::Shape::Text(b)) => a.galley.text() == b.galley.text(),
            _ => false,
        });
        assert_eq!(text.len(), 3, "title, metadata and Reconnect");
        assert!(text.iter().all(|&index| index < logo), "{text:?} {logo}");
        assert!(
            !shapes.iter().any(|shape| matches!(shape,
                    egui::Shape::Rect(rect) if rect.fill.a() > 0)),
            "no backing plate"
        );
    }
}

/// The walking dots never move the words: each step lays out as wide as
/// `...`, in the overlay's monospace and the slot's proportional font, and
/// words without dots get no padding.
#[test]
fn walking_dots_keep_the_words_still() {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let palette = crate::theme::get(ctx);
        for font in [palette.mono_font(META_PT), palette.body_font(SLOT_PT)] {
            let width = |text: &str| {
                ctx.fonts(|fonts| {
                    steady(
                        palette.loading_meta(),
                        fonts,
                        text,
                        font.clone(),
                        f32::INFINITY,
                    )
                    .size()
                    .x
                })
            };
            let widest = width("Replay · attempting...");
            assert_eq!(width("Replay · attempting."), widest);
            assert_eq!(width("Replay · attempting.."), widest);
            let bare = ctx.fonts(|fonts| {
                fonts.layout_no_wrap(
                    "loading history".to_owned(),
                    font.clone(),
                    palette.loading_meta(),
                )
            });
            assert_eq!(width("loading history"), bare.size().x);
        }
    });
}
