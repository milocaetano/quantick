//! The aggression-bubble family: [`BubbleStyle`], the three enums it is made
//! of, the φ ladder its default radii are rungs of, and the short-float
//! serializer that keeps the presets file reviewable.
//!
//! Every field here is display-only. The parent module owns the shared
//! validation (`finite_clamp`) and the [`HeatmapConfig`](super::HeatmapConfig)
//! that carries one of these.

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use serde::{Deserialize, Serialize};

use super::finite_clamp;

/// The golden ratio, φ.
///
/// The aggression layer's proportions are steps on this one ladder rather than
/// a scatter of independently tuned numbers. Radius is *area*-proportional to
/// quantity, so a φ step in radius is a φ² ≈ 2.6× step in quantity — "roughly
/// two and a half times the print" is a size class a flow trader already reads
/// in, which is what makes the ladder a measuring stick and not an ornament.
///
/// It is used where a *proportion* is being expressed. It is deliberately not
/// used for pixel floors (below ~3px the display grid rules, and a 0.38px
/// stroke is a rounding error) nor for alphas, which are perceptual and tuned
/// against the heat ramp's actual luminance.
pub const PHI: f32 = 1.618_034;
/// 1/φ ≈ 0.618 — see [`PHI`]. Each rung is derived from the last rather than
/// written out, so the ladder cannot drift apart one literal at a time.
pub const INV_PHI: f32 = 1.0 / PHI;
/// 1/φ² ≈ 0.382 — see [`PHI`].
pub const INV_PHI_2: f32 = INV_PHI / PHI;
/// 1/φ³ ≈ 0.236 — see [`PHI`].
///
/// The bottom rung of the ladder, `1/φ⁴`, is deliberately not a constant of
/// its own: nothing computes with it at runtime — the smallest radius is a
/// stored default, not a derived value — so it would be a name with no
/// caller. It is written as `INV_PHI_3 / PHI` where it is needed.
pub const INV_PHI_3: f32 = INV_PHI_2 / PHI;
/// The golden angle, 360°/φ² ≈ 137.5°, in radians.
///
/// The sweep a fully consuming print's crown reaches. Being 1/φ² of the rim
/// rather than all of it is what makes the crown structurally incapable of
/// closing into a second circle around the bubble — the exact ambiguity the
/// old closed impact ring created with the disc's own edge.
pub const GOLDEN_ANGLE: f32 = std::f32::consts::TAU * INV_PHI_2;
/// Default alpha applied to aggression bubbles.
///
/// High enough that a bubble occludes rather than tints: translucency over a
/// coloured heat cell drags the fill toward the heat's own hue, and occlusion
/// is the strongest cue for counting overlapping prints. Short of opaque, so a
/// bubble still admits it is sitting over the book.
pub const DEFAULT_BUBBLE_OPACITY: f32 = 0.9;
/// Default on-screen radius, in points, of the largest aggression bubble.
pub const DEFAULT_BUBBLE_MAX_RADIUS: f32 = 15.0;
/// Smallest accepted maximum bubble radius.
pub const MIN_BUBBLE_MAX_RADIUS: f32 = 4.0;
/// Largest accepted maximum bubble radius.
pub const MAX_BUBBLE_MAX_RADIUS: f32 = 48.0;
/// Largest accepted radius for the smallest drawn bubble.
pub const MAX_BUBBLE_MIN_RADIUS: f32 = 12.0;
/// Default radius of the smallest drawn print: the bottom rung of the φ
/// ladder, `max / φ⁴`. See [`PHI`].
///
/// Written as a literal, not as `DEFAULT_BUBBLE_MAX_RADIUS * INV_PHI_4`, for
/// one mechanical reason: the presets file stores floats to
/// [`SERIALIZED_FLOAT_PLACES`] decimals, so a default carrying more precision
/// than that would come back different from a save-and-reload and every
/// preset round trip would drift. The rungs are therefore the exact ladder
/// rounded to what the file can hold — asserted by
/// `the_default_radii_are_rungs_of_one_golden_ladder`.
pub const DEFAULT_BUBBLE_MIN_RADIUS: f32 = 2.1885;
/// Default radius at which a bubble can afford its dressing — shading and the
/// separator hair. The `max / φ³` rung.
///
/// It lands where hue starts working: below roughly 3.5px the eye's chromatic
/// channel is too low-pass to separate a green speck from a red one, so that
/// is exactly the size at which shading a bubble by side begins to pay. Under
/// it a print is one flat disc, which is also what keeps the per-frame
/// tessellation budget flat on a fast tape.
pub const DEFAULT_DETAIL_MIN_RADIUS: f32 = 3.541;
/// Default radius, in pixels, below which a bubble is treated as unreadable
/// on its own: small enough to be a routine print, large enough that side
/// colour and the side nudge actually register at a glance. The `max / φ²`
/// rung, and the floor the consumption crown and the pie split gate on.
pub const DEFAULT_READABLE_MIN_RADIUS: f32 = 5.7295;
/// Default half-length of the vertical consumption front, as a multiple of the
/// radius: `1/φ`, at the precision the presets file stores. Only
/// [`ConsumptionMark::Front`] reads it.
pub const DEFAULT_FRONT_LENGTH_SCALE: f32 = 0.618;
/// Smallest radius that gets a label, as a fraction of
/// [`DEFAULT_BUBBLE_MAX_RADIUS`].
///
/// Not a φ rung, and deliberately not dressed up as one: the rule is "the top
/// of the radius range", because laying out text is the most expensive thing
/// a bubble can ask for and only the largest prints have room to hold it.
pub const DEFAULT_LABEL_MIN_RADIUS_SHARE: f32 = 0.9;
/// Smallest radius that gets a label, in pixels — see
/// [`DEFAULT_LABEL_MIN_RADIUS_SHARE`].
pub const DEFAULT_LABEL_MIN_RADIUS: f32 =
    DEFAULT_BUBBLE_MAX_RADIUS * DEFAULT_LABEL_MIN_RADIUS_SHARE;
/// Largest accepted readability floor.
pub const MAX_READABLE_MIN_RADIUS: f32 = 32.0;
/// Default edge darkening of a sphere-rendered bubble.
///
/// Enough contrast that two piled bubbles keep a boundary, and no more:
/// counting overlapping discs needs only a soft edge, while a hard one reads
/// as a black outline drawn around every print on the tape.
pub const DEFAULT_SPHERE_SHADING: f32 = 0.34;
/// Default highlight strength of a sphere-rendered bubble.
///
/// A specular cue, not a second colour. Pushed harder, the sell side's core
/// washes out to pink — the loudest thing on the canvas and the one that makes
/// the layer read as decoration rather than as instrumentation.
pub const DEFAULT_SPHERE_HIGHLIGHT: f32 = 0.18;

/// How the quantity that maps to a full-size bubble is chosen.
///
/// The automatic modes measure the whole recorded tape, never the visible
/// window: zoom decides what is on screen, not what a quantity means, so the
/// scale only moves when the recording itself does — a new print beyond the
/// old reference, or retention trimming the floor. `VisibleP99` keeps one
/// outlier sweep from shrinking every other print to a dot, at the cost of
/// making the top 1% all render at the maximum radius. `VisibleMax` restores
/// a strict ordering among recorded prints (clusters a zoomed-out view merges
/// past it saturate at full size); `Fixed` pins the scale so bubble size
/// means the same thing across sessions and symbols. The `Visible*` names are
/// the wire format shipped presets already use; what they measure stopped
/// being the viewport the day zoom stopped being allowed to rescale a print.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BubbleSizeReference {
    /// 99th percentile of the recorded clustered prints.
    #[default]
    VisibleP99,
    /// The largest recorded clustered print.
    VisibleMax,
    /// An explicit quantity, [`BubbleStyle::size_reference_quantity`].
    Fixed,
}

/// How a print that ate resting liquidity says so.
///
/// The signal has to survive three things at once: a dense tape where prints
/// overlap, a canvas that is nearly black, and a heat map underneath that is
/// the only layer carrying liquidity. That rules out any mark that works by
/// *subtraction* — a dark stroke is invisible over the canvas and a hole
/// punched in the book everywhere else — and it rules out any mark that
/// crosses the disc, whose area is the quantity reading.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumptionMark {
    /// An open arc outside the rim, on the side of the book that was eaten:
    /// buy aggression lifts the ask, so its crown sits above; sell aggression
    /// hits the bid and wears it below.
    ///
    /// Arc *length* carries how much of the print matched resting liquidity —
    /// the one channel the eye reads ordinally without a reference beside it —
    /// from [`INV_PHI_2`] of the [`GOLDEN_ANGLE`] at a nibble to the whole
    /// golden angle at a full sweep. Because it never closes and never crosses
    /// the disc, it cannot be confused with the bubble's own rim, and stacked
    /// crowns at one price row read as the nested brackets that absorption
    /// actually is.
    #[default]
    Crown,
    /// The vertical line through the bubble that shipped before the crown.
    ///
    /// Kept because a chart is someone's muscle memory and a preset that asked
    /// for it should still get it — not because it reads well on a fast tape,
    /// where it slices every disc it marks.
    Front,
    /// No consumption mark at all. The heat map to the right of the print
    /// still shows whether the level refilled, which is the honest record.
    None,
}

impl ConsumptionMark {
    /// Whether this mark draws the vertical front.
    #[must_use]
    pub fn is_front(self) -> bool {
        matches!(self, Self::Front)
    }
}

/// How a bubble's fill is painted.
///
/// `Flat` is the classic solid disc. `Sphere` shades each bubble like a ball
/// lit from the upper left — an offset highlight over a darkened rim — so
/// overlapping prints on a dense tape keep a visible boundary instead of
/// merging into one solid blob. Purely visual: geometry, clustering and
/// liquidity association are identical in both modes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BubbleRenderMode {
    /// Solid 2D disc.
    #[default]
    Flat,
    /// Shaded 3D-looking sphere.
    Sphere,
}

/// Decimal places kept when a bubble float is written to the presets file.
///
/// Four is past the precision any pixel radius or alpha carries, and well
/// inside what `f32` represents exactly enough to survive a round trip.
pub(super) const SERIALIZED_FLOAT_PLACES: i32 = 4;

/// Serialize an `f32` as a short decimal.
///
/// TOML floats are `f64`, so an `f32` promoted straight through prints its
/// exact binary expansion: `0.78` is written back as `0.7799999713897705`.
/// Presets are tracked in git precisely so a look can be reviewed and rolled
/// back like code, and a diff of noise defeats that — this is the write path
/// for every visual float, so rounding here fixes the file for all of them.
fn serialize_short_f32<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let scale = 10_f64.powi(SERIALIZED_FLOAT_PLACES);
    let rounded = (f64::from(*value) * scale).round() / scale;
    serializer.serialize_f64(rounded)
}

/// Everything the "aggression bubbles" panel owns: bubble geometry, colour,
/// labels and the two marks a bubble draws when it ate resting liquidity — the
/// vertical consumption front (the "risco") and the trail that leaks into the
/// consumed side (the "rastro").
///
/// Every field here is display-only. [`min_quantity`](Self::min_quantity) and
/// the size reference reach the projection, but only to decide which bubbles
/// are drawn and how large — never what is captured, retained, or associated
/// with an L2 reduction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BubbleStyle {
    /// Radius of the smallest drawn print, in pixels.
    #[serde(serialize_with = "serialize_short_f32")]
    pub min_radius: f32,
    /// Radius of a full-size print, in pixels. Area stays proportional to
    /// quantity between the two.
    #[serde(serialize_with = "serialize_short_f32")]
    pub max_radius: f32,
    /// Alpha of the bubble fill.
    #[serde(serialize_with = "serialize_short_f32")]
    pub opacity: f32,
    /// Rim stroke width; zero draws no rim.
    #[serde(serialize_with = "serialize_short_f32")]
    pub outline_width: f32,
    /// Soft halo drawn behind the fill, as a fraction of full alpha.
    #[serde(serialize_with = "serialize_short_f32")]
    pub halo_strength: f32,
    /// Bubbles below this radius are drawn as a plain dot (no halo, rim or
    /// impact ring). Raising it trades detail for frame time on a fast tape.
    #[serde(serialize_with = "serialize_short_f32")]
    pub detail_min_radius: f32,
    /// Radius, in pixels, below which a bubble cannot be read on its own.
    ///
    /// The readability floor both the dust merge and
    /// [`hollow_small_buys`](Self::hollow_small_buys) gate on. Deliberately
    /// *not* [`detail_min_radius`](Self::detail_min_radius): that one decides
    /// how much dressing a bubble can afford, and a sphere-heavy look sets it
    /// low on purpose — the "3d spheres" and "dense tape btc" presets put it
    /// at or below `min_radius`, which would leave nothing to fold.
    #[serde(serialize_with = "serialize_short_f32")]
    pub readable_min_radius: f32,
    /// Whether buy prints below [`readable_min_radius`](Self::readable_min_radius)
    /// are drawn as an open ring instead of a solid dot.
    ///
    /// Off by default. The intent was that shape survives where colour does
    /// not, but the ring spends most of the disc's area to say it: at 2px a
    /// green ring around a 22%-alpha interior carries *less* green than the
    /// dot it replaced, so the side became harder to read, not easier — and
    /// the two sides ended up drawn in visibly different media. Side is
    /// carried instead by the buy/sell luminance gap, which the achromatic
    /// channel resolves several times smaller than the chromatic one, and by
    /// [`side_offset`](Self::side_offset). The knob stays for anyone who
    /// preferred the ring.
    pub hollow_small_buys: bool,
    /// Whether the fill is a flat disc or a shaded sphere.
    pub render_mode: BubbleRenderMode,
    /// How much a sphere-rendered bubble darkens toward its rim. Zero shades
    /// nothing (the fill reads flat again); one pushes the rim to black.
    #[serde(serialize_with = "serialize_short_f32")]
    pub sphere_shading: f32,
    /// Strength of the off-center light spot on a sphere-rendered bubble.
    #[serde(serialize_with = "serialize_short_f32")]
    pub sphere_highlight: f32,
    /// How the full-size reference quantity is chosen.
    pub size_reference: BubbleSizeReference,
    /// Quantity mapped to [`max_radius`](Self::max_radius) when the reference
    /// is [`BubbleSizeReference::Fixed`].
    pub size_reference_quantity: f64,
    /// Prints below this exact quantity are not drawn. Zero draws everything.
    ///
    /// Display-only and applied *after* liquidity association, so hiding small
    /// prints never turns an aggression-aligned reduction into an
    /// unattributed one.
    pub min_quantity: f64,
    /// Vertical separation, in pixels, between the two sides: buy bubbles are
    /// nudged up (they lift the ask), sell bubbles down (they hit the bid).
    ///
    /// Deliberately unfaithful to the exact price — with a one-tick spread both
    /// sides land on the same row and stack into an unreadable line. Zero
    /// restores exact price placement.
    #[serde(serialize_with = "serialize_short_f32")]
    pub side_offset: f32,
    /// How a bubble that ate resting liquidity says so.
    ///
    /// Replaces the older `show_consumption_front` switch. A preset written
    /// before the crown existed simply does not carry this key, so it opens
    /// wearing the crown — which is the point of changing the default, and is
    /// covered by a test that such a preset still loads.
    pub consumption_mark: ConsumptionMark,
    /// Width of the vertical front, in pixels. Unused by
    /// [`ConsumptionMark::Crown`], whose width is a proportion of the radius.
    #[serde(serialize_with = "serialize_short_f32")]
    pub front_width: f32,
    /// Half-length of the front as a multiple of the bubble radius.
    #[serde(serialize_with = "serialize_short_f32")]
    pub front_length_scale: f32,
    /// Whether a consuming bubble draws a ring around its rim.
    pub show_impact_ring: bool,
    /// Width of that ring, in pixels.
    #[serde(serialize_with = "serialize_short_f32")]
    pub impact_ring_width: f32,
    /// Length of the consumption trail leaking to the right, in pixels. Zero
    /// draws no trail.
    #[serde(serialize_with = "serialize_short_f32")]
    pub trail_length: f32,
    /// Alpha of the trail at the bubble's edge; it fades to nothing.
    #[serde(serialize_with = "serialize_short_f32")]
    pub trail_opacity: f32,
    /// Whether large bubbles print their quantity.
    pub show_quantity_labels: bool,
    /// Whether large bubbles print how many trades they cluster.
    pub show_trade_count: bool,
    /// Smallest radius that gets a label at all.
    #[serde(serialize_with = "serialize_short_f32")]
    pub label_min_radius: f32,
    /// Buy-side colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_color: Option<[u8; 3]>,
    /// Sell-side colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell_color: Option<[u8; 3]>,
    /// Consumption-front colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front_color: Option<[u8; 3]>,
    /// Trail colour override; absent follows the front colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trail_color: Option<[u8; 3]>,
    /// Label colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_color: Option<[u8; 3]>,
}

impl Default for BubbleStyle {
    fn default() -> Self {
        Self {
            // Area stays quantity-proportional, and the four radii are rungs
            // of one φ ladder off `max_radius` — see `PHI`.
            min_radius: DEFAULT_BUBBLE_MIN_RADIUS,
            max_radius: DEFAULT_BUBBLE_MAX_RADIUS,
            opacity: DEFAULT_BUBBLE_OPACITY,
            // Zero, not a hairline: a sub-pixel stroke is antialiased into a
            // grey fringe around every print, and in sphere mode the darkened
            // rim already is the boundary. A second edge on top of it is what
            // made each bubble wear three outlines.
            outline_width: 0.0,
            halo_strength: 0.1,
            detail_min_radius: DEFAULT_DETAIL_MIN_RADIUS,
            readable_min_radius: DEFAULT_READABLE_MIN_RADIUS,
            hollow_small_buys: false,
            // Shading is what keeps piled bubbles countable where their rims
            // coincide, which a dense tape does constantly.
            render_mode: BubbleRenderMode::Sphere,
            sphere_shading: DEFAULT_SPHERE_SHADING,
            sphere_highlight: DEFAULT_SPHERE_HIGHLIGHT,
            size_reference: BubbleSizeReference::VisibleP99,
            size_reference_quantity: 100.0,
            min_quantity: 0.0,
            // Wide enough that two prints on one price row visibly straddle
            // it, which is the second channel carrying side at speck size.
            side_offset: 3.0,
            consumption_mark: ConsumptionMark::Crown,
            front_width: 1.6,
            front_length_scale: DEFAULT_FRONT_LENGTH_SCALE,
            // The crown says "ate the book" without closing a second circle
            // around the rim; a closed ring on top of it restates the same
            // fact in the one shape that blurs the disc's own edge.
            show_impact_ring: false,
            impact_ring_width: 1.4,
            // Off. The trail fired on nearly every print, so it carried almost
            // no information, and it painted over the strip of heat map that
            // answers the question it raised — did the level refill? The knob
            // stays; the default stops smearing the book.
            trail_length: 0.0,
            trail_opacity: 0.45,
            show_quantity_labels: true,
            show_trade_count: true,
            // Only the largest bubbles get a (text-layout-costly) label: the
            // top of the radius range, not a number picked beside it.
            label_min_radius: DEFAULT_LABEL_MIN_RADIUS,
            buy_color: None,
            sell_color: None,
            front_color: None,
            trail_color: None,
            label_color: None,
        }
    }
}

impl BubbleStyle {
    /// Clamp every numeric field to a value that is safe for geometry and math.
    pub fn sanitize(&mut self) {
        self.min_radius = finite_clamp(
            self.min_radius,
            0.5,
            MAX_BUBBLE_MIN_RADIUS,
            DEFAULT_BUBBLE_MIN_RADIUS,
        );
        self.max_radius = finite_clamp(
            self.max_radius,
            self.min_radius,
            MAX_BUBBLE_MAX_RADIUS,
            DEFAULT_BUBBLE_MAX_RADIUS,
        );
        self.opacity = finite_clamp(self.opacity, 0.05, 1.0, DEFAULT_BUBBLE_OPACITY);
        self.outline_width = finite_clamp(self.outline_width, 0.0, 6.0, 0.0);
        self.halo_strength = finite_clamp(self.halo_strength, 0.0, 1.0, 0.1);
        self.detail_min_radius =
            finite_clamp(self.detail_min_radius, 0.0, 32.0, DEFAULT_DETAIL_MIN_RADIUS);
        self.readable_min_radius = finite_clamp(
            self.readable_min_radius,
            0.0,
            MAX_READABLE_MIN_RADIUS,
            DEFAULT_READABLE_MIN_RADIUS,
        );
        self.sphere_shading = finite_clamp(self.sphere_shading, 0.0, 1.0, DEFAULT_SPHERE_SHADING);
        self.sphere_highlight =
            finite_clamp(self.sphere_highlight, 0.0, 1.0, DEFAULT_SPHERE_HIGHLIGHT);
        if !self.size_reference_quantity.is_finite() || self.size_reference_quantity <= 0.0 {
            self.size_reference_quantity = 100.0;
        }
        if !self.min_quantity.is_finite() || self.min_quantity < 0.0 {
            self.min_quantity = 0.0;
        }
        self.side_offset = finite_clamp(self.side_offset, 0.0, 40.0, 3.0);
        self.front_width = finite_clamp(self.front_width, 0.5, 12.0, 1.6);
        self.front_length_scale = finite_clamp(
            self.front_length_scale,
            0.2,
            8.0,
            DEFAULT_FRONT_LENGTH_SCALE,
        );
        self.impact_ring_width = finite_clamp(self.impact_ring_width, 0.5, 8.0, 1.4);
        self.trail_length = finite_clamp(self.trail_length, 0.0, 120.0, 0.0);
        self.trail_opacity = finite_clamp(self.trail_opacity, 0.0, 1.0, 0.45);
        self.label_min_radius =
            finite_clamp(self.label_min_radius, 4.0, 64.0, DEFAULT_LABEL_MIN_RADIUS);
    }

    /// Quantity at which a print stops being dust: the exact quantity whose
    /// bubble lands on [`readable_min_radius`](Self::readable_min_radius).
    ///
    /// Inverts the renderer's area mapping — `radius = sqrt(min² + size² ·
    /// (max² − min²))` at `size² = quantity / reference` — so the threshold
    /// follows whatever radius range is configured instead of pinning a second
    /// magic number beside it. `None` means nothing can be dust: no reference
    /// to size against, or a floor already at the smallest drawn radius.
    #[must_use]
    pub fn dust_quantity(&self, reference: Decimal) -> Option<Decimal> {
        if reference <= Decimal::ZERO || self.readable_min_radius <= self.min_radius {
            return None;
        }
        let span = self.max_radius.powi(2) - self.min_radius.powi(2);
        if span <= 0.0 {
            return None;
        }
        let size_sq = (self.readable_min_radius.powi(2) - self.min_radius.powi(2)) / span;
        Decimal::from_f32(size_sq.clamp(0.0, 1.0)).map(|share| reference * share)
    }

    /// Exact quantity below which a print is not drawn, if any.
    #[must_use]
    pub fn min_quantity_decimal(&self) -> Option<Decimal> {
        (self.min_quantity > 0.0)
            .then(|| Decimal::from_f64_retain(self.min_quantity))
            .flatten()
    }

    /// Explicit full-size quantity, when the reference is fixed.
    #[must_use]
    pub fn fixed_reference_decimal(&self) -> Option<Decimal> {
        Decimal::from_f64_retain(self.size_reference_quantity)
            .filter(|value| *value > Decimal::ZERO)
    }
}
