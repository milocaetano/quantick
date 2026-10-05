//! The desk's plain geometry: where a price is, and what is under the pointer.
//!
//! Every rectangle a press has to agree with is computed here, from plain
//! numbers, so the chart's paint and the desk's hit-test ask one function the
//! same question and get the same answer: the ✕ is pressable exactly while it
//! is painted. The host turns its toolkit's points and rectangles into
//! [`Point`] and [`Bounds`] at the boundary and back again to draw; nothing
//! here knows a toolkit exists.
//!
//! The arithmetic mirrors the toolkit's own (`min + size`, `(min + max) / 2`,
//! inclusive containment) operation for operation, so a rectangle that makes
//! the round trip lands on the very same floats it would have had.

/// A screen position, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    /// A point at `(x, y)`.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned rectangle, in pixels: `min` is the top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bounds {
    pub min: Point,
    pub max: Point,
}

impl Bounds {
    /// The rectangle between two corners, taken as given.
    #[must_use]
    pub const fn from_min_max(min: Point, max: Point) -> Self {
        Self { min, max }
    }

    /// The rectangle `width` by `height` with its top-left corner at `min`.
    #[must_use]
    pub fn from_min_size(min: Point, width: f32, height: f32) -> Self {
        Self {
            min,
            max: Point::new(min.x + width, min.y + height),
        }
    }

    #[must_use]
    pub fn left(&self) -> f32 {
        self.min.x
    }

    #[must_use]
    pub fn right(&self) -> f32 {
        self.max.x
    }

    #[must_use]
    pub fn top(&self) -> f32 {
        self.min.y
    }

    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.max.y
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }

    /// Whether `point` is inside, edges included.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        self.min.x <= point.x
            && point.x <= self.max.x
            && self.min.y <= point.y
            && point.y <= self.max.y
    }
}

/// How a price meets the screen, as the chart's own scale states it.
///
/// A trait rather than a copy of the scale: price-to-pixel is one law, owned
/// by the chart, and a second implementation here would be a second answer
/// to "where is this price" that could drift from the first.
pub trait PriceAxis {
    /// The pixel row of `price`.
    fn y(&self, price: f64) -> f32;
    /// The price at pixel row `y`.
    fn price_at(&self, y: f32) -> f64;
    /// Whether low prices ride at the top of the pane.
    fn is_inverted(&self) -> bool;
}

/// Grab distance for order lines — the drawings' select radius, so the two
/// grammars feel identical under the pointer.
pub const LINE_GRAB_RADIUS_PX: f32 = 10.0;
/// Height of an in-plot tag (fits mono 11 plus its padding).
pub const TAG_HEIGHT_PX: f32 = 20.0;
/// Gap between a tag's right edge and the plot's right edge — the inside
/// mirror of the gutter chips' `AXIS_LABEL_GAP_PX`.
pub const TAG_GAP_PX: f32 = 6.0;
/// Width of the ✕ zone a hovered tag reveals. An overlay convenience —
/// every action here has a ≥ 28 px twin in the chrome.
pub const TAG_BUTTON_PX: f32 = 20.0;
/// How far around a tag the hover that reveals the bracket handles still
/// counts.
pub const TAG_HOVER_SLACK_PX: f32 = 4.0;
/// Width of a labelled SL/TP bracket handle on the entry line.
pub const HANDLE_WIDTH_PX: f32 = 20.0;
/// Height of a labelled SL/TP bracket handle on the entry line.
pub const HANDLE_HEIGHT_PX: f32 = 14.0;
/// Vertical clearance between the entry line and a bracket handle — past
/// the tag's half height, so handle and tag never overlap.
pub const HANDLE_CLEAR_PX: f32 = 12.0;
/// How far (in pixels) a press on the entry line must travel before it
/// commits to creating one bracket leg — the drawings' drag threshold.
pub const CREATE_DECIDE_THRESHOLD_PX: f32 = 4.0;
/// Vertical clearance between a paper chip's centre and the last-price
/// chip's, in pixels — just over one chip height, so the two can never
/// overprint. At the instant a market order fills, the entry price *is* the
/// last price, and without this the one persistent "you are long" statement
/// is born unreadable.
pub const CHIP_CLEAR_PX: f32 = 16.0;

/// A tag's vertical center: the line's row, kept fully inside the plot.
#[must_use]
pub fn clamp_tag_center(y: f32, top: f32, bottom: f32) -> f32 {
    let half = TAG_HEIGHT_PX / 2.0;
    // A band too short to hold a tag has nothing to clamp into, and
    // `f32::clamp` does not merely saturate there — it panics outright the
    // moment its bounds cross. That band is reachable: `plot_split` floors
    // the plot at 20 px, but `indicators::split_panes` then carves the
    // indicator strips out of it with no floor of its own, so a squeezed
    // window with enough panes really does leave the candles a few pixels.
    // Centre the tag in what there is rather than taking a live session
    // down.
    if bottom - top <= TAG_HEIGHT_PX {
        return f32::midpoint(top, bottom);
    }
    y.clamp(top + half, bottom - half)
}

/// The ✕ zone every closable tag reserves at its right edge — a fixed
/// position derivable without measuring text, which is what lets the
/// paint and the press-time geometric hit-test share one truth.
#[must_use]
pub fn close_button_rect(tag_right: f32, center_y: f32) -> Bounds {
    let right = tag_right - TAG_GAP_PX;
    Bounds::from_min_max(
        Point::new(right - TAG_BUTTON_PX, center_y - TAG_HEIGHT_PX / 2.0),
        Point::new(right, center_y + TAG_HEIGHT_PX / 2.0),
    )
}

/// Whether a bracket owner's `SL`/`TP` handles paint this frame.
///
/// The pointer clause is the one that is easy to miss. A pane that is not
/// feeding paper input has no pointer here, and `reveal` can still be true
/// over there — an order's tag opens on *every* pane at once, by design, so
/// one hover reads on both charts. Without the clause the other pane drew a
/// pressable-looking handle beside an order whose presses it does not take:
/// the exact inversion of the layer rule that an invisible control is not a
/// control, and no better.
#[must_use]
pub fn handles_visible(pointer: Option<Point>, reveal: bool, over_handle: bool) -> bool {
    pointer.is_some() && (reveal || over_handle)
}

/// A bracket handle's rect: the ✕ column, one clear step above or below
/// the entry line so it never overlaps the position tag between them.
#[must_use]
pub fn bracket_handle_rect(tag_right: f32, entry_y: f32, above: bool) -> Bounds {
    let right = tag_right - TAG_GAP_PX;
    let y = if above {
        entry_y - HANDLE_CLEAR_PX - HANDLE_HEIGHT_PX
    } else {
        entry_y + HANDLE_CLEAR_PX
    };
    Bounds::from_min_size(
        Point::new(right - HANDLE_WIDTH_PX, y),
        HANDLE_WIDTH_PX,
        HANDLE_HEIGHT_PX,
    )
}

/// Keep a gutter chip legible when it would land on the last-price chip:
/// push it just clear of the reserved row, towards its own side of the
/// price, clamped into the pane. At the exact fill price (no distance at
/// all) the chip steps down, below the last-price chip. When the reserved
/// row itself hugs a pane edge the clamp can land the chip back inside the
/// band — accepted: a chip pinned at the edge beats one pushed out of the
/// pane, and the next print separates them.
#[must_use]
pub fn dodged_chip_y(y: f32, reserved: Option<f32>, top: f32, bottom: f32) -> f32 {
    let Some(reserved) = reserved else {
        return y;
    };
    let delta = y - reserved;
    if delta.abs() >= CHIP_CLEAR_PX {
        return y;
    }
    let dodged = if delta >= 0.0 {
        reserved + CHIP_CLEAR_PX
    } else {
        reserved - CHIP_CLEAR_PX
    };
    dodged.clamp(top, bottom)
}

/// The row around a line where its in-plot tag counts as hovered: the
/// line's own grab band, plus the row a tag was clamped into near a chart
/// edge (where the two part company). Text-free geometry on purpose — a
/// press-time hit-test has no painter to measure a galley with, and the
/// paint must be able to ask the same question.
#[must_use]
pub fn tag_row_hit(pointer: Point, y: f32, chart: Bounds) -> bool {
    if !chart.contains(pointer) {
        return false;
    }
    let center = clamp_tag_center(y, chart.top(), chart.bottom());
    (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        || (pointer.y - center).abs() <= TAG_HEIGHT_PX / 2.0 + TAG_HOVER_SLACK_PX
}
