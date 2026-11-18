//! When the drawing context bar stands dismissed.
//!
//! The bar a selection raises closes on any press that is not aimed at it:
//! the canvas, another pane, a panel, the toolbar, another window. It stays
//! closed — the object still selected — until the trader presses that object
//! again or selects another. A temporary suppression (a pan, a zoom) brings
//! the bar back on its own; a dismissal does not.

/// Where a press began, as the context bar sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressOn {
    /// The bar itself or its open popover: how the trader reaches it.
    Bar,
    /// The selected object: the one way back to a dismissed bar.
    SelectedDrawing,
    /// Anything else in any Quantick window.
    Elsewhere,
}

impl PressOn {
    /// Classify a press. `pick` is what the chart's Pointer gesture picked
    /// for it (`Some(None)`: empty canvas; `None`: a press it never saw, which
    /// falls back to `in_bounds`, the object's painted bounds on the chart).
    #[must_use]
    pub fn classify(
        on_bar: bool,
        pick: Option<Option<usize>>,
        selected: usize,
        in_bounds: bool,
    ) -> Self {
        if on_bar {
            Self::Bar
        } else if pick.map_or(in_bounds, |pick| pick == Some(selected)) {
            Self::SelectedDrawing
        } else {
            Self::Elsewhere
        }
    }
}

/// The dismissal state, stepped once per press.
#[derive(Debug, Default)]
pub struct BarDismissal {
    dismissed: bool,
    /// Start time of the last press decided, so a held press is read once.
    last_press: Option<f64>,
    /// Window focus last frame; `None` before the first report.
    focused: Option<bool>,
}

impl BarDismissal {
    /// A new selection is a new bar.
    pub fn note_selection_changed(&mut self) {
        self.dismissed = false;
    }

    /// Decide the press that started at `started`, once. Answers whether
    /// this call dismissed the bar.
    pub fn note_press(&mut self, started: f64, on: PressOn) -> bool {
        if self.last_press == Some(started) {
            return false;
        }
        self.last_press = Some(started);
        let was = self.dismissed;
        match on {
            PressOn::Bar => {}
            PressOn::SelectedDrawing => self.dismissed = false,
            PressOn::Elsewhere => self.dismissed = true,
        }
        self.dismissed && !was
    }

    /// The window lost focus: a click in another window, Quantick's or not.
    /// Answers whether this call dismissed the bar.
    pub fn note_focus(&mut self, focused: bool) -> bool {
        let lost = self.focused == Some(true) && !focused;
        self.focused = Some(focused);
        let was = self.dismissed;
        if lost {
            self.dismissed = true;
        }
        self.dismissed && !was
    }

    #[must_use]
    pub fn dismissed(&self) -> bool {
        self.dismissed
    }
}

#[cfg(test)]
#[path = "bar_dismissal_tests.rs"]
mod tests;
