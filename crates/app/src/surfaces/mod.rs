//! Shared surface catalog adapters. Models own cached state; views paint and
//! emit intents through the typed adapter.
//! See quantick-ui-model/src/catalog.rs for the single registration seam.

#[cfg(test)]
#[macro_use]
#[path = "chrome/tests/probe_view.rs"]
mod chrome_probe_view;

/// The actual paper owner and frame facts needed by a paper panel.
pub(crate) struct PaperPanelEnv<'a> {
    /// Whether the alert manager is up, so the rail's bell reads as pressed.
    pub alerts_open: bool,
    pub paper: &'a mut crate::paper_trading::PaperTrading,
    pub replay: Option<&'a quantick_feed::ReplayLink>,
    pub timezone: crate::timezone::TzOffset,
}
/// Validated indicator address and authoritative descriptor/preset services.
pub(crate) struct IndicatorPanelEnv<'a> {
    pub target: Option<quantick_chart::indicator_registry::TabSlot>,
    pub view: Option<&'a mut crate::indicators::IndicatorView>,
    pub kind: Option<&'a quantick_workspace::indicator_document::SavedKind>,
    pub presets: &'a mut crate::indicators::preset_file::PresetStore,
    pub presets_path: &'a std::path::Path,
}
/// The access host is optional during initialization and teardown.
pub(crate) struct AccessPanelEnv<'a> {
    pub access: Option<&'a mut crate::control::ControlAccess>,
}
/// A pass lends one domain facet; models and responses never retain it.
pub(crate) enum BorrowedSurfaceEnv<'a> {
    Access(AccessPanelEnv<'a>),
    Indicators(IndicatorPanelEnv<'a>),
    Paper(PaperPanelEnv<'a>),
    Drawings(&'a DrawingEnv<'a>),
}

use std::time::Instant;

use eframe::egui;

pub(crate) use bar_switch::BarSwitchRequest;
pub(crate) use drawing_chrome::{DrawingChromeSurface, DrawingEnv};
pub(crate) use market_palette::MarketPaletteSurface;
pub(crate) use quantick_ui_model::HostEffect;
pub(crate) use quantick_ui_model::chart_settings::{
    ChartSettingsTab, State as ChartSettingsSurface,
};
pub(crate) use strategy_popup::StrategyPopupSurface;
pub(crate) use toast::ToastSurface;

/// What the surfaces need from the application, kept to what they actually
/// read so a surface cannot quietly acquire the whole app.
pub(crate) struct SurfaceEnv<'a> {
    pub alerts: Option<&'a quantick_alerts::AlertBook>,
    pub alert_error: Option<&'a quantick_alerts::AlertError>,
    pub alert_context: Option<&'a alerts::Context>,
    #[cfg(any(feature = "scenario-harness", test))]
    pub picker_pane_id: u64,
    pub picker_scripts: &'a [quantick_stores::script_library::ScriptEntry],
    /// The frame's clock reading, passed in rather than sampled, so a surface
    /// that expires on a deadline stays as testable as the engine is.
    pub now: Instant,
    /// The chart rectangle a settings dialog is currently previewing a draft
    /// on, when one is. `None` — the ordinary case — means every pane is
    /// showing the values on disk.
    pub indicator_preview_area: Option<egui::Rect>,
    /// The focused pane's chart rectangle, or `None` before it has been laid
    /// out once. Read by the surfaces whose capture hook has to paint
    /// somewhere plausible without a real target to aim at.
    pub focused_chart_area: Option<egui::Rect>,
    /// The chart surface the candle editor previews.
    pub style_surface: quantick_appearance::ChartSurface,
    /// Which pane has focus, so a window editing "the focused chart" can say
    /// in its title which chart that is.
    pub focused_side: crate::pane::PaneSide,
    /// What "Open a market" lists. Built by the host **only while the
    /// palette is open**; `None` otherwise.
    pub markets: Option<&'a std::sync::Arc<quantick_workspace::market_list::MarketSnapshot>>,
    /// The **stable id** of the tab on screen. The arming dialog speaks for
    /// one drawing on one tab, and drawing ids are per-pane counters — the
    /// same id over there names an unrelated object — so it closes when this
    /// changes.
    ///
    /// The id and not the index: `close_tab` clamps the active index rather
    /// than shifting it, so an index can keep comparing equal while pointing
    /// at a different market.
    pub active_tab: u64,
    /// The sides whose bar rule closes on a **count**, so a fraction of the
    /// bar is a thing that exists.
    ///
    /// An adaptive rule closes on a condition, and there is no share of it to
    /// wait for; the alarm can only speak at the close. Built by the host
    /// only while the dialog that reads it is open, like the open markets.
    pub counted_bar_sides: &'a [crate::pane::PaneSide],
    /// Why the last sound could not be played, if it could not. Shown under
    /// the alarm controls, so a trader auditioning a clip learns immediately
    /// that the signal would have been silent too.
    pub alert_failure: Option<&'a str>,
}

#[cfg(test)]
impl SurfaceEnv<'static> {
    /// An environment where nothing is happening, for the surface tests.
    ///
    /// Every test that needs one field set writes
    /// `SurfaceEnv { now, ..SurfaceEnv::quiet(now) }`, so a
    /// field added here costs one line in this file rather than an edit in
    /// every surface's test module — which is the difference between an env
    /// that can grow and one nobody wants to touch.
    pub fn quiet(now: Instant) -> Self {
        Self {
            alerts: None,
            alert_error: None,
            alert_context: None,
            picker_pane_id: 0,
            picker_scripts: &[],
            now,
            indicator_preview_area: None,
            focused_chart_area: None,
            style_surface: quantick_appearance::ChartSurface::Paper,
            focused_side: crate::pane::PaneSide::Flow,
            markets: None,
            active_tab: 0,
            counted_bar_sides: &[],
            alert_failure: None,
        }
    }
}

/// What drawing the surfaces asked the application to do.
///
/// A struct of defaults: every field means
/// "this was not asked for" until a surface sets it, so adding a request never
/// touches an existing surface.
///
/// Not `Eq`: the appearance a surface hands back carries opacities and
/// widths, and a float has no total equality to derive. `PartialEq` is what
/// the tests compare with, and it is the honest one.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct SurfaceResponse {
    pub paper_panel: Option<Box<PaperPanelEffects>>,
    /// Typed effects belonging to the host that drew a scoped catalog entry.
    pub model_effects: Vec<HostEffect>,
    pub alerts: Option<Box<alerts::Request>>,
    pub indicator_actions: Vec<crate::toolbar::ToolbarAction>,
    pub script_editor_actions: Vec<quantick_stores::script_library::ScriptEditorAction>,
    /// The toast's Undo button was pressed — the host owns the drawing stack,
    /// and the sweep of any strategy left orphaned by the undo.
    pub undo_drawing: bool,
    /// The appearance window edited the chart style. The host owns the style
    /// every renderer reads, and the revision counter they watch, so it takes
    /// the edited copy rather than lending the original out.
    pub style: Option<crate::style::ChartStyle>,
    /// An appearance change settled and is owed a log line.
    pub log_style_change: Option<StyleLogRequest>,
    /// The footprint window settled on a setup, or asked for the chart's
    /// override to be dropped.
    pub footprint: Option<FootprintChange>,
    /// The market palette asked for something only the application can do.
    pub market: Option<quantick_workspace::market_list::PaletteIntent>,
    /// The arming dialog's **Arm** was pressed. The host answers through
    /// `quantick_ui_model::strategy_popup::Intent::SettleArm`, because the answer decides whether
    /// the dialog closes or shows a refusal.
    pub arm_strategy: Option<ArmRequest>,
    /// A sound was auditioned. The host owns the one speaker every armed
    /// instance shares, so it plays this and reports what happened.
    pub test_alert: Option<crate::audio::Cue>,
    pub bar_switch: Option<BarSwitchRequest>,
}

/// Host outputs of the complete borrowed paper-panel pass.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct PaperPanelEffects {
    /// The rail's bell: open the alert manager.
    pub open_alerts: bool,
    pub close_replay: bool,
    pub open_replay_browser: bool,
    pub navigate_to_trade: Option<(i64, i64)>,
    pub journal_edit: Option<quantick_paper::report::session_edit::JournalEdit>,
    pub pick_trades_dir: bool,
    pub cmd_trading_changed: bool,
    pub order_strategies_changed: bool,
    pub risk_settings_changed: bool,
}
impl PaperPanelEffects {
    fn merge(&mut self, other: Self) {
        self.open_alerts |= other.open_alerts;
        self.close_replay |= other.close_replay;
        self.open_replay_browser |= other.open_replay_browser;
        self.navigate_to_trade = self.navigate_to_trade.or(other.navigate_to_trade);
        self.journal_edit = self.journal_edit.take().or(other.journal_edit);
        self.pick_trades_dir |= other.pick_trades_dir;
        self.cmd_trading_changed |= other.cmd_trading_changed;
        self.order_strategies_changed |= other.order_strategies_changed;
        self.risk_settings_changed |= other.risk_settings_changed;
    }
}

pub(crate) use quantick_ui_model::strategy_popup::ArmRequest;

/// What the footprint window asked the host to do with the focused chart's
/// setup.
///
/// An enum rather than two fields because the two are exclusive answers to
/// one question — a frame cannot both set an override and drop it — and this
/// enum is closed by that fact, not by a hope that nobody adds a variant.
/// [`SurfaceResponse`] stays a struct; only the *value* one field carries has
/// two shapes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FootprintChange {
    /// A knob moved: this is the focused chart's setup now, and the window
    /// default with it.
    ///
    /// Boxed because the configuration is by far the largest thing a response
    /// carries, and every other surface would pay for its size in a response
    /// that is returned by value on every frame.
    Applied(Box<crate::footprint_config::FootprintConfig>),
    /// Put the focused chart back on the window default.
    ResetToDefault,
}

/// Ask the host to record an appearance change.
///
/// Carries only what the surface knows that the host does not: whether the
/// trader reached the new appearance by clicking a named preset, which the
/// log prefers over guessing the name back from the values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StyleLogRequest {
    /// The preset the trader clicked this frame, if they clicked one.
    pub applied_preset: Option<crate::style::CandlePreset>,
}

impl SurfaceResponse {
    /// Validate the complete residual before a paper host performs any effect.
    /// Direct model/domain changes made during painting are not rolled back.
    pub(crate) fn into_paper_panel(mut self) -> Result<PaperPanelEffects, Box<Self>> {
        let paper = self.paper_panel.take();
        if self != Self::default() {
            self.paper_panel = paper;
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "PAPER_PANEL_UNSUPPORTED_RESPONSE", response = ?self,
                "paper panel response retained without external host execution");
            return Err(Box::new(self));
        }
        Ok(paper.map(|paper| *paper).unwrap_or_default())
    }

    /// Fold one surface's response into the frame's. Requests are
    /// independent, so a set flag stays set: no surface can cancel another's
    /// ask by being drawn after it.
    ///
    /// Where a request carries a value rather than a flag, the **first**
    /// surface to ask wins and a later one in the same frame is dropped. Two
    /// surfaces cannot both be right about one workspace name, and dropping
    /// the later ask deterministically beats writing the file twice or letting
    /// draw order decide in silence. Only one surface produces it today; the
    /// rule is pinned by a test so a second producer meets a documented answer
    /// rather than a surprise.
    fn merge(&mut self, other: Self) {
        if let Some(paper) = other.paper_panel {
            self.paper_panel
                .get_or_insert_with(Default::default)
                .merge(*paper);
        }
        self.model_effects.extend(other.model_effects);
        self.alerts = self.alerts.take().or(other.alerts);
        self.indicator_actions.extend(other.indicator_actions);
        self.script_editor_actions
            .extend(other.script_editor_actions);
        self.undo_drawing |= other.undo_drawing;
        self.style = self.style.take().or(other.style);
        self.log_style_change = self.log_style_change.take().or(other.log_style_change);
        self.footprint = self.footprint.take().or(other.footprint);
        self.market = self.market.take().or(other.market);
        self.arm_strategy = self.arm_strategy.take().or(other.arm_strategy);
        self.test_alert = self.test_alert.take().or(other.test_alert);
        self.bar_switch = self.bar_switch.take().or(other.bar_switch);
    }
}

/// App host operations for the shared typed Model/View adapter.
pub(crate) trait Surface {
    #[cfg(test)]
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        self.draw_in(&mut ViewContext { ctx, ui: None }, env)
    }
    fn capture_keyboard(&mut self, ctx: &egui::Context);
    fn draw_in(&mut self, target: &mut ViewContext<'_>, env: &SurfaceEnv<'_>) -> SurfaceResponse;
}

/// The existing egui host supplies either an overlay context or its chrome Ui.
/// Chrome views use `ui` directly; overlay views use `context()`.
pub(crate) struct ViewContext<'a> {
    pub ctx: &'a egui::Context,
    pub ui: Option<&'a mut egui::Ui>,
}
impl ViewContext<'_> {
    pub fn context(&self) -> &egui::Context {
        self.ui.as_ref().map_or(self.ctx, |ui| ui.ctx())
    }
}

/// Paint cached model state and return an intent only when input changes it.
/// Time-dependent views include the supplied time in their intent.
pub(crate) trait View<M: quantick_ui_model::Model> {
    fn capture_keyboard(&mut self, _state: &M::State, _ctx: &egui::Context) {}
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) -> Option<M::Intent> {
        None
    }
    fn id(&self) -> &'static str;
    /// Default single-stage views keep their existing input path. Interactive
    /// editors may emit bounded updates before painting dependent widgets.
    fn paint_updated(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<'_, M>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        if let Some(intent) = self.paint(session.state(), target, env) {
            session.update(intent);
        }
    }
    /// Borrow domain owners only for this row's paint transaction.
    fn paint_borrowed(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<'_, M>,
        target: &mut ViewContext<'_>,
        _slot: quantick_ui_model::Slot,
        _env: Option<&mut BorrowedSurfaceEnv<'_>>,
    ) -> SurfaceResponse {
        self.paint_updated(session, target, None);
        SurfaceResponse::default()
    }
    fn paint(
        &mut self,
        state: &M::State,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) -> Option<M::Intent>;
}

/// Host conversion around the headless scoped adapter; storage is UI-independent.
pub(crate) type Adapter<M, V> = quantick_ui_model::Adapter<M, V, SurfaceResponse>;
impl<M: quantick_ui_model::Model, V: View<M>> Surface for Adapter<M, V> {
    fn capture_keyboard(&mut self, ctx: &egui::Context) {
        self.view.capture_keyboard(&self.model_state, ctx);
    }
    fn draw_in(&mut self, target: &mut ViewContext<'_>, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let mut response = SurfaceResponse::default();
        self.with_updates(
            |effect| response.merge(effect),
            |view, session| {
                view.paint_updated(session, target, Some(env));
            },
        );
        response
    }
}

// Filter explicit host opt-ins before expanding enum variants and match arms.
// Local views and hooks still use the same typed adapter and model session.
macro_rules! register_host_routes {
    ([$($rows:tt)*] ; Local [$($attr:meta),*] $tag:ident $model:ident $field:ident; $($rest:tt)*) => {
        register_host_routes! { [$($rows)*] ; $($rest)* }
    };
    ([$($rows:tt)*] ; Host [$($attr:meta),*] $tag:ident $model:ident $field:ident; $($rest:tt)*) => {
        register_host_routes! { [$($rows)* ([$($attr),*] $tag $model $field)] ; $($rest)* }
    };
    ([$(([$($attr:meta),*] $tag:ident $model:ident $field:ident))*] ;) => {
        /// Only catalog entries with an external host route have a variant.
        pub(crate) enum HostIntent {
            $($(#[$attr])* $tag(Box<<quantick_ui_model::$model::Model as quantick_ui_model::Model>::Intent>),)*
        }
        impl Surfaces {
            pub fn update(&mut self, intent: HostIntent) -> SurfaceResponse {
                match intent { $($(#[$attr])* HostIntent::$tag(intent) => self.$field.update(*intent),)* }
            }
            pub fn update_scoped(&mut self, scope: u64, intent: HostIntent) -> SurfaceResponse {
                match intent { $($(#[$attr])* HostIntent::$tag(intent) => self.$field.update_scoped(scope, *intent),)* }
            }
        }
    };
}

macro_rules! register_surfaces {
    (modules { $($module:ident;)* }
     models { $( [$($model_attr:meta),*] [$($view_attr:meta),*]
        $field:ident: $tag:ident => $model:ident = $model_file:literal, $view:ident = $view_file:literal,
        $effects:path, $slot:ident $(| $additional_slot:ident)*, $dispatch:ident; )* }) => {
        register_host_routes! { [] ; $(
            $dispatch [$($view_attr),*] $tag $model $field;
        )* }
        $(pub(crate) mod $module;)*
        $($(#[$view_attr])* #[path = $view_file] pub(crate) mod $view;)*
        pub(crate) struct Surfaces {
            $($(#[$view_attr])* pub $field: Adapter<quantick_ui_model::$model::Model, $view::View>,)*
            pub widgets: crate::widget_surfaces::WidgetSurfaces,
            #[cfg(any(feature = "scenario-harness", test))]
            hooks_applied: bool,
            #[cfg(test)]
            phase_trace: Option<Vec<(&'static str, &'static str)>>,
        }
        impl Default for Surfaces {
            fn default() -> Self {
                Self {
                    $($(#[$view_attr])* $field: Adapter::new(Default::default(), $effects).in_slots(&[quantick_ui_model::Slot::$slot $(, quantick_ui_model::Slot::$additional_slot)*]),)*
                    widgets: Default::default(),
                    #[cfg(any(feature = "scenario-harness", test))]
                    hooks_applied: false,
                    #[cfg(test)]
                    phase_trace: None,
                }
            }
        }
        impl Surfaces {
            pub fn retain_slot_scopes(&mut self, slot: quantick_ui_model::Slot, keep: impl Fn(u64) -> bool) {
                $($(#[$view_attr])* if self.$field.supports_pass(slot) { self.$field.retain_scopes(&keep); })*
            }

            pub fn draw_slot_scoped(&mut self, scope: u64, slot: quantick_ui_model::Slot, target: &mut ViewContext<'_>) -> SurfaceResponse {
                let mut response = SurfaceResponse::default();
                $($(#[$view_attr])* if self.$field.supports_pass(slot) && (slot != quantick_ui_model::Slot::Overlay || !self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay)) {
                    self.$field.with_scoped_updates(scope, |effect| response.merge(effect), |view, session| {
                        view.paint_updated(session, target, None);
                    }); })*
                response
            }

            /// Preserve ordinary callers through the same borrowed-domain traversal.
            pub fn draw_slot_unscoped(&mut self, slot: quantick_ui_model::Slot, target: &mut ViewContext<'_>) -> SurfaceResponse {
                self.draw_slot_unscoped_with_env(slot, target, None)
            }

            /// Reborrow domain authority for each catalog row, never retain it.
            pub fn draw_slot_unscoped_with_env(&mut self, slot: quantick_ui_model::Slot, target: &mut ViewContext<'_>, mut env: Option<&mut BorrowedSurfaceEnv<'_>>) -> SurfaceResponse {
                let mut response = SurfaceResponse::default();
                $($(#[$view_attr])* if self.$field.supports_pass(slot) && (slot != quantick_ui_model::Slot::Overlay || !self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay)) {
                    let returned = self.$field.with_updates(|effect| response.merge(effect), |view, session| {
                        view.paint_borrowed(session, target, slot, env.as_deref_mut())
                    });
                    response.merge(returned); })*
                response
            }

            /// Modal input follows the catalog before host hotkeys.
            pub fn capture_keyboard(&mut self, ctx: &egui::Context) {
                $($(#[$view_attr])* if self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay) { #[cfg(test)] self.trace_phase("capture", stringify!($field)); self.$field.capture_keyboard(ctx); })*
                $($(#[$view_attr])* if !self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay) { #[cfg(test)] self.trace_phase("capture", stringify!($field)); self.$field.capture_keyboard(ctx); })*
            }
            /// Route only entries belonging to this existing frame location.
            pub fn draw_slot(&mut self, slot: quantick_ui_model::Slot, target: &mut ViewContext<'_>, env: &SurfaceEnv<'_>) -> SurfaceResponse {
                let mut response = SurfaceResponse::default();
                if matches!(slot, quantick_ui_model::Slot::LeadingOverlay | quantick_ui_model::Slot::Overlay) {
                    #[cfg(any(feature = "scenario-harness", test))]
                    if !self.hooks_applied {
                        self.hooks_applied = true;
                        $($(#[$view_attr])* if self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay) {
                            #[cfg(test)] self.trace_phase("hook", stringify!($field));
                            if let Some(intent) = self.$field.view.apply_env_hook(env) { response.merge(self.$field.update(intent)); }
                        })*
                        $($(#[$view_attr])* if !self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay) {
                            #[cfg(test)] self.trace_phase("hook", stringify!($field));
                            if let Some(intent) = self.$field.view.apply_env_hook(env) { response.merge(self.$field.update(intent)); }
                        })*
                    }
                }
                $($(#[$view_attr])* if self.$field.supports_pass(slot) && (slot != quantick_ui_model::Slot::Overlay || !self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay)) { #[cfg(test)] self.trace_phase("paint", stringify!($field)); response.merge(self.$field.draw_in(target, env)); })*
                response
            }
        }
    };
}
quantick_ui_model::surface_catalog!(register_surfaces);

impl Surfaces {
    #[cfg(test)]
    fn trace_phase(&mut self, phase: &'static str, entry: &'static str) {
        if let Some(trace) = self.phase_trace.as_mut() {
            trace.push((phase, entry));
        }
    }
    /// Whether the capture hooks have still to run.
    ///
    /// The host reads this when it decides whether to build the environment
    /// slices that only an open surface needs. A hook opens its surface
    /// *inside* [`Self::draw_all`], which is after those slices were
    /// gathered, so on that one frame "is it open" is the wrong question —
    /// the right one is "is it about to be". Without it a hook-opened market
    /// palette has no snapshot on its first frame and draws nothing, so a
    /// capture of that frame shows a window the palette was meant to cover:
    /// a state the application would never reach on its own.
    pub fn hooks_pending(&self) -> bool {
        #[cfg(any(feature = "scenario-harness", test))]
        {
            !self.hooks_applied
        }
        #[cfg(not(any(feature = "scenario-harness", test)))]
        {
            false
        }
    }

    /// Draw overlay entries, preserving their catalog order and effect fold.
    pub fn draw_all(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let mut target = ViewContext { ctx, ui: None };
        let mut response =
            self.draw_slot(quantick_ui_model::Slot::LeadingOverlay, &mut target, env);
        response.merge(self.draw_slot(quantick_ui_model::Slot::Overlay, &mut target, env));
        response
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tests/overlay_order.rs"]
mod overlay_order_tests;
