//! The acknowledgement toast, as a [`Surface`].
//!
//! Transient confirmation that the window did what was asked, with an escape
//! hatch when the act has one. Undo works from the button for
//! [`UNDO_MS`] and from Ctrl+Z for as long as the history holds.
//!
//! This is **the** window-level acknowledgement channel — the only one. The
//! paper-trading panel used to keep a second, per tab and on its own 4-second
//! clock, in this same lane 96px up instead of 44; it posts here now
//! (`paper_trading.rs`, `take_toast`). It floats over the chart's bottom edge
//! instead of taking a cell on the status line, and that is the reason: the
//! status bar's readings live at fixed positions the trader's eye returns to
//! without looking, and a cell that appeared for eight seconds and then left
//! would slide `bars` and `arrival` sideways twice per acknowledgement
//! (`statusbar.rs`: "the layout never moves").
//!
//! # One slot, and what happens when two acts land in it
//!
//! Newest wins, with one exception: a message that carries **Undo** is not
//! displaced by one that does not. The button is the trader's only
//! discoverable way back from a delete, and a fill on some other market
//! landing two seconds later must not take it away — so the plain message
//! waits [`Pending::deferred`] and goes up when the undoable one leaves,
//! whether it left by expiring or by being used.
//!
//! The Undo the button asks for is *not* performed here. The drawing stack
//! and the strategies riding on it belong to the host, so the surface reports
//! the request through [`SurfaceResponse`] and the host acts — the boundary
//! that keeps a surface from reaching back into `QuantickApp`.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use eframe::egui;

use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::theme;

/// How long the delete toast keeps its Undo affordance on screen (UX spec).
const UNDO_MS: u64 = 8_000;
/// Vertical clearance between the toast and the bottom chrome.
const BOTTOM_MARGIN_PX: f32 = 44.0;

/// A message waiting to be read, and whether it can be taken back.
#[derive(Debug)]
struct Pending {
    /// Borrowed for the fixed messages, owned when the act has a count to
    /// report. Acknowledgements are event-driven and rare — never a frame
    /// path — so an allocation here costs nothing anyone can see.
    message: Cow<'static, str>,
    shown_at: Instant,
    /// Whether the toast offers Undo. A delete does; the honest clear after
    /// a bar rebuild does not — its history is gone with the drawings, and
    /// a dead Undo button would lie. Neither does a workspace save: the file
    /// it replaced is gone, and `Reset startup layout` is the real way back.
    offers_undo: bool,
}

/// The window's acknowledgement channel: at most one message on screen.
#[derive(Default)]
pub(crate) struct ToastSurface {
    pending: Option<Pending>,
    /// A plain acknowledgement that arrived while an undoable one was still
    /// on screen, held until that one leaves.
    ///
    /// One slot, not a queue: a trader reading an acknowledgement from ten
    /// seconds ago while the current one waits behind it is worse than
    /// missing the older one, which is the same rule the visible slot
    /// follows.
    deferred: Option<Cow<'static, str>>,
    /// Where the Undo button landed, for the tests that click it. Test-only
    /// state, and `#[cfg(test)]` so it neither exists in the shipped binary
    /// nor changes what the surface does.
    #[cfg(test)]
    undo_rect: Option<egui::Rect>,
}

impl ToastSurface {
    /// Acknowledge an act that cannot be taken back.
    ///
    /// Waits its turn behind a message that still offers Undo: taking that
    /// button away mid-window would remove the trader's only discoverable way
    /// back from a delete, for the sake of a message about something else.
    pub fn note(&mut self, message: impl Into<Cow<'static, str>>, now: Instant) {
        if self.offers_live_undo(now) {
            self.deferred = Some(message.into());
            return;
        }
        self.pending = Some(Pending {
            message: message.into(),
            shown_at: now,
            offers_undo: false,
        });
    }

    /// Whether an Undo the trader can still press is on screen.
    fn offers_live_undo(&self, now: Instant) -> bool {
        self.pending.as_ref().is_some_and(|pending| {
            pending.offers_undo
                && now.saturating_duration_since(pending.shown_at) < Duration::from_millis(UNDO_MS)
        })
    }

    /// Acknowledge an act the trader can undo, and offer the button for it.
    ///
    /// Displaces whatever is on screen, including another undoable message:
    /// the newer act is the one whose way back the trader is looking for.
    pub fn note_with_undo(&mut self, message: impl Into<Cow<'static, str>>, now: Instant) {
        self.deferred = None;
        self.pending = Some(Pending {
            message: message.into(),
            shown_at: now,
            offers_undo: true,
        });
    }

    /// Take the current message off screen. Test-only: in the product a
    /// toast leaves on its own deadline or on Undo, never by command.
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.pending = None;
        self.deferred = None;
    }

    /// Put a held message up, if the slot has come free.
    ///
    /// Called at the top of the draw, after expiry, so a deferred message
    /// starts its own eight seconds from the frame it appears rather than
    /// from the frame it was raised — it would otherwise be shown for
    /// whatever was left of the message it waited behind.
    fn promote_deferred(&mut self, now: Instant) {
        if self.pending.is_some() {
            return;
        }
        if let Some(message) = self.deferred.take() {
            self.pending = Some(Pending {
                message,
                shown_at: now,
                offers_undo: false,
            });
        }
    }

    /// The message on screen, if any.
    #[cfg(test)]
    pub fn message(&self) -> Option<&str> {
        self.pending.as_ref().map(|pending| &*pending.message)
    }

    /// Whether the message on screen offers Undo.
    #[cfg(test)]
    pub fn offers_undo(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.offers_undo)
    }

    /// Where the Undo button was painted last frame, for tests that click it.
    #[cfg(test)]
    pub fn undo_rect(&self) -> Option<egui::Rect> {
        self.undo_rect
    }
}

impl Surface for ToastSurface {
    fn id(&self) -> &'static str {
        "toast"
    }

    /// `plain` for an act that cannot be taken back, `undo` for one that can.
    ///
    /// Both states are answers to something the trader just did — a delete, a
    /// save — so a capture that never performs the act can photograph neither,
    /// and the Undo button is the affordance most worth seeing. Goes through
    /// the same two entry points the application uses.
    ///
    /// `paper` is handled by the host and not here: it stages the message
    /// through the simulator's own `show_toast`, so what it photographs is
    /// the whole route into this lane — the panel's outbox, the drain, and
    /// the naming a background market gets — rather than this surface alone.
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) {
        match std::env::var("QUANTICK_TOAST").as_deref() {
            Ok("plain") => self.note("Workspace saved.", env.now),
            Ok("undo") => self.note_with_undo("Trend line deleted.", env.now),
            // A typo must not photograph the wrong state and call it a pass.
            _ => {}
        }
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let id = self.id();
        // Expire first, so the borrow taken below is only ever of a toast that
        // is still on screen.
        if self.pending.as_ref().is_some_and(|pending| {
            env.now.saturating_duration_since(pending.shown_at) >= Duration::from_millis(UNDO_MS)
        }) {
            self.pending = None;
        }
        self.promote_deferred(env.now);
        let Some(pending) = &self.pending else {
            return SurfaceResponse::default();
        };
        // Borrowed, never cloned: the toast is painted on every frame of its
        // eight seconds, and an owned message copied per frame would be ~500
        // allocations for a string that never changes.
        let message: &str = &pending.message;
        let offers_undo = pending.offers_undo;
        let mut undo_clicked = false;
        #[cfg(test)]
        let mut undo_rect = None;
        egui::Area::new(egui::Id::new(id))
            .anchor(
                egui::Align2::CENTER_BOTTOM,
                egui::vec2(0.0, -BOTTOM_MARGIN_PX),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::TAG_BG)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(6.0_f32)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(message);
                            if offers_undo {
                                let undo = ui.button("Undo");
                                #[cfg(test)]
                                {
                                    undo_rect = Some(undo.rect);
                                }
                                undo_clicked = undo.clicked();
                            }
                        });
                    });
            });
        #[cfg(test)]
        {
            self.undo_rect = undo_rect;
        }
        if undo_clicked {
            self.pending = None;
            // The message that was waiting goes up on the next frame, from
            // the same door every other acknowledgement uses.

            return SurfaceResponse {
                undo_drawing: true,
                ..SurfaceResponse::default()
            };
        }
        SurfaceResponse::default()
    }
}

crate::hooks::declare_hooks!["QUANTICK_TOAST"];

#[cfg(test)]
mod tests {
    use super::*;

    fn env(now: Instant) -> SurfaceEnv<'static> {
        SurfaceEnv::quiet(now)
    }

    /// A toast leaves on its own after the undo window, without anyone having
    /// to dismiss it.
    #[test]
    fn a_toast_expires_after_the_undo_window() {
        let ctx = egui::Context::default();
        let mut surface = ToastSurface::default();
        let shown = Instant::now();
        surface.note("Workspace saved.", shown);
        assert_eq!(surface.message(), Some("Workspace saved."));

        let _ = ctx.run(Default::default(), |ctx| {
            surface.draw(ctx, &env(shown + Duration::from_millis(UNDO_MS - 1)));
        });
        assert_eq!(
            surface.message(),
            Some("Workspace saved."),
            "still inside the window"
        );

        let _ = ctx.run(Default::default(), |ctx| {
            surface.draw(ctx, &env(shown + Duration::from_millis(UNDO_MS)));
        });
        assert_eq!(surface.message(), None, "the window closed on the boundary");
    }

    /// The undo-bearing message is a different act from the plain one, and
    /// only the former paints a button.
    #[test]
    fn only_an_undoable_act_paints_the_button() {
        let ctx = egui::Context::default();
        let now = Instant::now();

        let mut plain = ToastSurface::default();
        plain.note("Drawings cleared.", now);
        let _ = ctx.run(Default::default(), |ctx| {
            plain.draw(ctx, &env(now));
        });
        assert!(plain.undo_rect().is_none());

        let mut undoable = ToastSurface::default();
        undoable.note_with_undo("Trend line deleted.", now);
        let _ = ctx.run(Default::default(), |ctx| {
            undoable.draw(ctx, &env(now));
        });
        assert!(undoable.undo_rect().is_some());
    }

    /// A fill on another market must not take away the Undo button on a
    /// delete the trader made two seconds ago. It waits, and goes up when the
    /// undoable message leaves.
    #[test]
    fn a_plain_message_waits_behind_a_live_undo() {
        let ctx = egui::Context::default();
        let mut surface = ToastSurface::default();
        let shown = Instant::now();
        surface.note_with_undo("Trend line deleted.", shown);

        surface.note("SIM: stop filled.", shown + Duration::from_secs(2));
        assert_eq!(
            surface.message(),
            Some("Trend line deleted."),
            "the way back stays on screen"
        );
        assert!(surface.offers_undo(), "and so does its button");

        // Once the undo window closes, the held message takes the slot — and
        // starts its own eight seconds from here, not from when it was raised.
        let expired = shown + Duration::from_millis(UNDO_MS);
        let _ = ctx.run(Default::default(), |ctx| {
            surface.draw(ctx, &env(expired));
        });
        assert_eq!(surface.message(), Some("SIM: stop filled."));
        assert!(!surface.offers_undo());

        let _ = ctx.run(Default::default(), |ctx| {
            surface.draw(ctx, &env(expired + Duration::from_millis(UNDO_MS - 1)));
        });
        assert_eq!(
            surface.message(),
            Some("SIM: stop filled."),
            "it gets a full window of its own, not the remainder of another's"
        );
    }

    /// Only a *live* offer defers anything. Once the undo window has closed,
    /// the next acknowledgement takes the slot directly rather than queueing
    /// behind a button nobody can press.
    #[test]
    fn a_spent_undo_defers_nothing() {
        let mut surface = ToastSurface::default();
        let shown = Instant::now();
        surface.note_with_undo("Trend line deleted.", shown);
        surface.note("Workspace saved.", shown + Duration::from_millis(UNDO_MS));
        assert_eq!(surface.message(), Some("Workspace saved."));
        assert!(!surface.offers_undo());
    }

    /// A newer undoable act does displace an older one — the trader is
    /// looking for the way back from what they just did — and it clears
    /// anything that was waiting, which belonged to a slot that no longer
    /// exists.
    #[test]
    fn a_newer_undo_takes_the_slot_and_the_queue_with_it() {
        let mut surface = ToastSurface::default();
        let shown = Instant::now();
        surface.note_with_undo("Trend line deleted.", shown);
        surface.note("SIM: stop filled.", shown + Duration::from_secs(1));
        surface.note_with_undo("Rectangle deleted.", shown + Duration::from_secs(2));
        assert_eq!(surface.message(), Some("Rectangle deleted."));

        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            surface.draw(
                ctx,
                &env(shown + Duration::from_secs(2) + Duration::from_millis(UNDO_MS)),
            );
        });
        assert_eq!(
            surface.message(),
            None,
            "the held message went with the slot it was waiting for"
        );
    }

    /// Drawing a quiet surface asks the host for nothing. The undo request is
    /// raised only by the click, which is what keeps the host's drawing stack
    /// out of reach of an ordinary frame.
    #[test]
    fn an_untouched_toast_asks_the_host_for_nothing() {
        let ctx = egui::Context::default();
        let now = Instant::now();
        let mut surface = ToastSurface::default();
        surface.note_with_undo("Trend line deleted.", now);
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(now));
        });
        assert_eq!(response, SurfaceResponse::default());
    }
}
