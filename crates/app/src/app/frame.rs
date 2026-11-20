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

use super::LayersPort;
use super::control_host::ControlWindow;
use super::drawing_controller::{DrawingAccess, DrawingReadAccess};
#[cfg(any(feature = "scenario-harness", feature = "drawing-harness", test))]
use super::owners_desk::OwnersDesk as _;
#[cfg(test)]
use super::paper_wiring::panel_effects::paper_panel_steps;
use super::paper_wiring::panel_effects::{begin_paper_panel_effects, finish_paper_panel_effects};
use super::{DrawingResponsePort as _, FrameCalls as _, GatewayPort as _};
use crate::strategy_desk;
use crate::surfaces::drawing_chrome::DrawingChromeExt as _;
use crate::surfaces::replay_browser::ReplayBrowserExt as _;
use crate::surfaces::{AccessPanelEnv, BorrowedSurfaceEnv, ViewContext};
use crate::toolrail::ToolRailExt as _;
use quantick_chart_interaction::frame_plan::StageScope;
use quantick_chart_interaction::frame_tail_plan::{FrameTailPlan, FrameTailStage};
use quantick_control_handlers::market_palette::MarketPaletteOwners;
use quantick_control_handlers::notify::AttentionHost as _;
use quantick_control_handlers::script_alarms::journal as journal_scripts;
use quantick_feed::stall::Stall;
use quantick_ui_model::market_palette::Intent as PaletteUiIntent;
use quantick_ui_model::strategy_popup::Intent::SettleArm;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::dock::DockTab;
use crate::pane;
use crate::statusbar;
use crate::tab::CanvasChrome;

use super::QuantickApp;
use super::menu_bar::MenuCommand;

// The frame's stages, each harness stage compiled only where its hooks are:
// a default build's plan has none of them, not a no-op arm for each.
quantick_chart_interaction::frame_stages!(
    scenario: any(feature = "scenario-harness", test),
    control: any(feature = "control-harness", test),
    scripted: any(feature = "scenario-harness", feature = "drawing-harness", test),
);

mod canvas;
#[cfg(test)]
pub(super) use panels::settle_rail_effects;
pub(super) mod panels;
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
    /// The one wall-clock reading the connection surfaces share this frame.
    now_ms: i64,
    status: statusbar::StatusResponse,
    canvas: canvas::CanvasAnswers,
}

/// One pass over `scope`: every stage the plan declares there, in its order,
/// through the one executor. A window's pass stops if that window closes
/// under it.
fn run_scope(app: &mut QuantickApp, ctx: &egui::Context, now: Instant, scope: StageScope) {
    let window = app.runtime.tabs.windows().presented();
    let mut scratch = FrameScratch::default();
    let mut tail = Some(FrameTailPlan::stages());
    for stage in FramePlan::stages_in(scope) {
        if scope.ends_with_its_window() && app.runtime.tabs.windows().window(window).is_none() {
            break;
        }
        let spawn = &mut quantick_feed::spawn_live;
        app.run_frame_stage(stage, ctx, now, &mut scratch, spawn, &mut tail);
    }
}

/// Sources, gateway and workers, once a tick, never once per market window:
/// scheduled by the root even when its native shell is hidden.
pub(super) fn process(app: &mut QuantickApp, ctx: &egui::Context, now: Instant) {
    // Every viewport shares this context; the appearance publishes once a frame.
    app.presentation.context = Some(ctx.clone());
    sync_appearance(Some(ctx), &mut app.runtime, &mut app.drawings.presets);
    run_scope(app, ctx, now, StageScope::Tick);
}

/// Only input, paint and the shown tab's consequences run for each viewport.
pub(super) fn paint(app: &mut QuantickApp, ctx: &egui::Context, now: Instant) {
    run_scope(app, ctx, now, StageScope::Window);
}

/// The tabs no window shows, once, after every window painted.
pub(super) fn settle(app: &mut QuantickApp, ctx: &egui::Context, now: Instant) {
    run_scope(app, ctx, now, StageScope::Settle);
}

/// Rail shortcuts (Esc, 1, 2) and the drawing keys. Chrome panels then
/// claim their zones outside-in (§5): menu and toolbar on top, the status
/// line at the very bottom with the replay transport directly above it,
/// then the edge-docked drawing rail and the right dock. The chart keeps
/// whatever remains.
fn handle_market_keys(app: &mut QuantickApp, ctx: &egui::Context, now: Instant) {
    // The close or switch prompt, painted later this frame, answers Esc and Enter alone.
    if app.chrome.window_menu.tab_workspace.asking() {
        return;
    }
    app.surfaces.toolrail.handle_keys(ctx);
    let alerts_open = app.surfaces.alerts.is_open();
    let effects = app
        .drawings
        .with(&mut app.surfaces.drawing_chrome)
        .handle_drawing_keys(
            &mut DrawingAccess::new(&mut app.runtime.tabs),
            &mut app.surfaces.toolrail,
            &mut *app.audio.alerts,
            ctx,
            now,
            crate::dock::holds_pointer(&app.surfaces.dock.model_state, ctx, alerts_open),
        );
    effects.apply(&mut app.surfaces.toast, &mut app.surfaces.toolrail);
}

/// Do what the palette asked. A refusal stays under its field.
/// Home's page where the chart would be, with the canvas's answers seeded
/// for the tail.
fn draw_home_canvas_stage(app: &mut QuantickApp, ctx: &egui::Context, scratch: &mut FrameScratch) {
    let background = scratch
        .background
        .unwrap_or_else(|| pane::background_color(&app.runtime.style));
    let popup_tab = app.runtime.tabs.active_id();
    scratch.canvas = canvas::CanvasAnswers::seeded(app, popup_tab, &scratch.status);
    app.surfaces
        .drawing_chrome
        .sync_content_editing(&mut app.runtime.tabs);
    if super::home_wiring::draw_stage(
        ctx,
        app.runtime
            .bound_mut(&app.presentation.binding)
            .active_tab_mut(),
        background,
        &mut app.surfaces,
    ) {
        app.surfaces
            .market_palette
            .update(PaletteUiIntent::open("", None));
    }
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
        super::native_windows::draw(self, ctx, now);
    }

    /// The health line, from the counters over the bound workspace.
    pub(super) fn maybe_emit_summary(&mut self, now: Instant, ctx: &egui::Context) {
        let (surface, view) = (self.chrome.surface.as_ref(), &self.presentation.binding);
        (self.health).maybe_emit_summary(now, ctx, self.runtime.bound_mut(view), surface);
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
        let presented = self.presentation.resolve(&self.runtime, ctx).is_ok();
        if (stage.needs_presentation() && !presented) || super::home_wiring::hides(self, stage) {
            return;
        }
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
                for tab in self.runtime.tabs.iter_mut() {
                    tab.expire_history_note(now);
                }
            }
            #[cfg(any(feature = "scenario-harness", test))]
            FrameStage::HistoryNoteHook => {
                self.chrome
                    .harness
                    .apply_history_note_hook(&mut self.runtime.tabs);
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
            FrameStage::AnnotateHooks => super::demo_hooks::apply_control_annotate_hooks(self),
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
            FrameStage::ScenarioHooks => super::demo_hooks::apply_scenario_hooks(self.owners()),
            #[cfg(any(feature = "scenario-harness", test))]
            FrameStage::WindowStartupHook => self.chrome.window_startup.apply(ctx),
            FrameStage::WindowHousekeeping => {
                self.maybe_emit_summary(now, ctx);
                self.workspace_save_adapter().maintain_workspace(ctx);
            }
            FrameStage::TabKeys => {
                scratch.background = Some(pane::background_color(&self.runtime.style));
                self.surfaces.capture_keyboard(ctx);
                let surface = &mut self.chrome.title_bar.surface;
                super::home_wiring::observe_market(&self.runtime.tabs, surface);
                self.handle_tab_keys(ctx);
            }
            FrameStage::MarketKeys => handle_market_keys(self, ctx, now),
            FrameStage::MenuBar => {
                self.draw_menu_bar(ctx);
                self.surfaces.draw_slot_unscoped_with_env(
                    quantick_ui_model::Slot::MenuDialogs,
                    &mut ViewContext { ctx, ui: None },
                    Some(&mut BorrowedSurfaceEnv::Access(AccessPanelEnv {
                        access: self.control.control_access.as_mut(),
                    })),
                );
            }
            FrameStage::HomeToolbar => super::home_wiring::stand_in_for_toolbar(self),
            FrameStage::Toolbar => super::chrome::toolbar::draw(self, ctx),
            FrameStage::IndicatorSurfaces => self.service_indicator_requests(),
            FrameStage::IndicatorDialogs => self.indicator_wiring().draw_indicator_surfaces(ctx),
            FrameStage::StrategyPopupRequests => surfaces::open_requested_strategy_popups(
                &mut self.runtime.tabs,
                &mut self.surfaces.strategy_popup,
            ),
            FrameStage::Surfaces => self.draw_surface_stage(ctx, now),
            FrameStage::IndicatorMaintenance => {
                surfaces::reload_changed_scripts(
                    &mut self.runtime.indicators,
                    &mut self.runtime.tabs,
                    self.runtime.workspace.layouts(),
                );
                self.layout_adapter().apply_pending_indicator_state();
                self.layer_wiring().maintain(ctx);
            }
            FrameStage::StatusLine => self.draw_status_line(ctx, scratch),
            FrameStage::LayoutDialogs => self.layout_adapter().draw_layout_delete_confirm(ctx),
            FrameStage::ReplayBrowser => self.draw_replay_browser_stage(ctx),
            FrameStage::DrawingRail => {
                panels::draw_drawing_rail(ctx, &mut self.surfaces, &mut self.runtime.tabs);
                // A star clicked this frame is on disk this frame: the pinned
                // rail is what the trader reaches for without looking, and
                // rebuilding it after a crash is not a thing anyone should
                // have to do twice.
                if self.surfaces.toolrail.take_favorites_change() {
                    self.workspace_save_adapter().write_favorites();
                }
                if let Some(on) = self.surfaces.toolrail.take_magnet_request() {
                    crate::control::flip_rail_magnet(self, on);
                }
            }
            FrameStage::Dock => {
                let _unsupported = self.draw_dock_stage(ctx);
            }
            // Chrome like the dock: declared before the central canvas so
            // the chart pays its width.
            FrameStage::PinnedInspector => {
                let ask = panels::draw_pinned_inspector(
                    ctx,
                    &self.drawings,
                    &mut self.surfaces,
                    &self.runtime.tabs,
                );
                self.resolve_drawing_response(ask, now);
            }
            FrameStage::LayoutSwitches => self.apply_layout_switches(),
            FrameStage::FeedSwitch => self.apply_feed_switch(),
            FrameStage::MirrorLoadingWaits => {
                panels::mirror_loading_waits(&self.surfaces.replay_browser, &mut self.runtime.tabs);
            }
            FrameStage::HomeCanvas => draw_home_canvas_stage(self, ctx, scratch),
            FrameStage::Canvas => self.draw_canvas_stage(ctx, scratch),
            FrameStage::DrawingChrome => self.draw_drawing_chrome_stage(ctx, now, scratch),
            // The menus above may have disarmed a bot over a resting retest
            // limit; its cancel goes to the simulator on this same frame, not
            // on the next print.
            FrameStage::StrategyCleanup => self.active_tab_mut().apply_strategy_cleanup(),
            FrameStage::Tail => {
                if let Some(tail) = tail.take() {
                    let _unsupported = self.run_frame_tail(ctx, now, &scratch.canvas, spawn, tail);
                }
            }
            // Live feed: keep polling the channel ~60×/s without busy-spinning.
            FrameStage::RequestRepaint => ctx.request_repaint_after(Duration::from_millis(16)),
            FrameStage::BackgroundStrategyCleanup => {
                super::frame_tail::clean_up_background_strategies(&mut self.runtime.tabs);
            }
            FrameStage::PlayAlerts => {
                let scripts = super::alerts_runtime::deliver(
                    &mut self.runtime,
                    &mut self.audio,
                    &mut self.surfaces.toast,
                    &mut self.surfaces.agent_popup,
                );
                if !scripts.is_empty() {
                    self.workspace_calls()
                        .as_hand(|_, to| journal_scripts(to, &scripts));
                }
            }
            FrameStage::BackgroundPaperPanels => super::frame_tail::settle_background_paper_panels(
                &mut self.runtime.tabs,
                &mut self.surfaces.toast,
                now,
            ),
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

    /// The registered surfaces, then what they asked for. The environment is
    /// built after the indicator dialogs, and that placement is load-bearing:
    /// the preview watermark reads whether a settings dialog is previewing an
    /// unapplied draft, and drawing that dialog is what sets it — built
    /// before, the banner would reach the screen a frame after the legend
    /// chip that says the same thing. Two surfaces the trader reads as one is
    /// this repo's own bug class.
    fn draw_surface_stage(&mut self, ctx: &egui::Context, now: Instant) {
        let palette = &self.surfaces.market_palette.model_state;
        let markets = (self.surfaces.hooks_pending() || palette.is_open())
            .then(|| std::sync::Arc::new(MarketPaletteOwners::listed_markets(self)));
        let late = surfaces::draw_surfaces(
            ctx,
            now,
            &mut self.surfaces,
            surfaces::SurfaceOwners {
                alerts: &mut self.runtime.alerts,
                display_offset_minutes: self.runtime.tz.minutes(),
                indicators: &mut self.runtime.indicators,
                store: self.runtime.workspace.layouts(),
                alert_failure: self.audio.alert_failure.as_deref(),
                style: &self.runtime.style,
                footprint_config: &self.runtime.footprint_config,
                tabs: &mut self.runtime.tabs,
                markets: markets.as_ref(),
            },
        );
        if let Some(request) = late.alerts {
            super::alerts_runtime::apply_alert_request(self.indicator_wiring(), *request);
        }
        for action in late.indicator_actions {
            self.apply_toolbar_action(action);
        }
        if let Some(style) = late.style {
            let _ = self.layer_wiring().apply_editor_style(style);
        }
        // After the assignment, never before: the log line reports the
        // appearance that is now in force, and the revision it landed on.
        if let Some(request) = late.log_style_change {
            self.layer_wiring().log_editor_style(request.applied_preset);
        }
        // The audition goes through the one speaker every armed instance
        // shares, and reports a sound that could not be heard exactly as a
        // missed signal would.
        if let Some(cue) = late.test_alert
            && let Some(note) = self.audio.play(&[cue])
        {
            self.attention().show_toast(note);
        }
        if let Some(request) = late.arm_strategy {
            let outcome = self
                .runtime
                .tabs
                .runtime_mut(self.runtime.tabs.active_index())
                .arm_strategy_instance(
                    &mut *self.audio.alerts,
                    request.side,
                    request.drawing,
                    &request.form,
                    request.label,
                );
            self.surfaces.strategy_popup.update(SettleArm(outcome));
        }
        if let Some(request) = late.market {
            self.apply_palette_intent(request);
        }
        if let Some(change) = late.footprint {
            self.layer_wiring().apply_footprint_change(change);
        }
        late.undo_drawing
            .then(|| self.active_tab_mut().undo_drawing());
    }

    fn draw_status_line(&mut self, ctx: &egui::Context, scratch: &mut FrameScratch) {
        let now_ms = crate::metrics::wall_clock_ms();
        scratch.now_ms = now_ms;
        scratch.stall = self.active_tab().stall_at(&self.config, now_ms);
        let stall = scratch.stall.as_ref();
        panels::observe_connections(ctx, &mut self.runtime.tabs, &self.config, now_ms, stall);
        let open =
            self.surfaces.feed_notice.model_state.open() == Some(self.runtime.tabs.active_id());
        let tab = &self.runtime.tabs[self.runtime.tabs.active_index()];
        let lead = statusbar::StatusLead::of(tab, stall, now_ms, open);
        scratch.status = statusbar::draw(ctx, &self.status_model(), self.runtime.tz, &lead);
        if scratch.status.open_display_settings {
            let display = quantick_control_schema::settings_sections::SettingsSection::Display;
            super::menu_bar::apply_menu_command_on(self, ctx, MenuCommand::OpenSettings(display));
            ctx.request_repaint();
        }
        if scratch.status.open_trading_tab {
            self.surfaces.dock.open_tab(DockTab::Trading);
        }
    }

    fn draw_replay_browser_stage(&mut self, ctx: &egui::Context) {
        let action =
            panels::draw_replay_browser(ctx, &mut self.surfaces, &self.runtime.tabs, &self.config);
        if let Some(action) = action {
            let (tab, config) = self.active_with_config();
            super::replay_and_history::apply_replay_action(tab, config, action);
        }
        // A folder the trader just pointed the browser at is written down on
        // the frame they pointed it, not at exit: "it forgot my folder again"
        // must not be one crash away.
        if let Some(pick) = self.surfaces.replay_browser.take_folder_change() {
            self.workspace_save_adapter()
                .write_replay_folder(pick.as_deref());
        }
        // The same, for the tick that decides whether yesterday is on the
        // chart. Either row can have been the one clicked; the browser owns
        // the setting, so there is one place to pick the change up.
        if let Some(enabled) = self.surfaces.replay_browser.take_day_before_change() {
            self.workspace_save_adapter()
                .write_replay_day_before(enabled);
        }
    }

    fn draw_dock_stage(&mut self, ctx: &egui::Context) -> Option<crate::surfaces::SurfaceResponse> {
        let open = self.surfaces.dock.tab() == Some(crate::dock::DockTab::Strategies);
        let mut rail = open.then(|| strategy_desk::Rail::read(self as &mut ControlWindow));
        let response = panels::draw_dock(ctx, &mut self.surfaces, &mut self.runtime, rail.as_mut());
        strategy_desk::settle(self as &mut ControlWindow, ctx, rail);
        let response = match begin_dock_effects(self, ctx, response) {
            Ok(response) => response,
            Err(response) => return Some(*response),
        };
        // The strategy editor is a window of the active tab's ticket, drawn
        // whatever the dock is showing and whether it is showing at all: it
        // is opened from the Trading tab but it does not belong to it, and a
        // trader who opens it and then looks at the ledger has not asked for
        // it to close.
        #[cfg(test)]
        paper_panel_steps::record("strategy-editor");
        if self.active_tab_mut().paper.draw_strategy_editor(ctx) {
            self.runtime
                .paper_settings()
                .persist(super::paper_wiring::PaperSettingsChange::OrderStrategies);
        }
        #[cfg(test)]
        paper_panel_steps::record("strategy-editor-settled");
        finish_paper_panel_effects(
            &mut self.runtime.tabs,
            &self.config,
            &mut self.runtime.workspace,
            response,
        );
        #[cfg(test)]
        paper_panel_steps::record("trades-picker-poll");
        self.runtime.paper_settings().poll_trades_dir_picker();
        #[cfg(test)]
        paper_panel_steps::record("bundle-picker-poll");
        self.workspace_bundle_adapter().poll_picker();
        None
    }

    /// The chart takes whatever the panels left.
    fn draw_canvas_stage(&mut self, ctx: &egui::Context, scratch: &mut FrameScratch) {
        // Read before the canvas borrows the tabs, and answered after it
        // lets go.
        let popup_tab = self.runtime.tabs.active_id();
        let mut answers = canvas::CanvasAnswers::seeded(self, popup_tab, &scratch.status);
        // The layer menu offers what this source can produce, and the
        // footprint legend carries the side-honesty label: both resolved once
        // here rather than per pane, per entry, inside the canvas.
        let bar_progress = self.runtime.layout_state().bar_progress_on(popup_tab);
        // Told before the canvas paints, not after: the object holding the
        // words the editor is showing must stand down on the *same* frame,
        // or the note flashes its placeholder under the field for one.
        let (background, capabilities, side_inferred) = canvas::prepare(
            &mut self.runtime.tabs,
            &mut self.surfaces,
            &self.config,
            &self.runtime.style,
            scratch.background,
        );
        let (stall, now_ms) = (scratch.stall.as_ref(), scratch.now_ms);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(background))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                let active = self.runtime.tabs.active_index();
                let (tab, panel_folds) =
                    self.runtime.tabs.with_panel_folds(active).expect("active");
                let mut chrome = CanvasChrome {
                    surfaces: &mut self.surfaces,
                    presets: &self.drawings.presets,
                    begin_text_edit: &mut answers.begin_text_edit,
                    style: &self.runtime.style,
                    tz: self.runtime.tz,
                    capabilities,
                    side_inferred,
                    bar_progress,
                    footprint: &mut self.runtime.footprint_config,
                    layers: &mut self.runtime.workspace.layers_mut().actions,
                    panel_folds,
                };
                tab.draw_canvas(popup_tab, ui, area, &mut chrome);
                super::canvas_wiring::settle_canvas(self, ui.ctx());
                // Each visible pane published its own reserved footer during
                // the canvas split. Draw the shared catalogue into every one
                // now, while their exact same-frame rectangles are available.
                self.layout_adapter().draw_layout_strips(ui);
                // The grid, the indicator state and the price tag's progress
                // belong to the window or the market, not to the pane whose
                // menu switched them.
                self.layer_wiring().apply_actions();
                let tab = &self.runtime.tabs[self.runtime.tabs.active_index()];
                canvas::overlays(
                    &mut self.surfaces.feed_notice,
                    ui,
                    area,
                    tab,
                    &self.config,
                    (stall, now_ms),
                    &mut answers,
                );
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
        let open = self.surfaces.alerts.is_editing();
        self.surfaces
            .drawing_chrome
            .frame_facts(scratch.canvas.begin_text_edit, open);
        let mut drawings = self.drawings.with(&mut self.surfaces.drawing_chrome);
        let ask = drawings.draw_drawing_chrome(
            ctx,
            &DrawingReadAccess::new(&self.runtime.tabs),
            &self.surfaces.toolrail,
            &self.runtime.alerts.core.book,
        );
        self.resolve_drawing_response(ask, now);
    }

    fn run_frame_tail(
        &mut self,
        ctx: &egui::Context,
        now: Instant,
        answers: &canvas::CanvasAnswers,
        spawn: &mut crate::tab::LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameTailStage>,
    ) -> Vec<crate::surfaces::SurfaceResponse> {
        let presented = self.presentation.resolve(&self.runtime, ctx).is_ok();
        let input = super::frame_tail::FrameTailInput {
            presentation: presented.then_some(ctx),
            now,
            tz: self.runtime.tz,
            notice_action: answers.notice_action.clone(),
            popup_tab: answers.popup_tab,
            chip_clicked: answers.chip_clicked,
            dismissed: answers.dismissed,
            drawn: answers.drawn,
        };
        super::frame_tail::run(
            super::frame_tail::FrameTailOwners {
                tabs: &mut self.runtime.tabs,
                config: &self.config,
                surfaces: &mut self.surfaces,
                drawn: &mut self.chrome.feed_drawn,
            },
            &input,
            &mut self.runtime.workspace,
            spawn,
            stages,
        )
    }
}

/// Begin the dock's effects, then draw the alert manager panel: it draws every
/// frame it is open, whichever way the dock pass ended.
fn begin_dock_effects(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    response: crate::surfaces::SurfaceResponse,
) -> Result<crate::surfaces::PaperPanelEffects, Box<crate::surfaces::SurfaceResponse>> {
    let mut begun = begin_paper_panel_effects(
        &mut app.runtime.tabs,
        &mut app.surfaces.replay_browser,
        response,
    );
    if let Ok(effects) = begun.as_mut()
        && let Some(press) = effects.route.take()
    {
        app.runtime.route(&app.config).press(press);
    }
    let bell = begun.as_ref().is_ok_and(|effects| effects.open_alerts);
    if bell {
        _ = quantick_control_handlers::alerts::AlertDefinitionsPort::alert_open(app);
    }
    super::alerts_runtime::after_dock(&mut app.runtime, &mut app.surfaces, ctx);
    begun
}

#[cfg(test)]
impl QuantickApp {
    /// Exercise the real tail interpreter and recipient without unrelated frame IO.
    pub(super) fn run_report_tail_test(
        &mut self,
        ctx: &egui::Context,
        stages: impl IntoIterator<Item = FrameTailStage>,
    ) -> Vec<crate::surfaces::SurfaceResponse> {
        self.run_frame_tail(
            ctx,
            Instant::now(),
            &canvas::CanvasAnswers::default(),
            &mut |_, _, _, _| panic!("paper tail must not spawn a feed"),
            stages,
        )
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
        self.presentation.context = Some(ctx.clone());
        sync_appearance(Some(ctx), &mut self.runtime, &mut self.drawings.presets);
        let mut scratch = FrameScratch::default();
        let mut tail = Some(tail);
        for stage in stages {
            self.run_frame_stage(stage, ctx, now, &mut scratch, spawn, &mut tail);
        }
    }
}

/// Publish already-validated per-owner defaults before any frame operation creates or paints objects.
pub(super) fn sync_appearance(
    ctx: Option<&egui::Context>,
    runtime: &mut super::workspace_runtime::WorkspaceRuntime,
    presets: &mut quantick_drawings::presets::PresetStore,
) {
    if let Some(ctx) = ctx {
        crate::theme::sync(ctx, &runtime.appearance);
    }
    presets.sync_theme(&runtime.appearance);
    for pane in runtime.tabs.iter_mut().flat_map(|tab| tab.panes_mut()) {
        let surface = pane.surface();
        pane.presentation
            .candles
            .follow_theme(runtime.appearance.resolved(), surface);
    }
    if runtime
        .chart_theme
        .sync(&mut runtime.style, &runtime.appearance)
    {
        runtime.style_revision = runtime.style_revision.saturating_add(1);
    }
}
