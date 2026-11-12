//! The window side of the toolbar (§6).
//!
//! [`crate::toolbar`] owns the grouping and the overflow rule and returns
//! a [`crate::toolbar::ToolbarAction`]; these three methods build the model
//! it draws from and carry out what it asked. The split is the port: the
//! toolbar module never touches `QuantickApp`, and this file never decides
//! what a toolbar looks like.

use eframe::egui;
use quantick_control_handlers::market_palette::MarketPaletteHost as _;
use quantick_engine::Side;

use crate::app::indicator_manager::IndicatorEdit;
use crate::indicator_worker::SlotId;
use crate::toolbar::{self, ToolbarAction};

use crate::app::{QuantickApp, TabsMutPort};

// A failed recut still owns older prints after venue paging is exhausted.
// The history button retries those retained pages through its ordinary action.
fn toolbar_caps(
    tab: &crate::tab::Tab,
    config: &crate::config::AppConfig,
) -> crate::config::FeedCapabilities {
    let mut caps = tab.capabilities(config);
    caps.history_paging |= tab.panes().any(|(pane, _)| pane.history_failed());
    caps
}

/// Build the toolbar's model from the app's state, draw it, and carry
/// out whatever it asked (§6 — the toolbar module owns grouping and the
/// overflow rule; this method owns the side effects).
pub(in crate::app) fn draw(app: &mut QuantickApp, ctx: &egui::Context) {
    let chip = app.config.venue_chip(&app.active_tab().feed_id);
    let feed = &app.active_tab().link;
    let replay = feed.replay.as_ref().map(|link| toolbar::ReplaySource {
        label: link.label(),
        hover: format!(
            "Replaying {}\nSide source: {}",
            link.session.path.display(),
            link.session
                .header
                .side_source
                .as_deref()
                .unwrap_or("not recorded"),
        ),
    });
    let capabilities = toolbar_caps(app.active_tab(), &app.config);
    let candles_held = app.active_tab().session.venue_candles_held();
    let older_candles = app.active_tab().older_candles(capabilities);
    let dock_visible = app.dock.visible();
    // The History button names this tab's target and run.
    let history = toolbar::HistoryButton {
        main: app.active_tab().session.main_history_reach(),
        status: app.active_tab().session.history_status(),
        tz: app.runtime.tz,
    };

    let capsule = crate::app::chrome::capsule::snapshot(app, ctx);

    let deal_recording = app.active_tab().deal_recording_view();
    #[cfg(any(feature = "scenario-harness", test))]
    let deal_recording_menu =
        deal_recording.is_some() && app.chrome.harness.take_deal_recording_menu();
    #[cfg(not(any(feature = "scenario-harness", test)))]
    let deal_recording_menu = false;
    #[cfg(any(feature = "scenario-harness", test))]
    if app.chrome.harness.take_bars_menu() {
        let popup = crate::pane_header::menu_id(app.focused_pane().id);
        ctx.memory_mut(|memory| memory.open_popup(popup));
    }
    let scope = app.runtime.tabs.windows().presented().0;
    app.surfaces
        .retain_slot_scopes(quantick_ui_model::Slot::Toolbar, |scope| {
            app.runtime
                .tabs
                .windows()
                .window(quantick_workspace::arrangement::windows::WindowId(scope))
                .is_some()
        });
    let tab = app.active_tab();
    let snapshot = quantick_ui_model::chrome_toolbar::Snapshot {
        capsule: capsule.clone(),
        market: (chip, tab.chip_label().to_owned()),
        replay,
        deal_recording,
        focused_bars: tab.focused_pane().spec.spec(),
        history,
        history_candles: candles_held,
        older_candles,
        capabilities,
        dock_visible,
        paper: tab
            .paper
            .trade_bar(&ctx.format_shortcut(&crate::app::menu_bar::PAPER_REVERSE_SHORTCUT)),
    };
    app.surfaces.update_scoped(
        scope,
        crate::surfaces::HostIntent::ChromeToolbar(Box::new(
            quantick_ui_model::chrome_toolbar::Intent::Refresh(Box::new(snapshot)),
        )),
    );
    app.surfaces.chrome_toolbar.scoped(scope).1.recording_menu = deal_recording_menu;
    let response = app.surfaces.draw_slot_scoped(
        scope,
        quantick_ui_model::Slot::Toolbar,
        &mut crate::surfaces::ViewContext { ctx, ui: None },
    );
    let view = app.surfaces.chrome_toolbar.scoped(scope).1;
    app.chrome.history_menu_rect = view.history;
    let (capsule_rects, toolbar_door) = (view.capsule, view.door);
    let mut actions = response.indicator_actions;
    for effect in response.model_effects {
        if let quantick_ui_model::HostEffect::ChromeToolbar(effects) = effect {
            for effect in effects.0 {
                use quantick_ui_model::chrome_toolbar::Effect;
                match effect {
                    Effect::Action(action) => actions.push(action),
                    Effect::SetLayoutPopover(open) => app.chrome.layout_picker_open = open,
                    Effect::SetAddMenu(open) => app.chrome.add_menu.open = open,
                }
            }
        }
    }
    let settle = crate::app::chrome::capsule::settle;
    for action in settle(app, ctx, &capsule, capsule_rects, toolbar_door, actions) {
        app.apply_toolbar_action(action);
    }
}

impl QuantickApp {
    /// Switch the active tab to `preset`.
    ///
    /// **The one path.** The toolbar's picker, the `View → Layout` menu and
    /// the keyboard all arrive here, so none of them can grow its own idea of
    /// what applying a layout does. A control-plane capability joins them by
    /// calling this, never by repeating it.
    pub(in crate::app) fn apply_layout_preset(
        &mut self,
        preset: &'static crate::canvas_layout::LayoutPreset,
    ) {
        crate::tab::layout::pick(self.active_tab_mut(), preset);
    }

    /// One toolbar side effect.
    pub(in crate::app) fn apply_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::LoadHistory(reach) => {
                let active = self.runtime.tabs.active_index();
                self.tabs_mut().press_history(active, reach);
            }
            ToolbarAction::CancelHistory => _ = self.active_tab_mut().cancel_history(),
            ToolbarAction::SwitchMarket => _ = self.open_market_palette("", true),
            ToolbarAction::DealRecording(action) => {
                self.active_tab_mut().apply_deal_recording(action);
            }
            ToolbarAction::LoadOlderCandles => self.tabs_mut().load_older_candles(),
            ToolbarAction::SetLayout(preset) => self.apply_layout_preset(preset),
            // Carried out by `chrome::capsule::settle`, which knows the window.
            ToolbarAction::ToggleWindowMenu
            | ToolbarAction::ToggleLayoutPopover
            | ToolbarAction::ToggleAddMenu
            | ToolbarAction::ToggleWorkspaceMenu
            | ToolbarAction::SaveWorkspace => {}
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            // Every indicator command lands on the focused pane (§11), which
            // is the flow pane whenever the canvas is not split.
            // Adding an indicator by hand is the plainest possible request to
            // see one, so it opens a folded legend rather than letting the new
            // row land inside the puck — the trader would get one more dot and
            // no way to tell the add from a no-op. Not the auto-collapse the
            // design ruled out: that rule protects against hiding what nobody
            // asked to hide, and unfolding hides nothing. It lives on this
            // path, the trader's own, and not in `ChartPane::add_indicator`,
            // which the workspace restore and the harness hooks also travel —
            // there it would erase the fold on every launch.
            ToolbarAction::AddNative(id) => {
                crate::app::indicator_manager::IndicatorState::set_legend_collapsed(
                    self.focused_pane_mut(),
                    false,
                );
                let target = (
                    self.runtime.tabs.active_id(),
                    self.active_tab().focused_side(),
                );
                let tabs = &mut self.runtime.tabs;
                let pane = tabs.runtime_mut(tabs.active_index()).pane_mut(target.1);
                let attached = self.runtime.indicators.attach_native(pane, target, id);
                self.indicator_wiring()
                    .apply_indicator_edit(IndicatorEdit::Attached(attached));
            }
            ToolbarAction::ToggleIndicatorHidden(slot) => {
                let mut wiring = self.indicator_wiring();
                let target = wiring.runtime.target_slot(SlotId(slot));
                wiring.apply_indicator_legend_action(
                    target.tab,
                    target.side,
                    crate::indicator_legend::LegendAction::ToggleHidden(target.slot),
                );
            }
            ToolbarAction::RemoveIndicator(slot) => {
                let mut wiring = self.indicator_wiring();
                let target = wiring.runtime.target_slot(SlotId(slot));
                wiring.apply_indicator_edit(IndicatorEdit::Remove(target));
            }
            ToolbarAction::AddScriptIndicator(index) => {
                crate::app::indicator_manager::IndicatorState::set_legend_collapsed(
                    self.focused_pane_mut(),
                    false,
                );
                let target = (
                    self.runtime.tabs.active_id(),
                    self.active_tab().focused_side(),
                );
                let tabs = &mut self.runtime.tabs;
                let pane = tabs.runtime_mut(tabs.active_index()).pane_mut(target.1);
                if let Some((_, added)) = self.runtime.indicators.add_library(pane, target, index) {
                    if let Some(attached) = added.attachment {
                        self.indicator_wiring()
                            .apply_indicator_edit(IndicatorEdit::Attached(attached));
                    }
                    self.runtime.indicators.watch_attachment(added.watch);
                }
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let mut wiring = self.indicator_wiring();
                let target = wiring.runtime.target_slot(SlotId(slot));
                wiring.apply_indicator_legend_action(
                    target.tab,
                    target.side,
                    crate::indicator_legend::LegendAction::OpenSettings(target.slot),
                );
            }
            // The toolbar acts on the market it is showing: the active tab's
            // simulator, whose tape the buttons' price came from.
            ToolbarAction::PaperBuy => self.active_tab_mut().paper.market(Side::Buy),
            ToolbarAction::PaperSell => self.active_tab_mut().paper.market(Side::Sell),
            ToolbarAction::PaperClose => self.active_tab_mut().paper.close_position(),
            ToolbarAction::PaperReverse => self.active_tab_mut().paper.reverse_position(),
        }
    }
}
