//! When the drawing context bar stands dismissed.
//!
//! The bar a selection raises closes on any press that is not aimed at it:
//! the canvas, another pane, a panel, the toolbar, another window. It stays
//! closed — the object still selected — until the trader presses that object
//! again or another bar target comes up (another object, pane or tab). A
//! temporary suppression (a pan, a zoom) brings the bar back on its own; a
//! dismissal does not.

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

/// The bar a dismissal belongs to: tab, pane and the selected object's id.
/// A different target is a new bar, never a dismissed one.
pub type BarTarget = (u64, u64, u64);

/// The dismissal state, stepped every frame.
#[derive(Debug, Default)]
pub struct BarDismissal {
    dismissed: bool,
    target: Option<BarTarget>,
    /// Start time of the last press decided, so a held press is read once.
    last_press: Option<f64>,
    /// Window focus last frame; `None` before the first report.
    focused: Option<bool>,
}

impl BarDismissal {
    /// Step one frame: the bar's target (`None` without a selection), the
    /// window focus, and the press in flight with its start time. Losing
    /// focus is a click in another window, Quantick's or not. Answers whether
    /// this step dismissed the bar.
    pub fn step(
        &mut self,
        target: Option<BarTarget>,
        focused: bool,
        press: Option<(f64, PressOn)>,
    ) -> bool {
        if self.target != target {
            self.target = target;
            self.dismissed = false;
        }
        let lost = self.focused == Some(true) && !focused;
        self.focused = Some(focused);
        let press = press.filter(|(started, _)| self.last_press != Some(*started));
        if let Some((started, _)) = press {
            self.last_press = Some(started);
        }
        if target.is_none() {
            return false;
        }
        let was = self.dismissed;
        self.dismissed |= lost;
        match press.map(|(_, on)| on) {
            Some(PressOn::SelectedDrawing) => self.dismissed = false,
            Some(PressOn::Elsewhere) => self.dismissed = true,
            Some(PressOn::Bar) | None => {}
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
