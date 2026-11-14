use super::*;
#[test]
fn first_sync_installs_text_roles_even_after_an_early_palette_read() {
    let ctx = egui::Context::default();
    let early = get(&ctx);
    ctx.style_mut(|style| {
        style
            .text_styles
            .get_mut(&egui::TextStyle::Body)
            .unwrap()
            .size = 77.0;
    });
    sync(&ctx, &Appearance::default());
    assert_eq!(
        ctx.style().text_styles[&egui::TextStyle::Body],
        early.font(FontRole::Body)
    );
}

#[test]
fn configured_painter_fonts_and_axis_reservations_grow_together() {
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut state = Appearance::default();
    sync(&ctx, &state);
    let dark = get(&ctx);
    let mut original_width = 0.0;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        original_width = ctx.fonts(|fonts| {
            fonts
                .layout_no_wrap("12345.67".into(), dark.axis_font(11.0), dark.text_primary())
                .size()
                .x
        });
    });
    state
        .apply("version = 1\nname = 'Large'\n[typography.chart_axis]\nsize = 22")
        .unwrap();
    sync(&ctx, &state);
    let large = get(&ctx);
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let width = ctx.fonts(|fonts| {
            fonts
                .layout_no_wrap(
                    "12345.67".into(),
                    large.axis_font(11.0),
                    large.text_primary(),
                )
                .size()
                .x
        });
        assert!(width > original_width * 1.8);
        assert!(width < quantick_appearance::AxisMetrics::new(&large.resolved.typography).gutter);
    });
}

#[test]
fn contexts_are_isolated_and_repeated_frames_reuse_the_adapter() {
    let dark_ctx = egui::Context::default();
    let light_ctx = egui::Context::default();
    let mut light = Appearance::default();
    light.apply(quantick_appearance::LIGHT).unwrap();
    sync(&dark_ctx, &Appearance::default());
    sync(&light_ctx, &light);
    assert_ne!(
        get(&dark_ctx).color(Token::Chrome),
        get(&light_ctx).color(Token::Chrome)
    );
    let before = get(&light_ctx);
    sync(&light_ctx, &light);
    assert!(Arc::ptr_eq(&before, &get(&light_ctx)));
    assert!(light.apply("version = 99\nname = 'invalid'").is_err());
    sync(&light_ctx, &light);
    assert!(Arc::ptr_eq(&before, &get(&light_ctx)));
}
#[test]
fn typography_updates_preserve_icon_font_definitions() {
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let mut state = Appearance::default();
    sync(&ctx, &state);
    let original = get(&ctx);
    state
        .apply("version = 1\nname = 'Modest'\n[typography.body]\nsize = 13.5\nfamily = 'monospace'")
        .unwrap();
    sync(&ctx, &state);
    assert!(!Arc::ptr_eq(&original, &get(&ctx)));
    assert_eq!(ctx.style().text_styles[&egui::TextStyle::Body].size, 13.5);
    // No set_fonts call is made by the adapter; this named fallback remains registered.
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(egui_phosphor::regular::GEAR);
            let font = &ctx.style().text_styles[&egui::TextStyle::Body];
            assert_eq!(font.family, egui::FontFamily::Monospace);
            ctx.fonts(|fonts| {
                assert!(fonts.has_glyphs(font, egui_phosphor::regular::GEAR));
                assert!(fonts.has_glyphs(font, "\u{0394}\u{00b1}"));
            });
        });
    });
}
