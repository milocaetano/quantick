//! The window side of the toolbar (§6).
//!
//! [`crate::toolbar`] owns the grouping and the overflow rule and returns
//! a [`crate::toolbar::ToolbarAction`]; these three methods build the model
//! it draws from and carry out what it asked. The split is the port: the
//! toolbar module never touches `QuantickApp`, and this file never decides
//! what a toolbar looks like.

use eframe::egui;

use crate::indicator_worker::SlotId;
use crate::pane::PaneSide;
use crate::tab::CanvasLayout;
use crate::toolbar::{self, ToolbarAction};

use super::QuantickApp;

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
        let capabilities = self.active_tab().capabilities(&self.config);
        let candles_held = self.active_tab().venue_candles_held();
        let older_candles = self.active_tab().older_candles(capabilities);
        let feed_display_name = self.active_tab().feed_display_name(&self.config).to_owned();
        // One reading per lamp, taken through the call the semantic scene
        // makes too, so the button and what an operator captures cannot
        // disagree about a layer. Every lamp reports the *switch* rather than
        // what the source lets through it — the rule `heatmap_lamp_on` names,
        // now the whole group's.
        let layers = toolbar::LayerToggle::ALL.map(|toggle| {
            let (on, blocked) =
                self.active_tab()
                    .layer_toggle_state(toggle.layer(), &self.style, capabilities);
            toolbar::LayerToggleState { on, blocked }
        });
        // The focused pane's slots (§11): the menu lists what a command from
        // it would act on, and never the pane beside it.
        let indicators: Vec<toolbar::IndicatorMenuEntry> = self
            .focused_pane()
            .indicators
            .all()
            .iter()
            .map(|view| toolbar::IndicatorMenuEntry {
                slot: view.slot.0,
                label: view.label().to_owned(),
                hidden: view.hidden,
                errored: view.error.is_some(),
                stale: view.stale.is_some(),
            })
            .collect();
        let scripts: Vec<String> = self
            .indicators
            .script_library
            .entries()
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        let dock_visible = self.dock.visible();
        let show_style = self.surfaces.style_panel.is_open();
        // Read before the tab is borrowed mutably. The reach is the window's
        // standing choice, like the progressive-history switch — a trader who
        // picked "previous session" once means it in the next tab too — so it
        // is split off and written back the way the layout picker's flags are.
        let history_reach_running = self.active_tab().history_reach_running();
        let mut history_reach = self.history.history_reach;
        let mut history_reach_span_minutes = self.history.history_reach_span_minutes;
        let mut history_menu_rect = self.chrome.history_menu_rect;
        // The SOURCE group writes straight into the active tab: a feed or
        // symbol change is that tab's market switch. The BARS group writes
        // into the *focused pane* — the pane the status bar reads and every
        // indicator command lands on (§11) — so the three chrome surfaces
        // can never disagree about which chart a command describes, and in
        // the Time layout the group governs the chart actually on screen.
        // Split off the picker's flags before the tab borrow: the model wants
        // both, and they live on the same struct.
        let mut layout_picker_open = self.chrome.layout_picker_open;
        // One shot: the hook opens the popover on the first drawn frame and
        // then gets out of the way, so a trader's click can close it.
        let layout_picker_autostart = self.harness.take_layout_picker_autostart();
        let tab = self.active_tab_mut();
        let focused = tab.focused_side();
        let pane = match focused {
            PaneSide::Time(slot) => tab.time_panes.get_mut(slot).unwrap_or(&mut tab.flow_pane),
            PaneSide::Flow => &mut tab.flow_pane,
        };
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
            kind: &mut pane.kind,
            tick_n: &mut pane.tick_n,
            volume_units: &mut pane.volume_units,
            dollar_notional: &mut pane.dollar_notional,
            time_interval_ms: &mut pane.time_interval_ms,
            imbalance_target: &mut pane.imbalance_target,
            imbalance_unit: &mut pane.imbalance_unit,
            history_step: &mut tab.history_step,
            history_menu_rect: &mut history_menu_rect,
            history_reach_span_minutes: &mut history_reach_span_minutes,
            history_reach: &mut history_reach,
            history_reach_running,
            history_trades: tab.history_trades,
            history_candles: candles_held,
            older_candles,
            capabilities,
            layers,
            dock_visible,
            appearance_open: show_style,
            paper: toolbar::PaperTradeModel {
                // The lock reaches the toolbar too. Gating only the dock's
                // pair left these lit while the ticket refused, so a fast
                // click here only toasted - and the doc promises the entry
                // pair disables.
                ready: tab.paper.ready() && !tab.paper.risk_report().1,
                buy_label: tab.paper.entry_label(quantick_engine::Side::Buy),
                sell_label: tab.paper.entry_label(quantick_engine::Side::Sell),
                buy_hover: tab.paper.entry_hover(quantick_engine::Side::Buy),
                sell_hover: tab.paper.entry_hover(quantick_engine::Side::Sell),
                close_label: tab.paper.close_button_label(),
            },
            indicators,
            scripts,
        };
        let actions = toolbar::draw(ctx, &mut model);
        // The popover's own state, back where it lives. Without this the flag
        // resets every frame and the button never reads as open.
        drop(model);
        self.chrome.layout_picker_open = layout_picker_open;
        self.set_history_reach(history_reach);
        // Through the setter, so a value dragged past the campaign's own span
        // cap is clamped in the one place that knows the cap.
        self.set_history_reach_span_minutes(history_reach_span_minutes);
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

    /// One toolbar side effect. Layer toggles reuse the same code paths the
    /// old checkboxes took, so provider gating and command acknowledgement
    /// rules are unchanged.
    pub(super) fn apply_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::LoadOlder => {
                let (tab, config) = self.active_with_config();
                tab.request_older_history(config);
            }
            ToolbarAction::LoadOlderCandles => {
                // Read before the tab is borrowed mutably — and the capability
                // block rather than the whole config, because that is all the
                // request needs to know.
                let capabilities = self.active_tab().capabilities(&self.config);
                self.active_tab_mut()
                    .request_older_ohlcv_history(capabilities);
            }
            ToolbarAction::SetHeatmap(shown) => {
                self.active_tab_mut().tape_mut().set_depth_visible(shown);
            }
            ToolbarAction::SetBubbles(enabled) => {
                self.active_tab_mut()
                    .tape_mut()
                    .set_bubbles_enabled(enabled);
            }
            ToolbarAction::SetLiveStrip(shown) => {
                self.active_tab_mut().flow_pane.live_strip_visible = shown;
            }
            // The focused pane's own field, through the same setter the pane's
            // layer menu calls — so the button, the menu and the lamp can
            // never disagree about which chart the command described.
            ToolbarAction::SetFootprint(shown) => {
                self.focused_pane_mut().footprint_visible = shown;
            }
            ToolbarAction::OpenFootprintSettings => self.surfaces.footprint_settings.open(),
            ToolbarAction::OpenDockTab(tab) => self.dock.open_tab(tab),
            ToolbarAction::SetLayout(preset) => self.apply_layout_preset(preset),
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            ToolbarAction::ToggleAppearance => self.surfaces.style_panel.toggle(),
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
                self.set_focused_legend_collapsed(false);
                self.add_native_indicator(id);
            }
            ToolbarAction::ToggleIndicatorHidden(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.toggle_indicator_hidden_at(target);
            }
            ToolbarAction::RemoveIndicator(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.remove_indicator_at(target);
            }
            ToolbarAction::AddScriptIndicator(index) => {
                self.set_focused_legend_collapsed(false);
                self.add_script_indicator(index);
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.open_indicator_settings_at(target);
            }
            // The toolbar acts on the market it is showing: the active tab's
            // simulator, whose tape the buttons' price came from.
            ToolbarAction::PaperBuy => self
                .active_tab_mut()
                .paper
                .market(quantick_engine::Side::Buy),
            ToolbarAction::PaperSell => self
                .active_tab_mut()
                .paper
                .market(quantick_engine::Side::Sell),
            ToolbarAction::PaperClose => self.active_tab_mut().paper.close_position(),
        }
    }
}
