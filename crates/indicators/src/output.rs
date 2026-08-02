//! Plot output model: what an indicator declares it draws, and the committed
//! values it has drawn.
//!
//! The plot *set* is fixed when an indicator loads (Pine's rule: `plot*` at
//! top level only) — rendering, delta events and golden fixtures all rely on
//! a stable column layout. Hiding a plot on some bars is done with `na`
//! values, never by adding or removing plots at runtime.

/// Identifies one declared plot: an index into
/// [`IndicatorDescriptor::plots`](crate::IndicatorDescriptor) and into the
/// columns of the matching [`PlotBuffer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlotId(usize);

impl PlotId {
    /// Wrap a plot index.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The plot's column index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// How a plot column is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotStyle {
    /// Connected polyline (NaN breaks it into segments).
    Line,
    /// Horizontal-then-vertical steps.
    StepLine,
    /// Thin vertical bars from zero.
    Histogram,
    /// Wide vertical bars from zero.
    Columns,
    /// A dot per bar.
    Circles,
    /// A cross marker per bar.
    Cross,
    /// Filled area between the value and zero, plus the outline.
    Area,
}

/// An RGBA color, UI-toolkit-agnostic (this crate never depends on egui).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba8 {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha (255 = opaque).
    pub a: u8,
}

impl Rgba8 {
    /// A color from its four channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// An opaque color.
    #[must_use]
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    /// Pack as `0xRRGGBBAA` — the encoding color series columns store.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        ((self.r as u32) << 24) | ((self.g as u32) << 16) | ((self.b as u32) << 8) | self.a as u32
    }

    /// Unpack from `0xRRGGBBAA`.
    #[must_use]
    pub const fn from_u32(packed: u32) -> Self {
        Self {
            r: (packed >> 24) as u8,
            g: (packed >> 16) as u8,
            b: (packed >> 8) as u8,
            a: packed as u8,
        }
    }
}

/// One declared plot: identity, label and rendering hints.
#[derive(Debug, Clone, PartialEq)]
pub struct PlotSpec {
    /// Which column this spec describes.
    pub id: PlotId,
    /// Label shown in the UI and used as the golden-fixture column header.
    pub title: String,
    /// Rendering style.
    pub style: PlotStyle,
    /// Default color (per-bar overrides come later, with dynamic colors).
    pub base_color: Rgba8,
    /// Stroke width in points.
    pub width: f32,
    /// Horizontal shift in bars (positive = rightward), Pine's `offset=`.
    pub offset: i32,
}

/// The committed plot values of one indicator: one f64 column per declared
/// plot, one row per closed bar. `NaN` cells are `na` — "nothing to draw
/// here" (warmup, conditional plots) — and renderers break lines on them.
///
/// Only *committed* rows live here; the forming bar's values travel in a
/// [`PreviewFrame`] and are never appended.
#[derive(Debug, Clone, Default)]
pub struct PlotBuffer {
    columns: Vec<Vec<f64>>,
}

impl PlotBuffer {
    /// A buffer with `plot_count` empty columns.
    #[must_use]
    pub fn new(plot_count: usize) -> Self {
        Self {
            columns: vec![Vec::new(); plot_count],
        }
    }

    /// Number of plot columns.
    #[must_use]
    pub fn plot_count(&self) -> usize {
        self.columns.len()
    }

    /// Number of committed rows (closed bars evaluated).
    #[must_use]
    pub fn len(&self) -> usize {
        self.columns.first().map_or(0, Vec::len)
    }

    /// True when no rows have been committed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append one row, one value per plot in [`PlotId`] order.
    ///
    /// # Panics
    ///
    /// Panics if `row.len()` differs from the column count — a row of the
    /// wrong shape is a bug upstream, and committing it would silently
    /// misalign every later read.
    pub fn push_row(&mut self, row: &[f64]) {
        assert_eq!(
            row.len(),
            self.columns.len(),
            "plot row has {} values but {} plots are declared",
            row.len(),
            self.columns.len()
        );
        for (column, &value) in self.columns.iter_mut().zip(row) {
            column.push(value);
        }
    }

    /// The full committed column of one plot.
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of range.
    #[must_use]
    pub fn column(&self, id: PlotId) -> &[f64] {
        &self.columns[id.index()]
    }

    /// One committed cell; NaN when `row` is out of range (an honest `na`,
    /// consistent with series history reads).
    #[must_use]
    pub fn value(&self, id: PlotId, row: usize) -> f64 {
        self.columns[id.index()]
            .get(row)
            .copied()
            .unwrap_or(f64::NAN)
    }

    /// Drop all rows, keep the column layout — ready for a full replay.
    pub fn clear(&mut self) {
        for column in &mut self.columns {
            column.clear();
        }
    }
}

/// The output of one preview run: what the forming bar would plot if it
/// closed right now. Never committed anywhere — the renderer keeps only the
/// latest frame and replaces it wholesale (latest-wins).
///
/// Extended in later milestones with transient draw objects and per-bar
/// colors for the forming bar.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewFrame {
    /// One value per declared plot, in [`PlotId`] order; NaN = nothing to
    /// draw.
    pub values: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_row_appends_across_all_columns() {
        let mut buffer = PlotBuffer::new(2);
        buffer.push_row(&[1.0, 10.0]);
        buffer.push_row(&[2.0, 20.0]);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.column(PlotId::new(0)), &[1.0, 2.0]);
        assert_eq!(buffer.column(PlotId::new(1)), &[10.0, 20.0]);
        assert_eq!(buffer.value(PlotId::new(1), 0), 10.0);
        assert!(buffer.value(PlotId::new(1), 5).is_nan());
    }

    #[test]
    #[should_panic(expected = "2 values but 3 plots")]
    fn wrong_row_shape_is_rejected_loudly() {
        let mut buffer = PlotBuffer::new(3);
        buffer.push_row(&[1.0, 2.0]);
    }

    #[test]
    fn clear_keeps_the_layout() {
        let mut buffer = PlotBuffer::new(1);
        buffer.push_row(&[1.0]);
        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.plot_count(), 1);
    }

    #[test]
    fn rgba8_round_trips_through_u32() {
        let c = Rgba8::new(0x12, 0x34, 0x56, 0x78);
        assert_eq!(c.to_u32(), 0x1234_5678);
        assert_eq!(Rgba8::from_u32(0x1234_5678), c);
        assert_eq!(Rgba8::opaque(1, 2, 3).a, 255);
    }
}
