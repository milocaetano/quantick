use super::*;

pub(crate) fn update_indicator_settings(
    app: &mut QuantickApp,
    intent: quantick_ui_model::indicator_settings::Intent,
) {
    let mut effects = Vec::new();
    app.surfaces.indicator_settings.with_updates(
        |effect| effects.extend(effect.model_effects),
        |_, session| session.update(intent),
    );
    for effect in effects {
        if let quantick_ui_model::HostEffect::IndicatorSettings(effects) = effect {
            assert!(
                effects.local_operation.is_none(),
                "fixture does not execute preset IO"
            );
            for change in effects.changes {
                app.indicator_wiring()
                    .apply_indicator_settings_change(change.into());
            }
        }
    }
}

/// A pane indicator with `values` as its single plot, delivered the way
/// the worker delivers one.
pub(super) fn add_pane_indicator(app: &mut QuantickApp, title: &str, values: Vec<f64>) -> SlotId {
    let slot = app
        .active_tab_mut()
        .flow_pane
        .indicators
        .allocate_slot("test.indicator");
    rebuild_pane_indicator(app, slot, title, values);
    slot
}

/// The same indicator, recomputed — an edited input, a hot reload, older
/// trades re-cutting the series.
pub(super) fn rebuild_pane_indicator(
    app: &mut QuantickApp,
    slot: SlotId,
    title: &str,
    values: Vec<f64>,
) {
    app.active_tab_mut().flow_pane.indicators.apply(
        crate::indicator_worker::event_fixture::rebuilt(
            slot,
            quantick_indicators::IndicatorDescriptor {
                title: title.to_owned(),
                short_title: None,
                overlay: false,
                plots: vec![quantick_indicators::PlotSpec {
                    id: quantick_indicators::PlotId::new(0),
                    title: title.to_owned(),
                    style: quantick_indicators::PlotStyle::Line,
                    base_color: quantick_indicators::Rgba8::opaque(255, 255, 255),
                    width: 1.0,
                    offset: 0,
                    marker: None,
                }],
                inputs: Vec::new(),
                fills: Vec::new(),
            },
            vec![values],
        ),
    );
}

/// The `(lo, hi)` a pane is drawing with right now: its manual range if the
/// user has taken control of the axis, else the fit the last frame made.
pub(super) fn pane_range(app: &QuantickApp, slot: SlotId) -> (f64, f64) {
    let view = app
        .active_tab()
        .flow_pane
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == slot)
        .expect("the pane is still there");
    let auto = view.last_auto.expect("a frame fitted this pane");
    view.scale.resolve(auto)
}

/// Whether a pane is still auto-fitting its values.
pub(super) fn pane_is_auto(app: &QuantickApp, slot: SlotId) -> bool {
    app.active_tab()
        .flow_pane
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == slot)
        .expect("the pane is still there")
        .scale
        .is_auto()
}

/// The gutter band beside pane `index`, computed the way the frame does.
pub(super) fn pane_gutter(app: &QuantickApp, index: usize) -> egui::Rect {
    let pane = &app.active_tab().flow_pane;
    let areas = plot_split(
        pane.frame.plot_area.expect("a frame has been drawn"),
        pane.live_strip_width(app.active_tab().capabilities(&app.config)),
        pane.indicators
            .pane_sizing(&mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES]),
    );
    areas.pane_gutters[index]
}

/// The plot band of pane `index` — where its curve is drawn, as opposed to
/// the gutter where its numbers are.
pub(super) fn pane_body(app: &QuantickApp, index: usize) -> egui::Rect {
    let pane = &app.active_tab().flow_pane;
    let areas = plot_split(
        pane.frame.plot_area.expect("a frame has been drawn"),
        pane.live_strip_width(app.active_tab().capabilities(&app.config)),
        pane.indicators
            .pane_sizing(&mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES]),
    );
    areas.indicator_panes[index].rect
}

/// The pane band as the last drawn frame carved it.
pub(super) fn pane_slots(app: &QuantickApp) -> Vec<crate::indicators::PaneSlot> {
    let pane = &app.active_tab().flow_pane;
    plot_split(
        pane.frame.plot_area.expect("a frame has been drawn"),
        pane.live_strip_width(app.active_tab().capabilities(&app.config)),
        pane.indicators
            .pane_sizing(&mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES]),
    )
    .indicator_panes
}

/// An app showing every pane it allows, drawn once at `size`.
pub(super) fn app_with_full_pane_band(
    ctx: &egui::Context,
    size: egui::Vec2,
) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, cmd_rx) = app_with_history(200);
    for index in 0..crate::indicators::MAX_PANES {
        add_pane_indicator(
            &mut app,
            &format!("pane{index}"),
            (0..200).map(f64::from).collect(),
        );
    }
    run_frame_at(&mut app, ctx, size);
    (app, cmd_rx)
}

/// Let every pane's indicator worker finish what it was sent, then apply
/// its events — the two steps the frame loop takes, made deterministic.
pub(in crate::app) fn settle_indicators(app: &mut QuantickApp) {
    for pane in app.active_tab_mut().panes_mut() {
        pane.indicator_worker.flush();
        pane.apply_indicator_events();
    }
}

/// The set an edit on the active tab's pane on `side` writes to: the tab's own
/// copy of the strip layout it shows. A strip layout takes no edit: it reaches
/// every tab.
pub(in crate::app) fn pane_set(
    app: &QuantickApp,
    side: PaneSide,
) -> &[crate::indicators::state_file::SavedIndicator] {
    let own = app
        .active_tab()
        .pane(side)
        .layout_id()
        .expect("a seeded pane");
    let layouts = app.runtime.layout_state().layouts();
    &layouts.get(own).expect("the tab's copy").indicators
}

/// What is on the pane, by indicator kind — the shape a detach has to
/// restore exactly.
pub(super) fn indicator_kinds(app: &QuantickApp) -> Vec<String> {
    app.active_tab()
        .focused_pane()
        .indicators
        .all()
        .iter()
        .map(|view| view.kind.to_string())
        .collect()
}

pub(super) fn attach_script_for_test(
    app: &mut QuantickApp,
    name: String,
    text: String,
    by_operator: bool,
) -> (u64, crate::control::PaneSideDto, SlotId) {
    let target = (
        app.runtime.tabs.active_id(),
        app.active_tab().focused_side(),
    );
    let attached = app.runtime.indicators.attach_script(
        app.runtime
            .tabs
            .runtime_mut(app.runtime.tabs.active_index())
            .pane_mut(target.1),
        target,
        name,
        text,
        by_operator,
    );
    let owner = attached.target;
    app.indicator_wiring().apply_indicator_edit(
        crate::app::indicator_manager::IndicatorEdit::Attached(attached),
    );
    (
        owner.tab,
        crate::control::pane_side_dto(owner.side),
        owner.slot,
    )
}

pub(super) fn add_library_for_test(app: &mut QuantickApp, index: usize) -> Option<SlotId> {
    let target = (
        app.runtime.tabs.active_id(),
        app.active_tab().focused_side(),
    );
    let (slot, added) = app.runtime.indicators.add_library(
        app.runtime
            .tabs
            .runtime_mut(app.runtime.tabs.active_index())
            .pane_mut(target.1),
        target,
        index,
    )?;
    if let Some(attached) = added.attachment {
        app.indicator_wiring().apply_indicator_edit(
            crate::app::indicator_manager::IndicatorEdit::Attached(attached),
        );
    }
    app.runtime.indicators.watch_attachment(added.watch);
    Some(slot)
}
