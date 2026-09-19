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
use crate::config::AppConfig;
use crate::footprint_config::FootprintConfig;
use crate::indicator_panel::SettingsDialog;
use crate::pane::PaneSide;
use crate::style::ChartStyle;
use crate::surfaces::{StrategyPopupSurface, SurfaceEnv, SurfaceResponse, Surfaces};
use crate::symbols_file::AddedSymbols;
use crate::workspace_store::WorkspaceStore;

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
    pub indicators: &'a IndicatorState,
    pub alert_failure: Option<&'a str>,
    pub workspace: &'a WorkspaceStore,
    pub style: &'a ChartStyle,
    pub footprint_config: &'a FootprintConfig,
    pub tabs: &'a ArrangementHost,
    pub config: &'a AppConfig,
    pub added_symbols: &'a AddedSymbols,
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
        alert_failure,
        workspace,
        style,
        footprint_config,
        tabs,
        config,
        added_symbols,
    } = owners;
    // `hooks_pending` is the frame a capture hook opens a surface from
    // inside `draw_all`: it is not open yet when this runs, but it is about
    // to be, and it must not draw its first frame against an empty
    // environment.
    let staging = registry.hooks_pending();
    let focused_tab = &tabs[tabs.active_index()];
    // The bar rules the arming dialog's alarm section reads: a share of the
    // bar only means something where the rule closes on a count. Built only
    // while that dialog is open, like the open markets below.
    let counted_bar_sides: SmallVec<[PaneSide; MAX_CANVAS_PANES]> =
        if staging || registry.strategy_popup.is_open() {
            focused_tab
                .sides()
                .filter(|side| focused_tab.pane(*side).state.progress().is_some())
                .collect()
        } else {
            SmallVec::new()
        };
    // The markets tabs are showing: the dialog greys out removing one of
    // those, because a tab left on a symbol the catalog no longer offers
    // gets silently retargeted by the next SOURCE correction. Built only
    // while the dialog is open — it is a `String` pair per tab, and no frame
    // should pay for it to be thrown away.
    let open_markets: Vec<(String, String)> = if staging || registry.source_picker.is_open() {
        tabs.iter()
            .map(|tab| (tab.feed_id.clone(), tab.symbol.clone()))
            .collect()
    } else {
        Vec::new()
    };
    // Read once. `focused_pane` resolves the same side internally, and the
    // answer is not a field lookup — it reads the layout, because focus on a
    // collapsed pane is focus on nothing.
    let focused_side = focused_tab.focused_side();
    let focused_pane = focused_tab.pane(focused_side);
    registry.draw_all(
        ctx,
        &SurfaceEnv {
            bookmarks: workspace.session().bookmarks(),
            now,
            indicator_preview_area: indicator_preview_area(
                tabs,
                indicators.indicator_settings.as_ref(),
                indicators.indicator_settings_target,
            ),
            focused_chart_area: focused_pane.frame.chart_area,
            style,
            footprint: focused_pane.footprint_config(footprint_config),
            footprint_customized: focused_pane.footprint.config.is_some(),
            focused_side,
            config,
            added_symbols,
            open_markets: &open_markets,
            active_tab: tabs.active_id(),
            counted_bar_sides: &counted_bar_sides,
            alert_failure,
        },
    )
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
    if !dialog.is_some_and(|dialog| dialog.previewed) {
        return None;
    }
    tabs.by_id(target.tab)
        .map(|tab| tab.pane(target.side))
        .and_then(|pane| pane.frame.chart_area)
}

/// Hand every script file that changed on disk to the pane that runs it.
pub(super) fn reload_changed_scripts(indicators: &mut IndicatorState, tabs: &mut ArrangementHost) {
    for (owner, name, text) in indicators.poll_script_files() {
        IndicatorState::log_reload(owner, &name);
        if let Some(tab) = tabs.by_id_mut(owner.tab) {
            tab.pane_mut(owner.side).indicator_worker.send(
                crate::indicator_worker::IndicatorCommand::Reload {
                    slot: owner.slot,
                    source: crate::indicator_worker::IndicatorSource::Script { name, text },
                },
            );
        }
    }
}
