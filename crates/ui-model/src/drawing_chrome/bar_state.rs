//! What the drawing context bar remembers between frames, apart from how it
//! is drawn: whether a gesture holds it down, whether a press dismissed it,
//! where a hand parked it, the rect it was last drawn at and who owns the
//! press in flight. The host keeps only what its toolkit needs (the open
//! popover and its layer) and closes that popover when a step here says so.

use super::bar_dismissal::{BarDismissal, BarTarget, Press};
use quantick_drawings::{Px, PxRect};

/// How long a gesture with no release event — wheel zoom, window resize —
/// keeps the bar suppressed after its last event.
pub const TRANSIENT_SUPPRESS_MS: u64 = 200;

/// Why the bar is currently not on screen.
///
/// Suppression exists to protect the bar from *gestures*, not from the
/// market moving. A drawing that drifts with the auto-scroll simply carries
/// the bar along, smoothly; suppressing that would make the bar blink all
/// session on a live tape. What must never happen is the bar measuring a
/// world the same gesture is still moving.
#[derive(Debug, Default)]
struct Suppression {
    /// A gesture with a release event — dragging the object, a handle, the
    /// canvas, a pane divider — is holding the bar down.
    gesture: bool,
    /// A gesture with no release — wheel zoom, window resize — expires.
    until_ms: Option<u64>,
}

/// The bar's state: everything that must outlive a frame.
#[derive(Debug, Default)]
pub struct BarState {
    /// Where the trader dragged the bar. Absolute screen position, and it
    /// outlives the selection it was chosen under.
    ///
    /// Nobody drags this bar to a corner *for one object*: they drag it off
    /// the price action they are reading, and the next object is on the same
    /// chart with the same thing behind it.
    ///
    /// So the hand outranks the placement here exactly as it does for the
    /// properties popup, and by the same argument: a trader who chose where
    /// this goes has already answered the question the rule exists to guess.
    /// The way back is the grip's double-click, named in its own tooltip and
    /// aimed at the bar and nothing else — see [`Self::clear_manual`].
    ///
    /// The host clamps it into the chart before drawing — see
    /// `QuantickApp::draw_drawing_context_bar` — so a position parked under a
    /// wide layout can never leave the view under a narrow one.
    ///
    /// Session-only, and deliberately: the properties popup records its parked
    /// point in the workspace (`SavedChrome::inspector_position`) because it is
    /// a window of settings, while a *context* bar reappearing in a corner at
    /// the next launch, far from any object and with nothing to explain it,
    /// is a different decision and not one this change makes. The argument
    /// below is about which of hand and rule wins within a session.
    manual: Option<Px>,
    suppression: Suppression,
    dismissal: BarDismissal,
    last_selection: Option<usize>,
    /// Last window size seen, so a resize can suppress like a zoom does.
    last_screen: Option<PxRect>,
    /// Last resolved rect, for the wake radius and for tests.
    rect: Option<PxRect>,
    /// What [`Self::owns_press`] decided, keyed by the press's start time.
    press_owned: Option<(f64, bool)>,
}

impl BarState {
    /// A gesture that has a release event starts here.
    pub fn suppress_gesture(&mut self) {
        self.suppression.gesture = true;
    }

    /// …and ends here. The bar comes back on the next frame, measuring the
    /// world *after* the gesture settled.
    pub fn release_gesture(&mut self) {
        self.suppression.gesture = false;
    }

    /// Watch the window for a resize, which is a gesture with no release of
    /// its own — the pointer is on a border the OS owns. Answers whether
    /// this was one.
    pub fn note_screen(&mut self, screen: PxRect, now_ms: u64) -> bool {
        let resized = self.last_screen.is_some_and(|last| last != screen);
        if resized {
            self.suppress_transient(now_ms);
        }
        self.last_screen = Some(screen);
        resized
    }

    /// A gesture with no release — wheel zoom, resize — renews a deadline.
    pub fn suppress_transient(&mut self, now_ms: u64) {
        self.suppression.until_ms = Some(now_ms + TRANSIENT_SUPPRESS_MS);
    }

    #[must_use]
    pub fn suppressed(&self, now_ms: u64) -> bool {
        self.suppression.gesture
            || (self.suppression.until_ms).is_some_and(|deadline| now_ms < deadline)
    }

    /// Called every frame with the current selection. Answers whether the
    /// selection changed.
    ///
    /// What a new object drops is what belongs to the *old* one: an open
    /// popover (a colour palette hanging off a bar that is about to move is
    /// pointing at nothing — the host closes it on `true`) and the last rect
    /// (the wake radius must not measure against geometry from the previous
    /// object). A hand-placed position is not one of those — see
    /// [`Self::manual`].
    pub fn note_selection(&mut self, selection: Option<usize>) -> bool {
        if self.last_selection == selection {
            return false;
        }
        self.last_selection = selection;
        self.rect = None;
        true
    }

    /// Step [`BarDismissal`] with this frame, answering whether this step
    /// dismissed the bar. Dismissing drops the rect (and the host's popover),
    /// so a later press where the bar was is not read as one on it.
    pub fn step_dismissal(
        &mut self,
        target: Option<BarTarget>,
        focused: bool,
        press: Option<Press>,
    ) -> bool {
        let dismissed = self.dismissal.step(target, focused, press);
        if dismissed {
            self.rect = None;
        }
        dismissed
    }

    #[must_use]
    pub fn dismissed(&self) -> bool {
        self.dismissal.dismissed()
    }

    #[must_use]
    pub fn manual(&self) -> Option<Px> {
        self.manual
    }

    /// The trader dragged the bar by the grip: it stops following the object
    /// and stays where they put it, across the selections that come after —
    /// see [`Self::manual`].
    ///
    /// It still hides through a pan, zoom or canvas drag like a following bar:
    /// suppression is what lets a drag reach the canvas underneath it.
    pub fn set_manual(&mut self, position: Px) {
        self.manual = Some(position);
    }

    /// Give the placement back to the rule, reporting whether there was
    /// anything to give.
    ///
    /// The named door out of the parked state, for a hand and for an operator
    /// without one: the grip's double-click *calls* this rather than clearing
    /// the field itself, so there is one way back and not two.
    ///
    /// Not Escape: a key a trader presses many times an hour would quietly
    /// undo where they put the bar — see the escape stack in
    /// `QuantickApp::drawing_keys`.
    pub fn clear_manual(&mut self) -> bool {
        self.manual.take().is_some()
    }

    #[must_use]
    pub fn last_rect(&self) -> Option<PxRect> {
        self.rect
    }

    /// The rect the bar was drawn at this frame.
    pub fn set_rect(&mut self, rect: PxRect) {
        self.rect = Some(rect);
    }

    /// Whether the press (origin, start time) began on the bar at `current`
    /// or, `on_popover`, on its open popover's layer — a drag there is not a
    /// chart drag. Decided once per press and held to the release: a grip
    /// drag moves the bar, and egui calls the release a drag. Keyed by start
    /// time, so a release this never saw cannot hand its answer to the next
    /// press.
    pub fn owns_press(
        &mut self,
        press: Option<(Px, f64)>,
        on_popover: bool,
    ) -> bool {
        let Some((origin, started)) = press else {
            return self.press_owned.take().is_some_and(|(_, owned)| owned);
        };
        if let Some((at, owned)) = self.press_owned
            && at == started
        {
            return owned;
        }
        let owned = on_popover || self.rect.is_some_and(|rect| rect.contains(origin));
        self.press_owned = Some((started, owned));
        owned
    }
}

#[cfg(test)]
#[path = "bar_state_tests.rs"]
mod tests;
