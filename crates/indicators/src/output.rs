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

/// Resolve a stack of bar paint requests — front of the stack first — into
/// the one colour a candle wears.
///
/// **The last request wins.** Indicators are held in insertion order, which
/// is render order: the overlay added later already draws over the ones added
/// before it, so paint obeys the rule the chart already has. Where the later
/// indicator asks for nothing on *this* bar, the one below it still shows
/// through — the resolution is per bar, not per chart.
///
/// One function rather than one rule per consumer: the host answers for the
/// backtest and the bot, the chart answers from its own mirror of the same
/// indicators, and a bot that saw a different colour than the trader would be
/// a quiet lie. Both go through here.
pub fn resolve_bar_paint<I>(stack: I) -> Option<Rgba8>
where
    I: IntoIterator<Item = Option<Rgba8>>,
    I::IntoIter: DoubleEndedIterator,
{
    stack.into_iter().rev().find_map(|paint| paint)
}

/// A marker shape for `plotshape`/`plotchar` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerShape {
    /// `shape.triangleup`
    TriangleUp,
    /// `shape.triangledown`
    TriangleDown,
    /// `shape.circle`
    Circle,
    /// `shape.labelup` (a small up-pointing tag)
    LabelUp,
    /// `shape.labeldown`
    LabelDown,
    /// `shape.cross`
    Cross,
}

/// Where a marker anchors vertically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerLocation {
    /// Just above the bar's high.
    AboveBar,
    /// Just below the bar's low.
    BelowBar,
    /// At the plotted value itself.
    Absolute,
}

/// Marker rendering for a `plotshape`/`plotchar` column: the column's cells
/// are na (draw nothing) or a value (draw the marker; for
/// [`MarkerLocation::Absolute`] the value is the y).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerSpec {
    /// What to draw.
    pub shape: MarkerShape,
    /// Where to anchor it.
    pub location: MarkerLocation,
    /// Optional text under/over the marker (`plotshape(text=…)`, or the
    /// single character of `plotchar`).
    pub text: Option<String>,
}

/// A fill between two plot columns (`fill(p1, p2, color)`): drawn behind
/// both plots wherever the two cells are non-na.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillSpec {
    /// One side of the band.
    pub a: PlotId,
    /// The other side.
    pub b: PlotId,
    /// The band color (usually translucent).
    pub color: Rgba8,
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
    /// `Some`: this column renders as markers (`plotshape`/`plotchar`)
    /// instead of through its [`PlotStyle`].
    pub marker: Option<MarkerSpec>,
}

/// The committed plot values of one indicator: one f64 column per declared
/// plot, one row per closed bar. `NaN` cells are `na` — "nothing to draw
/// here" (warmup, conditional plots) — and renderers break lines on them.
///
/// Alongside the plot columns sits the **bar paint channel**: the candle
/// colour an indicator asks the chart to paint each bar with (Pine's
/// `barcolor()`). It lives here, and not in a store of its own, because it is
/// the same shape as a plot column — one value per committed bar, produced by
/// the same commit run — and a second store would need its own alignment
/// rules, its own clear, and its own way to be wrong.
///
/// Only *committed* rows live here; the forming bar's values travel in a
/// [`PreviewFrame`] and are never appended.
#[derive(Debug, Clone, Default)]
pub struct PlotBuffer {
    columns: Vec<Vec<f64>>,
    /// Packed `0xRRGGBBAA` paint per bar, `0` = this bar asked for none.
    ///
    /// Kept only as far as the last *painted* bar: an indicator that never
    /// paints — every native one today — leaves it empty and pays a `Vec`
    /// header, no allocation. Reads past the end answer "no paint", the same
    /// honest out-of-range answer [`PlotBuffer::value`] gives.
    ///
    /// Fully transparent black packs to `0` and so collapses into "no paint".
    /// Nothing renderable is lost: neither puts a pixel on screen.
    bar_paint: Vec<u32>,
    /// Tracked explicitly rather than derived from a column: an indicator
    /// with zero plots (a draw-objects-only script) still commits rows.
    rows: usize,
}

impl PlotBuffer {
    /// A buffer with `plot_count` empty columns.
    ///
    /// Prefer [`PlotBuffer::for_plots`] when the declared specs are in hand:
    /// it takes the column count from them instead of asking the caller to
    /// keep two numbers in step.
    #[must_use]
    pub fn new(plot_count: usize) -> Self {
        Self {
            columns: vec![Vec::new(); plot_count],
            bar_paint: Vec::new(),
            rows: 0,
        }
    }

    /// A buffer sized from the plots an indicator declares.
    ///
    /// [`PlotSpec::id`] is defined as the spec's own position in
    /// [`IndicatorDescriptor::plots`](crate::IndicatorDescriptor), and readers
    /// rely on the two agreeing: the golden writer takes column headers from
    /// iteration order and values from `spec.id`. Building the buffer from the
    /// specs is what keeps that true.
    ///
    /// # Panics
    ///
    /// Panics if any spec's id differs from its position — the descriptor is
    /// built once at load time, so a mismatch is a bug in the indicator, and
    /// letting it through would mislabel every column downstream.
    #[must_use]
    pub fn for_plots(plots: &[PlotSpec]) -> Self {
        for (index, spec) in plots.iter().enumerate() {
            assert_eq!(
                spec.id,
                PlotId::new(index),
                "plot {:?} declares id {:?} but sits at position {index}",
                spec.title,
                spec.id
            );
        }
        Self::new(plots.len())
    }

    /// Number of plot columns.
    #[must_use]
    pub fn plot_count(&self) -> usize {
        self.columns.len()
    }

    /// Number of committed rows (closed bars evaluated).
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows
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
        self.rows += 1;
    }

    /// Append one row together with the candle paint that bar asks for.
    ///
    /// `None` — the common case — costs nothing: the paint channel is only
    /// extended when a bar actually asks for a colour, and the bars skipped
    /// in between are filled with "no paint" at that moment.
    ///
    /// # Panics
    ///
    /// Same as [`push_row`](PlotBuffer::push_row).
    pub fn push_row_painted(&mut self, row: &[f64], paint: Option<Rgba8>) {
        self.push_row(row);
        if let Some(color) = paint {
            // Every bar between the previous painted one and this one asked
            // for nothing; `0` is that answer, written only now so an
            // indicator that never paints never allocates here.
            self.bar_paint.resize(self.rows - 1, 0);
            self.bar_paint.push(color.to_u32());
        }
    }

    /// The candle paint one committed bar asked for, if any. Out of range —
    /// past the last painted bar, or past history entirely — is `None`.
    #[must_use]
    pub fn bar_paint(&self, row: usize) -> Option<Rgba8> {
        match self.bar_paint.get(row).copied() {
            None | Some(0) => None,
            Some(packed) => Some(Rgba8::from_u32(packed)),
        }
    }

    /// Whether this indicator has painted any bar at all — the cheap check a
    /// renderer uses to skip the channel entirely for the indicators (all the
    /// native ones today) that never touch it.
    #[must_use]
    pub fn paints_any(&self) -> bool {
        !self.bar_paint.is_empty()
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
        self.bar_paint.clear();
        self.rows = 0;
    }
}

/// The output of one preview run: what the forming bar would plot if it
/// closed right now. Never committed anywhere — the renderer keeps only the
/// latest frame and replaces it wholesale (latest-wins).
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewFrame {
    /// One value per declared plot, in [`PlotId`] order; NaN = nothing to
    /// draw.
    pub values: Vec<f64>,
    /// The forming bar's draw objects (the full transient set), when the
    /// indicator draws any: replaces the committed set while the bar forms,
    /// exactly like plot previews (latest-wins).
    pub objects: Option<crate::objects::ObjectSnapshot>,
    /// The candle paint the forming bar asks for, `None` = none.
    ///
    /// As transient as the rest of the frame: it is what the bar would commit
    /// if it closed on this print, it is replaced wholesale by the next
    /// preview, and it never reaches the committed channel. A bar that reads
    /// as exhaustion at one moment and as plain force at the next therefore
    /// changes colour on screen while it forms — the preview is honest about
    /// the state of an unfinished bar, not a promise about the closed one.
    pub paint: Option<Rgba8>,
}

impl PreviewFrame {
    /// A frame of plot values with no draw objects and no paint.
    #[must_use]
    pub fn new(values: Vec<f64>) -> Self {
        Self {
            values,
            objects: None,
            paint: None,
        }
    }
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
    fn zero_plot_indicators_still_count_rows() {
        let mut buffer = PlotBuffer::new(0);
        buffer.push_row(&[]);
        buffer.push_row(&[]);
        assert_eq!(buffer.len(), 2, "rows exist even with no plot columns");
        buffer.clear();
        assert!(buffer.is_empty());
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
    fn an_indicator_that_never_paints_carries_no_paint_channel() {
        let mut buffer = PlotBuffer::new(1);
        buffer.push_row(&[1.0]);
        buffer.push_row(&[2.0]);
        assert!(!buffer.paints_any(), "no bar asked for paint");
        assert_eq!(buffer.bar_paint(0), None);
        assert_eq!(buffer.bar_paint(1), None);
    }

    #[test]
    fn paint_lands_on_its_own_bar_and_the_gaps_read_as_none() {
        let red = Rgba8::opaque(255, 0, 0);
        let mut buffer = PlotBuffer::new(1);
        buffer.push_row_painted(&[1.0], None);
        buffer.push_row_painted(&[2.0], Some(red));
        buffer.push_row_painted(&[3.0], None);
        assert_eq!(buffer.len(), 3);
        assert!(buffer.paints_any());
        assert_eq!(buffer.bar_paint(0), None, "before the first painted bar");
        assert_eq!(buffer.bar_paint(1), Some(red));
        assert_eq!(buffer.bar_paint(2), None, "after it, unpainted again");
        assert_eq!(buffer.bar_paint(99), None, "past history entirely");
    }

    #[test]
    fn painting_leaves_the_plot_columns_alone() {
        // The channel rides beside the values; it must not shift them.
        let mut buffer = PlotBuffer::new(2);
        buffer.push_row_painted(&[1.0, 10.0], Some(Rgba8::opaque(0, 255, 0)));
        buffer.push_row_painted(&[2.0, 20.0], None);
        assert_eq!(buffer.column(PlotId::new(0)), &[1.0, 2.0]);
        assert_eq!(buffer.column(PlotId::new(1)), &[10.0, 20.0]);
    }

    #[test]
    fn clear_drops_the_paint_channel_too() {
        let mut buffer = PlotBuffer::new(1);
        buffer.push_row_painted(&[1.0], Some(Rgba8::opaque(255, 0, 0)));
        buffer.clear();
        assert!(!buffer.paints_any(), "a replay starts unpainted");
        assert_eq!(buffer.bar_paint(0), None);
    }

    #[test]
    fn fully_transparent_paint_reads_as_no_paint() {
        // Both put zero pixels on screen, so collapsing them loses nothing —
        // and it is what keeps the channel four bytes a bar.
        let mut buffer = PlotBuffer::new(1);
        buffer.push_row_painted(&[1.0], Some(Rgba8::new(0, 0, 0, 0)));
        assert_eq!(buffer.bar_paint(0), None);
    }

    #[test]
    fn a_fresh_preview_frame_asks_for_no_paint() {
        assert_eq!(PreviewFrame::new(vec![1.0]).paint, None);
    }

    #[test]
    fn the_last_request_in_the_stack_wins() {
        let red = Rgba8::opaque(255, 0, 0);
        let blue = Rgba8::opaque(0, 0, 255);
        assert_eq!(resolve_bar_paint([Some(red), Some(blue)]), Some(blue));
        assert_eq!(
            resolve_bar_paint([Some(red), None]),
            Some(red),
            "the top of the stack asking for nothing uncovers the one below"
        );
        assert_eq!(resolve_bar_paint([None, None]), None);
        assert_eq!(resolve_bar_paint(Vec::new()), None, "an empty chart");
    }

    fn spec(index: usize, title: &str) -> PlotSpec {
        PlotSpec {
            id: PlotId::new(index),
            title: title.to_owned(),
            style: PlotStyle::Line,
            base_color: Rgba8::opaque(255, 255, 255),
            width: 1.0,
            offset: 0,
            marker: None,
        }
    }

    #[test]
    fn for_plots_sizes_the_buffer_from_the_declared_specs() {
        let plots = vec![spec(0, "upper"), spec(1, "lower")];
        let buffer = PlotBuffer::for_plots(&plots);
        assert_eq!(buffer.plot_count(), plots.len());
    }

    #[test]
    #[should_panic(expected = "sits at position")]
    fn for_plots_rejects_an_id_that_disagrees_with_its_position() {
        // The golden writer takes headers from iteration order and values from
        // `spec.id`; a disagreement would label a column with another's title.
        let plots = vec![spec(0, "upper"), spec(0, "lower")];
        let _ = PlotBuffer::for_plots(&plots);
    }

    #[test]
    fn rgba8_round_trips_through_u32() {
        let c = Rgba8::new(0x12, 0x34, 0x56, 0x78);
        assert_eq!(c.to_u32(), 0x1234_5678);
        assert_eq!(Rgba8::from_u32(0x1234_5678), c);
        assert_eq!(Rgba8::opaque(1, 2, 3).a, 255);
    }
}
