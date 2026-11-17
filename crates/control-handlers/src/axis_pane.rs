//! A chart pane's framing, as the axis calls act on it: the window resolves
//! the addressed pane ([`crate::addressed_pane::AddressedPanePort`]), the pane
//! frames its axes and its native tape frames itself. Its callers:
//! `chart.price_axis.set` and `chart.tape_view.set` ([`crate::price_axis`])
//! and `chart.time_axis.reset` ([`crate::time_axis`]). They take the same
//! paths the axis gestures take, so the call and the hand cannot drift. The
//! pane's marks are [`crate::pane_marks::PaneMarks`].
use quantick_control_schema::chart::ViewportSnapshot;
use quantick_control_schema::tape_view::TapeViewSnapshot;
use quantick_orderflow::LaneWindow;
use quantick_orderflow::tape_view::TapeEnd;

/// A chart pane, as its axis and tape calls act on it. Its names
/// differ from the pane's own methods, so an implementation that forwards to
/// one cannot fall back on itself.
pub trait AxisPane {
    /// Frame prices from `low` to `high`, in the axis mode the pane draws.
    fn frame_price_range(&mut self, low: f64, high: f64);
    /// Fit prices to the bars in view again, in the axis mode the pane draws.
    fn fit_prices(&mut self);
    /// A time strip's double click: the tape's own strip when `tape`, else
    /// the chart's opening framing. Answers whether the pane has that strip.
    fn reset_time_strip(&mut self, tape: bool) -> bool;
    /// The chart strip's triple click: every bar in view; `false` when the
    /// pane cannot show that.
    fn show_live_overview(&mut self) -> bool;
    /// Centre the live bar, zoomed in when `zoom_in`; `false` when the pane
    /// cannot.
    fn centre_live(&mut self, zoom_in: bool) -> bool;
    /// The pane's framing as its last frame laid it out.
    fn framing(&self) -> ViewportSnapshot;
    /// The pane's order-flow view, when it draws a native tape.
    fn native_tape(&mut self) -> Option<&mut dyn NativeTape>;
}

/// A native tape, as `chart.tape_view.set` frames it.
pub trait NativeTape {
    /// Show `window` of market time; `file` takes the selector's path, which
    /// files the window with the asset, else the wheel's, which leaves it.
    fn move_tape_window(&mut self, window: LaneWindow, file: bool);
    /// Put the tape's right edge at `end`.
    fn move_tape_end(&mut self, end: TapeEnd);
    /// The tape's frame on the wire.
    fn tape_frame(&self) -> Option<TapeViewSnapshot>;
}
