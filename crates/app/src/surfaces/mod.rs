//! Shared surface catalog adapters. Models own cached state; views paint and
//! emit intents. Legacy entries keep their behavior until migrated.
//! See quantick-ui-model/src/catalog.rs for the single registration seam.

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
/// A struct of defaults, like [`crate::dock::DockResponse`]: every field means
/// "this was not asked for" until a surface sets it, so adding a request never
/// touches an existing surface.
///
/// Not `Eq`: the appearance a surface hands back carries opacities and
/// widths, and a float has no total equality to derive. `PartialEq` is what
/// the tests compare with, and it is the honest one.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct SurfaceResponse {
    /// Typed effects belonging to the host that drew a scoped catalog entry.
    pub model_effects: Vec<HostEffect>,
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
        self.model_effects.extend(other.model_effects);
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

/// One floating surface: it owns its state, draws itself, and reports what it
/// needs the host to do.
pub(crate) trait Surface {
    /// Stable identifier. Each surface paints under `egui::Id::new(self.id())`
    /// rather than a literal of its own, so the name a screenshot, a hook and a
    /// log line use cannot drift from the layer the pixels are actually on.
    fn id(&self) -> &'static str;

    /// Draw this frame. A surface with nothing on screen returns early and
    /// costs a branch, which is what keeps [`Surfaces::draw_all`] cheap
    /// enough to run unconditionally per frame.
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse;

    /// Modal input runs before chrome hotkeys.
    fn capture_keyboard(&mut self, _ctx: &egui::Context) {}

    /// Shared entry for a floating view or a view inside existing chrome.
    fn draw_in(&mut self, target: &mut ViewContext<'_>, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        self.draw(target.context(), env)
    }

    /// Open whatever this surface's `QUANTICK_*` capture hook asks for, once,
    /// before the first frame is drawn. Takes the same [`SurfaceEnv`] the
    /// draw does, so a hook that needs to know something about the
    /// application — where the focused chart is, which markets are open —
    /// reads it from the one place a surface is allowed to read.
    ///
    /// On the trait rather than in a list held beside it, so a surface that
    /// forgets its hook shows up as an empty override instead of compiling
    /// silently: `ui-harness` requires one per surface, and a hook nobody
    /// wrote is invisible to every test and to the size guard alike. The
    /// default is no hook, which is the honest answer for a surface that
    /// needs none.
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {}
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
    fn id(&self) -> &'static str {
        self.view.id()
    }
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        self.draw_in(&mut ViewContext { ctx, ui: None }, env)
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

fn ignore_keyboard<S>(_: &mut S, _: &egui::Context) {}

struct LegacyView<'a, S> {
    surface: &'a mut S,
    keyboard: fn(&mut S, &egui::Context),
    draw: fn(&mut S, &egui::Context, &SurfaceEnv<'_>) -> SurfaceResponse,
}
impl<S: Surface> View<quantick_ui_model::Legacy<SurfaceResponse>> for LegacyView<'_, S> {
    fn capture_keyboard(&mut self, _: &(), ctx: &egui::Context) {
        (self.keyboard)(self.surface, ctx);
    }
    fn id(&self) -> &'static str {
        self.surface.id()
    }
    fn paint(
        &mut self,
        _: &(),
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) -> Option<SurfaceResponse> {
        env.map(|env| (self.draw)(self.surface, target.context(), env))
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
    (legacy { $( $legacy:ident: $ty:ident => $draw:path, $keyboard:path; )* }
     modules { $($module:ident;)* }
     models { $( [$($model_attr:meta),*] [$($view_attr:meta),*]
        $field:ident: $tag:ident => $model:ident = $model_file:literal, $view:ident = $view_file:literal,
        $effects:path, $slot:ident $(| $additional_slot:ident)*, $dispatch:ident; )* }) => {
        register_host_routes! { [] ; $(
            $dispatch [$($view_attr),*] $tag $model $field;
        )* }
        $(pub(crate) mod $legacy;)*
        $(pub(crate) mod $module;)*
        $($(#[$view_attr])* #[path = $view_file] pub(crate) mod $view;)*
        pub(crate) struct Surfaces {
            $(pub $legacy: $legacy::$ty,)*
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
                    $($legacy: Default::default(),)*
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

            /// Modal input follows the catalog before host hotkeys.
            pub fn capture_keyboard(&mut self, ctx: &egui::Context) {
                $($(#[$view_attr])* if self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay) { #[cfg(test)] self.trace_phase("capture", stringify!($field)); self.$field.capture_keyboard(ctx); })*
                $(#[cfg(test)] self.trace_phase("capture", stringify!($legacy)); Adapter::<quantick_ui_model::Legacy<SurfaceResponse>, _>::new(LegacyView { surface: &mut self.$legacy, draw: $draw, keyboard: $keyboard }, |response| response).capture_keyboard(ctx);)*
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
                        $(#[cfg(test)] self.trace_phase("hook", stringify!($legacy)); self.$legacy.apply_env_hook(env);)*
                        $($(#[$view_attr])* if !self.$field.supports_pass(quantick_ui_model::Slot::LeadingOverlay) {
                            #[cfg(test)] self.trace_phase("hook", stringify!($field));
                            if let Some(intent) = self.$field.view.apply_env_hook(env) { response.merge(self.$field.update(intent)); }
                        })*
                    }
                }
                if slot == quantick_ui_model::Slot::Overlay {
                    $(#[cfg(test)] self.trace_phase("paint", stringify!($legacy)); response.merge(Adapter::<quantick_ui_model::Legacy<SurfaceResponse>, _>::new(LegacyView { surface: &mut self.$legacy, draw: $draw, keyboard: $keyboard }, |response| response).draw_in(target, env));)*
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
mod tests {
    use super::*;
    use quantick_workspace::market_list::PaletteIntent;

    #[test]
    fn catalog_passes_share_state_and_fold_each_effect_once() {
        use quantick_ui_model::Slot;
        let mut registry = Surfaces::default();
        let mut effects = SurfaceResponse::default();
        let mut calls = 0;
        let mut canvas_selection = 3;
        let before = registry.first.with_pass(
            Slot::PinnedInspector,
            |effect| effects.merge(effect),
            |view, session| {
                calls += 1;
                view.enabled = true;
                session.update((canvas_selection, 17));
                *session.state()
            },
        );
        assert_eq!(before, Some(3));
        let wrong = registry.first.with_pass(
            Slot::Toolbar,
            |effect| effects.merge(effect),
            |_, session| {
                calls += 1;
                session.update((99, 17));
            },
        );
        assert_eq!(wrong, None);
        // The canvas changes the host's borrowed input between the two passes.
        canvas_selection = 7;
        let after = registry.first.with_pass(
            Slot::DrawingChrome,
            |effect| effects.merge(effect),
            |view, session| {
                calls += 1;
                assert!(view.enabled);
                assert_eq!(*session.state(), 3);
                session.update((canvas_selection, 18));
                *session.state()
            },
        );
        assert_eq!(after, Some(10));
        assert_eq!(calls, 2);
        assert_eq!(registry.first.model_state, 10);
        assert_eq!(
            effects.model_effects,
            vec![HostEffect::First((3, 17)), HostEffect::First((10, 18))]
        );
    }

    #[test]
    fn scoped_catalog_keeps_panes_independent_and_reclaims_removed_owners() {
        let mut registry = Surfaces::default();
        registry.update_scoped(10, HostIntent::First(Box::new((3, 17))));
        registry.second.update_scoped(10, (0, 17));
        registry.update_scoped(20, HostIntent::First(Box::new((8, 17))));
        assert_eq!(*registry.first.scoped(10).0, 3);
        assert_eq!(*registry.first.scoped(20).0, 8);
        let ctx = egui::Context::default();
        registry.first.scoped(10).1.enabled = true;
        registry.first.scoped(10).1.intent = Some((5, 17));
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let target = &mut ViewContext { ctx, ui: Some(ui) };
                assert!(
                    !registry
                        .draw_slot_scoped(10, quantick_ui_model::Slot::Toolbar, target)
                        .undo_drawing
                );
                assert!(
                    registry
                        .draw_slot_scoped(10, quantick_ui_model::Slot::Overlay, target)
                        .undo_drawing
                );
            });
        });
        assert_eq!(*registry.first.scoped(10).0, 8);
        assert_eq!(registry.first.model_state, 0);
        registry.retain_slot_scopes(quantick_ui_model::Slot::Overlay, |scope| scope == 20);
        assert_eq!(*registry.first.scoped(10).0, 0);
        assert_eq!(*registry.first.scoped(20).0, 8);
    }

    #[test]
    fn slot_cleanup_keeps_other_owners_and_removes_closed_windows() {
        use quantick_ui_model::Slot;
        let mut registry = Surfaces::default();
        registry.update_scoped(10, HostIntent::First(Box::new((3, 17))));
        registry.second.update_scoped(10, (7, 17));
        registry.second.update_scoped(20, (9, 17));
        registry.retain_slot_scopes(Slot::PaneHeader, |_| false);
        assert_eq!(*registry.first.scoped(10).0, 3);
        assert_eq!(*registry.second.scoped(10).0, 14);
        registry.retain_slot_scopes(Slot::Toolbar, |scope| scope == 20);
        assert_eq!(*registry.first.scoped(10).0, 3);
        assert_eq!(*registry.second.scoped(10).0, 0);
        assert_eq!(*registry.second.scoped(20).0, 18);
        registry.retain_slot_scopes(Slot::Overlay, |_| false);
        assert_eq!(*registry.first.scoped(10).0, 0);
        assert_eq!(*registry.second.scoped(20).0, 18);
    }

    #[test]
    fn registered_view_routes_intents() {
        let mut registry = Surfaces::default();
        registry.second.view.enabled = true;
        registry.second.view.intent = Some((4, 17));
        let ctx = egui::Context::default();
        let mut effects = SurfaceResponse::default();
        let output = ctx.run(Default::default(), |ctx| {
            assert!(!registry.draw_all(ctx, &env()).undo_drawing);
            for slot in [
                quantick_ui_model::Slot::PaneHeader,
                quantick_ui_model::Slot::TitleBar,
                quantick_ui_model::Slot::MenuBar,
                quantick_ui_model::Slot::TabStrip,
                quantick_ui_model::Slot::StatusLine,
            ] {
                assert!(
                    !registry
                        .draw_slot(slot, &mut ViewContext { ctx, ui: None }, &env())
                        .undo_drawing
                );
                assert_eq!(
                    registry.second.model_state, 0,
                    "other slots cannot consume an intent"
                );
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                effects = registry.draw_slot(
                    quantick_ui_model::Slot::Toolbar,
                    &mut ViewContext { ctx, ui: Some(ui) },
                    &env(),
                );
            });
        });
        assert!(!output.shapes.is_empty());
        assert_eq!(registry.second.view.painted, Some(0));
        assert_eq!(registry.second.model_state, 8);
        assert_eq!(registry.first.model_state, 0);
        assert!(effects.undo_drawing);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(
                    !registry
                        .draw_slot(
                            quantick_ui_model::Slot::Toolbar,
                            &mut ViewContext { ctx, ui: Some(ui) },
                            &env()
                        )
                        .undo_drawing
                );
            });
        });
        assert_eq!(registry.second.view.painted, Some(8));
        assert_eq!(
            registry.second.model_state, 8,
            "painting does not run update"
        );
    }

    /// The second implementation of the port, which is what proves it is a
    /// port at all: a surface written entirely outside this module, wired in
    /// by nothing but the trait. `new-extension` asks for exactly this before
    /// an abstraction is allowed to call itself one.
    #[derive(Default)]
    struct FakeSurface {
        drawn: usize,
        asks_undo: bool,
        /// A request that carries a **value**, not a flag. Added when the
        /// second batch of surfaces did: a fake that only ever raises a
        /// boolean cannot show that the fold keeps a payload, which is the
        /// half of [`SurfaceResponse::merge`] a new field is most likely to
        /// get wrong.
        asks_style: Option<crate::style::ChartStyle>,
    }

    impl Surface for FakeSurface {
        fn id(&self) -> &'static str {
            "fake"
        }

        fn draw(&mut self, _ctx: &egui::Context, _env: &SurfaceEnv<'_>) -> SurfaceResponse {
            self.drawn += 1;
            SurfaceResponse {
                undo_drawing: self.asks_undo,
                style: self.asks_style,
                ..SurfaceResponse::default()
            }
        }
    }

    fn env() -> SurfaceEnv<'static> {
        SurfaceEnv::quiet(Instant::now())
    }

    /// Fold a surface's response exactly as [`Surfaces::draw_all`] does.
    ///
    /// The registry names its members as fields, so a fake cannot be inserted
    /// into it. What can be shared is the thing that actually defines the
    /// port: the call through `&mut dyn Surface` and the fold of what comes
    /// back. Driving an outside type and a shipped one through the *same*
    /// function is what makes this a port test rather than a cast.
    fn drive(
        surface: &mut dyn Surface,
        ctx: &egui::Context,
        env: &SurfaceEnv<'_>,
    ) -> SurfaceResponse {
        let mut response = SurfaceResponse::default();
        response.merge(surface.draw(ctx, env));
        response
    }

    /// A type declared outside this module satisfies [`Surface`] and its
    /// request survives the fold — no field on the registry, no branch in the
    /// host, nothing but the trait. `new-extension` asks for exactly this
    /// before an abstraction is allowed to call itself a port.
    #[test]
    fn a_second_implementation_needs_only_the_trait() {
        let ctx = egui::Context::default();
        let mut fake = FakeSurface {
            asks_undo: true,
            ..FakeSurface::default()
        };
        assert_eq!(Surface::id(&fake), "fake");
        let response = drive(&mut fake, &ctx, &env());
        assert!(
            response.undo_drawing,
            "an outside surface's ask reaches the host through the fold"
        );
        assert_eq!(fake.drawn, 1);
    }

    /// The port grew six fields when the second batch of surfaces moved onto
    /// it, and the point of them being *fields* is that an outside type can
    /// raise one without this module knowing it exists. A fake asking for a
    /// chart appearance — a request carrying a whole value, not a flag —
    /// reaches the host through the same fold as everything else.
    #[test]
    fn an_outside_surface_can_raise_a_request_that_carries_a_value() {
        let ctx = egui::Context::default();
        let mut appearance = crate::style::ChartStyle::default();
        appearance.canvas.grid_enabled = !appearance.canvas.grid_enabled;
        let mut fake = FakeSurface {
            asks_style: Some(appearance),
            ..FakeSurface::default()
        };
        let response = drive(&mut fake, &ctx, &env());
        assert_eq!(
            response.style.map(|style| style.canvas.grid_enabled),
            Some(appearance.canvas.grid_enabled),
            "the value arrives as it was asked for, not as a default"
        );
        assert!(
            !response.undo_drawing,
            "and asking for one thing does not ask for another"
        );
    }

    /// Every value-carrying request folds first-wins, not just the workspace
    /// name that was the only one when the rule was written. Checked over the
    /// fields together, because the failure this guards against is a new
    /// field being folded with assignment — which silently lets whichever
    /// surface is drawn last overwrite an earlier ask.
    #[test]
    fn every_valued_request_keeps_the_first_asker() {
        let mut first = SurfaceResponse {
            style: Some(crate::style::ChartStyle::default()),
            log_style_change: Some(StyleLogRequest {
                applied_preset: None,
            }),
            footprint: Some(FootprintChange::ResetToDefault),
            market: Some(PaletteIntent::Remove {
                feed_id: "first".to_owned(),
                symbol: "FIRST".to_owned(),
            }),
            test_alert: Some(crate::audio::Cue::default()),
            ..SurfaceResponse::default()
        };
        let mut contested = crate::style::ChartStyle::default();
        contested.canvas.grid_enabled = !contested.canvas.grid_enabled;
        first.merge(SurfaceResponse {
            model_effects: Vec::new(),
            indicator_actions: Vec::new(),
            script_editor_actions: Vec::new(),
            style: Some(contested),
            log_style_change: Some(StyleLogRequest {
                applied_preset: Some(crate::style::CandlePreset::Classic),
            }),
            footprint: Some(FootprintChange::Applied(Box::default())),
            market: Some(PaletteIntent::Remove {
                feed_id: "second".to_owned(),
                symbol: "SECOND".to_owned(),
            }),
            arm_strategy: None,
            bar_switch: None,
            test_alert: Some(crate::audio::Cue::new(
                crate::audio::AlertSound::Critical,
                None,
            )),
            undo_drawing: false,
        });
        assert_eq!(first.footprint, Some(FootprintChange::ResetToDefault));
        assert_eq!(
            first.market,
            Some(PaletteIntent::Remove {
                feed_id: "first".to_owned(),
                symbol: "FIRST".to_owned(),
            })
        );
        // The three the first version of this test set but never contested,
        // which is how a field folded with assignment instead of `or` would
        // have shipped: an appearance edit silently clobbered by whatever
        // surface happened to draw after the style panel.
        assert_eq!(
            first.style.map(|style| style.canvas.grid_enabled),
            Some(crate::style::ChartStyle::default().canvas.grid_enabled),
            "the first surface's appearance survives a later one's"
        );
        assert_eq!(
            first.log_style_change,
            Some(StyleLogRequest {
                applied_preset: None
            })
        );
        assert_eq!(first.test_alert, Some(crate::audio::Cue::default()));
    }

    /// The same function drives a surface that really ships, so the fake is
    /// held to the contract the product is held to rather than a private one.
    #[test]
    fn a_shipped_surface_and_a_fake_take_the_same_path() {
        let ctx = egui::Context::default();
        let mut real = Adapter::<quantick_ui_model::agent_popup::Model, _>::new(
            agent_popup::View,
            agent_popup::effects,
        );
        let mut fake = FakeSurface::default();
        let quiet_real = ctx.run(Default::default(), |ctx| {
            let _ = drive(&mut real, ctx, &env());
        });
        let _ = quiet_real;
        assert_eq!(
            drive(&mut fake, &ctx, &env()),
            SurfaceResponse::default(),
            "a surface with nothing to show asks for nothing, whoever wrote it"
        );
    }

    /// A surface that declares no capture hook gets the trait's default and
    /// does nothing, rather than failing to compile — which is what lets the
    /// trait carry the requirement without forcing every surface to restate
    /// it.
    #[test]
    fn a_surface_without_a_hook_uses_the_default() {
        let mut fake = FakeSurface::default();
        fake.apply_env_hook(&env());
        assert_eq!(fake.drawn, 0);
        assert!(!fake.asks_undo);
    }

    /// A response is built once per registered surface and once for the
    /// fold, **every frame**, and returned by value each time. So its size is
    /// a per-frame cost, and the way it grows is by someone adding a field
    /// that carries a whole configuration by value instead of boxing it —
    /// which is exactly what [`FootprintChange::Applied`] and
    /// [`ArmRequest::form`] box for.
    ///
    /// The ceiling is generous rather than exact: struct layout is the
    /// compiler's business and pinning today's number would fail on a
    /// harmless field reorder. It is set where an unboxed configuration
    /// would break it and ordinary growth would not — 264 bytes today, and
    /// one unboxed `StoredPreset` is over 300 on its own.
    const RESPONSE_SIZE_CEILING: usize = 512;

    /// See [`RESPONSE_SIZE_CEILING`].
    #[test]
    fn a_response_stays_small_enough_to_return_every_frame() {
        let size = std::mem::size_of::<SurfaceResponse>();
        assert!(
            size <= RESPONSE_SIZE_CEILING,
            "SurfaceResponse is {size} bytes, over the {RESPONSE_SIZE_CEILING} ceiling — a new \
             request is carrying a configuration by value where it should box it"
        );
    }

    /// Order must not decide the outcome: a later surface with nothing to ask
    /// cannot clear an earlier surface's request. Folding with `|=` rather
    /// than assignment is what buys that, and this pins it.
    #[test]
    fn a_quiet_surface_cannot_cancel_an_earlier_request() {
        let mut merged = SurfaceResponse {
            undo_drawing: true,
            ..SurfaceResponse::default()
        };
        merged.merge(SurfaceResponse::default());
        assert!(merged.undo_drawing);
    }

    #[test]
    fn registered_view_local_receipts_reach_the_same_converter_before_later_controls() {
        let mut registry = Surfaces::default();
        registry.first.view.staged = Some([(3, 10), (2, 20)]);
        let ctx = egui::Context::default();
        let mut response = SurfaceResponse::default();
        let mut calls = 0;
        let _ = ctx.run(Default::default(), |ctx| {
            registry.first.with_updates(
                |effect| response.merge(effect),
                |view, session| {
                    session.with_local_effects(
                        &mut |effect| {
                            calls += 1;
                            (effect, Some((1, 99)))
                        },
                        |session| {
                            view.paint_updated(session, &mut ViewContext { ctx, ui: None }, None)
                        },
                    );
                },
            );
        });
        assert_eq!(
            calls, 2,
            "terminal receipts do not re-enter local execution"
        );
        assert_eq!(registry.first.view.painted, Some(4));
        assert_eq!(registry.first.model_state, 7);
        assert_eq!(
            response.model_effects,
            vec![
                HostEffect::First((3, 10)),
                HostEffect::First((4, 99)),
                HostEffect::First((6, 20)),
                HostEffect::First((7, 99)),
            ]
        );
    }
}

#[cfg(test)]
#[path = "tests/overlay_order.rs"]
mod overlay_order_tests;
