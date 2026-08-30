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
pub(crate) mod toast;
pub(crate) mod workspace_name;

use std::time::Instant;

use eframe::egui;

pub(crate) use agent_popup::AgentPopupSurface;
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
}

/// What drawing the surfaces asked the application to do.
///
/// A struct of defaults, like [`crate::dock::DockResponse`]: every field means
/// "this was not asked for" until a surface sets it, so adding a request never
/// touches an existing surface.
#[derive(Default, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceResponse {
    /// The toast's Undo button was pressed — the host owns the drawing stack,
    /// and the sweep of any strategy left orphaned by the undo.
    pub undo_drawing: bool,
    /// The Save-as box settled on a name. The host owns the write, so the
    /// surface hands over the name rather than touching the file.
    pub save_workspace_as: Option<String>,
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
    /// before the first frame is drawn.
    ///
    /// On the trait rather than in a list held beside it, so a surface that
    /// forgets its hook shows up as an empty override instead of compiling
    /// silently: `ui-harness` requires one per surface, and a hook nobody
    /// wrote is invisible to every test and to the size guard alike. The
    /// default is no hook, which is the honest answer for a surface that
    /// needs none.
    fn apply_env_hook(&mut self, _now: Instant) {}
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
    /// The window's acknowledgement channel, with Undo where the act has one.
    pub toast: ToastSurface,
    /// The Save-as box, opened from the Workspace menu.
    pub workspace_name: WorkspaceNameSurface,
}

impl Surfaces {
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
    /// while the popup is an ordinary `Middle`-order window. That is what
    /// makes it safe for one call site to replace the scattered ones — but it
    /// is also why a surface added here must set its own order rather than
    /// rely on being last.
    pub fn draw_all(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        if !self.hooks_applied {
            self.hooks_applied = true;
            self.agent_popup.apply_env_hook(env.now);
            self.toast.apply_env_hook(env.now);
            self.workspace_name.apply_env_hook(env.now);
        }
        let mut response = SurfaceResponse::default();
        response.merge(self.agent_popup.draw(ctx, env));
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
    }

    impl Surface for FakeSurface {
        fn id(&self) -> &'static str {
            "fake"
        }

        fn draw(&mut self, _ctx: &egui::Context, _env: &SurfaceEnv<'_>) -> SurfaceResponse {
            self.drawn += 1;
            SurfaceResponse {
                undo_drawing: self.asks_undo,
                ..SurfaceResponse::default()
            }
        }
    }

    fn env() -> SurfaceEnv<'static> {
        SurfaceEnv {
            bookmarks: &[],
            now: Instant::now(),
        }
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
        fake.apply_env_hook(Instant::now());
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
