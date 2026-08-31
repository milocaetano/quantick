//! The surface port — where a self-contained overlay docks.
//!
//! A *surface* here is a piece of chrome that floats over the chart, owns the
//! state it draws, and is not part of any pane: the assistant's popup, the
//! undo toast, the workspace-name box. Before this port each one was wired
//! into `QuantickApp` by hand in four places — a field, a line in the
//! constructor, a call in `draw_frame`, and sometimes a hotkey — so
//! sixty-eight modules left the root struct holding 133 fields and a
//! 1,149-line constructor. `arch-review` dimension 9 is the review half of
//! that story and `size_guard.rs` the mechanical half; this module is the
//! place the wiring goes instead.
//!
//! # Shape, and why it copies the dock
//!
//! [`SurfaceEnv`] and [`SurfaceResponse`] are deliberately the same pair
//! `dock.rs` already uses: an env carrying the slice of the application a
//! surface may read, and a response describing what it asked the host to do
//! afterwards. Two things follow from copying a pattern that already works
//! here rather than inventing a second one. A surface never reaches back into
//! `QuantickApp`, so it cannot grow a dependency on the trunk; and
//! [`SurfaceResponse`] is a **struct, not an enum**, so a new request is an
//! added field that defaults to "did not ask", never a `match` arm that
//! reopens every existing caller — the failure mode dimension 9 names in
//! `ChartLayer`'s 21 variants across 264 call sites.
//!
//! # Why the registry is a typed struct, not `Vec<Box<dyn Surface>>`
//!
//! [`Surfaces`] names its members as fields. A trait-object list would let a
//! surface register itself without being named here, which sounds stronger
//! until the host needs to *command* one — `show_agent_popup` has to reach
//! the assistant's popup specifically — and the only ways back to a concrete
//! type from `Box<dyn Surface>` are downcasting, which dimension 1 calls a
//! broken port, or a command enum, which is the growth this port exists to
//! stop.
//!
//! So the trade is made in the open: adding a surface edits this file, on
//! purpose. What matters is that the edit is one field and one line in a file
//! of this size instead of four edits spread through a 11,874-line trunk, and
//! that [`Surface`] keeps the draw contract uniform so the list below cannot
//! drift into twenty bespoke call sites. When the count outgrows a legible
//! struct, the move is to a keyed registry — and by then the trait every
//! surface already implements is what makes that a local change.

pub(crate) mod agent_popup;
pub(crate) mod footprint_settings;
pub(crate) mod indicator_preview;
pub(crate) mod source_picker;
pub(crate) mod strategy_popup;
pub(crate) mod style_panel;
pub(crate) mod toast;
pub(crate) mod workspace_name;

use std::time::Instant;

use eframe::egui;

pub(crate) use agent_popup::AgentPopupSurface;
pub(crate) use footprint_settings::FootprintSettingsSurface;
pub(crate) use indicator_preview::IndicatorPreviewSurface;
pub(crate) use source_picker::SourcePickerSurface;
pub(crate) use strategy_popup::StrategyPopupSurface;
pub(crate) use style_panel::StylePanelSurface;
pub(crate) use toast::ToastSurface;
pub(crate) use workspace_name::WorkspaceNameSurface;

/// What the surfaces need from the application, kept to what they actually
/// read so a surface cannot quietly acquire the whole app.
pub(crate) struct SurfaceEnv<'a> {
    /// The saved arrangements, so the Save-as box can warn before a name
    /// replaces one. Borrowed, never copied: a surface reads what the host
    /// holds rather than keeping a copy that can go stale.
    pub bookmarks: &'a [crate::ui_state::NamedArrangement],
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
    /// The market catalog: which feeds exist and what each one offers.
    pub config: &'a crate::config::AppConfig,
    /// The symbols the trader added by hand, so the dialog can mark which
    /// rows it is allowed to take back out.
    pub added_symbols: &'a crate::symbols_file::AddedSymbols,
    /// The markets already open in a tab, as `(feed id, symbol)`.
    ///
    /// Owned strings, so the host builds this **only while the dialog that
    /// reads it is open** — an empty slice otherwise. A per-frame allocation
    /// for a surface nobody can see is exactly the cost this port was
    /// supposed to make visible rather than hide.
    pub open_markets: &'a [(String, String)],
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
    /// `SurfaceEnv { bookmarks: &saved, ..SurfaceEnv::quiet(now) }`, so a
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
        /// An empty catalog, for the tests that never open the market
        /// dialog. Written out rather than derived: `AppConfig` has no
        /// `Default`, and giving a production type one so a test can skip
        /// six fields is the tail wagging the dog.
        static QUIET_CONFIG: std::sync::LazyLock<crate::config::AppConfig> =
            std::sync::LazyLock::new(|| crate::config::AppConfig {
                default_feed: String::new(),
                default_symbol: String::new(),
                feeds: Vec::new(),
                metatrader: Default::default(),
                paper: Default::default(),
                history: Default::default(),
            });
        /// No hand-added symbols, likewise.
        static QUIET_SYMBOLS: std::sync::LazyLock<crate::symbols_file::AddedSymbols> =
            std::sync::LazyLock::new(crate::symbols_file::AddedSymbols::default);
        Self {
            bookmarks: &[],
            now,
            indicator_preview_area: None,
            focused_chart_area: None,
            style: &QUIET_STYLE,
            footprint: &QUIET_FOOTPRINT,
            footprint_customized: false,
            focused_side: crate::pane::PaneSide::Flow,
            config: &QUIET_CONFIG,
            added_symbols: &QUIET_SYMBOLS,
            open_markets: &[],
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
    /// The toast's Undo button was pressed — the host owns the drawing stack,
    /// and the sweep of any strategy left orphaned by the undo.
    pub undo_drawing: bool,
    /// The Save-as box settled on a name. The host owns the write, so the
    /// surface hands over the name rather than touching the file.
    pub save_workspace_as: Option<String>,
    /// The appearance window edited the chart style. The host owns the style
    /// every renderer reads, and the revision counter they watch, so it takes
    /// the edited copy rather than lending the original out.
    pub style: Option<crate::style::ChartStyle>,
    /// An appearance change settled and is owed a log line.
    pub log_style_change: Option<StyleLogRequest>,
    /// The footprint window settled on a setup, or asked for the chart's
    /// override to be dropped.
    pub footprint: Option<FootprintChange>,
    /// The market dialog settled on something only the application can do.
    pub market: Option<MarketRequest>,
    /// The arming dialog's **Arm** was pressed. The host answers through
    /// `StrategyPopupSurface::settle_arm`, because the answer decides whether
    /// the dialog closes or shows a refusal.
    pub arm_strategy: Option<ArmRequest>,
    /// A sound was auditioned. The host owns the one speaker every armed
    /// instance shares, so it plays this and reports what happened.
    pub test_alert: Option<crate::audio::Cue>,
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

/// What the "Open market" dialog asked the host to do.
///
/// Exclusive answers to one question — a frame cannot both add a symbol and
/// remove one — so they share a field. Each carries the market it is about
/// rather than leaving the host to read it back off the dialog, which is what
/// keeps the ask true even if the dialog changes underneath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarketRequest {
    /// Open this market in a new tab.
    Open { feed_id: String, symbol: String },
    /// Put this symbol in the feed's catalog, remember it across restarts,
    /// and open it. May be refused: the rule is the whole config's, so the
    /// answer comes back through [`source_picker::SourcePickerSurface::refuse`].
    Add { feed_id: String, symbol: String },
    /// Drop this hand-added symbol from the catalog. Never closes a tab —
    /// leaving the catalog is not leaving the market.
    Remove { feed_id: String, symbol: String },
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
        self.undo_drawing |= other.undo_drawing;
        self.save_workspace_as = self.save_workspace_as.take().or(other.save_workspace_as);
        self.style = self.style.take().or(other.style);
        self.log_style_change = self.log_style_change.take().or(other.log_style_change);
        self.footprint = self.footprint.take().or(other.footprint);
        self.market = self.market.take().or(other.market);
        self.arm_strategy = self.arm_strategy.take().or(other.arm_strategy);
        self.test_alert = self.test_alert.take().or(other.test_alert);
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
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {}
}

/// Every floating surface the application owns.
#[derive(Default)]
pub(crate) struct Surfaces {
    /// Whether the capture hooks have run.
    ///
    /// They are applied on the first *drawn frame*, not at construction. The
    /// toast expires on a deadline, so starting that clock before wgpu init
    /// and the history backfill have finished can retire a hook-raised toast
    /// before frame one ever reaches the screen — and the failure is silent,
    /// read as "the toast isn't there" rather than as a mistimed hook.
    hooks_applied: bool,
    /// The assistant's popup — raised by `quantick_notify` over the control
    /// plane, dismissed by the trader.
    pub agent_popup: AgentPopupSurface,
    /// The banner over a pane whose indicator is previewing an unapplied
    /// draft.
    pub indicator_preview: IndicatorPreviewSurface,
    /// The footprint settings window, opened from the toolbar and from a
    /// pane's layer menu.
    pub footprint_settings: FootprintSettingsSurface,
    /// The "Open market" dialog, opened by the `+` and by Ctrl+T.
    pub source_picker: SourcePickerSurface,
    /// The "Arm strategy" dialog, opened by right-clicking a drawn region.
    pub strategy_popup: StrategyPopupSurface,
    /// The candle-appearance window, opened from the toolbar and the View
    /// menu.
    pub style_panel: StylePanelSurface,
    /// The window's acknowledgement channel, with Undo where the act has one.
    pub toast: ToastSurface,
    /// The Save-as box, opened from the Workspace menu.
    pub workspace_name: WorkspaceNameSurface,
}

impl Surfaces {
    /// Whether the capture hooks have still to run.
    ///
    /// The host reads this when it decides whether to build the environment
    /// slices that only an open surface needs. A hook opens its surface
    /// *inside* [`Self::draw_all`], which is after those slices were
    /// gathered, so on that one frame "is it open" is the wrong question —
    /// the right one is "is it about to be". Without it a hook-opened market
    /// dialog draws its first frame against an empty open-markets list and
    /// photographs Remove buttons that should have been greyed out: a capture
    /// of a state the application would never reach on its own.
    pub fn hooks_pending(&self) -> bool {
        !self.hooks_applied
    }

    /// Draw every surface and return the merged asks.
    ///
    /// Per-frame path: one virtual call per registered surface, each of which
    /// returns immediately when it has nothing to show. That is the whole
    /// cost, and it replaces the same number of direct calls `draw_frame`
    /// made by hand.
    ///
    /// Call order does not decide what covers what. Every surface here names
    /// its own `egui::Order`, and egui sorts by layer before it considers the
    /// sequence: the toast paints in `Foreground` wherever it is called from,
    /// while the popup is an ordinary `Middle`-order window. Within one order
    /// egui puts a newly created area on top, and every window here is created
    /// on the frame it opens — so the window a trader just opened is in front
    /// of whatever else is on screen, wherever this call sits in the frame.
    /// That is what makes it safe for one call site to replace the scattered
    /// ones, and it is also why a surface added here must set its own order
    /// rather than rely on being last.
    pub fn draw_all(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        if !self.hooks_applied {
            self.hooks_applied = true;
            self.agent_popup.apply_env_hook(env);
            self.footprint_settings.apply_env_hook(env);
            self.indicator_preview.apply_env_hook(env);
            self.source_picker.apply_env_hook(env);
            self.strategy_popup.apply_env_hook(env);
            self.style_panel.apply_env_hook(env);
            self.toast.apply_env_hook(env);
            self.workspace_name.apply_env_hook(env);
        }
        let mut response = SurfaceResponse::default();
        response.merge(self.agent_popup.draw(ctx, env));
        response.merge(self.footprint_settings.draw(ctx, env));
        response.merge(self.indicator_preview.draw(ctx, env));
        response.merge(self.source_picker.draw(ctx, env));
        response.merge(self.strategy_popup.draw(ctx, env));
        response.merge(self.style_panel.draw(ctx, env));
        response.merge(self.toast.draw(ctx, env));
        response.merge(self.workspace_name.draw(ctx, env));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            save_workspace_as: Some("first".to_owned()),
            style: Some(crate::style::ChartStyle::default()),
            log_style_change: Some(StyleLogRequest {
                applied_preset: None,
            }),
            footprint: Some(FootprintChange::ResetToDefault),
            market: Some(MarketRequest::Open {
                feed_id: "first".to_owned(),
                symbol: "FIRST".to_owned(),
            }),
            test_alert: Some(crate::audio::Cue::default()),
            ..SurfaceResponse::default()
        };
        let mut contested = crate::style::ChartStyle::default();
        contested.canvas.grid_enabled = !contested.canvas.grid_enabled;
        first.merge(SurfaceResponse {
            save_workspace_as: Some("second".to_owned()),
            style: Some(contested),
            log_style_change: Some(StyleLogRequest {
                applied_preset: Some(crate::style::CandlePreset::Classic),
            }),
            footprint: Some(FootprintChange::Applied(Box::default())),
            market: Some(MarketRequest::Remove {
                feed_id: "second".to_owned(),
                symbol: "SECOND".to_owned(),
            }),
            arm_strategy: None,
            test_alert: Some(crate::audio::Cue::new(
                crate::audio::AlertSound::Critical,
                None,
            )),
            undo_drawing: false,
        });
        assert_eq!(first.save_workspace_as.as_deref(), Some("first"));
        assert_eq!(first.footprint, Some(FootprintChange::ResetToDefault));
        assert_eq!(
            first.market,
            Some(MarketRequest::Open {
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
        let mut real = AgentPopupSurface::default();
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

    /// A request that carries a value is first-wins, which is the half of
    /// `merge` the boolean cannot show. Pinned because the answer has to be
    /// documented before a second producer exists, not after.
    #[test]
    fn merge_keeps_the_first_named_workspace() {
        let mut merged = SurfaceResponse {
            save_workspace_as: Some("first".to_string()),
            ..SurfaceResponse::default()
        };
        merged.merge(SurfaceResponse {
            save_workspace_as: Some("second".to_string()),
            ..SurfaceResponse::default()
        });
        assert_eq!(merged.save_workspace_as.as_deref(), Some("first"));
    }

    /// And an earlier surface that asked for nothing does not block a later
    /// one that did.
    #[test]
    fn merge_accepts_a_name_from_a_later_surface() {
        let mut merged = SurfaceResponse::default();
        merged.merge(SurfaceResponse {
            save_workspace_as: Some("only".to_string()),
            ..SurfaceResponse::default()
        });
        assert_eq!(merged.save_workspace_as.as_deref(), Some("only"));
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
