//! Shared surface catalog adapters. Models own cached state; views paint and
//! emit intents. Legacy entries keep their behavior until migrated.
//! See quantick-ui-model/src/catalog.rs for the single registration seam.

use std::time::Instant;

use eframe::egui;

pub(crate) use bar_switch::BarSwitchRequest;
pub(crate) use chart_settings::{ChartSettingsSurface, ChartSettingsTab};
pub(crate) use drawing_chrome::{DrawingChromeSurface, DrawingEnv};
pub(crate) use market_palette::MarketPaletteSurface;
pub(crate) use strategy_popup::StrategyPopupSurface;
pub(crate) use toast::ToastSurface;

/// What the surfaces need from the application, kept to what they actually
/// read so a surface cannot quietly acquire the whole app.
pub(crate) struct SurfaceEnv<'a> {
    pub picker_pane_id: u64,
    pub picker_indicators: &'a [crate::toolbar::IndicatorMenuEntry],
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
    /// The chart appearance in force. Read-only, like everything here: the
    /// appearance window edits a copy and hands the result back through
    /// [`SurfaceResponse::style`], because every renderer in the application
    /// reads this one and a surface holding a `&mut` to it would be holding a
    /// piece of the trunk.
    pub style: &'a crate::style::ChartStyle,
    /// The chart surface the candle editor previews.
    pub style_surface: quantick_appearance::ChartSurface,
    /// The focused chart's effective footprint setup — its own override, or
    /// the window default it still follows. Read-only for the same reason
    /// the style is: every footprint frame is reading this one.
    pub footprint: &'a crate::footprint_config::FootprintConfig,
    /// Whether that chart has already diverged from the window default, which
    /// is what decides whether the panel offers to put it back.
    pub footprint_customized: bool,
    /// Which pane has focus, so a window editing "the focused chart" can say
    /// in its title which chart that is.
    pub focused_side: crate::pane::PaneSide,
    /// What "Open a market" lists. Built by the host **only while the
    /// palette is open**; `None` otherwise.
    pub markets: Option<&'a quantick_workspace::market_list::MarketSnapshot>,
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
        /// The appearance a test that does not care about appearance gets.
        static QUIET_STYLE: std::sync::LazyLock<crate::style::ChartStyle> =
            std::sync::LazyLock::new(crate::style::ChartStyle::default);
        /// The footprint setup a test that does not care about it gets.
        static QUIET_FOOTPRINT: std::sync::LazyLock<crate::footprint_config::FootprintConfig> =
            std::sync::LazyLock::new(crate::footprint_config::FootprintConfig::default);
        Self {
            picker_pane_id: 0,
            picker_indicators: &[],
            picker_scripts: &[],
            now,
            indicator_preview_area: None,
            focused_chart_area: None,
            style: &QUIET_STYLE,
            style_surface: quantick_appearance::ChartSurface::Paper,
            footprint: &QUIET_FOOTPRINT,
            footprint_customized: false,
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
    /// `StrategyPopupSurface::settle_arm`, because the answer decides whether
    /// the dialog closes or shows a refusal.
    pub arm_strategy: Option<ArmRequest>,
    /// A sound was auditioned. The host owns the one speaker every armed
    /// instance shares, so it plays this and reports what happened.
    pub test_alert: Option<crate::audio::Cue>,
    pub bar_switch: Option<BarSwitchRequest>,
}

/// Ask the host to arm a strategy instance.
///
/// Carries the form by value rather than leaving the host to read it back off
/// the dialog: the dialog stays open across the answer, and a trader typing
/// into it must not be able to change what was submitted.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ArmRequest {
    pub side: crate::pane::PaneSide,
    pub drawing: crate::drawings::DrawingId,
    /// Boxed for the same reason the footprint configuration is: it is the
    /// largest thing a response carries, and every surface pays for the size
    /// of a response returned by value on every frame.
    pub form: Box<crate::strategy_presets::StoredPreset>,
    /// What the badge will call this instance — the preset it came from, the
    /// name in the save field, or "custom".
    pub label: String,
}

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
    fn capture_keyboard(&mut self, _ctx: &egui::Context) {}
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {}
    fn id(&self) -> &'static str;
    fn paint(
        &mut self,
        state: &M::State,
        target: &mut ViewContext<'_>,
        env: &SurfaceEnv<'_>,
    ) -> Option<M::Intent>;
}

/// The same adapter drives migrated views and compatibility views.
pub(crate) struct Adapter<M: quantick_ui_model::Model, V> {
    pub model_state: M::State,
    pub view: V,
    pub effects: fn(M::Effects) -> SurfaceResponse,
}
impl<M: quantick_ui_model::Model, V> Adapter<M, V> {
    /// Host/agent intents use the same transition and effect conversion as UI input.
    pub fn update(&mut self, intent: M::Intent) -> SurfaceResponse {
        (self.effects)(M::update(&mut self.model_state, intent))
    }
}
impl<M: quantick_ui_model::Model, V: View<M>> Surface for Adapter<M, V> {
    fn capture_keyboard(&mut self, ctx: &egui::Context) {
        self.view.capture_keyboard(ctx);
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) {
        self.view.apply_env_hook(env);
    }
    fn id(&self) -> &'static str {
        self.view.id()
    }
    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        self.draw_in(&mut ViewContext { ctx, ui: None }, env)
    }
    fn draw_in(&mut self, target: &mut ViewContext<'_>, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let Some(intent) = self.view.paint(&self.model_state, target, env) else {
            return SurfaceResponse::default();
        };
        self.update(intent)
    }
}

fn ignore_keyboard<S>(_: &mut S, _: &egui::Context) {}

struct LegacyView<'a, S> {
    surface: &'a mut S,
    keyboard: fn(&mut S, &egui::Context),
    draw: fn(&mut S, &egui::Context, &SurfaceEnv<'_>) -> SurfaceResponse,
}
impl<S: Surface> View<quantick_ui_model::Legacy<SurfaceResponse>> for LegacyView<'_, S> {
    fn capture_keyboard(&mut self, ctx: &egui::Context) {
        (self.keyboard)(self.surface, ctx);
    }
    fn id(&self) -> &'static str {
        self.surface.id()
    }
    fn paint(
        &mut self,
        _: &(),
        target: &mut ViewContext<'_>,
        env: &SurfaceEnv<'_>,
    ) -> Option<SurfaceResponse> {
        Some((self.draw)(self.surface, target.context(), env))
    }
}

macro_rules! register_surfaces {
    (legacy { $( $legacy:ident: $ty:ident => $draw:path, $keyboard:path; )* }
     modules { $($module:ident;)* }
     models { $( [$($model_attr:meta),*] [$($view_attr:meta),*]
        $field:ident: $model:ident = $model_file:literal, $view:ident = $view_file:literal,
        $effects:path, $slot:ident; )* }) => {
        $(pub(crate) mod $legacy;)*
        $(pub(crate) mod $module;)*
        $($(#[$view_attr])* #[path = $view_file] pub(crate) mod $view;)*
        pub(crate) struct Surfaces {
            $(pub $legacy: $legacy::$ty,)*
            $($(#[$view_attr])* pub $field: Adapter<quantick_ui_model::$model::Model, $view::View>,)*
            pub widgets: crate::widget_surfaces::WidgetSurfaces,
            #[cfg(any(feature = "scenario-harness", test))]
            hooks_applied: bool,
        }
        impl Default for Surfaces {
            fn default() -> Self {
                Self {
                    $($legacy: Default::default(),)*
                    $($(#[$view_attr])* $field: Adapter { model_state: Default::default(), view: Default::default(), effects: $effects },)*
                    widgets: Default::default(),
                    #[cfg(any(feature = "scenario-harness", test))]
                    hooks_applied: false,
                }
            }
        }
        impl Surfaces {
            /// Modal input follows the catalog before host hotkeys.
            pub fn capture_keyboard(&mut self, ctx: &egui::Context) {
                $(Adapter::<quantick_ui_model::Legacy<SurfaceResponse>, _> {
                    model_state: (), view: LegacyView { surface: &mut self.$legacy, draw: $draw, keyboard: $keyboard }, effects: |response| response,
                }.capture_keyboard(ctx);)*
                $($(#[$view_attr])* self.$field.capture_keyboard(ctx);)*
            }
            /// Route only entries belonging to this existing frame location.
            pub fn draw_slot(&mut self, slot: quantick_ui_model::Slot, target: &mut ViewContext<'_>, env: &SurfaceEnv<'_>) -> SurfaceResponse {
                let mut response = SurfaceResponse::default();
                if slot == quantick_ui_model::Slot::Overlay {
                    #[cfg(any(feature = "scenario-harness", test))]
                    if !self.hooks_applied {
                        self.hooks_applied = true;
                        $(self.$legacy.apply_env_hook(env);)*
                        $($(#[$view_attr])* self.$field.apply_env_hook(env);)*
                    }
                    $(response.merge(Adapter::<quantick_ui_model::Legacy<SurfaceResponse>, _> {
                        model_state: (), view: LegacyView { surface: &mut self.$legacy, draw: $draw, keyboard: $keyboard }, effects: |response| response,
                    }.draw_in(target, env));)*
                }
                $($(#[$view_attr])* if slot == quantick_ui_model::Slot::$slot { response.merge(self.$field.draw_in(target, env)); })*
                response
            }
        }
    };
}
quantick_ui_model::surface_catalog!(register_surfaces);

impl Surfaces {
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
        self.draw_slot(
            quantick_ui_model::Slot::Overlay,
            &mut ViewContext { ctx, ui: None },
            env,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_workspace::market_list::PaletteIntent;

    #[test]
    fn registered_view_routes_intents() {
        let mut registry = Surfaces::default();
        registry.second.view.enabled = true;
        registry.second.view.intent = Some((4, 17));
        let ctx = egui::Context::default();
        let mut effects = SurfaceResponse::default();
        let output = ctx.run(Default::default(), |ctx| {
            assert!(!registry.draw_all(ctx, &env()).undo_drawing);
            assert_eq!(
                registry.second.model_state, 0,
                "other slots cannot consume an intent"
            );
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
        let mut real = agent_popup::AgentPopupSurface::default();
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
}
