//! The floating surfaces' stages: requests the panes raised for them, the
//! environment they read, and the scripts the indicator dialogs watch.
//!
//! Each function takes the owners it reads and writes, never the window:
//! a surface cannot be handed the trunk it is being kept out of.

use eframe::egui;
use smallvec::SmallVec;
use std::time::Instant;

use crate::app::arrangement_host::ArrangementHost;
use crate::app::indicator_manager::IndicatorState;
use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::footprint_config::FootprintConfig;
use crate::indicator_panel::SettingsDialog;
use crate::pane::PaneSide;
use crate::style::ChartStyle;
use crate::surfaces::{StrategyPopupSurface, SurfaceEnv, SurfaceResponse, Surfaces};
use quantick_chart::bar_progress::BarProgressReading;

use super::super::TabSlot;

/// A pane's right-click asked to arm one of its drawings. Drained into the
/// surface that owns the dialog: the click happens while the canvas draws,
/// which is later in the frame than the surfaces are, so the dialog opens on
/// the next one — a frame the trader cannot see, and the price of the dialog
/// no longer living in the trunk.
pub(super) fn open_requested_strategy_popups(
    tabs: &mut ArrangementHost,
    popup: &mut StrategyPopupSurface,
) {
    let tab_id = tabs.active_id();
    let tab = tabs.runtime_mut(tabs.active_index());
    let sides: SmallVec<[PaneSide; MAX_CANVAS_PANES]> = tab.sides().collect();
    for side in sides {
        if let Some(drawing) = tab.pane_mut(side).strategies.popup_request.take() {
            let form =
                crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
            popup.open(tab_id, side, drawing, form);
        }
    }
}

/// What the surfaces' environment is read from.
pub(super) struct SurfaceOwners<'a> {
    pub indicators: &'a mut IndicatorState,
    pub store: &'a crate::workspace_store::LayoutStore,
    pub alert_failure: Option<&'a str>,
    pub style: &'a ChartStyle,
    pub footprint_config: &'a FootprintConfig,
    pub tabs: &'a mut ArrangementHost,
    pub markets: Option<&'a quantick_workspace::market_list::MarketSnapshot>,
}

/// Draw every registered surface against this frame's environment and
/// return what they asked for.
pub(super) fn draw_surfaces(
    ctx: &egui::Context,
    now: Instant,
    registry: &mut Surfaces,
    owners: SurfaceOwners<'_>,
) -> SurfaceResponse {
    let SurfaceOwners {
        indicators,
        store,
        alert_failure,
        style,
        footprint_config,
        tabs,
        markets,
    } = owners;
    // `hooks_pending` is the frame a capture hook opens a surface from
    // inside `draw_all`: it is not open yet when this runs, but it is about
    // to be, and it must not draw its first frame against an empty
    // environment.
    super::super::chart_settings_wiring::initialize(&mut registry.chart_settings.model_state);
    let staging = registry.hooks_pending();
    let pinned = registry.chart_settings.model_state.editor.target;
    let target_tab = pinned.and_then(|target| tabs.by_id(target.tab_id));
    let target_pane = pinned.and_then(|target| {
        target_tab.and_then(|tab| {
            tab.panes()
                .find(|(pane, _)| pane.id == target.pane_id)
                .map(|(pane, _)| pane)
        })
    });
    if let Some(target) = pinned {
        registry.chart_settings.model_state.editor.observe(
            target,
            target_pane.map(|pane| pane.presentation.candles.context()),
        );
    }
    let focused_tab = &tabs[tabs.active_index()];
    // The bar rules the arming dialog's alarm section reads: a share of the
    // bar only means something where the rule closes on a count. Built only
    // while that dialog is open, like the open markets below.
    let counted_bar_sides: SmallVec<[PaneSide; MAX_CANVAS_PANES]> =
        if staging || registry.strategy_popup.is_open() {
            focused_tab
                .sides()
                .filter(|side| {
                    let reading = focused_tab.pane(*side).state.bar_progress_at(None);
                    reading
                        .and_then(BarProgressReading::fixed_progress)
                        .is_some()
                })
                .collect()
        } else {
            SmallVec::new()
        };
    // Read once. `focused_pane` resolves the same side internally, and the
    // answer is not a field lookup — it reads the layout, because focus on a
    // collapsed pane is focus on nothing.
    let focused_side = focused_tab.focused_side();
    let focused_pane = focused_tab.pane(focused_side);
    let picker_indicators: Vec<_> = if staging || registry.indicator_picker.state.target.is_some() {
        focused_pane
            .indicators
            .all()
            .iter()
            .map(|view| crate::toolbar::IndicatorMenuEntry {
                slot: view.slot.0,
                label: view.label().to_owned(),
                hidden: view.hidden,
                errored: view.error.is_some(),
                stale: view.stale.is_some(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let editor_pane = target_pane.unwrap_or(focused_pane);
    let editor_tab = target_tab.unwrap_or(focused_tab);
    let palette = editor_pane.palette(ctx);
    let editor_style = editor_pane
        .presentation
        .candles
        .resolve(style, &palette.resolved);
    registry.chart_settings.model_state.target_label =
        if let Some(target) = pinned.filter(|_| registry.chart_settings.model_state.editor.stale) {
            format!(
                "Unavailable chart / tab {} / pane {}",
                target.tab_id, target.pane_id
            )
        } else {
            format!(
                "{} / {} / {} {} / pane {}",
                editor_tab.symbol,
                editor_tab.feed_id,
                if editor_pane.orderflow.is_some() {
                    "Flow"
                } else {
                    "Time"
                },
                editor_pane.state.spec().to_config_string(),
                editor_pane.id
            )
        };
    registry.update(crate::surfaces::HostIntent::ChartSettings(Box::new(
        quantick_ui_model::chart_settings::Intent::Refresh {
            style: editor_style,
            footprint: focused_pane.footprint_config(footprint_config).clone(),
            target: format!("{} chart", focused_side.title().to_lowercase()),
            customized: focused_pane.presentation.footprint.config.is_own(),
        },
    )));
    let mut asks = registry.draw_all(
        ctx,
        &SurfaceEnv {
            picker_pane_id: focused_pane.id,
            picker_indicators: &picker_indicators,
            picker_scripts: indicators.script_library.entries(),
            now,
            indicator_preview_area: indicator_preview_area(
                tabs,
                indicators.indicator_settings.as_ref(),
                indicators.indicator_settings_target,
            ),
            focused_chart_area: focused_pane.frame.chart_area,
            style: &editor_style,
            style_surface: editor_pane.surface(),
            footprint: focused_pane.footprint_config(footprint_config),
            footprint_customized: focused_pane.presentation.footprint.config.is_own(),
            focused_side,
            markets,
            active_tab: tabs.active_id(),
            counted_bar_sides: &counted_bar_sides,
            alert_failure,
        },
    );
    // A surface hook may have opened the editor inside draw_all.
    if registry.chart_settings.model_state.is_open() && pinned.is_none() {
        let target = registry.chart_settings.model_state.editor.target.unwrap_or(
            quantick_chart::pane_style::StyleTarget {
                tab_id: tabs.active_id(),
                pane_id: editor_pane.id,
            },
        );
        registry.chart_settings.model_state.editor.pin(target);
        registry
            .chart_settings
            .model_state
            .editor
            .observe(target, Some(editor_pane.presentation.candles.context()));
    }
    let mut retained = Vec::new();
    for effect in std::mem::take(&mut asks.model_effects) {
        match effect {
            quantick_ui_model::HostEffect::ChartSettings(effects) => {
                super::super::chart_settings_wiring::persist(
                    &mut registry.chart_settings.model_state,
                    effects.persistence,
                )
            }
            other => retained.push(other),
        }
    }
    asks.model_effects = retained;
    super::super::script_editor_wiring::frame(
        registry,
        indicators,
        tabs,
        store,
        std::mem::take(&mut asks.script_editor_actions),
        now,
    );
    if let Some(request) = asks.bar_switch.take() {
        apply_bar_switch(tabs, &mut registry.toast, request);
    }
    asks
}

/// Apply the quick bar switch's choice to the pane it opened over, through
/// the path `layout.pane.set_bar_spec` takes. A rule this feed cannot build
/// (volume bars on a venue without volume) is refused in the toast.
fn apply_bar_switch(
    tabs: &mut ArrangementHost,
    toast: &mut crate::surfaces::ToastSurface,
    request: crate::surfaces::BarSwitchRequest,
) {
    let Some(tab) = tabs.by_id_mut(request.tab) else {
        return;
    };
    if let Err(error) = tab.set_pane_bar_spec(request.side.index(), request.config) {
        toast.note(format!("Bars not changed: {error}"), Instant::now());
    }
}

/// The chart rectangle a settings dialog is previewing an unapplied draft on,
/// if one is.
///
/// The pane the *dialog* was opened over, not the focused one: a trader can
/// preview a curve on the left pane and then click the right, and the banner
/// belongs over the numbers that are actually provisional. Per-frame, and
/// shaped to leave immediately: no dialog open — the ordinary case — is one
/// `Option` test before the tab scan is reached.
fn indicator_preview_area(
    tabs: &ArrangementHost,
    dialog: Option<&SettingsDialog>,
    target: TabSlot,
) -> Option<egui::Rect> {
    if target.workspace != tabs.owner() || !dialog.is_some_and(|dialog| dialog.previewed) {
        return None;
    }
    tabs.by_id(target.tab)
        .and_then(|tab| tab.pane_at(target.side.index()))
        .filter(|pane| pane.id == target.pane)
        .and_then(|pane| pane.frame.chart_area)
}

/// Hand every script file that changed on disk to the pane that runs it.
pub(super) fn reload_changed_scripts(
    indicators: &mut IndicatorState,
    tabs: &mut ArrangementHost,
    store: &crate::workspace_store::LayoutStore,
) {
    super::super::script_reload::reload_changed_scripts(indicators, tabs, store);
}

#[cfg(test)]
mod bar_switch_tests {
    use super::*;
    use quantick_engine::bar_registry::BUILTIN_BARS;
    use quantick_workspace::{arrangement::TabId, lifetime::WorkspaceId};
    fn tab(id: u64, dir: &std::path::Path) -> crate::tab::Tab {
        let (_, events) = tokio::sync::mpsc::channel(1);
        let (_, book_events) = tokio::sync::mpsc::channel(1);
        let (commands, _) = tokio::sync::mpsc::channel(1);
        let mut tab = crate::tab::Tab::new(
            id,
            "fixture".into(),
            "TEST".into(),
            BUILTIN_BARS.parse("tick:50").unwrap(),
            crate::feed::FeedHandle {
                events,
                book_events,
                commands,
                notices: crate::feed::silent_notices(),
                capabilities: crate::feed::fixed_capabilities(
                    crate::config::FeedCapabilities::none(),
                ),
                latency: crate::feed::unsplit_latency(),
                replay: None,
            },
            dir.to_path_buf(),
        );
        tab.time_panes
            .push(crate::pane::ChartPane::time(id + 1, 60_000));
        tab
    }
    #[test]
    fn bar_switch_host_targets_stable_tab_and_pane_and_preserves_refusal_toast() {
        let dir = crate::scratch::ScratchDir::new("bar-switch-host");
        let mut tabs = ArrangementHost::new(WorkspaceId(1), TabId(10), tab(100, &dir));
        tabs.append(tabs.plan_open(), tab(200, &dir));
        let active = tabs.active_id();
        assert_ne!(active, 10);
        let mut toast = crate::surfaces::ToastSurface::default();
        let request = crate::surfaces::BarSwitchRequest {
            tab: 10,
            side: PaneSide::Time(0),
            config: BUILTIN_BARS.parse("time:5m").unwrap(),
        };
        apply_bar_switch(&mut tabs, &mut toast, request);
        assert_eq!(
            tabs.by_id(10).unwrap().time_panes[0].state.spec(),
            &crate::state::BarSpec::Time(300_000)
        );
        assert_eq!(
            tabs.by_id(active).unwrap().time_panes[0].state.spec(),
            &crate::state::BarSpec::Time(60_000)
        );
        assert_eq!(
            tabs.by_id(10).unwrap().flow_pane.state.spec(),
            &crate::state::BarSpec::Tick(50)
        );
        assert_eq!(tabs.active_id(), active);
        assert_eq!(toast.message(), None);
        apply_bar_switch(
            &mut tabs,
            &mut toast,
            crate::surfaces::BarSwitchRequest {
                config: BUILTIN_BARS.parse("volume:100").unwrap(),
                ..request
            },
        );
        assert_eq!(
            tabs.by_id(10).unwrap().time_panes[0].state.spec(),
            &crate::state::BarSpec::Time(300_000)
        );
        assert_eq!(
            toast.message(),
            Some("Bars not changed: this source quotes prices but prints no traded volume")
        );
        toast.clear();
        apply_bar_switch(
            &mut tabs,
            &mut toast,
            crate::surfaces::BarSwitchRequest {
                tab: u64::MAX,
                ..request
            },
        );
        assert_eq!(toast.message(), None);
        assert_eq!(tabs.active_id(), active);
    }
}
