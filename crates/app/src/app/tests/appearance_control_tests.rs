//! Real gateway authorization, publication and exact durable retries.
use super::*;
use quantick_appearance::{LIGHT, Token};

fn state(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({"scopes": ["appearance.state"]}),
    );
    success_result(&response)["scopes"]["appearance.state"]["value"].clone()
}

#[test]
fn appearance_gateway_publishes_every_owner_atomically_and_replays_durable_retry() {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let (mut app, _commands) = app_with_history(4);
    let files = crate::scratch::ScratchDir::new("appearance-control");
    let path = files.join("appearance.toml");
    app.runtime.appearance = quantick_stores::appearance::AppearanceStore::open(path.clone());
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("appearance-live");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = state(&mut app, &mut observer);
    assert_eq!(before["origin"], "bundled_dark");
    assert!(
        !before
            .to_string()
            .contains(&path.to_string_lossy().to_string())
    );
    let payload = json!({"source": LIGHT});
    let (denied, _) = unkeyed_call(&mut app, &mut observer, "appearance.apply", payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert_eq!(state(&mut app, &mut observer), before);
    let (applied, _) = unkeyed_call(&mut app, &mut cockpit, "appearance.apply", payload);
    assert_eq!(error_code(&applied), None);
    assert!(!path.exists());
    // No intervening frame: a subsequent command creates with the published version.
    let palette = crate::theme::get(&ctx);
    assert_eq!(palette.resolved, *app.runtime.appearance.resolved());
    assert_eq!(
        palette.resolved.orderflow,
        app.runtime.appearance.resolved().orderflow
    );
    assert_eq!(
        app.runtime.style.canvas.background_rgba(),
        palette.resolved.color(Token::Canvas).to_array()
    );
    let text = crate::drawings::DrawingTool::by_id("text").unwrap();
    let drawing = crate::drawings::new_drawing_from_defaults(&app.drawings.presets, text);
    assert_eq!(drawing.style.color, palette.resolved.color(Token::Accent));
    let revision = app.runtime.style_revision;
    let fonts = ctx.style().text_styles.clone();
    let formatted = format!("# same semantics\n{LIGHT}");
    let (same, _) = unkeyed_call(
        &mut app,
        &mut cockpit,
        "appearance.apply",
        json!({"source": formatted}),
    );
    assert_eq!(success_result(&same)["changed"], false);
    assert!(std::sync::Arc::ptr_eq(&palette, &crate::theme::get(&ctx)));
    assert_eq!(app.runtime.style_revision, revision);
    let input = json!({"source": LIGHT, "persist": true});
    let (first, served) = keyed_call(
        &mut app,
        &mut cockpit,
        "appearance-first",
        "appearance.apply",
        input.clone(),
        "appearance-save",
    );
    assert_eq!(error_code(&first), None);
    assert!(served.iter().any(|request| request.began));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
    assert_eq!(success_result(&first)["state"]["durable"], true);
    std::fs::remove_file(&path).unwrap();
    let (retry, served) = keyed_call(
        &mut app,
        &mut cockpit,
        "appearance-retry",
        "appearance.apply",
        input.clone(),
        "appearance-save",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert!(served.is_empty());
    assert!(!path.exists(), "keyed retry does not repeat the write");
    let (saved, _) = unkeyed_call(&mut app, &mut cockpit, "appearance.apply", input);
    assert_eq!(error_code(&saved), None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
    let before_invalid = state(&mut app, &mut observer);
    for source in [
        "version = 99\nname = 'bad'",
        "version = 1\nname = 'bad'\n[colors]\ncanvas = 'bad'",
    ] {
        let (refused, _) = unkeyed_call(
            &mut app,
            &mut cockpit,
            "appearance.apply",
            json!({"source": source, "persist": true}),
        );
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
        if let ResponseOutcome::Failure { error } = refused.outcome {
            assert_eq!(
                error.context.details.as_ref().unwrap()["code"],
                "invalid_theme"
            );
            assert!(error.context.details.unwrap()["field"].is_string());
        }
        assert_eq!(state(&mut app, &mut observer), before_invalid);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LIGHT);
    }
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(std::sync::Arc::ptr_eq(&palette, &crate::theme::get(&ctx)));
    assert_eq!(ctx.style().text_styles, fonts);
    assert_eq!(app.runtime.style_revision, revision);
    for input in [
        json!({}),
        json!({"builtin":"sepia"}),
        json!({"source":LIGHT,"builtin":"light"}),
    ] {
        let (refused, _) = unkeyed_call(&mut app, &mut cockpit, "appearance.apply", input);
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
        assert_eq!(state(&mut app, &mut observer), before_invalid);
    }
    let (chosen, _) = unkeyed_call(
        &mut app,
        &mut cockpit,
        "appearance.apply",
        json!({"builtin":"dark","persist":true}),
    );
    assert_eq!(error_code(&chosen), None);
    assert_eq!(success_result(&chosen)["state"]["choice"], "dark");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        LIGHT,
        "native choice preserves authored input"
    );
    assert_eq!(
        quantick_stores::appearance::AppearanceStore::open(path).builtin(),
        Some(quantick_appearance::Builtin::Dark)
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}
