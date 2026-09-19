//! One frame of the application, as registered stages.
//!
//! Everything [`eframe::App::update`] does that is not eframe's own
//! bookkeeping runs as the stages of
//! [`FramePlan`], declared below from `quantick_chart_interaction::frame_stages!`: drain the
//! feeds, run the harness hooks, lay the chrome out around the canvas, draw
//! the chart, then the frame tail. The plan declares which stage depends on
//! which, and the declaration is checked when it compiles; the frame loop
//! here walks it and runs one adapter per stage. The order the panels are
//! reserved in *is* the layout, and the plan is where that order lives.
//!
//! An adapter lends each stage the owners it reads. Stages whose work is
//! the window's own methods call them; stages over plain owners live in the
//! child modules, which never see the window.

use super::{AlertsPort, ChromePort, GatewayPort, LayersPort, LayoutPort, PaperPort};
use quantick_chart_interaction::frame_tail_plan::{FrameTailPlan, FrameTailStage};
use quantick_feed::stall::Stall;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::dock::DockTab;
use crate::pane;
use crate::statusbar;
use crate::tab::CanvasChrome;

use super::QuantickApp;

// The frame's stages, each harness stage compiled only where its hooks are:
// a default build's plan has none of them, not a no-op arm for each.
quantick_chart_interaction::frame_stages!(
    scenario: any(feature = "scenario-harness", test),
    control: any(feature = "control-harness", test),
    scripted: any(feature = "scenario-harness", feature = "drawing-harness", test),
);

mod canvas;
mod panels;
mod surfaces;

/// What one stage hands a later one within the same frame. Built on the
/// stack every frame; nothing in it outlives the frame or allocates.
#[derive(Default)]
struct FrameScratch {
    /// The chart background in force when the chrome opened.
    background: Option<egui::Color32>,
    /// This tab's judgement about its own feed, taken once for the frame:
    /// the status line reads it and the canvas corner reads it, and two
    /// readings a millisecond apart could disagree about whether a budget
    /// had run out.
    stall: Option<Stall>,
    canvas: canvas::CanvasAnswers,
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

    /// A frame whose own stages run in the given order, behind the same
    /// test-only door as the tail's.
    pub(super) fn draw_frame_stage_test_order(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        spawn: &mut crate::tab::LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameStage>,
    ) {
        self.draw_frame_with_stages(ctx, now, spawn, stages, FrameTailPlan::stages());
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
        tail: impl IntoIterator<Item = FrameTailStage>,
    ) {
        self.draw_frame_with_stages(ctx, now, spawn, FramePlan::stages(), tail);
    }

    /// Run `stages` in the order given. Production passes the validated
    /// plan; tests replay a swapped order through these same adapters, via
    /// the `#[cfg(test)]` door, to show the harm the declared dependencies
    /// prevent.
    fn draw_frame_with_stages<T: IntoIterator<Item = FrameTailStage>>(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        spawn: &mut crate::tab::LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameStage>,
        tail: T,
    ) {
        let mut scratch = FrameScratch::default();
        let mut tail = Some(tail);
        for stage in stages {
            self.run_frame_stage(stage, ctx, now, &mut scratch, spawn, &mut tail);
        }
    }

    fn run_frame_stage<T: IntoIterator<Item = FrameTailStage>>(
        &mut self,
        stage: FrameStage,
        ctx: &egui::Context,
        now: Instant,
        scratch: &mut FrameScratch,
        spawn: &mut crate::tab::LiveFeedSpawn<'_>,
        tail: &mut Option<T>,
    ) {
        match stage {
            FrameStage::RecordFrameTime => {
                if let Some(last) = self.health.last_frame {
                    self.health
                        .frames
                        .record((now - last).as_secs_f32() * 1000.0);
                }
                self.health.last_frame = Some(now);
            }
            FrameStage::DrainSources => self.drain_tabs(),
            // A "load older" outcome is a passing remark: it leaves after
            // `tab::HISTORY_NOTE_LINGER` whether or not anyone read it. Every
            // tab, not only the one on screen — a background tab keeps
            // draining, so it can settle a run while hidden, and bringing it
            // forward minutes later must not surface a sentence about a press
            // that is long over.
            FrameStage::ExpireHistoryNotes => {
                for tab in self.tabs.iter_mut() {
                    tab.expire_history_note(now);
                }
            }
            #[cfg(any(feature = "scenario-harness", test))]
            FrameStage::HistoryNoteHook => {
                self.chrome.harness.apply_history_note_hook(&mut self.tabs);
            }
            #[cfg(any(feature = "control-harness", test))]
            FrameStage::EnableControlAccess => self.enable_scenario_control(ctx),
            // Replay determinism: a session with a control trace beside it
            // re-injects its actions at their logical time, connected or not.
            FrameStage::ReplayTrace => {
                if let Some(mut access) = self.control.control_access.take() {
                    access.service_replay_trace(self);
                    self.control.control_access = Some(access);
                }
            }
            #[cfg(any(feature = "control-harness", test))]
            FrameStage::TakeMark => self.take_scenario_mark(),
            #[cfg(any(feature = "control-harness", test))]
            FrameStage::AnnotateHooks => self.apply_control_annotate_hooks(),
            #[cfg(any(feature = "control-harness", test))]
            FrameStage::EvidenceHook => super::demo_hooks::apply_control_evidence_hook(self, ctx),
            FrameStage::GatewayService => {
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
            }
            #[cfg(any(feature = "scenario-harness", feature = "drawing-harness", test))]
            FrameStage::ScenarioHooks => self.apply_scenario_hooks(),
            #[cfg(any(feature = "scenario-harness", test))]
            FrameStage::WindowStartupHook => self.chrome.window_startup.apply(ctx),
            FrameStage::WindowHousekeeping => {
                self.maybe_emit_summary(now, ctx);
                self.workspace_save_adapter().maintain_workspace(ctx);
            }
            FrameStage::TopChrome => self.draw_top_chrome(ctx, now, scratch),
            // Requests first, so a double click on a pane or a curve opens
            // the dialog on the same frame the gesture happened.
            FrameStage::IndicatorSurfaces => {
                self.service_indicator_requests();
                self.draw_indicator_surfaces(ctx);
            }
            FrameStage::StrategyPopupRequests => surfaces::open_requested_strategy_popups(
                &mut self.tabs,
                &mut self.surfaces.strategy_popup,
            ),
            FrameStage::Surfaces => self.draw_surface_stage(ctx, now),
            FrameStage::IndicatorMaintenance => {
                surfaces::reload_changed_scripts(&mut self.indicators, &mut self.tabs);
                self.layout_adapter().apply_pending_indicator_state();
                self.layer_wiring().maintain();
            }
            FrameStage::StatusLine => self.draw_status_line(ctx, scratch),
            FrameStage::LayoutDialogs => self.layout_adapter().draw_layout_delete_confirm(ctx),
            FrameStage::ReplayBrowser => self.draw_replay_browser_stage(ctx),
            FrameStage::DrawingRail => {
                panels::draw_drawing_rail(
                    ctx,
                    &mut self.toolrail,
                    &mut self.tabs,
                    &mut self.drawings,
                );
                // A star clicked this frame is on disk this frame: the pinned
                // rail is what the trader reaches for without looking, and
                // rebuilding it after a crash is not a thing anyone should
                // have to do twice.
                if self.toolrail.take_favorites_change() {
                    self.workspace_save_adapter().write_favorites();
                }
            }
            FrameStage::Dock => self.draw_dock_stage(ctx),
            // Chrome like the dock: declared before the central canvas so
            // the chart pays its width.
            FrameStage::PinnedInspector => {
                if let Some(ask) = self.drawings.draw_pinned_inspector(
                    ctx,
                    &super::drawing_controller::DrawingReadAccess::new(&self.tabs),
                    &self.toolrail,
                ) {
                    self.resolve_drawing_response(ask, now);
                }
            }
            FrameStage::FeedAndLayoutSwitches => self.apply_feed_and_layout_switches(),
            FrameStage::MirrorLoadingWaits => {
                panels::mirror_loading_waits(&self.replay_view, &mut self.tabs);
            }
            FrameStage::Canvas => self.draw_canvas_stage(ctx, scratch),
            FrameStage::DrawingChrome => self.draw_drawing_chrome_stage(ctx, now, scratch),
            // The menus above may have disarmed a bot over a resting retest
            // limit; its cancel goes to the simulator on this same frame, not
            // on the next print. Every tab, not just the active one: a menu
            // click and a tab switch can land on the same frame, and the old
            // tab's feed keeps running — its cancel must not sit stranded
            // until the tab is looked at again.
            FrameStage::StrategyCleanup => {
                for tab in self.tabs.iter_mut() {
                    tab.apply_strategy_cleanup();
                }
                if let Some(note) = self.audio.play_pending(&mut self.tabs) {
                    self.alerts().show_toast(note);
                }
            }
            FrameStage::Tail => {
                if let Some(tail) = tail.take() {
                    self.run_frame_tail(ctx, now, scratch, spawn, tail);
                }
            }
            // Live feed: keep polling the channel ~60×/s without busy-spinning.
            FrameStage::RequestRepaint => ctx.request_repaint_after(Duration::from_millis(16)),
        }
    }

    #[cfg(any(feature = "control-harness", test))]
    fn enable_scenario_control(&mut self, ctx: &egui::Context) {
        if self.control.scenarios.take_enable()
            && let Some(access) = self.control.control_access.as_mut()
        {
            access.enable(ctx);
        }
    }

    #[cfg(any(feature = "control-harness", test))]
    fn take_scenario_mark(&mut self) {
        if let Some(note) = self.control.scenarios.take_mark() {
            let note = (!note.is_empty()).then_some(note);
            self.take_mark(note);
        }
    }

    /// Scripted views, the drawing demos and pending history requests, in
    /// the order a launch composes them.
    #[cfg(any(feature = "scenario-harness", feature = "drawing-harness", test))]
    fn apply_scenario_hooks(&mut self) {
        #[cfg(any(feature = "scenario-harness", test))]
        self.chrome.harness.apply_scripted_view(&mut self.tabs);
        #[cfg(any(feature = "drawing-harness", test))]
        super::demo_hooks::apply_drawing_demo(self);
        #[cfg(any(feature = "scenario-harness", test))]
        self.chrome
            .harness
            .apply_load_older(&mut self.tabs, &self.config);
        #[cfg(any(feature = "scenario-harness", test))]
        self.chrome
            .harness
            .apply_load_older_candles(&mut self.tabs, &self.config);
        #[cfg(any(feature = "drawing-harness", test))]
        super::demo_hooks::apply_drawing_draft(self);
        #[cfg(any(feature = "scenario-harness", test))]
        super::demo_hooks::apply_venue_history_demo(self);
        #[cfg(any(feature = "drawing-harness", test))]
        super::demo_hooks::apply_frvp_demo(self);
        #[cfg(any(feature = "drawing-harness", test))]
        super::demo_hooks::apply_avwap_demo(self);
        #[cfg(any(feature = "scenario-harness", test))]
        super::demo_hooks::apply_strategy_demo(self);
        #[cfg(any(feature = "scenario-harness", test))]
        self.chrome
            .harness
            .apply_replay_restart(&mut self.tabs, &self.config);
    }

    /// Chrome panels claim their zones outside-in (§5): menu and toolbar on
    /// top here, the status line at the very bottom with the replay
    /// transport directly above it, then the edge-docked drawing rail and the
    /// right dock. The chart keeps whatever remains.
    fn draw_top_chrome(&mut self, ctx: &egui::Context, now: Instant, scratch: &mut FrameScratch) {
        scratch.background = Some(pane::background_color(&self.style));
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
        self.draw_menu_bar(ctx);
        if let Some(access) = self.control.control_access.as_mut() {
            access.draw_panel(ctx);
        }
        self.draw_toolbar(ctx);
    }

    /// The registered surfaces, then what they asked for. The environment is
    /// built after the indicator dialogs, and that placement is load-bearing:
    /// the preview watermark reads whether a settings dialog is previewing an
    /// unapplied draft, and drawing that dialog is what sets it — built
    /// before, the banner would reach the screen a frame after the legend
    /// chip that says the same thing. Two surfaces the trader reads as one is
    /// this repo's own bug class.
    fn draw_surface_stage(&mut self, ctx: &egui::Context, now: Instant) {
        let asks = surfaces::draw_surfaces(
            ctx,
            now,
            &mut self.surfaces,
            surfaces::SurfaceOwners {
                indicators: &self.indicators,
                alert_failure: self.audio.alert_failure.as_deref(),
                workspace: &self.workspace,
                style: &self.style,
                footprint_config: &self.footprint_config,
                tabs: &self.tabs,
                config: &self.config,
                added_symbols: &self.added_symbols,
            },
        );
        if let Some(name) = asks.save_workspace_as {
            self.workspace_save_adapter().save_named_workspace(&name);
        }
        if let Some(style) = asks.style {
            self.style = style;
            self.style_revision = self.style_revision.saturating_add(1);
        }
        // After the assignment, never before: the log line reports the
        // appearance that is now in force, and the revision it landed on.
        if let Some(request) = asks.log_style_change {
            super::health::emit_style_changed(
                &self.style,
                self.style_revision,
                request.applied_preset,
            );
        }
        // The audition goes through the one speaker every armed instance
        // shares, and reports a sound that could not be heard exactly as a
        // missed signal would.
        if let Some(cue) = asks.test_alert
            && let Some(note) = self.audio.play(&[cue])
        {
            self.alerts().show_toast(note);
        }
        if let Some(request) = asks.arm_strategy {
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
        if let Some(request) = asks.market {
            self.apply_market_request(request);
        }
        if let Some(change) = asks.footprint {
            self.layer_wiring().apply_footprint_change(change);
        }
        if asks.undo_drawing {
            let pane = self.drawing_pane_mut();
            pane.drawings.undo();
            // Same orphan risk as the keyboard undo: the drawing an armed
            // instance rides may just have been taken away.
            pane.strategies.sweep_orphans(&pane.drawings);
        }
    }

    fn draw_status_line(&mut self, ctx: &egui::Context, scratch: &mut FrameScratch) {
        scratch.stall = self
            .active_tab()
            .stall_at(&self.config, crate::metrics::wall_clock_ms());
        let offline_accent = self
            .chrome_reads()
            .feed_offline_accent(scratch.stall.as_ref());
        let status = self.status_model();
        let status_response = statusbar::draw(ctx, &status, &mut self.tz, offline_accent);
        if status_response.open_trading_tab {
            self.dock.open_tab(DockTab::Trading);
        }
    }

    fn draw_replay_browser_stage(&mut self, ctx: &egui::Context) {
        let action =
            panels::draw_replay_browser(ctx, &mut self.replay_view, &self.tabs, &self.config);
        if let Some(action) = action {
            let (tab, config) = self.active_with_config();
            super::replay_and_history::apply_replay_action(tab, config, action);
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
    }

    fn draw_dock_stage(&mut self, ctx: &egui::Context) {
        let response = panels::draw_dock(
            ctx,
            &mut self.dock,
            &mut self.tabs,
            &mut self.replay_view,
            self.tz,
        );
        // The strategy editor is a window of the active tab's ticket, drawn
        // whatever the dock is showing and whether it is showing at all: it
        // is opened from the Trading tab but it does not belong to it, and a
        // trader who opens it and then looks at the ledger has not asked for
        // it to close.
        if self.active_tab_mut().paper.draw_strategy_editor(ctx) {
            self.paper_settings()
                .persist(super::paper_wiring::PaperSettingsChange::OrderStrategies);
        }
        if response.restart_book_capture {
            self.active_tab_mut().restart_book_capture();
        }
        if let Some(action) = response.replay_action {
            // A click that lost its slot has the trader's next click behind
            // it; only the one-shot hook cares about the answer.
            let (tab, config) = self.active_with_config();
            let _ = super::replay_and_history::apply_replay_action(tab, config, action);
        }
        if let Some((opened, closed)) = response.navigate_to_trade {
            panels::center_flow_pane_on_trade(self.active_tab_mut(), opened, closed);
        }
        if response.pick_trades_dir {
            self.paper_settings().open_trades_dir_picker();
        }
        if response.order_strategies_changed {
            self.paper_settings()
                .persist(super::paper_wiring::PaperSettingsChange::OrderStrategies);
        }
        if response.cmd_trading_changed {
            self.paper_settings()
                .persist(super::paper_wiring::PaperSettingsChange::CmdTrading);
        }
        if response.risk_settings_changed {
            self.paper_settings()
                .persist(super::paper_wiring::PaperSettingsChange::RiskSettings);
        }
        self.paper_settings().poll_trades_dir_picker();
        self.workspace_bundle_adapter().poll_picker();
    }

    /// Respawn the feed if the feed/symbol selection changed (resets the
    /// chart), then settle the deferred layouts, a frame after the click
    /// that armed them, so the frame carrying the change paints its overlay
    /// first.
    fn apply_feed_and_layout_switches(&mut self) {
        let (tab, config) = self.active_with_config();
        tab.maybe_switch_feed(config);
        for (tab_id, tab) in self.tabs.iter_with_ids_mut() {
            tab.apply_pending_layout(tab_id, &self.config, &self.style, &mut self.pane_ids);
        }
        // Right after panes appear and markets switch, so a pane built this
        // frame is seeded this frame and a tab that changed symbol swaps its
        // drawings before anything paints them.
        self.layout_adapter().maintain_layouts();
        self.active_tab_mut().apply_spec_changes();
    }

    /// The chart takes whatever the panels left.
    fn draw_canvas_stage(&mut self, ctx: &egui::Context, scratch: &mut FrameScratch) {
        let background = scratch
            .background
            .unwrap_or_else(|| pane::background_color(&self.style));
        // Read before the canvas borrows the tabs, and answered after it
        // lets go.
        let popup_tab = self.tabs.active_id();
        let mut answers = canvas::CanvasAnswers {
            popup_tab,
            popup_open: self.chrome.feed_popup_tab == Some(popup_tab),
            ..canvas::CanvasAnswers::default()
        };
        // The layer menu offers what this source can produce, and the
        // footprint legend carries the side-honesty label: both resolved once
        // here rather than per pane, per entry, inside the canvas.
        let capabilities = self.active_tab().capabilities(&self.config);
        let side_inferred = self.active_tab().side_note(&self.config).is_some();
        // Told before the canvas paints, not after: the object holding the
        // words the editor is showing must stand down on the *same* frame,
        // or the note flashes its placeholder under the field for one.
        self.drawings.chrome.sync_content_editing(&mut self.tabs);
        let stall = scratch.stall.as_ref();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(background))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                let mut chrome = CanvasChrome {
                    toolrail: &mut self.toolrail,
                    presets: &self.drawings.presets,
                    drawing_chrome: &mut self.drawings.chrome,
                    begin_text_edit: &mut answers.begin_text_edit,
                    style: &self.style,
                    tz: self.tz,
                    capabilities,
                    side_inferred,
                    footprint: &mut self.footprint_config,
                    layers: &mut self.workspace.layers_mut().actions,
                };
                let tab_id = self.tabs.active_id();
                self.tabs.runtime_mut(self.tabs.active_index()).draw_canvas(
                    tab_id,
                    ui,
                    area,
                    &mut chrome,
                );
                // Each visible pane published its own reserved footer during
                // the canvas split. Draw the shared catalogue into every one
                // now, while their exact same-frame rectangles are available.
                self.layout_adapter().draw_layout_strips(ui);
                // The grid and the indicator state belong to the window, not
                // to the pane whose menu switched them.
                self.layer_wiring().apply_actions();
                canvas::draw_overlays(ui, area, self.active_tab(), stall, &mut answers);
            });
        scratch.canvas = answers;
    }

    /// Floating drawing controls must be registered after the opaque central
    /// canvas so they stay in front of the chart. That is why the drawing
    /// chrome is the one surface `Surfaces::draw_all` does not draw: it is
    /// anchored *to* the chart rather than floating over the window, so it is
    /// commanded by name from here instead.
    fn draw_drawing_chrome_stage(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        scratch: &FrameScratch,
    ) {
        if scratch.canvas.begin_text_edit {
            self.drawings.chrome.request_text_edit();
        }
        let ask = self.drawings.draw_drawing_chrome(
            ctx,
            &super::drawing_controller::DrawingReadAccess::new(&self.tabs),
            &self.toolrail,
        );
        self.resolve_drawing_response(ask, now);
    }

    fn run_frame_tail(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        scratch: &FrameScratch,
        spawn: &mut crate::tab::LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameTailStage>,
    ) {
        let answers = &scratch.canvas;
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
                notice_action: answers.notice_action,
                popup_tab: answers.popup_tab,
                popup_open: answers.popup_open,
                chip_clicked: answers.chip_clicked,
                dismissed: answers.dismissed,
                chip_rect: answers.chip_rect,
            },
            spawn,
            stages,
        );
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
