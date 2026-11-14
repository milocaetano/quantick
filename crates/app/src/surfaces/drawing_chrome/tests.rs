use super::*;
use quantick_ui_model::{HostEffect, Slot};

fn note() -> Drawing {
    let tool = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.holds_text())
        .unwrap();
    let mut store = crate::drawings::Drawings::default();
    store.place_on(
        tool,
        &crate::drawings::DrawingBand::Price,
        crate::drawings::ChartPoint::at(1.0, 10.0),
    );
    store.items()[0].clone()
}

#[test]
fn catalog_passes_commit_original_inline_target_once_before_queued_menu() {
    let mut owner = new_owner();
    let drawing = note();
    assert!(owner.begin_inline_text_edit(7, PaneSide::Flow, 3, &drawing));
    owner.ask_from_menu(
        42,
        DrawingChromeAsk {
            record_inline_edit: Some(Box::new(DrawingEdit {
                tab: 99,
                side: PaneSide::Flow,
                index: 8,
                before: drawing,
            })),
            manager_delete: Some(8),
            ..Default::default()
        },
    );
    let presets = crate::drawings::presets::PresetStore::load_from(std::path::Path::new(
        "missing-drawing-chrome-test-presets.toml",
    ));
    let env = DrawingEnv {
        linked_alerts: 0,
        pane_id: 42,
        selected: None,
        chart_area: None,
        automatic_bar_area: None,
        focused_chart_area: None,
        candles_right: None,
        legends: None,
        auto_range: None,
        selected_bbox: None,
        selected_band: None,
        tab: 8,
        side: PaneSide::Flow,
        drawing_tool_armed: false,
        toolbox_dock: ToolboxDock::default(),
        authored_objects: 0,
        manager_rows: &[],
        presets: &presets,
    };
    let ctx = egui::Context::default();
    let mut effects = DrawingChromeAsk::default();
    let mut original_commits = 0;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        for (slot, floating) in [
            (Slot::PinnedInspector, false),
            (Slot::DrawingChrome, true),
            (Slot::DrawingChrome, true),
        ] {
            owner.with_pass(
                slot,
                |response| {
                    for effect in response.model_effects {
                        let HostEffect::DrawingChrome(ask) = effect else {
                            panic!("drawing effect expected")
                        };
                        original_commits += usize::from(
                            ask.record_inline_edit
                                .as_ref()
                                .is_some_and(|edit| edit.tab == 7),
                        );
                        effects.merge(ask);
                    }
                },
                |view, session| view.paint_pass(session.reborrow(), ctx, &env, None, floating),
            );
        }
    });
    assert_eq!(original_commits, 1);
    let committed = effects.record_inline_edit.unwrap();
    assert_eq!(
        (committed.tab, committed.index),
        (7, 3),
        "inline close precedes queued menu effects"
    );
    assert_eq!(effects.manager_delete, Some(8));
    assert!(effects.content_editing_changed);
    assert_eq!(owner.content_editing_target(), None);
    assert_eq!(owner.menu_target(), None);
}

#[cfg(test)]
#[test]
pub(crate) fn pinned_inspector_borrowed_dispatch_shares_the_floating_owner() {
    use crate::surfaces::{BorrowedSurfaceEnv, Surfaces, ViewContext};
    let mut surfaces = Surfaces::default();
    surfaces
        .borrowed_second
        .update(quantick_ui_model::borrowed_second::Intent::Enable);
    let drawing = note();
    let presets = crate::drawings::presets::PresetStore::load_from(std::path::Path::new(
        "missing-pinned-borrowed-presets.toml",
    ));
    let mut env = DrawingEnv {
        linked_alerts: 0,
        pane_id: 42,
        selected: Some(SelectedDrawing {
            index: 3,
            drawing: &drawing,
        }),
        chart_area: None,
        automatic_bar_area: None,
        focused_chart_area: None,
        candles_right: None,
        legends: None,
        auto_range: None,
        selected_bbox: None,
        selected_band: None,
        tab: 7,
        side: PaneSide::Flow,
        drawing_tool_armed: false,
        toolbox_dock: ToolboxDock::default(),
        authored_objects: 0,
        manager_rows: &[],
        presets: &presets,
    };
    let ctx = egui::Context::default();
    let pass = |surfaces: &mut Surfaces, env: &DrawingEnv<'_>| {
        let mut response = crate::surfaces::SurfaceResponse::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let mut borrowed = BorrowedSurfaceEnv::Drawings(env);
            response = surfaces.draw_slot_unscoped_with_env(
                Slot::PinnedInspector,
                &mut ViewContext { ctx, ui: None },
                Some(&mut borrowed),
            );
        });
        response
    };
    // Closing this real row must never prevent another registered row from painting.
    let closed = pass(&mut surfaces, &env);
    assert!(surfaces.borrowed_second.view.painted.is_some());
    assert_eq!(surfaces.borrowed_second.view.immediate, Some(true));
    assert_eq!(
        closed
            .model_effects
            .iter()
            .filter(|effect| matches!(effect, HostEffect::BorrowedSecond(_)))
            .count(),
        2,
        "session and returned host output each arrive once"
    );
    assert!(!surfaces.drawing_chrome.inspector_pinned());
    // Selection reaches floating chrome before the user opens and pins settings.
    let floating = |surfaces: &mut Surfaces, env: &DrawingEnv<'_>| {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            surfaces.drawing_chrome.with_pass(
                Slot::DrawingChrome,
                |_| {},
                |view, session| {
                    view.paint_pass(session.reborrow(), ctx, env, None, true);
                },
            );
        });
    };
    floating(&mut surfaces, &env);
    surfaces.drawing_chrome.set_inspector_pinned(true);
    surfaces.drawing_chrome.set_inspector_open(true);
    let _ = pass(&mut surfaces, &env);
    assert_eq!(
        surfaces
            .drawing_chrome
            .model_state
            .core
            .shared
            .last_selection,
        Some(3)
    );
    let panel = egui::containers::panel::PanelState::load(
        &ctx,
        egui::Id::new((ctx.viewport_id(), "drawing_inspector_panel")),
    )
    .expect("the registered inspector reserves its native panel geometry");
    assert!(panel.rect.width() >= INSPECTOR_MIN_WIDTH_PX);
    // The unchanged floating pass reads the very state the borrowed pass updated.
    floating(&mut surfaces, &env);
    assert!(surfaces.drawing_chrome.inspector_pinned());
    assert!(surfaces.drawing_chrome.inspector_open());
    assert_eq!(
        surfaces
            .drawing_chrome
            .model_state
            .core
            .shared
            .last_selection,
        Some(3)
    );
    env.selected = None;
    let _ = pass(&mut surfaces, &env);
    // The later floating context bar owns selection-change closure.
    floating(&mut surfaces, &env);
    assert!(!surfaces.drawing_chrome.inspector_open());
    assert!(surfaces.borrowed_second.view.painted.is_some());
    assert_eq!(surfaces.borrowed_second.view.immediate, Some(false));
}
