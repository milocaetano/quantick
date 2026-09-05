//! The paper simulator's wiring into the window: where its trades are saved,
//! what it persists when the trader changes a setting, and how a tab or a
//! duplicated drawing inherits a strategy.
//!
//! A child of `app` rather than a sibling so it can reach the app's own
//! fields. Nothing here decides anything about a simulated trade -- that is
//! [`crate::paper`] and [`crate::paper_account`]; this is only the plumbing
//! between those and the window that owns them.

use crate::drawings;
use crate::pane;
use crate::state::BarSpec;
use crate::tab::Tab;
use quantick_feed::FeedHandle;

use super::QuantickApp;

impl QuantickApp {
    /// Ask the operating system for a trades folder, off the UI thread —
    /// the panel's "choose where trades are saved". One dialog at a time.
    pub(super) fn open_trades_dir_picker(&mut self) {
        if self.workspace.trades_dir_picker_open() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        // Start where trades actually go right now — under an env override
        // that is the override's folder, not the stored base.
        let start = self.active_tab().paper.account().trades_dir().to_path_buf();
        std::thread::Builder::new()
            .name("quantick-trades-dir-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title("Choose where trades are saved");
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_folder());
            })
            .expect("spawn trades-dir picker thread");
        self.workspace.open_trades_dir_picker(receiver);
    }

    /// Land the picked folder: every tab journals there from now on, and
    /// the choice is remembered across restarts (`paper-state.toml`) —
    /// files already written stay where they are.
    pub(super) fn poll_trades_dir_picker(&mut self) {
        let Some(receiver) = self.workspace.trades_dir_picker() else {
            return;
        };
        let Ok(choice) = receiver.try_recv() else {
            return;
        };
        self.workspace.close_trades_dir_picker();
        let Some(dir) = choice else { return };
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.trades_dir = Some(dir.display().to_string());
        crate::paper_state::save(&path, &state);
        self.workspace.set_trades_dir(dir);
        for tab in &mut self.tabs {
            tab.paper
                .account_mut()
                .set_trades_dir(self.workspace.trades_dir().to_path_buf());
        }
    }

    /// Persist the active tab's cmd-trading settings and fan them out —
    /// one gesture, one meaning, every tab (the trades-dir rule).
    pub(super) fn persist_cmd_trading(&mut self) {
        let settings = self.active_tab().paper.account().cmd_trading();
        for tab in &mut self.tabs {
            tab.paper.set_cmd_trading(settings);
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.cmd_trading_enabled = Some(settings.enabled);
        state.cmd_buy_modifier = Some(settings.buy.as_str().to_owned());
        state.cmd_entry_kind = Some(settings.kind.as_str().to_owned());
        state.cmd_sell_modifier = Some(settings.sell.as_str().to_owned());
        crate::paper_state::save(&path, &state);
    }

    /// Save and fan out the strategies after a capability changed them, so a
    /// named call leaves the same durable trace a click does.
    pub(crate) fn control_persist_order_strategies(&mut self) {
        self.persist_order_strategies();
    }

    /// Save and fan out the risk per trade after a capability changed it.
    pub(crate) fn control_persist_risk_settings(&mut self) {
        self.persist_risk_settings();
    }

    /// Persist the risk per trade, the declared capital and the instrument
    /// money, and fan all three out.
    ///
    /// App-wide, like the ticket's other settings: a ceiling a trader sets
    /// in one tab is one they mean everywhere, and what a point of WIN is
    /// worth does not change because a second tab is looking at it.
    pub(crate) fn persist_risk_settings(&mut self) {
        let risk = self.active_tab().paper.account().risk_settings().clone();
        let capital = self.active_tab().paper.account().capital().clone();
        let book = self.active_tab().paper.account().instrument_money().clone();
        for tab in &mut self.tabs {
            tab.paper.account_mut().set_risk_settings(risk.clone());
            tab.paper.account_mut().set_capital(capital.clone());
            tab.paper.account_mut().set_instrument_money(book.clone());
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.risk_per_trade_basis = Some(risk.basis.token().to_owned());
        state.risk_per_trade_amount = Some(risk.amount.normalize().to_string());
        state.risk_per_trade_percent = Some(risk.percent.normalize().to_string());
        state.risk_per_trade_lock = Some(risk.lock);
        state.paper_capital = crate::risk_sizing::records_from_capital(&capital);
        state.instrument_money = crate::risk_sizing::records_from_book(&book);
        crate::paper_state::save(&path, &state);
    }

    /// Persist the named exit strategies and the ticket's selection, and fan
    /// them out - app-wide like cmd trading, because a ladder a trader built
    /// in one tab is a ladder they mean everywhere.
    pub(super) fn persist_order_strategies(&mut self) {
        // The wheel's per-instrument step rides with the strategies: both
        // are ticket settings the trader configures once, and both are
        // app-wide rather than per tab.
        let steps: std::collections::BTreeMap<String, String> = self
            .active_tab()
            .paper
            .ruler_steps()
            .iter()
            .map(|(symbol, step)| (symbol.clone(), step.normalize().to_string()))
            .collect();
        let strategies = self
            .active_tab()
            .paper
            .account()
            .order_strategies()
            .to_vec();
        let selected = self
            .active_tab()
            .paper
            .account()
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone());
        for tab in &mut self.tabs {
            tab.paper
                .account_mut()
                .set_order_strategies(strategies.clone(), selected.as_deref());
            tab.paper.set_ruler_steps(
                steps
                    .iter()
                    .filter_map(|(symbol, step)| {
                        step.parse().ok().map(|value| (symbol.clone(), value))
                    })
                    .collect(),
            );
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.order_strategies = Some(strategies);
        state.selected_order_strategy = selected;
        state.ruler_steps = steps;
        crate::paper_state::save(&path, &state);
    }

    /// Take a market that is already streaming as a new tab, and make it the
    /// active one.
    ///
    /// The bar spec is inherited from the tab you were on: opening a second
    /// market to compare it against the first is the reason to do this, and
    /// landing on a different aggregation would defeat that. A feed that
    /// declares its own `default_bars`/`default_layout` overrides the
    /// inheritance — the declaration exists because that market reads
    /// differently, which is exactly when inheriting would mislead.
    /// `spec` overrides both, and exists for the one caller that already knows
    /// the answer: a workspace restoring the bar rule this market was last
    /// read on. Inheriting there would quietly discard what the user saved.
    pub(super) fn adopt_tab(
        &mut self,
        feed_id: String,
        symbol: String,
        feed: FeedHandle,
        spec: Option<BarSpec>,
    ) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TAB_OPENED",
            tab = id,
            feed = %feed_id,
            symbol = %symbol,
            tabs = self.tabs.len() + 1,
            action = "activate_new_tab",
            "opening a market in a new tab"
        );
        let spec = spec.unwrap_or_else(|| {
            self.config
                .startup_spec_for(&feed_id)
                .unwrap_or_else(|| self.active_tab().flow_pane.state.spec().clone())
        });
        let trades_dir = self.workspace.trades_dir().to_path_buf();
        // Cmd trading is app-wide (the trades-dir rule): a new tab starts
        // with the settings every other tab already carries.
        let cmd_trading = self.active_tab().paper.account().cmd_trading();
        let inherited_strategies = self
            .active_tab()
            .paper
            .account()
            .order_strategies()
            .to_vec();
        let inherited_selection = self
            .active_tab()
            .paper
            .account()
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone());
        // Orientation travels with the working state the new tab inherits —
        // a market opened to compare against the active one is only
        // comparable the same way up. Per pane; a pane the source tab does
        // not have follows its flow chart.
        // The layers the active tab is *actually showing*, read before the new
        // tab is pushed. This used to be `self.layer_defaults` — the map read
        // off the file at startup — which was only harmless while that map was
        // whatever partial thing the trader's file happened to hold. Now that a
        // file's silence resolves to the shipped answer (`chart_layers::load`),
        // that map speaks for every layer, and applying it here would undo the
        // switches of the session mid-flight. Reading the live state is also
        // what the comment below has always promised.
        let inherited_risk = self.active_tab().paper.account().risk_settings().clone();
        let inherited_capital = self.active_tab().paper.account().capital().clone();
        let inherited_money = self.active_tab().paper.account().instrument_money().clone();
        let inherited_layers = self.active_tab().flow_pane.layer_states(&self.style);
        let flow_inverted = self.active_tab().flow_pane.price_view.is_inverted();
        let time_inverted = self
            .active_tab()
            .time_pane()
            .map_or(flow_inverted, |pane| pane.price_view.is_inverted());
        // The layout the trader is looking at is the one the new chart
        // opens on — read before the new tab takes the focus.
        let inherited_layout = (!self.tabs.is_empty()).then(|| self.focused_pane_layout());
        let flow_pane_id = self.pane_ids.alloc();
        let mut tab = Tab::new(id, flow_pane_id, feed_id, symbol, spec, feed, trades_dir);
        tab.paper.set_cmd_trading(cmd_trading);
        tab.paper
            .account_mut()
            .set_order_strategies(inherited_strategies, inherited_selection.as_deref());
        // The risk per trade travels with them. It is app-wide like the rest
        // of the ticket's settings, and a tab that opened without it would
        // hand the trader a bare quantity field on a market they meant to
        // size the same way as the one beside it.
        tab.paper.account_mut().set_risk_settings(inherited_risk);
        tab.paper.account_mut().set_capital(inherited_capital);
        tab.paper
            .account_mut()
            .set_instrument_money(inherited_money);
        tab.flow_pane.layout = inherited_layout;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        self.active_tab_mut().ensure_book_capture(&config);
        self.active_tab_mut().apply_feed_bubble_preset(&config);
        self.active_tab_mut().apply_feed_declared_layout(&config);
        // The new tab opens on the layers the user left showing, over the
        // preset it just put on: opening a second market is not a request to
        // bring back the chrome they switched off.
        self.active_tab_mut()
            .flow_pane
            .apply_layer_states(&inherited_layers);
        // The scripted footprint/zoom hooks reach tabs opened later too: the
        // replay tab a validation run autostarts is the tab the run means,
        // and it does not exist yet when the boot hooks fire.
        if self.harness.footprint() {
            self.active_tab_mut().flow_pane.footprint_visible = true;
        }
        if let Some(px) = self.harness.candle_width() {
            self.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
        }
        // After the declared layout ran: that is what decides whether the
        // new tab has a time pane to orient at all.
        let tab = self.active_tab_mut();
        tab.flow_pane.price_view.set_inverted(flow_inverted);
        for time_pane in tab.time_panes.iter_mut() {
            time_pane.price_view.set_inverted(time_inverted);
        }
    }

    pub(super) fn arm_strategy_instance(
        &mut self,
        side: pane::PaneSide,
        drawing: drawings::DrawingId,
        form: &crate::strategy_presets::StoredPreset,
        preset_label: String,
    ) -> Result<(), String> {
        let Some(compiled) = form.to_kernel() else {
            return Err(
                "a field does not parse: quantity, factors and multipliers must be numbers, \
                 and an instance that neither trades nor alarms cannot be armed"
                    .to_owned(),
            );
        };
        let crate::strategy_presets::CompiledPreset {
            params,
            force,
            alarm,
        } = compiled;
        let tab = self.active_tab_mut();
        let replaced_cleanup = {
            let pane = tab.pane_mut(side);
            // Re-validate everything the menu's gate promised: this is also
            // the seam a future programmatic caller (the NL layer) comes
            // through, and it must not be able to arm what the menu would
            // refuse — the wrong shape, another band, a drawing with no
            // footing here, or one nobody can see.
            let Some(index) = pane.drawings.index_of(drawing) else {
                return Err("the drawing is gone".to_owned());
            };
            let target = &pane.drawings.items()[index];
            if target.tool.id() != drawings::RECTANGLE_TOOL_ID
                || target.band != drawings::DrawingBand::Price
                || target.points.len() != 2
            {
                return Err("only price-band rectangles carry strategies".to_owned());
            }
            if target.foreign_market || target.off_series {
                return Err(
                    "this drawing belongs to another market or lost its series — redraw the \
                     region here first"
                        .to_owned(),
                );
            }
            if target.hidden || pane.drawings.all_hidden() {
                return Err("unhide the drawing first — an armed region stays visible".to_owned());
            }
            // A region whose drawn span can no longer cover a future bar
            // can never fire: the badge would show "armed" over a bot that
            // is structurally done — the silent halt the named disarms
            // exist to prevent. One predicate, shared with re-arm and the
            // evaluation sweep (`Pane::strategy_region_can_fire`), refuses
            // it with the fix in hand.
            if !pane.strategy_region_can_fire(drawing) {
                return Err(
                    "the region ends before the next bar, so nothing can ever fire — \
                     stretch it past the right edge, or turn on \"extend right\" in its \
                     Region settings"
                        .to_owned(),
                );
            }
            let mut armed = quantick_strategy::ArmedStrategy::new(
                params,
                Box::new(quantick_strategy::ForceTrigger::new(force.clone())),
            );
            // Warm the ruler on the bars the chart is already showing —
            // armed means armed now, not after another twenty bars of
            // warmup the trader cannot see the reason for. The trigger
            // declares its own depth (`warmup_bars`), and the pane keeps
            // venue-prefix candles out: they measure another ruler
            // entirely (a 1-minute body dwarfs a tick-bar body).
            armed.warm(&pane.strategy_warmup_bars(armed.trigger().warmup_bars()));
            pane.strategies
                .arm(crate::strategy_anchors::AnchoredInstance {
                    drawing,
                    preset: preset_label,
                    spec: form.clone(),
                    armed,
                    alarm: alarm.map(|setup| quantick_strategy::SignalAlarm::new(setup.params)),
                    cue: alarm.map(|setup| setup.cue).unwrap_or_default(),
                    mark: crate::strategy_anchors::AlarmMark::Quiet,
                })
        };
        for command in replaced_cleanup {
            // Arming over an instance with a pending entry sweeps that
            // entry — a resting order must never outlive its bot.
            let _ = tab.paper.account_mut().apply_strategy_command(command);
        }
        tab.paper.account_mut().set_bot_listening(true);
        // Only now, past every gate: the sink opens its device at arm time
        // so the first signal does not pay for it on the tape's path, but a
        // *refused* arm must open nothing. Ctrl+D over a band the copy
        // cannot be armed on discards the `Err` by design — the absent
        // badge is the message — and warming above the gates turned that
        // silence into an audio stack enumerated once per keypress, against
        // the sink's own promise that a chart which never arms an alarm
        // never touches a device.
        if let Some(setup) = alarm {
            self.alerts.warm_up(setup.cue);
        }
        Ok(())
    }
}
