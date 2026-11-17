//! A chart pane's marks, as `annotate.drawing_magnet.set` and
//! `annotate.drawing_scope.set` act on them ([`crate::drawing_magnet`]):
//! the window resolves the addressed pane
//! ([`crate::addressed_pane::AddressedPanePort`]) and the calls change the same
//! marks the context bar does.
use quantick_drawings::{DrawingTool, Drawings};
use quantick_engine::bar_registry::BarConfiguration;

/// A chart pane's marks and the bar rule they are cut to. Its names differ
/// from the pane's own methods, so an implementation that forwards to one
/// cannot fall back on itself.
pub trait PaneMarks {
    /// The pane's marks.
    fn marks(&mut self) -> &mut Drawings<DrawingTool>;
    /// The bar rule the pane's marks are cut to.
    fn marks_bars(&self) -> BarConfiguration;
}
