//! Carves a tab's canvas into pixels. The model it carves from, pane identity,
//! presets, widths, shares and the split itself, is
//! `quantick_workspace::canvas_layout`; this lays its spans out as rectangles.

use eframe::egui;
pub use quantick_workspace::canvas_layout::*;
use smallvec::SmallVec;

/// One row of the canvas, carved.
///
/// `panes[i]` is the area of the `i`th pane; `dividers[i]` is the draggable
/// rule between pane `i` and pane `i + 1`, so there are always exactly one
/// fewer dividers than panes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowAreas {
    /// Left to right, as drawn.
    pub panes: SmallVec<[egui::Rect; MAX_CANVAS_PANES]>,
    /// One per seam. `dividers[i]` sits between `panes[i]` and `panes[i + 1]`.
    pub dividers: SmallVec<[egui::Rect; MAX_CANVAS_PANES]>,
}

/// Carve `area` into one pane per entry in `widths`, with a draggable divider
/// on every seam, as [`split`] spends the width. The column painter lays its
/// row out through `column_frame`; this is the row its tests carve.
#[cfg(test)]
#[must_use]
pub fn split_row(area: egui::Rect, widths: &[PaneWidth]) -> RowAreas {
    let band = |(from, to): Span| egui::Rect::from_x_y_ranges(from..=to, area.y_range());
    carve(split(area.left(), area.width(), widths), band)
}

/// Carve `area` top to bottom instead: the context column's stacked panes.
#[must_use]
pub fn split_column(area: egui::Rect, heights: &[PaneWidth]) -> RowAreas {
    let band = |(from, to): Span| egui::Rect::from_x_y_ranges(area.x_range(), from..=to);
    carve(split(area.top(), area.height(), heights), band)
}

fn carve(split: Split, band: impl Fn(Span) -> egui::Rect) -> RowAreas {
    RowAreas {
        panes: split.panes().iter().map(|span| band(*span)).collect(),
        dividers: split.dividers().iter().map(|span| band(*span)).collect(),
    }
}

#[cfg(test)]
mod row_tests;

#[cfg(test)]
mod registry_tests;

#[cfg(test)]
mod column_tests;

#[cfg(test)]
mod collapse_tests;
