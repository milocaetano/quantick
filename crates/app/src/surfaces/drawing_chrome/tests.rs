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
