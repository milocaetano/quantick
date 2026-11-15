use super::*;

#[test]
fn nested_paint_scopes_restore_ui_and_do_not_leak_between_contexts() {
    let ctx = egui::Context::default();
    let other = egui::Context::default();
    let mut state = Appearance::default();
    state.apply(quantick_appearance::LIGHT).unwrap();
    sync(&ctx, &state);
    sync(&other, &state);
    let root = get(&ctx);
    let surface = bookmap(&ctx);
    let visuals = ctx.style().visuals.clone();
    {
        let _outer = bookmap_scope(&ctx);
        assert!(Arc::ptr_eq(&get(&ctx), &surface));
        assert!(Arc::ptr_eq(&axes(&ctx), &root));
        assert_eq!(get(&other).canvas(), root.canvas());
        {
            let _inner = bookmap_scope(&ctx);
            assert!(Arc::ptr_eq(&axes(&ctx), &root));
        }
        assert!(Arc::ptr_eq(&get(&ctx), &surface));
        assert_eq!(ctx.style().visuals, visuals);
    }
    assert!(Arc::ptr_eq(&get(&ctx), &root));
    sync(&ctx, &state);
    assert!(Arc::ptr_eq(&bookmap(&ctx), &surface));
    state.apply(quantick_appearance::DARK).unwrap();
    sync(&ctx, &state);
    assert!(!Arc::ptr_eq(&bookmap(&ctx), &surface));
    assert_eq!(
        get(&ctx).canvas(),
        egui::Color32::from_rgb(0x13, 0x17, 0x22)
    );
    assert_ne!(get(&ctx).canvas(), surface.canvas());
    assert_eq!(get(&other).canvas(), root.canvas());
}

/// Light's lit flow pane paints on white with its own order-flow palette;
/// Dark has none and paints its island whatever is asked.
#[test]
fn the_lit_surface_is_lights_white_and_darks_island() {
    let ctx = egui::Context::default();
    let mut state = Appearance::default();
    state.apply(quantick_appearance::LIGHT).unwrap();
    sync(&ctx, &state);
    let lit = surface(&ctx, ChartSurface::LitFlow);
    assert_eq!(lit.color(Token::ChartCanvas), egui::Color32::WHITE);
    assert!(lit.resolved.orderflow.surface_bound);
    assert!(!Arc::ptr_eq(&lit, &bookmap(&ctx)));
    {
        let _scope = scope(&ctx, lit.clone());
        assert!(Arc::ptr_eq(&get(&ctx), &lit));
    }
    state.apply(quantick_appearance::DARK).unwrap();
    sync(&ctx, &state);
    let dark = surface(&ctx, ChartSurface::LitFlow);
    assert!(Arc::ptr_eq(&dark, &bookmap(&ctx)));
    assert!(Arc::ptr_eq(&surface(&ctx, ChartSurface::Paper), &get(&ctx)));
}
