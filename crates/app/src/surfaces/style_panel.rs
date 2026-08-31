//! The candle-appearance window, as a [`Surface`].
//!
//! The window itself has lived in `candle_view.rs` since it was written; what
//! sat in the trunk was the wiring around it — the open flag, the revision
//! bump, and the debounce that decides when an appearance change is worth a
//! log line. That wiring is this surface.
//!
//! # Why the surface edits a copy
//!
//! [`crate::candle_view::draw_style_window`] takes `&mut ChartStyle`, and the
//! style itself is host state: every renderer in the application reads it, so
//! it cannot move in here. Rather than widen [`SurfaceEnv`] to hand out
//! mutable access to the trunk — which is the one thing the port exists to
//! prevent — the surface edits a **copy** and returns it, exactly as
//! `footprint_panel`'s caller already did. `ChartStyle` is `Copy`, and the
//! copy is taken only on the frames the window is actually open, so the
//! per-frame cost of a closed window is one boolean test.
//!
//! # The debounce, and why it is the surface's
//!
//! Dragging an opacity slider changes the style on every frame of the drag.
//! Logging each one would bury the event stream under a gesture, so the log
//! waits [`STYLE_LOG_DEBOUNCE`] after the last change — or fires immediately
//! when the window closes, because a trader who shut the panel is done and
//! the record should not wait on a timer. Both halves are about *this
//! window's* interaction, so both live here; the host is left with the one
//! thing it owns, which is writing the line.

use std::time::{Duration, Instant};

use eframe::egui;

use super::{StyleLogRequest, Surface, SurfaceEnv, SurfaceResponse};
use crate::candle_view::draw_style_window;

/// How long the appearance log waits for a gesture to settle. Longer than a
/// frame at any refresh rate, shorter than the pause between two deliberate
/// clicks.
const STYLE_LOG_DEBOUNCE: Duration = Duration::from_millis(350);

/// The appearance window and the debounce that decides when its changes are
/// worth recording.
#[derive(Default)]
pub(crate) struct StylePanelSurface {
    open: bool,
    /// A change has happened that has not been logged yet.
    log_pending: bool,
    /// When the last change landed, for the debounce.
    last_change: Option<Instant>,
}

impl StylePanelSurface {
    /// Whether a change that has not been recorded yet should be recorded
    /// now: once the gesture has settled, or the moment the window closes.
    ///
    /// Its own function because the second half is unreachable from a test
    /// that drives [`Surface::draw`] — closing the window is
    /// `draw_style_window`'s doing, on a click a headless context cannot
    /// deliver. Naming the rule makes both halves testable, which is the
    /// only reason it is not written inline.
    fn should_log(&self, now: Instant) -> bool {
        let settled = self
            .last_change
            .is_some_and(|changed| now.saturating_duration_since(changed) >= STYLE_LOG_DEBOUNCE);
        self.log_pending && (settled || !self.open)
    }

    /// Whether the window is on screen — read by the toolbar, whose
    /// Appearance button lights while it is.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Flip the window, as the toolbar's Appearance button does.
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Put the window on screen, as the menu entry that opens it does.
    pub fn open(&mut self) {
        self.open = true;
    }
}

impl Surface for StylePanelSurface {
    fn id(&self) -> &'static str {
        "style-panel"
    }

    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        if std::env::var("QUANTICK_STYLE_PANEL").is_ok_and(|value| value == "1") {
            self.open();
        }
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        if !self.open {
            return SurfaceResponse::default();
        }
        let mut edited = *env.style;
        let panel = draw_style_window(ctx, &mut self.open, &mut edited);
        let mut response = SurfaceResponse::default();
        if panel.changed {
            self.log_pending = true;
            self.last_change = Some(env.now);
            response.style = Some(edited);
        }
        // Read after `draw_style_window`, so a window the trader just closed
        // flushes on the same frame it left.
        if self.should_log(env.now) {
            self.log_pending = false;
            response.log_style_change = Some(StyleLogRequest {
                applied_preset: panel.applied_preset,
            });
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ChartStyle;

    fn env(style: &ChartStyle, now: Instant) -> SurfaceEnv<'_> {
        SurfaceEnv {
            style,
            ..SurfaceEnv::quiet(now)
        }
    }

    /// A closed window reads nothing and asks for nothing — the branch that
    /// keeps it off the frame budget.
    #[test]
    fn a_closed_window_asks_for_nothing() {
        let ctx = egui::Context::default();
        let style = ChartStyle::default();
        let mut surface = StylePanelSurface::default();
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&style, Instant::now()));
        });
        assert_eq!(response, SurfaceResponse::default());
        assert!(!surface.is_open());
    }

    /// An open window on a frame the trader touched nothing reports no edit —
    /// otherwise every open panel would bump the style revision at the
    /// refresh rate.
    #[test]
    fn an_open_window_that_was_not_touched_reports_no_edit() {
        let ctx = egui::Context::default();
        let style = ChartStyle::default();
        let mut surface = StylePanelSurface::default();
        surface.open();
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&style, Instant::now()));
        });
        assert!(response.style.is_none());
        assert!(response.log_style_change.is_none());
    }

    /// The toolbar's Appearance button and the View menu both go through
    /// these, so a click and a named call cannot disagree about the window.
    #[test]
    fn the_window_opens_and_closes_through_its_own_door() {
        let mut surface = StylePanelSurface::default();
        assert!(!surface.is_open());
        surface.toggle();
        assert!(surface.is_open());
        surface.toggle();
        assert!(!surface.is_open());
        surface.open();
        assert!(surface.is_open());
    }

    /// The debounce holds a change back while the gesture is still running
    /// and lets it go once the trader stops, then stays quiet however many
    /// frames follow. Driven through the state the draw sets rather than
    /// through a synthetic drag: what is pinned here is the timing rule, not
    /// egui's slider.
    #[test]
    fn the_log_waits_for_the_gesture_to_settle() {
        let ctx = egui::Context::default();
        let style = ChartStyle::default();
        let start = Instant::now();
        let mut surface = StylePanelSurface {
            open: true,
            log_pending: true,
            last_change: Some(start),
        };

        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&style, start + STYLE_LOG_DEBOUNCE / 2));
        });
        assert!(
            response.log_style_change.is_none(),
            "still mid-gesture, nothing to record yet"
        );

        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&style, start + STYLE_LOG_DEBOUNCE));
        });
        assert!(response.log_style_change.is_some(), "the gesture settled");

        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&style, start + STYLE_LOG_DEBOUNCE * 4));
        });
        assert!(
            response.log_style_change.is_none(),
            "one change is one log line, however many frames follow it"
        );
    }

    /// Closing the window does not make the trader wait out a timer for the
    /// record of what they just did, and an open window mid-gesture still
    /// does wait.
    #[test]
    fn closing_the_window_logs_without_waiting() {
        let now = Instant::now();
        let mid_gesture = StylePanelSurface {
            open: true,
            log_pending: true,
            last_change: Some(now),
        };
        assert!(
            !mid_gesture.should_log(now),
            "open and mid-gesture: the debounce holds"
        );
        let closed = StylePanelSurface {
            open: false,
            ..mid_gesture
        };
        assert!(
            closed.should_log(now),
            "the window left, so the record goes now rather than in 350ms"
        );
    }

    /// Nothing pending, nothing logged — a window opened and closed without a
    /// single edit writes no line at all.
    #[test]
    fn a_window_that_changed_nothing_logs_nothing() {
        let now = Instant::now();
        let untouched = StylePanelSurface {
            open: false,
            log_pending: false,
            last_change: None,
        };
        assert!(!untouched.should_log(now));
    }
}
