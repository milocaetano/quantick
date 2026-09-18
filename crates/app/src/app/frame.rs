//! One frame of the application, from the top.
//!
//! Everything [`eframe::App::update`] does that is not eframe's own
//! bookkeeping: drain the feed, lay the chrome out around the canvas, draw
//! the chart, then run the per-frame appliers. It is one method because a
//! frame is one sequence — the order the panels are reserved in *is* the
//! layout — and it is in its own file because that sequence is the longest
//! single thing the window does.

use quantick_chart_interaction::frame_tail_plan::{FrameTailPlan, FrameTailStage};
use std::time::{Duration, Instant};

use eframe::egui;
use smallvec::SmallVec;

use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::dock::{DockEnv, DockTab};
use crate::feed_notice;
use crate::indicator_panel::SettingsDialog;
use crate::loading::{self, LoadingScope, LoadingTask};
use crate::metrics;
use crate::pane::{self, PaneSide};
use crate::statusbar;
use crate::tab::{CanvasChrome, Tab};

use super::indicator_manager::IndicatorState;
use super::replay_and_history::AlertState;
use super::{QuantickApp, TabSlot};

/// The chart rectangle a settings dialog is previewing an unapplied draft on,
/// if one is.
///
/// The pane the *dialog* was opened over, not the focused one: a trader can
/// preview a curve on the left pane and then click the right, and the banner
/// belongs over the numbers that are actually provisional.
///
/// A free function rather than a method because its only caller has already
/// split `QuantickApp` into disjoint borrows to build the surface
/// environment, and a method would want the whole of `self` back. Per-frame,
/// and shaped to leave immediately: no dialog open — the ordinary case — is
/// one `Option` test before the tab scan is reached.
fn indicator_preview_area(
    tabs: &crate::app::arrangement_host::ArrangementHost,
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

#[cfg(test)]
impl QuantickApp {
    /// A frame whose tail runs `stages` in the given order: the mutant-order
    /// proof's door, closed to production callers so the plan's `const`
    /// validation cannot be bypassed.
    pub(super) fn draw_frame_test_order(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        spawn: &mut crate::tab::LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameTailStage>,
    ) {
        self.draw_frame_with_tail(ctx, now, spawn, stages);
    }
}

impl QuantickApp {
    /// One frame of the application: drain, lay out the chrome, draw the
    /// chart.
    ///
    /// Everything `update` does that is not eframe's own bookkeeping, so a
    /// test can run a real frame against a headless [`egui::Context`] and read
    /// what was painted — the only honest way to assert that a chart is on
    /// screen rather than a blank rectangle.
    pub(super) fn draw_frame(&mut self, ctx: &egui::Context, now: Instant) {
        self.draw_frame_with_tail(
            ctx,
            now,
            &mut quantick_feed::spawn_live,
            FrameTailPlan::stages(),
        );
    }

    fn draw_frame_with_tail(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        spawn: &mut crate::tab::LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameTailStage>,
    ) {
        if let Some(last) = self.health.last_frame {
            self.health
                .frames
                .record((now - last).as_secs_f32() * 1000.0);
        }
        self.health.last_frame = Some(now);

        self.drain_tabs();
        // A "load older" outcome is a passing remark: it leaves after
        // `tab::HISTORY_NOTE_LINGER` whether or not anyone read it. Every tab,
        // not only the one on screen — a background tab keeps draining, so it
        // can settle a run while hidden, and bringing it forward minutes later
        // must not surface a sentence about a press that is long over.
        for tab in self.tabs.iter_mut() {
            tab.expire_history_note(now);
        }
        // After the expiry, never before it: the hook re-raises a note it
        // finds absent, and running it first would let a note expire *after*
        // it looked, drawing one frame with an empty lane before the next
        // raise. A shutter timed on the linger catches exactly that frame.
        self.apply_history_note_hook();
        #[cfg(any(feature = "control-harness", test))]
        if self.control.scenarios.take_enable()
            && let Some(access) = self.control.control_access.as_mut()
        {
            access.enable(ctx);
        }
        // Replay determinism: a session with a control trace beside it
        // re-injects its actions at their logical time, connected or not.
        // Before the hook's mark, so a loaded sidecar has seeded the trace
        // sequence the mark will take.
        if let Some(mut access) = self.control.control_access.take() {
            access.service_replay_trace(self);
            self.control.control_access = Some(access);
        }
        #[cfg(any(feature = "control-harness", test))]
        if let Some(note) = self.control.scenarios.take_mark() {
            let note = (!note.is_empty()).then_some(note);
            self.take_mark(note);
        }
        #[cfg(any(feature = "control-harness", test))]
        self.apply_control_annotate_hooks();
        // After the annotate hooks and before the gateway's own drain: a
        // bundle captured from a launch then describes the window an
        // assistant has already written on, which is the state a validation
        // run is actually asking about.
        #[cfg(any(feature = "control-harness", test))]
        self.apply_control_evidence_hook(ctx);
        if self
            .control
            .control_access
            .as_ref()
            .is_some_and(crate::control::ControlAccess::needs_frame_service)
            && let Some(mut access) = self.control.control_access.take()
        {
            access.begin_frame(self, ctx);
            self.control.control_access = Some(access);
        }
        self.apply_scripted_view();
        #[cfg(any(feature = "drawing-harness", test))]
        self.apply_drawing_demo();
        self.apply_load_older();
        self.apply_load_older_candles();
        #[cfg(any(feature = "drawing-harness", test))]
        self.apply_drawing_draft();
        self.apply_venue_history_demo();
        #[cfg(any(feature = "drawing-harness", test))]
        self.apply_frvp_demo();
        #[cfg(any(feature = "drawing-harness", test))]
        self.apply_avwap_demo();
        self.apply_strategy_demo();
        self.apply_replay_restart();
        self.chrome.window_startup.apply(ctx);
        self.maybe_emit_summary(now, ctx);
        self.workspace_save_adapter().maintain_workspace(ctx);

        let bg = pane::background_color(&self.style);
        // Rail shortcuts first: Esc/1/2 must be read before any widget can
        // claim the keyboard this frame.
        self.toolrail.handle_keys(ctx);
        self.handle_tab_keys(ctx);
        let effects = self.drawings.handle_drawing_keys(
            &mut super::drawing_controller::DrawingAccess::new(&mut self.tabs),
            &mut self.toolrail,
            &mut *self.audio.alerts,
            ctx,
            now,
        );
        effects.apply_notice(&mut self.surfaces.toast);
        // Chrome panels claim their zones outside-in (§5): menu and toolbar
        // on top, the status line at the very bottom with the replay
        // transport directly above it, then the edge-docked drawing rail and
        // the right dock. The chart keeps whatever remains.
        self.draw_menu_bar(ctx);
        if let Some(access) = self.control.control_access.as_mut() {
            access.draw_panel(ctx);
        }
        self.draw_toolbar(ctx);
        // Before the dialog is drawn, so a double click on a pane or a curve
        // opens it on the same frame the gesture happened rather than the next.
        self.service_indicator_requests();
        self.draw_indicator_surfaces(ctx);
        // **After** the dialogs above, and that placement is load-bearing.
        // The preview watermark reads whether a settings dialog is previewing
        // an unapplied draft, and `IndicatorState::draw_settings` is what sets that
        // — so an environment built before it would put the banner on screen
        // a frame after the legend chip that says the same thing, and take it
        // off a frame later too. Two surfaces the trader reads as one is this
        // repo's own bug class; sixteen milliseconds of it is still one
        // frame a capture can photograph.
        //
        // It is also where the windows this pass drew used to sit: the
        // appearance and footprint panels ran after the toolbar that toggles
        // them, so a click on LOOK opens the panel on the same frame rather
        // than the next.
        // A pane's right-click asked to arm one of its drawings. Drained
        // here, into the surface that owns the dialog: the click happens
        // while the canvas draws, which is later in this frame than the
        // surfaces are, so the dialog opens on the next one — a frame the
        // trader cannot see, and the price of the dialog no longer living in
        // the trunk.
        let sides: SmallVec<[pane::PaneSide; MAX_CANVAS_PANES]> =
            self.active_tab().sides().collect();
        for side in sides {
            let request = self
                .active_tab_mut()
                .pane_mut(side)
                .strategies
                .popup_request
                .take();
            if let Some(drawing) = request {
                let form = crate::strategy_presets::StoredPreset::starting_point(
                    quantick_engine::Side::Buy,
                );
                let tab = self.tabs.active_id();
                self.surfaces.strategy_popup.open(tab, side, drawing, form);
            }
        }
        // The bar rules the arming dialog's alarm section reads: a share of
        // the bar only means something where the rule closes on a count.
        // Built only while that dialog is open, like the open markets below.
        // `hooks_pending` is the frame a capture hook opens a surface from
        // inside `draw_all`: it is not open yet when this runs, but it is
        // about to be, and it must not draw its first frame against an empty
        // environment.
        let staging = self.surfaces.hooks_pending();
        let counted_bar_sides: SmallVec<[pane::PaneSide; MAX_CANVAS_PANES]> =
            if staging || self.surfaces.strategy_popup.is_open() {
                let tab = self.active_tab();
                tab.sides()
                    .filter(|side| tab.pane(*side).state.progress().is_some())
                    .collect()
            } else {
                SmallVec::new()
            };
        // The markets tabs are showing: the dialog greys out removing one of
        // those, because a tab left on a symbol the catalog no longer offers
        // gets silently retargeted by the next SOURCE correction. Built only
        // while the dialog is open — it is a `String` pair per tab, and no
        // frame should pay for it to be thrown away.
        let open_markets: Vec<(String, String)> =
            if staging || self.surfaces.source_picker.is_open() {
                self.tabs
                    .iter()
                    .map(|tab| (tab.feed_id.clone(), tab.symbol.clone()))
                    .collect()
            } else {
                Vec::new()
            };
        // Split into disjoint borrows: the surfaces are drawn through `&mut`
        // while the environment they read is borrowed from the rest of the
        // application. That the compiler insists on the split is the port
        // working — a surface cannot be handed the trunk it is being kept
        // out of.
        let Self {
            indicators:
                IndicatorState {
                    indicator_settings,
                    indicator_settings_target,
                    ..
                },
            audio: AlertState { alert_failure, .. },
            surfaces: registry,
            workspace,
            style,
            footprint_config,
            tabs,
            config,
            added_symbols,
            ..
        } = self;
        let focused_tab = &tabs[tabs.active_index()];
        // Read once. `focused_pane` resolves the same side internally, and
        // the answer is not a field lookup — it reads the layout, because
        // focus on a collapsed pane is focus on nothing.
        let focused_side = focused_tab.focused_side();
        let focused_pane = focused_tab.pane(focused_side);
        let surfaces = registry.draw_all(
            ctx,
            &crate::surfaces::SurfaceEnv {
                bookmarks: workspace.session().bookmarks(),
                now,
                indicator_preview_area: indicator_preview_area(
                    tabs,
                    indicator_settings.as_ref(),
                    *indicator_settings_target,
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
                alert_failure: alert_failure.as_deref(),
            },
        );
        if let Some(name) = surfaces.save_workspace_as {
            self.workspace_save_adapter().save_named_workspace(&name);
        }
        if let Some(style) = surfaces.style {
            self.style = style;
            self.style_revision = self.style_revision.saturating_add(1);
        }
        // After the assignment, never before: the log line reports the
        // appearance that is now in force, and the revision it landed on.
        if let Some(request) = surfaces.log_style_change {
            super::health::emit_style_changed(
                &self.style,
                self.style_revision,
                request.applied_preset,
            );
        }
        // The audition goes through the one speaker every armed instance
        // shares, and reports a sound that could not be heard exactly as a
        // missed signal would.
        if let Some(cue) = surfaces.test_alert {
            let outcome = self.audio.alerts.play(&[cue]);
            self.report_alert_attempt(outcome);
        }
        if let Some(request) = surfaces.arm_strategy {
            let outcome = self
                .tabs
                .runtime_mut(self.tabs.active_index())
                .arm_strategy_instance(
                    &mut *self.audio.alerts,
                    request.side,
                    request.drawing,
                    &request.form,
                    request.label,
                );
            self.surfaces.strategy_popup.settle_arm(outcome);
        }
        if let Some(request) = surfaces.market {
            self.apply_market_request(request);
        }
        if let Some(change) = surfaces.footprint {
            self.apply_footprint_change(change);
        }
        if surfaces.undo_drawing {
            let pane = self.drawing_pane_mut();
            pane.drawings.undo();
            // Same orphan risk as the keyboard undo: the drawing an armed
            // instance rides may just have been taken away.
            pane.sweep_strategy_orphans();
        }
        for (owner, name, text) in self.indicators.poll_script_files() {
            IndicatorState::log_reload(owner, &name);
            if let Some(tab) = self.tabs.by_id_mut(owner.tab) {
                tab.pane_mut(owner.side).indicator_worker.send(
                    crate::indicator_worker::IndicatorCommand::Reload {
                        slot: owner.slot,
                        source: crate::indicator_worker::IndicatorSource::Script { name, text },
                    },
                );
            }
        }
        self.layout_adapter().apply_pending_indicator_state();
        self.maintain_chart_layers();
        // This tab's judgement about its own feed, taken once for the frame:
        // the status bar reads it here and the corner reads it below, and two
        // readings a millisecond apart could disagree about whether a budget
        // had run out.
        let stall = self
            .active_tab()
            .stall_at(&self.config, metrics::wall_clock_ms());
        let offline_accent = self.feed_offline_accent(stall.as_ref());
        let status = self.status_model();
        let status_response = statusbar::draw(ctx, &status, &mut self.tz, offline_accent);
        if status_response.open_trading_tab {
            self.dock.open_tab(DockTab::Trading);
        }
        self.layout_adapter().draw_layout_delete_confirm(ctx);
        // The browser window and, while the *active* tab plays a session, its
        // transport bar. A background tab's recording keeps advancing on its
        // own feed thread; what it does not get is the strip, which speaks for
        // one tab at a time (§11).
        let replay_action = {
            let Self {
                replay_view,
                tabs,
                config,
                ..
            } = self;
            let tab = &tabs[tabs.active_index()];
            // The instruments the download tab offers with one click. A dated
            // contract rolls every couple of months, and typing `WINV26` from
            // memory is not a thing a trader should have to get right to see
            // what they can replay.
            //
            // Filtered by what the download source actually serves, which the
            // source itself answers: offering a Binance pair to a MetaTrader
            // exporter would be a click that can only end in a refusal, and
            // the chart behind this window is often on another venue entirely.
            let serves = replay_view.download_provider();
            let market = crate::replay_view::MarketMenu {
                current: (config.provider_of(&tab.feed_id) == Some(serves))
                    .then_some(tab.symbol.as_str()),
                catalogue: config
                    .feeds
                    .iter()
                    .filter(|feed| feed.provider == serves)
                    .flat_map(|feed| feed.symbols.iter().map(String::as_str))
                    .collect(),
            };
            replay_view.draw(ctx, tab.replay.as_ref(), &market)
        };
        if let Some(action) = replay_action {
            self.apply_replay_action(action);
        }
        // A folder the trader just pointed the browser at is written down on
        // the frame they pointed it, not at exit: "it forgot my folder again"
        // must not be one crash away.
        if let Some(pick) = self.replay_view.take_folder_change() {
            self.workspace_save_adapter()
                .write_replay_folder(pick.as_deref());
        }
        // The same, for the tick that decides whether yesterday is on the
        // chart. Either row can have been the one clicked; the browser owns
        // the setting, so there is one place to pick the change up.
        if let Some(enabled) = self.replay_view.take_day_before_change() {
            self.workspace_save_adapter()
                .write_replay_day_before(enabled);
        }
        {
            // The focused pane's objects: the toolbox lists and manages what a
            // click on the canvas would act on.
            let side = self.active_tab().focused_side();
            // The flag lives with the window it opens, so it travels through
            // a local rather than a `&mut` handed out of the surface.
            let mut manager_open = self.drawings.chrome.manager_open();
            {
                let Self { toolrail, tabs, .. } = self;
                let tab = tabs.runtime_mut(tabs.active_index());
                toolrail.draw(ctx, &mut tab.pane_mut(side).drawings, &mut manager_open);
            }
            self.drawings.chrome.set_manager_open(manager_open);
        }
        // A star clicked this frame is on disk this frame, like the replay
        // folder above: the pinned rail is what the trader reaches for without
        // looking, and rebuilding it after a crash is not a thing anyone
        // should have to do twice.
        if self.toolrail.take_favorites_change() {
            self.workspace_save_adapter().write_favorites();
        }
        let dock_response = {
            let Self {
                dock,
                tabs,
                replay_view,
                tz,
                ..
            } = self;
            // The Trading tab speaks for the market on screen: one tab, one
            // simulator, and the dock reads the active tab's — exactly like
            // the tape and the session panel beside it.
            let Tab {
                flow_pane,
                replay,
                paper,
                ..
            } = tabs.runtime_mut(tabs.active_index());
            let orderflow = flow_pane
                .orderflow
                .as_mut()
                .expect("the flow pane is built with a tape and never drops it");
            dock.draw(
                ctx,
                &mut DockEnv {
                    orderflow,
                    replay_view,
                    replay: replay.as_ref(),
                    paper,
                    tz: *tz,
                },
            )
        };
        // The strategy editor is a window of the active tab's ticket, drawn
        // whatever the dock is showing and whether it is showing at all: it
        // is opened from the Trading tab but it does not belong to it, and a
        // trader who opens it and then looks at the ledger has not asked for
        // it to close.
        if self.active_tab_mut().paper.draw_strategy_editor(ctx) {
            self.persist_order_strategies();
        }
        if dock_response.restart_book_capture {
            self.active_tab_mut().restart_book_capture();
        }
        if let Some(action) = dock_response.replay_action {
            // A click that lost its slot has the trader's next click behind
            // it; only the one-shot hook below cares about the answer.
            let _ = self.apply_replay_action(action);
        }
        // The ledger's jump-to-trade: center the flow pane on the round
        // trip's midpoint, the object manager's own "select and centre".
        //
        // The covering lookup, the same one the marks are painted through:
        // a trade the flow chart's bars do not reach has nowhere to be
        // centred on, and scrolling to the clamped edge instead would land
        // the trader on a bar holding no mark and no explanation. Saying so
        // is the whole of the handling — the row stays in the ledger.
        //
        // The message names the flow chart rather than "the chart": in a
        // split tab the time pane keeps its own, longer window, so the same
        // round trip can be off this one and painted on that one.
        if let Some((opened, closed)) = dock_response.navigate_to_trade {
            let tab = self.active_tab_mut();
            let covered = tab
                .flow_pane
                .covering_slot_at_time(opened)
                .zip(tab.flow_pane.covering_slot_at_time(closed));
            match covered {
                Some((entry, exit)) => {
                    let pane = &mut tab.flow_pane;
                    if let Some(area) = pane.frame.chart_area {
                        let slots = pane.slots();
                        let mid = (entry + exit) as f32 / 2.0;
                        pane.viewport.center_on_bar(mid, area.width(), slots);
                    }
                }
                None => {
                    // Said as an event as well as on screen: an operator
                    // driving the ledger without eyes on the toast must be
                    // able to tell a refusal from a silent no-op.
                    tracing::info!(
                        target: "quantick::app",
                        event_code = "TRADE_NAVIGATE_OFF_TAPE",
                        opened_ms = opened,
                        closed_ms = closed,
                        "jump-to-trade refused: the flow chart has no bar for the fills"
                    );
                    tab.paper.show_toast(
                        "This trade is outside the bars on the flow chart - nothing to centre on."
                            .to_owned(),
                    );
                }
            }
        }
        if dock_response.pick_trades_dir {
            self.open_trades_dir_picker();
        }
        if dock_response.order_strategies_changed {
            self.persist_order_strategies();
        }
        if dock_response.cmd_trading_changed {
            self.persist_cmd_trading();
        }
        if dock_response.risk_settings_changed {
            self.persist_risk_settings();
        }
        self.poll_trades_dir_picker();
        self.poll_workspace_picker();
        // The pinned inspector is chrome: declared before the central canvas
        // so the chart pays its width, exactly like the dock.
        if let Some(ask) = self.drawings.draw_pinned_inspector(
            ctx,
            &super::drawing_controller::DrawingReadAccess::new(&self.tabs),
            &self.toolrail,
        ) {
            self.resolve_drawing_response(ask, now);
        }
        // Respawn the feed if the feed/symbol selection changed (resets the
        // chart), then apply any bar-type change (no-op if unchanged).
        let (tab, config) = self.active_with_config();
        tab.maybe_switch_feed(config);
        // Both deferrals settle here, a frame after the click that armed
        // them, so the frame carrying the change paints its overlay first.
        let Self {
            tabs,
            config,
            style,
            pane_ids,
            ..
        } = self;
        for (tab_id, tab) in tabs.iter_with_ids_mut() {
            tab.apply_pending_layout(tab_id, config, style, pane_ids);
        }
        // Right after panes appear and markets switch, so a pane built this
        // frame is seeded this frame and a tab that changed symbol swaps its
        // drawings before anything paints them.
        self.layout_adapter().maintain_layouts();
        self.active_tab_mut().apply_spec_changes();
        // Waits owned by other components, mirrored level-style each frame so
        // the overlay needs no push notifications from either.
        let replay_loading = self.replay_view.is_loading();
        let book_syncing = self.active_tab().tape().is_syncing();
        let tab = self.active_tab_mut();
        tab.loading
            .set_active(LoadingTask::ReplaySession, replay_loading);
        tab.loading.set_active(LoadingTask::BookSync, book_syncing);

        let mut notice_action = feed_notice::NoticeAction::None;
        // Read before the canvas borrows `self`, and answered after it lets go.
        let popup_tab = self.tabs.active_id();
        let popup_open = self.chrome.feed_popup_tab == Some(popup_tab);
        let mut chip_clicked = false;
        let mut dismissed = false;
        // Where the corner landed, and so whether there was one at all. A feed
        // that recovered while the popup was open closes it, rather than
        // leaving a stale explanation over a chart that is fine again.
        let mut chip_rect = None;
        // The layer menu offers what this source can produce; resolved once
        // here rather than per pane, per entry, inside the canvas.
        let capabilities = self.active_tab().capabilities(&self.config);
        // Same one-per-frame resolution for the side-honesty label the
        // footprint legend carries.
        let side_inferred = self.active_tab().side_note(&self.config).is_some();
        // Told before the canvas paints, not after: the object holding the
        // words the editor is showing must stand down on the *same* frame,
        // or the note flashes its placeholder under the field for one.
        self.drawings.chrome.sync_content_editing(&mut self.tabs);
        // Raised by a placement that wants its note typed, and handed to the
        // chrome below: the flag belongs to the editor that owns the caret,
        // not to the canvas that asks for it.
        let mut begin_text_edit = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                {
                    let Self {
                        tabs,
                        toolrail,
                        drawings,
                        style,
                        tz,
                        workspace,
                        footprint_config,
                        ..
                    } = self;
                    let mut chrome = CanvasChrome {
                        toolrail,
                        presets: &drawings.presets,
                        drawing_chrome: &mut drawings.chrome,
                        begin_text_edit: &mut begin_text_edit,
                        style,
                        tz: *tz,
                        capabilities,
                        side_inferred,
                        footprint: footprint_config,
                        layers: &mut workspace.layers_mut().actions,
                    };
                    let tab_id = tabs.active_id();
                    tabs.runtime_mut(tabs.active_index()).draw_canvas(
                        tab_id,
                        ui,
                        area,
                        &mut chrome,
                    );
                }
                // Each visible pane published its own reserved footer during
                // the canvas split. Draw the shared catalogue into every one
                // now, while their exact same-frame rectangles are available.
                self.layout_adapter().draw_layout_strips(ui);
                // The grid and the indicator state belong to the window, not
                // to the pane whose menu switched them.
                self.apply_layer_actions();
                let tab = self.active_tab();
                // Each wait on the surface it is about. The panes published
                // their rects on the draw just above, so these are this
                // frame's geometry rather than the previous one's.
                let history_note = tab.history_note();
                loading::overlay_scoped(ui, area, &tab.loading, LoadingScope::Whole, history_note);
                // A scope whose surface is not on screen falls back to the
                // canvas rather than dropping its wait: the flow pane is not
                // painted in the Time layout, and a flow-only layout has no
                // time pane, and in both cases the wait is still running. A
                // spinner in the wrong place is a placement complaint; a
                // missing one reads as a frozen application.
                loading::overlay_scoped(
                    ui,
                    tab.flow_pane.frame.area.unwrap_or(area),
                    &tab.loading,
                    LoadingScope::Flow,
                    history_note,
                );
                let time_panes: Vec<egui::Rect> = tab
                    .panes()
                    .filter(|(_, side)| matches!(side, PaneSide::Time(_)))
                    .filter_map(|(pane, _)| pane.frame.area)
                    .collect();
                if time_panes.is_empty() {
                    loading::overlay_scoped(
                        ui,
                        area,
                        &tab.loading,
                        LoadingScope::TimePanes,
                        history_note,
                    );
                } else {
                    for rect in time_panes {
                        loading::overlay_scoped(
                            ui,
                            rect,
                            &tab.loading,
                            LoadingScope::TimePanes,
                            history_note,
                        );
                    }
                }
                // And the feed's own report, in the corner rather than over
                // the chart. Progress never gets here: a first connection and a
                // history block already have the loading overlay above, and a
                // second badge beside it would be the interface talking about
                // itself twice.
                // The deal-recording chip, left of the offline chip's corner.
                let corner_area = tab
                    .panes()
                    .filter_map(|(pane, _)| pane.frame.area)
                    .max_by(|left, right| {
                        left.right()
                            .total_cmp(&right.right())
                            .then(left.bottom().total_cmp(&right.bottom()))
                    })
                    .unwrap_or(area);
                crate::deal_recording_ui::draw_corner(ui, corner_area, tab, stall.as_ref());
                if let Some(report) = feed_notice::report(&tab.notice, stall.as_ref())
                    && report.is_offline()
                {
                    // Measured once, then handed to everything that needs
                    // it: the chip's own hit test, the popup's anchor, the
                    // dismissal test, and the scene's bounds.
                    let chip = feed_notice::chip_rect(ui.painter(), corner_area);
                    chip_rect = Some(chip);
                    chip_clicked = feed_notice::draw_chip(ui, chip, &report, popup_open);
                    // A pane with nothing on it has room to say why, and a
                    // corner chip alone on a blank canvas is a puzzle. One
                    // muted line, no border and no buttons — the way out is
                    // still the corner.
                    //
                    // Not while the popup is up. The line and the popup carry
                    // the same headline, and on the empty chart that is
                    // exactly where both of them draw: one sentence, twice, a
                    // hand apart.
                    if !popup_open && let Some((pane_rect, 0)) = tab.starved_pane() {
                        feed_notice::draw_empty_pane_note(ui.painter(), pane_rect, &report);
                    }
                    if popup_open {
                        // A click anywhere else puts it away, measured against
                        // the rectangles that were actually drawn — so a click
                        // on the edge of what the trader can see is never read
                        // as a click outside it, and the popup is laid out
                        // once rather than measured again to ask.
                        let popup;
                        (notice_action, popup) = feed_notice::draw_popup(ui, area, chip, &report);
                        dismissed = ui.input(|input| {
                            input.pointer.any_click()
                                && input
                                    .pointer
                                    .interact_pos()
                                    .is_some_and(|at| !popup.contains(at) && !chip.contains(at))
                        });
                    }
                }
            });
        if begin_text_edit {
            self.drawings.chrome.request_text_edit();
        }
        // Floating drawing controls must be registered after the opaque
        // central canvas so they stay in front of the chart. That is why the
        // drawing chrome is the one surface `Surfaces::draw_all` does not
        // draw: it is anchored *to* the chart rather than floating over the
        // window, so it is commanded by name from here instead.
        let ask = self.drawings.draw_drawing_chrome(
            ctx,
            &super::drawing_controller::DrawingReadAccess::new(&self.tabs),
            &self.toolrail,
        );
        self.resolve_drawing_response(ask, now);
        // The menus above may have disarmed a bot over a resting retest
        // limit; its cancel goes to the simulator on this same frame, not
        // on the next print. Every tab, not just the active one: a menu
        // click and a tab switch can land on the same frame, and the old
        // tab's feed keeps running — its cancel must not sit stranded
        // until the tab is looked at again.
        for tab in self.tabs.iter_mut() {
            tab.apply_strategy_cleanup();
        }
        self.play_pending_alarms();
        super::frame_tail::FrameTailOwners {
            tabs: &mut self.tabs,
            config: &self.config,
            toast: &mut self.surfaces.toast,
            chip_rect: &mut self.chrome.feed_chip_rect,
            popup_tab: &mut self.chrome.feed_popup_tab,
        }
        .execute(
            super::frame_tail::FrameTailInput {
                ctx,
                now,
                tz: self.tz,
                notice_action,
                popup_tab,
                popup_open,
                chip_clicked,
                dismissed,
                chip_rect,
            },
            spawn,
            stages,
        );
        // Live feed: keep polling the channel ~60×/s without busy-spinning.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl QuantickApp {
    /// The registered capability runs synchronously before the remaining drawing
    /// response. Invalid input still leaves those ordinary commands to execute.
    pub(super) fn resolve_drawing_response(
        &mut self,
        mut ask: crate::surfaces::drawing_chrome::DrawingChromeAsk,
        now: Instant,
    ) {
        if let Some(action) = self.drawings.begin_registered_action(&mut ask) {
            let pending = action.pending;
            let result = self.control_action(
                action.capability,
                action.version,
                crate::control::ActionOrigin::Human,
                action.input,
            );
            let explain = self
                .drawings
                .finish_registered_action(pending, if result.is_ok() {
                    quantick_chart_interaction::quick_range::conversion_plan::PlacementOutcome::Placed
                } else {
                    quantick_chart_interaction::quick_range::conversion_plan::PlacementOutcome::ActionRefused
                });
            if let Err(error) = result {
                tracing::warn!(target:"quantick::control",event_code="QUICK_RANGE_PROFILE_REFUSED",code=%error.code,error=%error.message,"the quick-range drawing could not be placed");
                if explain {
                    self.surfaces.toast.note(
                        "The drawing could not be placed; the temporary range is still available.",
                        now,
                    );
                }
            }
        }
        let effects = self.drawings.apply_drawing_chrome(
            ask,
            &mut super::drawing_controller::DrawingAccess::new(&mut self.tabs),
            &mut *self.audio.alerts,
            now,
        );
        if effects.inspector_moved {
            self.workspace.session_mut().inspector_moved();
        }
        effects.apply_notice(&mut self.surfaces.toast);
    }
}
