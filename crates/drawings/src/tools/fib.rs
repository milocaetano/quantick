//! Fib is a level model, not seven fixed lines.
//!
//! Retracement and extension share this one module: the editable level list,
//! the built-in presets, the linear and log formulas, band fills, label
//! formatting (cached — labels are re-formatted only when an anchor price or
//! the configuration changes, never per frame) and the level-editor tab both
//! tools mount in the inspector. Only the geometry and the price formula
//! differ between the two kinds.

use crate::{
    Align2, FontSpec, MarkScene, MarkShape, Px, PxRect, RgbaExt, SELECTED_ANCHOR_RADIUS_PX, Stroke,
    pos2, rgba_unmultiplied,
};
use quantick_chart::style::PremultipliedRgba;
use std::any::Any;
use std::cell::{Ref, RefCell};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::{
    DrawContext, DrawingPayload, DrawingStyle, FIB_LABEL_OFFSET_PX, FIB_LABEL_SIZE_PX, IconDots,
    IconLetter, IconStrokes, ToolFamily, distance_to_segment, drawing_stroke,
};

/// Centre-x of the letter in both icons — one column, so on two stacked rail
/// buttons the eye finds the letter in the same place and only the letter
/// changes.
const FIB_LETTER_X: f32 = 0.20;
/// Height of the letter in both icons, as a fraction of the glyph box.
/// Large enough to read at rail size — at 0.46, and still at 0.70, the
/// trader could not make the letters out. Shared for the same reason: an `R` and a `P` at one size are a choice
/// between two tools, while two letters at two sizes read as two unrelated
/// marks.
const FIB_LETTER_HEIGHT: f32 = 0.90;
/// The leg the retracement was dragged across, named once: the strokes draw
/// it and the dots mark its ends, so nudging the glyph cannot leave the
/// anchors behind on the old line.
///
/// Its two ends are the 0 and the 1 of the level list — that is why they
/// carry a dot and no line of their own.
const RETRACEMENT_LEG: &[(f32, f32)] = &[(0.62, 0.92), (0.92, 0.30)];

/// The retracement icon, drawn as the gesture is drawn on a chart: the leg
/// across the move, its ends marked, and the levels hanging **between** those
/// ends — inside the move, which is exactly what a retracement measures.
///
/// The levels' side of the leg is the whole difference from
/// [`FIB_EXTENSION_ICON`], where the same lines sit *beyond* the move. Both
/// then say in one picture what the tool does with the leg it was given, and
/// the letter in the corner names it for the trader who would rather read
/// than compare.
pub const FIB_RETRACEMENT_ICON: IconStrokes = &[
    &[(0.66, 0.42), (0.98, 0.42)],
    &[(0.66, 0.58), (0.98, 0.58)],
    &[(0.66, 0.74), (0.98, 0.74)],
    RETRACEMENT_LEG,
];

/// The two ends of the leg the retracement is dragged across. They carry no
/// line of their own because they *are* 0 and 1, and every level the icon
/// draws hangs between them.
pub const FIB_RETRACEMENT_DOTS: IconDots = RETRACEMENT_LEG;

/// `R` for retracement — the half of the pair that measures *back into* a
/// move already made. It takes the top-left corner because that is the one
/// this icon's own drawing leaves empty.
pub const FIB_RETRACEMENT_LETTER: IconLetter = IconLetter {
    text: "R",
    at: (FIB_LETTER_X, 0.33),
    height: FIB_LETTER_HEIGHT,
};

/// The extension's leg, named once for the same reason as
/// [`RETRACEMENT_LEG`]: the one move the trader drags, whose far end is the
/// origin the projection hangs from.
const EXTENSION_LEG: &[(f32, f32)] = &[(0.62, 0.95), (0.94, 0.72)];

/// The extension icon: the same rungs as the retracement, moved **above** the
/// leg — price the move has not reached yet, which is what a projection is.
/// The leg under them is the gesture, the same drag the retracement takes;
/// the rungs' side of it is the whole difference.
///
/// Neither icon reaches into the box's top-right corner: a pinned tool wears
/// the favorites star there (`FAVORITE_BADGE_INSET_PX`), and an anchor under
/// a star is an anchor the trader cannot see.
pub const FIB_EXTENSION_ICON: IconStrokes = &[
    &[(0.66, 0.30), (0.98, 0.30)],
    &[(0.66, 0.43), (0.98, 0.43)],
    &[(0.66, 0.56), (0.98, 0.56)],
    EXTENSION_LEG,
];

/// The two anchors of the extension — where the move started and where it
/// ended, which is also where the projection starts.
pub const FIB_EXTENSION_DOTS: IconDots = EXTENSION_LEG;

/// `P` for projection, not `E` for extension: the letter has to say what the
/// tool does at a glance, and what this one does is throw the measured leg
/// *forward*, past the move, onto price that has not traded yet. `E` beside
/// `R` names the menu entry; `P` beside `R` names the difference. It takes
/// the bottom-left corner — the empty one here, since this icon's levels are
/// at the top.
pub const FIB_EXTENSION_LETTER: IconLetter = IconLetter {
    text: "P",
    at: (FIB_LETTER_X, 0.67),
    height: FIB_LETTER_HEIGHT,
};

/// The one rail family both Fib tools declare, shared here so the two
/// members can never drift onto different family ids.
pub(super) const FIB_FAMILY: ToolFamily = ToolFamily {
    id: "fib",
    title: "Fibonacci",
    icon: crate::icons::ROWS,
    icon_strokes: FIB_RETRACEMENT_ICON,
    icon_dots: FIB_RETRACEMENT_DOTS,
    icon_letter: Some(FIB_RETRACEMENT_LETTER),
};

/// Ratios closer than this are the same level; a duplicate is rejected.
pub const RATIO_DUPLICATE_TOLERANCE: f64 = 0.000_001;
/// Longest accepted custom level label.
pub const MAX_LEVEL_LABEL_LEN: usize = 40;
/// Upper bound of the band-fill alpha (~60% opacity, the spec's slider cap).
pub const MAX_BAND_ALPHA: u8 = 153;
/// Default band-fill alpha (~9%, inside the spec's recommended 8-18%).
pub const DEFAULT_BAND_ALPHA: u8 = 24;
/// Dash geometry of the anchor guides: shown while a retracement is
/// selected, and always on an extension.
const GUIDE_DASH_PX: f32 = 4.0;
/// Anchors of an extension saved before it took two: impulse, pullback and
/// a separate projection origin C. Such an object still loads and projects
/// from its own C; no new object is placed with three.
pub const LEGACY_EXTENSION_POINTS: usize = 3;
/// Inline capacity of the per-frame level projection — one slot above the
/// largest built-in preset (Extended, 11 levels), so no shipped preset ever
/// touches the heap while painting.
const PROJECTED_LEVELS_INLINE: usize = 12;
/// Version stamp of the exported preset format.
const PRESET_FORMAT_VERSION: u32 = 1;

/// Which price formula the object projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FibKind {
    /// Two anchors; levels give back part of the A→B move:
    /// `P(r) = PA + (PB − PA)·r`.
    Retracement,
    /// Two anchors; the A→B move is projected forward from B:
    /// `P(r) = PB + (PB − PA)·r`. A legacy object holding a third anchor C
    /// projects from C instead: `P(r) = PC + (PB − PA)·r`.
    Extension,
}

impl FibKind {
    #[must_use]
    pub fn required_points(self) -> usize {
        2
    }

    /// Whether an object of this kind may hold `count` anchors: the two a
    /// new one completes with, or the legacy three of an extension.
    #[must_use]
    pub fn accepts_points(self, count: usize) -> bool {
        count == self.required_points()
            || (self == Self::Extension && count == LEGACY_EXTENSION_POINTS)
    }
}

/// How a level's text is composed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LabelMode {
    #[default]
    PercentPrice,
    RatioPrice,
    PriceOnly,
}

impl LabelMode {
    pub const ALL: [Self; 3] = [Self::PercentPrice, Self::RatioPrice, Self::PriceOnly];

    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::PercentPrice => "Percent + price",
            Self::RatioPrice => "Ratio + price",
            Self::PriceOnly => "Price only",
        }
    }
}

/// Where the labels sit relative to the level span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LabelPosition {
    #[default]
    RightOutside,
    RightInside,
    Left,
}

impl LabelPosition {
    pub const ALL: [Self; 3] = [Self::RightOutside, Self::RightInside, Self::Left];

    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::RightOutside => "Right - outside",
            Self::RightInside => "Right - inside",
            Self::Left => "Left",
        }
    }
}

/// How far the level lines run horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Extend {
    /// Between the anchors and nowhere else — what a fresh retracement opens
    /// with, so the levels end where the trader stopped dragging.
    Anchors,
    /// Forward from the *last* anchor only — what a fresh extension opens
    /// with (see [`Extend::for_kind`], which is where a new object's span
    /// is decided; this `Default` only answers for a value nobody set).
    ///
    /// A Fib level is something price is going to meet, so it has to be drawn
    /// where price is — to the right of the swing that defined it. Levels
    /// that stop at the anchors vanish behind the tape the moment the market
    /// moves on, and an extension whose targets sit to the *left* of the leg
    /// they project is backwards on its face.
    ///
    /// Starting at the last anchor rather than the first also keeps the
    /// projection off the leg it was measured from: the rays say "from here
    /// on", which is what a target is.
    #[default]
    Forward,
    /// The whole anchor span *and* the projection onward.
    Right,
    Both,
}

impl Extend {
    pub const ALL: [Self; 4] = [Self::Anchors, Self::Forward, Self::Right, Self::Both];

    /// The span a fresh object of each kind opens with.
    ///
    /// **A retracement lives between its own anchors.** It measures how much
    /// of one move was given back, so its lines belong to that move: the
    /// trader drags from the swing low to the swing high and the levels fill
    /// in the leg under the pointer, ending where the pointer is. Reported
    /// twice from the running build — first opening at the *last* anchor,
    /// which put every line on the far side of the cursor during the very
    /// drag that defines them, then running on to the right edge, which is
    /// the trader's own words: "a linha deve ir até o ponto onde eu tô
    /// traçando e não ir para o infinito". Extending it is a choice on the
    /// Levels tab, and one that can now be saved as the default.
    ///
    /// **Extension keeps projecting from its last anchor.** Its levels are
    /// where the next leg may reach — a target, ahead of the move — so
    /// drawing them back over the leg they were measured from is backwards
    /// on its face.
    #[must_use]
    pub fn for_kind(kind: FibKind) -> Self {
        match kind {
            FibKind::Retracement => Self::Anchors,
            FibKind::Extension => Self::Forward,
        }
    }

    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Anchors => "Between anchors",
            Self::Forward => "From the last point",
            Self::Right => "Anchors + right",
            Self::Both => "Both sides",
        }
    }
}

/// One configurable level of a Fib object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FibLevelSpec {
    pub ratio: f64,
    /// Custom text; empty means the automatic ratio label.
    pub label: String,
    pub visible: bool,
    /// Per-level colour override; `None` inherits the object's colour.
    pub color: Option<[u8; 3]>,
    /// Fill from this level to the next visible one.
    pub fill_to_next: bool,
}

impl FibLevelSpec {
    #[must_use]
    pub fn new(ratio: f64) -> Self {
        Self {
            ratio,
            label: String::new(),
            visible: true,
            color: None,
            fill_to_next: false,
        }
    }
}

/// A named built-in ratio set. Built-ins are read-only; custom presets go
/// through the [`PresetHost`].
pub struct FibPreset {
    pub name: &'static str,
    pub ratios: &'static [f64],
}

pub const RETRACEMENT_PRESETS: [FibPreset; 4] = [
    FibPreset {
        name: "Standard",
        ratios: &[0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0],
    },
    FibPreset {
        name: "Compact",
        ratios: &[0.0, 0.382, 0.5, 0.618, 1.0],
    },
    FibPreset {
        name: "Deep",
        ratios: &[0.0, 0.236, 0.382, 0.5, 0.618, 0.705, 0.786, 0.886, 1.0],
    },
    FibPreset {
        name: "Extended",
        ratios: &[
            -0.618, -0.272, 0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0, 1.272, 1.618,
        ],
    },
];

pub const EXTENSION_PRESETS: [FibPreset; 3] = [
    FibPreset {
        name: "Targets",
        ratios: &[0.618, 1.0, 1.272, 1.618, 2.0, 2.618],
    },
    FibPreset {
        name: "Conservative",
        ratios: &[0.618, 1.0, 1.272, 1.618],
    },
    FibPreset {
        name: "Expanded",
        ratios: &[0.382, 0.618, 1.0, 1.272, 1.618, 2.0, 2.618, 4.236],
    },
];

#[must_use]
pub fn builtin_presets(kind: FibKind) -> &'static [FibPreset] {
    match kind {
        FibKind::Retracement => &RETRACEMENT_PRESETS,
        FibKind::Extension => &EXTENSION_PRESETS,
    }
}

/// Formatted level labels, rebuilt only when anchors or config change.
#[derive(Debug, Clone, Default, PartialEq)]
struct LabelCache {
    key: (u64, u64, u64, u64),
    labels: Vec<String>,
}

/// The tool-owned state of one Fib object.
#[derive(Debug)]
pub struct FibPayload {
    pub kind: FibKind,
    pub levels: Vec<FibLevelSpec>,
    pub band_alpha: u8,
    /// Explicit scale choice: log never follows the chart silently, and it
    /// only ever computes over positive prices.
    pub log_scale: bool,
    pub label_mode: LabelMode,
    pub label_position: LabelPosition,
    pub extend: Extend,
    /// Bumped by the editor so the label cache notices config edits.
    revision: u64,
    cache: RefCell<LabelCache>,
}

impl Clone for FibPayload {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            levels: self.levels.clone(),
            band_alpha: self.band_alpha,
            log_scale: self.log_scale,
            label_mode: self.label_mode,
            label_position: self.label_position,
            extend: self.extend,
            revision: self.revision,
            // The cache is memoization, not state: a clone (undo snapshot,
            // inspector pre-edit copy) starts cold instead of copying
            // formatted strings around.
            cache: RefCell::new(LabelCache::default()),
        }
    }
}

impl PartialEq for FibPayload {
    fn eq(&self, other: &Self) -> bool {
        // The cache and its revision are memoization, not state.
        self.kind == other.kind
            && self.levels == other.levels
            && self.band_alpha == other.band_alpha
            && self.log_scale == other.log_scale
            && self.label_mode == other.label_mode
            && self.label_position == other.label_position
            && self.extend == other.extend
    }
}

impl FibPayload {
    #[must_use]
    pub fn new(kind: FibKind) -> Self {
        let standard = builtin_presets(kind)[0].ratios;
        Self {
            kind,
            levels: standard
                .iter()
                .map(|&ratio| FibLevelSpec::new(ratio))
                .collect(),
            band_alpha: DEFAULT_BAND_ALPHA,
            log_scale: false,
            label_mode: LabelMode::default(),
            label_position: LabelPosition::default(),
            extend: Extend::for_kind(kind),
            revision: 0,
            cache: RefCell::new(LabelCache::default()),
        }
    }

    pub fn apply_preset(&mut self, preset: &FibPreset) {
        self.levels = preset
            .ratios
            .iter()
            .map(|&ratio| FibLevelSpec::new(ratio))
            .collect();
        self.touch();
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Whether `ratio` would duplicate an existing level (other than `skip`).
    #[must_use]
    pub fn is_duplicate(&self, ratio: f64, skip: Option<usize>) -> bool {
        self.levels.iter().enumerate().any(|(index, level)| {
            Some(index) != skip && (level.ratio - ratio).abs() < RATIO_DUPLICATE_TOLERANCE
        })
    }

    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.levels.iter().filter(|level| level.visible).count()
    }
}

/// The versioned on-disk shape of a saved preset. Coordinates, name, lock
/// and visibility never travel with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FibPresetData {
    version: u32,
    kind: FibKind,
    levels: Vec<FibLevelSpec>,
    band_alpha: u8,
    label_mode: LabelMode,
    label_position: LabelPosition,
    extend: Extend,
}

impl DrawingPayload for FibPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(self.clone())
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self == other)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn export_preset(&self) -> Option<toml::Value> {
        toml::Value::try_from(FibPresetData {
            version: PRESET_FORMAT_VERSION,
            kind: self.kind,
            levels: self.levels.clone(),
            band_alpha: self.band_alpha,
            label_mode: self.label_mode,
            label_position: self.label_position,
            extend: self.extend,
        })
        .ok()
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Ok(data) = value.clone().try_into::<FibPresetData>() else {
            return false;
        };
        if data.version != PRESET_FORMAT_VERSION
            || data.kind != self.kind
            || data.levels.is_empty()
            || !data.levels.iter().any(|level| level.visible)
            || data.levels.iter().any(|level| !level.ratio.is_finite())
        {
            return false;
        }
        self.levels = data.levels;
        self.band_alpha = data.band_alpha.min(MAX_BAND_ALPHA);
        self.label_mode = data.label_mode;
        self.label_position = data.label_position;
        self.extend = data.extend;
        self.touch();
        true
    }
}

/// Parse a level ratio the way a trader writes one: a bare number is always
/// a ratio (`.618`, `0.618`, `-0.272`), percent needs the sign (`61.8%`),
/// and the decimal comma is accepted alongside the dot.
#[must_use]
pub fn parse_ratio(input: &str) -> Option<f64> {
    let trimmed = input.trim().replace(',', ".");
    let (number, percent) = match trimmed.strip_suffix('%') {
        Some(rest) => (rest.trim_end(), true),
        None => (trimmed.as_str(), false),
    };
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(if percent { value / 100.0 } else { value })
}

/// Whether the log formulas are computable over these anchor prices.
#[must_use]
pub fn log_scale_allowed(a: f64, b: f64, c: f64) -> bool {
    a > 0.0 && b > 0.0 && c > 0.0
}

/// The price of one level. `c` is ignored by retracement; for extension it
/// is the projection origin. Log demands positive prices — `None` otherwise.
///
/// **Retracement runs backwards along the move, not forwards.** A "61.8 %
/// retracement" is a statement about how much of a move price has *given
/// back*: 0 % sits at the end of the move (`b`), 100 % at where it started
/// (`a`), and 61.8 % is 61.8 % of the way back from the end toward the start.
/// Drawn top-down over a fall, that puts 61.8 % high on the chart, where a
/// trader waits for the bounce to stall — the whole reason the level is
/// drawn. Anchoring 0 % at `a` instead, as this did, mirrors every level
/// through the middle of the move and names them all wrong.
///
/// Extension is the other question — where the *next* leg may reach — so it
/// projects forward from `c` by the size of the `a`→`b` move. `c` is `b` on
/// a two-anchor extension and the saved origin on a legacy three-anchor one.
#[must_use]
pub fn level_price(
    kind: FibKind,
    log_scale: bool,
    a: f64,
    b: f64,
    c: f64,
    ratio: f64,
) -> Option<f64> {
    if !log_scale {
        return Some(match kind {
            FibKind::Retracement => b + (a - b) * ratio,
            FibKind::Extension => c + (b - a) * ratio,
        });
    }
    if !log_scale_allowed(a, b, c) {
        return None;
    }
    let move_log = (b / a).ln();
    Some(match kind {
        // `b * (a / b)^ratio`, written with the same log the extension uses:
        // ln(a / b) is -ln(b / a).
        FibKind::Retracement => b * (-move_log * ratio).exp(),
        FibKind::Extension => c * (move_log * ratio).exp(),
    })
}

fn format_price(price: f64) -> String {
    quantick_engine::display::display_f64(price)
}

fn format_label(mode: LabelMode, level: &FibLevelSpec, price: f64) -> String {
    let ratio_part = if level.label.is_empty() {
        match mode {
            LabelMode::PercentPrice => format!("{:.1}%", level.ratio * 100.0),
            LabelMode::RatioPrice => format!("{:.3}", level.ratio),
            LabelMode::PriceOnly => String::new(),
        }
    } else {
        level.label.clone()
    };
    if ratio_part.is_empty() {
        format_price(price)
    } else {
        format!("{ratio_part} \u{00b7} {}", format_price(price))
    }
}

fn level_color(level: &FibLevelSpec, style: DrawingStyle) -> PremultipliedRgba {
    level.color.map_or(style.color, |[r, g, b]| {
        PremultipliedRgba::from_rgb(r, g, b)
    })
}

/// The effective log flag: the explicit choice, honoured only while every
/// anchor price is positive (never a silent approximation).
fn effective_log(payload: &FibPayload, a: f64, b: f64, c: f64) -> bool {
    payload.log_scale && log_scale_allowed(a, b, c)
}

struct Anchors {
    a: f64,
    b: f64,
    c: f64,
}

fn anchor_prices(payload: &FibPayload, ctxt: &DrawContext<'_>) -> Option<Anchors> {
    let required = payload.kind.required_points();
    if ctxt.anchors.len() < required {
        return None;
    }
    let a = ctxt.anchors[0].price;
    let b = ctxt.anchors[1].price;
    // A legacy extension keeps the origin it was saved with; every other
    // object projects from B.
    let c = match ctxt.anchors.get(2) {
        Some(origin) if payload.kind == FibKind::Extension => origin.price,
        _ => b,
    };
    Some(Anchors { a, b, c })
}

fn level_span(points: &[Px], chart_rect: PxRect, extend: Extend) -> (f32, f32) {
    let left = points
        .iter()
        .map(|point| point.x)
        .fold(f32::INFINITY, f32::min);
    let right = points
        .iter()
        .map(|point| point.x)
        .fold(f32::NEG_INFINITY, f32::max);
    match extend {
        Extend::Anchors => (left, right),
        // The newest anchor, not the last one placed: "forward" is a
        // statement about the market's direction, and a Fib drawn
        // right-to-left means the same thing as one drawn left-to-right.
        Extend::Forward => (right, chart_rect.right()),
        Extend::Right => (left, chart_rect.right()),
        Extend::Both => (chart_rect.left(), chart_rect.right()),
    }
}

/// Paint one Fib object (both kinds). Levels are computed in price space and
/// projected through the chart's scale, so linear and log both land exactly.
pub(super) fn paint(
    painter: &mut MarkScene,
    chart_rect: PxRect,
    style: DrawingStyle,
    points: &[Px],
    ctxt: &DrawContext<'_>,
) {
    let Some(payload) = ctxt.payload.as_any().downcast_ref::<FibPayload>() else {
        return;
    };
    let stroke = drawing_stroke(style);
    let Some(anchors) = anchor_prices(payload, ctxt) else {
        // Draft preview: connect the anchors placed so far.
        for pair in points.windows(2) {
            painter.line_segment([pair[0], pair[1]], stroke);
        }
        return;
    };
    let Anchors { a, b, c } = anchors;
    let log = effective_log(payload, a, b, c);
    let (left, right) = level_span(points, chart_rect, payload.extend);

    // Project every visible level once — the bands, the lines and the label
    // slots all read this one pass, so they can never drift apart.
    struct ProjectedLevel<'a> {
        level: &'a FibLevelSpec,
        y: f32,
        color: PremultipliedRgba,
    }
    let projected: SmallVec<[ProjectedLevel<'_>; PROJECTED_LEVELS_INLINE]> = payload
        .levels
        .iter()
        .filter(|level| level.visible)
        .filter_map(|level| {
            level_price(payload.kind, log, a, b, c, level.ratio).map(|price| ProjectedLevel {
                level,
                y: ctxt.scale.y(price),
                color: level_color(level, style),
            })
        })
        .collect();

    // Band fills first, under the level lines. Skipped on the halo pass.
    if !ctxt.halo && payload.band_alpha > 0 {
        for pair in projected.windows(2) {
            if !pair[0].level.fill_to_next {
                continue;
            }
            let fill = rgba_unmultiplied(
                pair[0].color.r(),
                pair[0].color.g(),
                pair[0].color.b(),
                payload.band_alpha,
            );
            painter.rect_filled(
                PxRect::from_min_max(
                    pos2(left, pair[0].y.min(pair[1].y)),
                    pos2(right, pair[0].y.max(pair[1].y)),
                ),
                0.0,
                fill,
            );
        }
    }

    // Level lines, with the cached labels on the real pass — the cache is
    // borrowed, never copied, on the frame path.
    let labels = (!ctxt.halo).then(|| labels_for(payload, a, b, c, log));
    for (index, level) in projected.iter().enumerate() {
        let line_stroke = if ctxt.halo {
            stroke
        } else {
            Stroke::new(style.width_px, level.color)
        };
        painter.line_segment([pos2(left, level.y), pos2(right, level.y)], line_stroke);
        if let Some(labels) = &labels
            && let Some(text) = labels.labels.get(index)
        {
            // A span that runs to the chart's edge has no "outside" left to
            // put a label in — it would be painted past the clip and simply
            // vanish, which is what projecting the levels forward would
            // otherwise cost. Fall inside instead.
            let position = if right >= chart_rect.right() - FIB_LABEL_OFFSET_PX
                && payload.label_position == LabelPosition::RightOutside
            {
                LabelPosition::RightInside
            } else {
                payload.label_position
            };
            let (anchor, x) = match position {
                LabelPosition::RightOutside => (Align2::LEFT_BOTTOM, right + FIB_LABEL_OFFSET_PX),
                LabelPosition::RightInside => (Align2::RIGHT_BOTTOM, right - FIB_LABEL_OFFSET_PX),
                LabelPosition::Left => (Align2::LEFT_BOTTOM, left + FIB_LABEL_OFFSET_PX),
            };
            painter.text(
                pos2(x, level.y),
                anchor,
                text,
                FontSpec::monospace(FIB_LABEL_SIZE_PX),
                level.color,
            );
        }
    }

    paint_anchor_guides(painter, style, points, ctxt, payload.kind);
}

/// The dashed A-B guide and, for an extension, a dot on A and on B.
///
/// A retracement shows its guide only while selected: its levels already
/// span the leg. An extension's levels start past its last anchor, so the
/// move they were measured from would vanish the moment it is deselected;
/// its leg and both ends therefore stay painted, selected or not. A legacy
/// extension's B-C guide keeps its old rule and shows only while selected.
/// Selection draws the normal handles over the dots.
fn paint_anchor_guides(
    painter: &mut MarkScene,
    style: DrawingStyle,
    points: &[Px],
    ctxt: &DrawContext<'_>,
    kind: FibKind,
) {
    let always = kind == FibKind::Extension;
    if ctxt.halo || points.len() < 2 || !(ctxt.selected || always) {
        return;
    }
    let guide = Stroke::new(1.0_f32, style.color);
    let legs = if ctxt.selected && always { 2 } else { 1 };
    for pair in points.windows(2).take(legs) {
        painter.add(MarkShape::dashed_line(
            &[pair[0], pair[1]],
            guide,
            GUIDE_DASH_PX,
            GUIDE_DASH_PX,
        ));
    }
    if always {
        for point in &points[..2] {
            painter.circle_filled(*point, SELECTED_ANCHOR_RADIUS_PX, style.color);
        }
    }
}

/// Rebuild the label cache if anchors or config changed, then hand out a
/// borrow of it — the frame path never copies the formatted strings.
fn labels_for(payload: &FibPayload, a: f64, b: f64, c: f64, log: bool) -> Ref<'_, LabelCache> {
    let key = (
        a.to_bits() ^ u64::from(log),
        b.to_bits(),
        c.to_bits(),
        payload.revision,
    );
    {
        let mut cache = payload.cache.borrow_mut();
        if cache.key != key {
            cache.key = key;
            cache.labels = payload
                .levels
                .iter()
                .filter(|level| level.visible)
                .filter_map(|level| {
                    level_price(payload.kind, log, a, b, c, level.ratio)
                        .map(|price| format_label(payload.label_mode, level, price))
                })
                .collect();
        }
    }
    payload.cache.borrow()
}

/// Hit-test one Fib object: any visible level line, or the anchor legs.
pub(super) fn hit_test(
    chart_rect: PxRect,
    points: &[Px],
    position: Px,
    radius_px: f32,
    ctxt: &DrawContext<'_>,
) -> bool {
    let Some(payload) = ctxt.payload.as_any().downcast_ref::<FibPayload>() else {
        return false;
    };
    let Some(Anchors { a, b, c }) = anchor_prices(payload, ctxt) else {
        return false;
    };
    if points.len() >= 2 && distance_to_segment(position, points[0], points[1]) <= radius_px {
        return true;
    }
    if payload.kind == FibKind::Extension
        && points.len() >= 3
        && distance_to_segment(position, points[1], points[2]) <= radius_px
    {
        return true;
    }
    let log = effective_log(payload, a, b, c);
    let (left, right) = level_span(points, chart_rect, payload.extend);
    position.x >= left - radius_px
        && position.x <= right + radius_px
        && payload
            .levels
            .iter()
            .filter(|level| level.visible)
            .filter_map(|level| level_price(payload.kind, log, a, b, c, level.ratio))
            .any(|price| (position.y - ctxt.scale.y(price)).abs() <= radius_px)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() < EPS
    }

    /// Each letter takes the corner its *own* drawing leaves empty, which is
    /// why the retracement's sits high and the projection's low: the rungs
    /// are what moved between the two icons, and the letter follows the free
    /// space rather than a fixed corner.
    #[test]
    fn the_two_fib_letters_take_opposite_corners() {
        assert!(
            FIB_RETRACEMENT_LETTER.at.1 < 0.5,
            "the R sits high, above the rungs hanging inside the move"
        );
        assert!(
            FIB_EXTENSION_LETTER.at.1 > 0.5,
            "the P sits low, under the rungs projected past the move"
        );
    }

    /// A retracement runs *backwards* along the move: 0 % at the end of it,
    /// 100 % at where it started. So on a fall drawn top-down, 61.8 % is high
    /// on the chart — that is what "price gave back 61.8 % of the drop"
    /// means, and where the trader waits for the bounce to stall. It used to
    /// be mirrored, putting 61.8 % near the low and naming every level wrong.
    #[test]
    fn retracement_measures_how_much_of_the_move_was_given_back() {
        // Down move: A above B (the prototype's 71,824.50 -> 69,111.50).
        let (a, b) = (71_824.50, 69_111.50);
        let span = a - b;
        let golden = [
            (0.0, b),
            (0.236, b + span * 0.236),
            (0.382, b + span * 0.382),
            (0.5, b + span * 0.5),
            (0.618, b + span * 0.618),
            (0.786, b + span * 0.786),
            (1.0, a),
        ];
        for (ratio, expected) in golden {
            let price = level_price(FibKind::Retracement, false, a, b, b, ratio).unwrap();
            assert!(
                close(price, expected),
                "ratio {ratio}: {price} vs {expected}"
            );
        }
        assert!(
            level_price(FibKind::Retracement, false, a, b, b, 0.618).unwrap() > (a + b) / 2.0,
            "on a fall, the 61.8 % line sits in the upper half of the move"
        );

        // Up move mirrors exactly: 61.8 % given back of a 100 -> 200 rally is
        // 200 - 0.618 * 100.
        let price = level_price(FibKind::Retracement, false, 100.0, 200.0, 200.0, 0.618).unwrap();
        assert!(close(price, 138.2), "{price}");
        assert!(
            level_price(FibKind::Retracement, false, 100.0, 200.0, 200.0, 0.618).unwrap() < 150.0,
            "on a rally, the 61.8 % line sits in the lower half of the move"
        );
    }

    /// The log formulas must land on the same two ends as the linear ones,
    /// or a trader switching the scale sees the levels renamed.
    #[test]
    fn the_log_retracement_shares_the_linear_ones_endpoints() {
        let (a, b) = (71_824.50, 69_111.50);
        let zero = level_price(FibKind::Retracement, true, a, b, b, 0.0).unwrap();
        let one = level_price(FibKind::Retracement, true, a, b, b, 1.0).unwrap();
        assert!(close(zero, b), "0 % is the end of the move: {zero}");
        assert!(close(one, a), "100 % is where it started: {one}");
        let middle = level_price(FibKind::Retracement, true, a, b, b, 0.618).unwrap();
        assert!(
            middle > (a + b) / 2.0,
            "and 61.8 % is still high on a fall: {middle}"
        );
    }

    #[test]
    fn extension_projects_the_impulse_from_origin_c_and_reverse_keeps_c() {
        let (a, b, c) = (100.0, 150.0, 120.0);
        let price = level_price(FibKind::Extension, false, a, b, c, 1.618).unwrap();
        assert!(close(price, 120.0 + 50.0 * 1.618));
        // Reverse swaps A and B and never moves C.
        let reversed = level_price(FibKind::Extension, false, b, a, c, 1.618).unwrap();
        assert!(close(reversed, 120.0 - 50.0 * 1.618));
    }

    #[test]
    fn log_formulas_compound_the_ratio_of_prices() {
        // Retracement: P(r) = PA * (PB/PA)^r.
        let price = level_price(FibKind::Retracement, true, 100.0, 400.0, 400.0, 0.5).unwrap();
        assert!(close(price, 200.0), "half the log move of 100->400 is 200");
        // Extension: P(r) = PC * (PB/PA)^r.
        let projected = level_price(FibKind::Extension, true, 100.0, 400.0, 50.0, 0.5).unwrap();
        assert!(close(projected, 100.0));
        // Reverse swaps A/B: the ratio of prices inverts.
        let reversed = level_price(FibKind::Retracement, true, 400.0, 100.0, 100.0, 0.5).unwrap();
        assert!(close(reversed, 200.0));
    }

    #[test]
    fn log_refuses_non_positive_prices_instead_of_guessing() {
        assert!(level_price(FibKind::Retracement, true, 0.0, 10.0, 10.0, 0.5).is_none());
        assert!(level_price(FibKind::Extension, true, 10.0, 20.0, -5.0, 0.5).is_none());
        assert!(!log_scale_allowed(-1.0, 2.0, 3.0));
        // Linear never needs the guard.
        assert!(level_price(FibKind::Retracement, false, -10.0, 10.0, 10.0, 0.5).is_some());
    }

    #[test]
    fn the_ratio_parser_reads_ratios_percents_and_decimal_commas() {
        assert!(close(parse_ratio(".618").unwrap(), 0.618));
        assert!(close(parse_ratio("0.618").unwrap(), 0.618));
        assert!(close(parse_ratio("61.8%").unwrap(), 0.618));
        assert!(close(parse_ratio("0,618").unwrap(), 0.618));
        assert!(close(parse_ratio("61,8 %").unwrap(), 0.618));
        assert!(close(parse_ratio("-0.272").unwrap(), -0.272));
        assert!(close(parse_ratio("161.8%").unwrap(), 1.618));
        assert!(parse_ratio("abc").is_none());
        assert!(parse_ratio("").is_none());
        assert!(parse_ratio("inf").is_none());
    }

    #[test]
    fn presets_start_every_fib_and_round_trip_through_the_preset_format() {
        let mut payload = FibPayload::new(FibKind::Retracement);
        assert_eq!(payload.levels.len(), 7, "Standard retracement has 7 levels");
        payload.levels[1].label = "golden".to_owned();
        payload.levels[1].color = Some([255, 159, 67]);
        payload.levels[1].fill_to_next = true;
        payload.band_alpha = 40;
        payload.label_mode = LabelMode::RatioPrice;
        payload.extend = Extend::Right;

        let exported = payload.export_preset().expect("fib exports its preset");
        let mut restored = FibPayload::new(FibKind::Retracement);
        assert!(restored.import_preset(&exported));
        assert_eq!(restored, payload, "the preset format round-trips");

        // A preset of the other kind is refused, not misapplied.
        let mut extension = FibPayload::new(FibKind::Extension);
        assert!(!extension.import_preset(&exported));
    }

    #[test]
    fn imports_reject_broken_or_future_preset_files() {
        let mut payload = FibPayload::new(FibKind::Retracement);
        let mut exported = payload.export_preset().unwrap();
        let table = exported.as_table_mut().unwrap();
        table.insert("version".into(), toml::Value::Integer(99));
        assert!(
            !payload.import_preset(&exported),
            "an unknown version must not half-apply"
        );
    }

    #[test]
    fn duplicate_ratios_within_tolerance_are_rejected() {
        let payload = FibPayload::new(FibKind::Retracement);
        assert!(payload.is_duplicate(0.618, None));
        assert!(payload.is_duplicate(0.618 + RATIO_DUPLICATE_TOLERANCE / 2.0, None));
        assert!(
            !payload.is_duplicate(0.618, Some(4)),
            "a level may keep its own ratio"
        );
        assert!(!payload.is_duplicate(0.9, None));
    }

    #[test]
    fn labels_follow_the_mode_and_custom_text() {
        let level = FibLevelSpec::new(0.618);
        assert_eq!(
            format_label(LabelMode::PercentPrice, &level, 70_147.866),
            "61.8% \u{00b7} 70147.87"
        );
        assert_eq!(
            format_label(LabelMode::RatioPrice, &level, 70_147.866),
            "0.618 \u{00b7} 70147.87"
        );
        assert_eq!(
            format_label(LabelMode::PriceOnly, &level, 70_147.866),
            "70147.87"
        );
        let mut named = FibLevelSpec::new(0.618);
        named.label = "golden".to_owned();
        assert_eq!(
            format_label(LabelMode::PercentPrice, &named, 1.0),
            "golden \u{00b7} 1"
        );
    }

    #[test]
    fn the_label_cache_rebuilds_only_when_prices_or_config_change() {
        let mut payload = FibPayload::new(FibKind::Retracement);
        let first = labels_for(&payload, 200.0, 100.0, 100.0, false)
            .labels
            .clone();
        assert_eq!(first.len(), 7);
        let again = labels_for(&payload, 200.0, 100.0, 100.0, false)
            .labels
            .clone();
        assert_eq!(first, again);
        // A config edit bumps the revision and the labels follow.
        payload.levels[0].label = "top".to_owned();
        payload.touch();
        let after = labels_for(&payload, 200.0, 100.0, 100.0, false);
        assert!(after.labels[0].starts_with("top"));
    }

    /// The drag is the whole reading gesture: press on the swing low, pull up
    /// and to the right, watch the levels fill in the leg under the pointer.
    /// The draft preview completes the geometry with the hovered point, so
    /// this is exactly the span a half-placed retracement paints — from the
    /// anchor that is already down to where the pointer is now, and no
    /// further. Both halves were reported from the running build.
    #[test]
    fn a_retracement_being_dragged_spans_the_first_click_to_the_pointer() {
        let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 500.0));
        // Anchor down at x = 200, pointer at x = 620: what the preview holds
        // mid-drag, up and to the right.
        let dragging = [pos2(200.0, 420.0), pos2(620.0, 90.0)];
        let (left, right) = level_span(&dragging, chart, Extend::for_kind(FibKind::Retracement));
        assert!(
            (left - 200.0).abs() < f32::EPSILON,
            "the levels must begin at the first click, not at the pointer: {left}"
        );
        assert!(
            (right - 620.0).abs() < f32::EPSILON,
            "and end under the pointer rather than running to the edge: {right}"
        );
    }

    /// The other kind asks the other question: an extension's targets are
    /// what comes after its last anchor, so they must not paint back over the
    /// leg they were measured from.
    #[test]
    fn an_extension_still_projects_from_its_last_anchor() {
        let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 500.0));
        let placed = [pos2(200.0, 420.0), pos2(400.0, 90.0), pos2(620.0, 260.0)];
        let (left, _) = level_span(&placed, chart, Extend::for_kind(FibKind::Extension));
        assert!(
            (left - 620.0).abs() < f32::EPSILON,
            "an extension starts where its projection origin is: {left}"
        );
    }

    fn context<'a>(
        payload: &'a FibPayload,
        anchors: &'a [crate::ChartPoint],
        scale: &'a crate::chart::PriceScale,
        selected: bool,
        halo: bool,
    ) -> DrawContext<'a> {
        DrawContext {
            payload,
            anchors,
            scale,
            px_per_bar: 1.0,
            unit: super::super::ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected,
            halo,
            content_editing: false,
            text: &crate::UNMEASURED,
        }
    }

    fn anchors(prices: &[f64]) -> Vec<crate::ChartPoint> {
        prices
            .iter()
            .enumerate()
            .map(|(index, &price)| crate::ChartPoint::at(index as f32, price))
            .collect()
    }

    /// The drag is the whole gesture now: A→B places the extension, and its
    /// levels hang from B by the size of that move — linear and log.
    #[test]
    fn a_new_extension_projects_the_leg_forward_from_b() {
        assert_eq!(FibKind::Extension.required_points(), 2);
        assert!(FibKind::Extension.accepts_points(2));
        assert!(FibKind::Extension.accepts_points(LEGACY_EXTENSION_POINTS));
        assert!(!FibKind::Retracement.accepts_points(3));

        let scale = crate::chart::PriceScale::from_range(0.0, 1000.0, 0.0, 1000.0);
        let mut payload = FibPayload::new(FibKind::Extension);
        let placed = anchors(&[100.0, 150.0]);
        let ctxt = context(&payload, &placed, &scale, false, false);
        let Anchors { a, b, c } = anchor_prices(&payload, &ctxt).expect("two anchors complete it");
        for ratio in [0.618, 1.0, 1.618] {
            let linear = level_price(FibKind::Extension, false, a, b, c, ratio).unwrap();
            assert!(close(linear, 150.0 + 50.0 * ratio), "{ratio}: {linear}");
            let log = level_price(FibKind::Extension, true, a, b, c, ratio).unwrap();
            assert!(close(log, 150.0 * 1.5_f64.powf(ratio)), "{ratio}: {log}");
        }

        // The log flag reads the same origin through the paint path.
        payload.log_scale = true;
        let ctxt = context(&payload, &placed, &scale, false, false);
        let Anchors { a, b, c } = anchor_prices(&payload, &ctxt).unwrap();
        assert!(effective_log(&payload, a, b, c));
    }

    /// A projection saved with three anchors loads as it was: the payload a
    /// workspace or the control plane wrote before the two-anchor gesture,
    /// with its own origin C, paints its levels at exactly the prices the
    /// three-anchor formula gave — never silently moved to B.
    #[test]
    fn a_legacy_three_anchor_extension_keeps_its_origin_and_its_prices() {
        const LEGACY_PAYLOAD: &str = r#"
            version = 1
            kind = "Extension"
            band_alpha = 24
            label_mode = "PercentPrice"
            label_position = "RightOutside"
            extend = "Forward"

            [[levels]]
            ratio = 1.0
            label = ""
            visible = true
            fill_to_next = false

            [[levels]]
            ratio = 1.618
            label = ""
            visible = true
            fill_to_next = false

            [[levels]]
            ratio = 2.618
            label = ""
            visible = true
            fill_to_next = false
        "#;
        let value: toml::Value = toml::from_str(LEGACY_PAYLOAD).expect("legacy payload parses");
        let mut payload = FibPayload::new(FibKind::Extension);
        assert!(payload.import_preset(&value), "the legacy payload loads");

        // Impulse 100 -> 150, pullback origin C at 120.
        let saved = anchors(&[100.0, 150.0, 120.0]);
        let expected = [170.0, 200.9, 250.9];
        let scale = crate::chart::PriceScale::from_range(0.0, 1000.0, 0.0, 1000.0);
        let ctxt = context(&payload, &saved, &scale, false, false);
        let Anchors { a, b, c } = anchor_prices(&payload, &ctxt).expect("three anchors still load");
        assert!(close(c, 120.0), "the saved origin is kept: {c}");
        for (level, want) in payload.levels.iter().zip(expected) {
            let price = level_price(payload.kind, false, a, b, c, level.ratio).unwrap();
            assert!(close(price, want), "{}: {price} vs {want}", level.ratio);
        }

        // And the painted level lines sit at those prices.
        let points: Vec<Px> = saved
            .iter()
            .enumerate()
            .map(|(index, anchor)| pos2(100.0 * (index as f32 + 1.0), scale.y(anchor.price)))
            .collect();
        let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 1000.0));
        let mut scene = MarkScene::default();
        paint(&mut scene, chart, DrawingStyle::default(), &points, &ctxt);
        let level_ys: Vec<f32> = scene
            .shapes
            .iter()
            .filter_map(|shape| match shape {
                MarkShape::Line { from, to, .. } if (from.y - to.y).abs() < f32::EPSILON => {
                    Some(from.y)
                }
                _ => None,
            })
            .collect();
        let want_ys: Vec<f32> = expected.iter().map(|&price| scale.y(price)).collect();
        assert_eq!(level_ys, want_ys);

        // Unselected, it shows its A-B leg and two dots like a new one; the
        // B-C guide stays a selection affordance, as it always was.
        let (dashed, dots) = guides_and_dots(&scene);
        assert_eq!(dashed, vec![points[..2].to_vec()]);
        assert_eq!(dots.len(), 2);
    }

    fn guides_and_dots(scene: &MarkScene) -> (Vec<Vec<Px>>, Vec<(Px, f32)>) {
        let mut dashed = Vec::new();
        let mut dots = Vec::new();
        for shape in &scene.shapes {
            match shape {
                MarkShape::DashedLine {
                    points, dash_px, ..
                } if (*dash_px - GUIDE_DASH_PX).abs() < f32::EPSILON => dashed.push(points.clone()),
                MarkShape::Circle {
                    center, radius_px, ..
                } => dots.push((*center, *radius_px)),
                _ => {}
            }
        }
        (dashed, dots)
    }

    /// An extension's levels start past B, so without its leg the move they
    /// measure is gone the moment it is deselected. Both anchors and the
    /// dashed A-B guide stay painted, selected or not; the halo pass and a
    /// retracement are untouched.
    #[test]
    fn an_unselected_extension_paints_both_anchors_and_its_dashed_leg() {
        let scale = crate::chart::PriceScale::from_range(0.0, 1000.0, 0.0, 1000.0);
        let chart = PxRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 1000.0));
        let placed = anchors(&[100.0, 150.0]);
        let points = [pos2(200.0, scale.y(100.0)), pos2(400.0, scale.y(150.0))];
        let extension = FibPayload::new(FibKind::Extension);

        for selected in [false, true] {
            let mut scene = MarkScene::default();
            let ctxt = context(&extension, &placed, &scale, selected, false);
            paint(&mut scene, chart, DrawingStyle::default(), &points, &ctxt);
            let (dashed, dots) = guides_and_dots(&scene);
            assert_eq!(dashed, vec![points.to_vec()], "selected = {selected}");
            assert_eq!(
                dots,
                points
                    .iter()
                    .map(|point| (*point, SELECTED_ANCHOR_RADIUS_PX))
                    .collect::<Vec<_>>(),
                "selected = {selected}"
            );
        }

        let mut halo = MarkScene::default();
        let ctxt = context(&extension, &placed, &scale, true, true);
        paint(&mut halo, chart, DrawingStyle::default(), &points, &ctxt);
        assert_eq!(guides_and_dots(&halo), (Vec::new(), Vec::new()));

        let retracement = FibPayload::new(FibKind::Retracement);
        let mut scene = MarkScene::default();
        let ctxt = context(&retracement, &placed, &scale, false, false);
        paint(&mut scene, chart, DrawingStyle::default(), &points, &ctxt);
        assert_eq!(
            guides_and_dots(&scene),
            (Vec::new(), Vec::new()),
            "an unselected retracement paints no guide"
        );
    }
}
