//! Drive the actual toolbar and native choice through the harness RawInput path.
use super::*;
use quantick_appearance::Builtin;
use quantick_stores::appearance::AppearanceStore;

fn drive(app: &mut QuantickApp, ctx: &egui::Context, token: &str) -> Vec<String> {
    app.chrome.harness = crate::harness::Harness::capture(
        &crate::hooks::ScenarioInputs::from_pairs(&[("QUANTICK_MENU", token)]),
    );
    let mut text = Vec::new();
    for _ in 0..30 {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
            ..Default::default()
        };
        eframe::App::raw_input_hook(app, ctx, &mut input);
        let now = frame_instant(ctx);
        let output = ctx.run(input, |ctx| app.draw_frame(ctx, now));
        text = painted_text(&output);
    }
    text
}

#[test]
fn native_theme_menu_opens_custom_label_without_changing_the_store() {
    let (mut app, _, _, _) = test_app();
    let files = crate::scratch::ScratchDir::new("theme-menu-custom");
    let path = files.join("appearance.toml");
    std::fs::write(&path, "version = 1\nname = 'Dark'\n").unwrap();
    app.runtime.appearance = AppearanceStore::open(path.clone());
    let before = app.runtime.appearance.inspection_revision();
    let text = drive(&mut app, &egui::Context::default(), "theme");
    for label in ["Custom theme", "Dark", "Light"] {
        assert!(
            text.iter().any(|item| item == label),
            "missing {label}: {text:?}"
        );
    }
    assert_eq!(app.runtime.appearance.builtin(), None);
    assert_eq!(app.runtime.appearance.inspection_revision(), before);
    assert!(!files.join("appearance.toml.choice.toml").exists());
}

#[test]
fn native_theme_menu_clicks_save_both_choices_and_preserve_authored_source() {
    for (token, choice) in [
        ("theme-light", Builtin::Light),
        ("theme-dark", Builtin::Dark),
    ] {
        let (mut app, _, _, _) = test_app();
        let files = crate::scratch::ScratchDir::new(token);
        let path = files.join("appearance.toml");
        let authored = "version = 1\nname = 'Custom'\n";
        std::fs::write(&path, authored).unwrap();
        app.runtime.appearance = AppearanceStore::open(path.clone());
        let before = app.runtime.appearance.inspection_revision();
        let ctx = egui::Context::default();
        let text = drive(&mut app, &ctx, token);
        assert!(
            !text.iter().any(|item| item == "Custom theme"),
            "menu closes after selection"
        );
        assert_eq!(app.runtime.appearance.builtin(), Some(choice));
        assert!(
            app.runtime.appearance.inspection_revision() > before,
            "native click publishes"
        );
        assert_eq!(
            crate::theme::get(&ctx).canvas().to_array(),
            quantick_appearance::ResolvedTheme::parse(choice.source())
                .unwrap()
                .color(quantick_appearance::Token::Canvas)
                .to_array(),
        );
        assert_eq!(AppearanceStore::open(path.clone()).builtin(), Some(choice));
        assert_eq!(std::fs::read_to_string(path).unwrap(), authored);
    }
}

#[test]
fn native_theme_menu_failed_save_keeps_custom_choice_and_shows_error() {
    let (mut app, _, _, _) = test_app();
    let files = crate::scratch::ScratchDir::new("theme-menu-failure");
    let path = files.join("appearance.toml");
    std::fs::write(&path, "version = 1\nname = 'Custom'\n").unwrap();
    app.runtime.appearance = AppearanceStore::open(path);
    std::fs::create_dir(files.join("appearance.toml.choice.toml")).unwrap();
    let before = app.runtime.appearance.inspection_revision();
    let text = drive(&mut app, &egui::Context::default(), "theme-light");
    assert_eq!(app.runtime.appearance.builtin(), None);
    assert_eq!(app.runtime.appearance.inspection_revision(), before);
    assert!(
        text.iter().any(|item| item == "Custom theme"),
        "menu remains open: {text:?}"
    );
    assert!(
        text.iter().any(|item| item.starts_with("theme file:")),
        "actionable error: {text:?}"
    );
}
