//! The window side of the toolbar (§6).
//!
//! [`crate::toolbar`] owns the grouping and the overflow rule and returns
//! a [`crate::toolbar::ToolbarAction`]; these three methods build the model
//! it draws from and carry out what it asked. The split is the port: the
//! toolbar module never touches `QuantickApp`, and this file never decides
//! what a toolbar looks like.

use eframe::egui;
use quantick_engine::Side;

use crate::indicator_worker::SlotId;
use crate::surfaces::ChartSettingsTab::Candles;
use crate::tab::CanvasLayout;
use crate::toolbar::{self, ToolbarAction};

use super::{LayersPort, QuantickApp, TabsMutPort};

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

impl QuantickApp {
    /// Build the toolbar's model from the app's state, draw it, and carry
    /// out whatever it asked (§6 — the toolbar module owns grouping and the
    /// overflow rule; this method owns the side effects).
    pub(super) fn draw_toolbar(&mut self, ctx: &egui::Context) {
        // Pre-collect owned option lists so the toolbar's combos don't borrow
        // `self.config` while they mutate `self.feed_id` / `self.active_tab().symbol`.
        // Providers that aren't streaming yet are labelled "(soon)" so the
        // menu is honest about what actually connects.
        let feeds: Vec<(String, String)> = self
            .config
            .feeds
            .iter()
            .map(|f| {
                let label = if f.provider.is_implemented() {
                    f.name.clone()
                } else {
                    format!("{} (soon)", f.name)
                };
                (f.id.clone(), label)
            })
            .collect();
        let symbols: Vec<String> = self
            .config
            .feed(&self.active_tab().feed_id)
            .map(|f| f.symbols.clone())
            .unwrap_or_default();
        // During a replay the SOURCE group gives way to what is actually
        // playing: a live venue cannot be picked without leaving the
        // recording first, and a combo that silently did so would throw away
        // the session mid-run.
        let replay = self
            .active_tab()
            .replay
            .as_ref()
            .map(|link| toolbar::ReplaySource {
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
        let capabilities = toolbar_caps(self.active_tab(), &self.config);
        let candles_held = self.active_tab().session.venue_candles_held();
        let older_candles = self.active_tab().older_candles(capabilities);
        let feed_display_name = self.active_tab().feed_display_name(&self.config).to_owned();
        let dock_visible = self.dock.visible();
        let show_style = self.surfaces.chart_settings.is_open_on(Candles);
        // The History button names this tab's target and run.
        let history = toolbar::HistoryButton {
            main: self.active_tab().session.main_history_reach(),
            status: self.active_tab().session.history_status(),
            tz: self.runtime.tz,
        };
        let mut history_menu_rect = self.chrome.history_menu_rect;
        // The SOURCE group writes straight into the active tab: a feed or
        // symbol change is that tab's market switch.
        // Split off the picker's flags before the tab borrow: the model wants
        // both, and they live on the same struct.
        let mut layout_picker_open = self.chrome.layout_picker_open;
        // One shot: the hook opens the popover on the first drawn frame and
        // then gets out of the way, so a trader's click can close it.
        #[cfg(any(feature = "scenario-harness", test))]
        let layout_picker_autostart = self.chrome.harness.take_layout_picker_autostart();
        #[cfg(not(any(feature = "scenario-harness", test)))]
        let layout_picker_autostart = false;
        let deal_recording = self.active_tab().deal_recording_view();
        #[cfg(any(feature = "scenario-harness", test))]
        let deal_recording_menu =
            deal_recording.is_some() && self.chrome.harness.take_deal_recording_menu();
        #[cfg(not(any(feature = "scenario-harness", test)))]
        let deal_recording_menu = false;
        #[cfg(any(feature = "scenario-harness", test))]
        if self.chrome.harness.take_bars_menu() {
            let popup = crate::pane_header::menu_id(self.focused_pane().id);
            ctx.memory_mut(|memory| memory.open_popup(popup));
        }
        let tab = self.active_tab_mut();
        let focused_bars = tab.focused_pane().spec.spec().definition();
        let mut model = toolbar::ToolbarModel {
            layout_preset: Some(tab.layout.preset()),
            layout_picker_open: &mut layout_picker_open,
            layout_picker_request_open: layout_picker_autostart,
            feeds,
            feed_id: &mut tab.feed_id,
            feed_display_name,
            symbols,
            symbol: &mut tab.symbol,
            replay,
            deal_recording,
            deal_recording_menu,
            focused_bars,
            history_menu_rect: &mut history_menu_rect,
            history,
            history_candles: candles_held,
            older_candles,
            capabilities,
            dock_visible,
            appearance_open: show_style,
            // The lock reaches the toolbar too: `blocked` carries it, so a
            // fast click here cannot only toast while the ticket refuses.
            paper: tab
                .paper
                .trade_bar(&ctx.format_shortcut(&super::menu_bar::PAPER_REVERSE_SHORTCUT)),
        };
        let actions = toolbar::draw(ctx, &mut model);
        // The popover's own state, back where it lives. Without this the flag
        // resets every frame and the button never reads as open.
        drop(model);
        self.chrome.layout_picker_open = layout_picker_open;
        self.chrome.history_menu_rect = history_menu_rect;
        // A newly picked feed may not offer the current symbol. Never during
        // a replay: the recorded instrument belongs to no live feed's menu,
        // and snapping it away would relabel the whole session — the status
        // bar and the logs must keep naming what is actually playing.
        if self.active_tab().replay.is_none() {
            let (tab, config) = self.active_with_config();
            tab.ensure_symbol_valid(config);
            tab.refresh_chip_label(config);
        }
        for action in actions {
            self.apply_toolbar_action(action);
        }
    }

    /// Switch the active tab to `preset`.
    ///
    /// **The one path.** The toolbar's picker, the `View → Layout` menu and
    /// the keyboard all arrive here, so none of them can grow its own idea of
    /// what applying a layout does. A control-plane capability joins them by
    /// calling this, never by repeating it.
    pub(super) fn apply_layout_preset(
        &mut self,
        preset: &'static crate::canvas_layout::LayoutPreset,
    ) {
        let Some(layout) = CanvasLayout::from_preset(preset) else {
            // A preset the canvas cannot draw yet is refused rather than
            // approximated: switching to the nearest arrangement would be the
            // picker showing one layout and the canvas drawing another.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUT_PRESET_UNSUPPORTED",
                preset = %preset.id,
                action = "layout_left_as_is",
                "the layout registry names an arrangement the canvas cannot draw yet"
            );
            return;
        };
        self.active_tab_mut().set_layout(layout);
    }

    /// One toolbar side effect.
    pub(super) fn apply_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::LoadHistory(reach) => {
                let active = self.runtime.tabs.active_index();
                self.tabs_mut().press_history(active, reach);
            }
            ToolbarAction::CancelHistory => _ = self.active_tab_mut().cancel_history(),
            ToolbarAction::DealRecording(action) => {
                self.active_tab_mut().apply_deal_recording(action);
            }
            ToolbarAction::LoadOlderCandles => {
                let (active, tab_id) = (
                    self.runtime.tabs.active_index(),
                    self.runtime.tabs.active_id(),
                );
                let act = quantick_session::Act::LoadOlderCandles { tab_id };
                self.runtime
                    .tabs
                    .runtime_mut(active)
                    .run_session(Some(&self.config), act);
            }
            ToolbarAction::SetLayout(preset) => self.apply_layout_preset(preset),
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            ToolbarAction::ToggleAppearance => self.layer_wiring().toggle_settings(Candles),
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
                super::indicator_manager::IndicatorState::set_legend_collapsed(
                    self.focused_pane_mut(),
                    false,
                );
                let target = (
                    self.runtime.tabs.active_id(),
                    self.active_tab().focused_side(),
                );
                let attached = self.runtime.indicators.attach_native(
                    self.runtime
                        .tabs
                        .runtime_mut(self.runtime.tabs.active_index())
                        .pane_mut(target.1),
                    target,
                    id,
                );
                self.apply_indicator_edit(super::indicator_manager::IndicatorEdit::Attached(
                    attached,
                ));
            }
            ToolbarAction::ToggleIndicatorHidden(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.apply_indicator_legend_action(
                    target.tab,
                    target.side,
                    crate::indicator_legend::LegendAction::ToggleHidden(target.slot),
                );
            }
            ToolbarAction::RemoveIndicator(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.apply_indicator_edit(super::indicator_manager::IndicatorEdit::Remove(target));
            }
            ToolbarAction::AddScriptIndicator(index) => {
                super::indicator_manager::IndicatorState::set_legend_collapsed(
                    self.focused_pane_mut(),
                    false,
                );
                let target = (
                    self.runtime.tabs.active_id(),
                    self.active_tab().focused_side(),
                );
                if let Some((_, added)) = self.runtime.indicators.add_library(
                    self.runtime
                        .tabs
                        .runtime_mut(self.runtime.tabs.active_index())
                        .pane_mut(target.1),
                    target,
                    index,
                ) {
                    if let Some(attached) = added.attachment {
                        self.apply_indicator_edit(
                            super::indicator_manager::IndicatorEdit::Attached(attached),
                        );
                    }
                    self.runtime.indicators.watch_attachment(added.watch);
                }
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.apply_indicator_legend_action(
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
