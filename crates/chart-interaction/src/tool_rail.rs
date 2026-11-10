//! The drawing-tool rail's geometry: which stage of the rail fits an extent,
//! how long each stage is, how far the scrolling tool band can move, and which
//! window edge a dragged rail lands on.
//!
//! Pure arithmetic over pixel extents, so the rules of
//! `docs/drawing-toolbar-ux.md` are decided and tested without a window. The
//! app's rail paints what these answer and records the band window a reader
//! that is not looking at the screen names its controls from.

use crate::scripted_hand::Area;

/// The touch target of one rail button: the hit square the accessibility
/// contract requires. The app's `TOOLRAIL_ICON` reads its hit size from here.
pub const BUTTON_HIT_PX: f32 = 32.0;
/// Rail cross axis, all four docks: `44 = 6 + 32 + 6`.
pub const TOOLBOX_THICKNESS_PX: f32 = 44.0;
/// Outer margin, both axes.
pub const TOOLBOX_MARGIN_PX: f32 = 6.0;
/// Gap between buttons inside a cluster.
pub const TOOLBOX_ITEM_GAP_PX: f32 = 4.0;
/// Grip extent along the rail.
pub const TOOLBOX_GRIP_LENGTH_PX: f32 = 18.0;
/// Smallest allowed gap between the leading and trailing clusters.
pub const TOOLBOX_MIN_CLUSTER_GAP_PX: f32 = 12.0;
/// The band chevron: a slim button at each end of the scrolling tool band.
/// Shorter than a tool button on purpose — it is chrome, not a tool, and a
/// full 32 px slot each end would cost a tool's worth of band.
pub const BAND_ARROW_LENGTH_PX: f32 = 14.0;
/// Fewest tool buttons the band must be able to show before the rail gives
/// up scrolling and falls back to Compact. A band showing one icon at a time
/// is worse than the More menu it would replace.
pub const BAND_MIN_VISIBLE_ITEMS: usize = 4;
/// A separator block along the long axis: the hairline; its 4 px clear space
/// each side comes from the cluster's item spacing.
pub const SEPARATOR_BLOCK_PX: f32 = 2.0 * TOOLBOX_ITEM_GAP_PX + 1.0;
/// A chevron block along the long axis: the arrow plus the gap after it.
pub const BAND_ARROW_BLOCK_PX: f32 = BAND_ARROW_LENGTH_PX + TOOLBOX_ITEM_GAP_PX;
/// The grip block: its extent plus the item gap that follows.
pub const GRIP_BLOCK_PX: f32 = TOOLBOX_GRIP_LENGTH_PX + TOOLBOX_ITEM_GAP_PX;

/// One of the three window edges the toolbar can dock against.
///
/// The right edge is deliberately absent. That border belongs to the price
/// axis and to whatever the trader is reading as the tape prints; a rail
/// parked there covers the one column of the chart that is always moving,
/// and it is reachable by accident — a grip drag that drifts right used to
/// land it in the way. Three edges, and the busy one stays clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolboxDock {
    #[default]
    Left,
    Top,
    Bottom,
}

impl ToolboxDock {
    #[must_use]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Left)
    }

    /// The nearest edge by *normalised* distance — raw pixels would bias
    /// every drop toward top/bottom on a wide window. Ties resolve
    /// Left > Top > Bottom, so the result is deterministic. A drop on the
    /// right half simply lands on whichever of the three offered edges is
    /// closest; there is no right dock to fall into.
    #[must_use]
    pub fn nearest(pointer: (f32, f32), screen: Area) -> Self {
        let x = pointer.0.clamp(screen.left, screen.right);
        let y = pointer.1.clamp(screen.top, screen.bottom);
        let width = (screen.right - screen.left).max(f32::EPSILON);
        let height = (screen.bottom - screen.top).max(f32::EPSILON);
        let candidates = [
            (Self::Left, (x - screen.left) / width),
            (Self::Top, (y - screen.top) / height),
            (Self::Bottom, (screen.bottom - y) / height),
        ];
        let mut best = candidates[0];
        for candidate in &candidates[1..] {
            if candidate.1 < best.1 {
                best = *candidate;
            }
        }
        best.0
    }
}

/// How much of the rail fits along its long axis. Stages are pure functions
/// of the available extent, so a resize is hysteresis-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailStage {
    /// Every tool slot visible.
    Full,
    /// Every tool still reachable, but the run scrolls between two chevrons.
    /// Grip, Pointer, Crosshair and the pinned favorites stay anchored.
    Scroll,
    /// Pointer, Crosshair, the armed tool and More; full trailing cluster.
    Compact,
    /// Pointer, the armed tool, More and Objects.
    Minimal,
}

/// One control the rail actually paints, as something that is not looking at
/// the screen would name it.
///
/// A family that folded into one slot is *one* control here, not one per
/// member: the members behind its flyout have no button of their own, and a
/// scene that named them would be describing a rail nobody is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RailControl {
    /// Which registry the id below is drawn from — the namespace that keeps
    /// two different buttons from answering to one name.
    pub kind: RailControlKind,
    /// The tool's registered id, or the family's — stable either way.
    pub id: &'static str,
    pub label: &'static str,
    /// Whether the armed tool is this control, or a member behind it.
    pub armed: bool,
}

/// Which of the rail's three button registries a [`RailControl`] came from.
///
/// The three share no namespace: `ToolFamily::id` and `DrawingTool::id` are
/// separate registries that already collide (`brush` and `measure` name both
/// a family and a tool), and a starred tool is painted a second time in the
/// pinned section beside its slot in the run. Without this, one identifier
/// would name the family flyout on a wide window and the tool itself on a
/// narrow one, and a favorite would answer to the same name as the run slot
/// it is pinned from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailControlKind {
    /// A single tool's own button.
    Tool,
    /// One slot standing for a family, with its members behind a flyout.
    Family,
    /// A starred tool, pinned to the rail's head with a star badge.
    Favorite,
}

/// The slice of the scrolling band one draw put on screen.
///
/// The band's content is the spilled favorites followed by every tool slot;
/// the viewport shows `visible` whole buttons of it starting at `first`, and
/// the chevrons move that window. Recorded because a reader that is not
/// looking at the screen cannot re-derive it: it depends on the extent the
/// layout handed the rail and on wherever the trader last scrolled to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandWindow {
    /// Favorites anchored ahead of the band, outside its clip.
    pub anchored: usize,
    /// Index into the band's content of its first visible button.
    pub first: usize,
    /// Whole buttons the viewport shows.
    pub visible: usize,
}

/// Long-axis length of a run of `count` rail buttons, gaps between them.
pub fn run_length(count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let n = count as f32;
    n * BUTTON_HIT_PX + (n - 1.0) * TOOLBOX_ITEM_GAP_PX
}

/// The anchored head of the leading cluster: grip, Pointer, Crosshair, the
/// separator, and the pinned favorites behind a separator of their own. This
/// never scrolls — the trader starred these to keep them under the pointer,
/// and the two navigation tools are the way out of any drawing mode.
pub fn leading_anchor_length(anchored_favorites: usize) -> f32 {
    let favorites = if anchored_favorites == 0 {
        0.0
    } else {
        SEPARATOR_BLOCK_PX + anchored_favorites as f32 * (BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX)
    };
    GRIP_BLOCK_PX + (2.0 * BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX) + SEPARATOR_BLOCK_PX + favorites
}

/// Everything the tool band cannot spend: margins, the anchored head, the
/// minimum cluster gap and the trailing cluster.
pub fn fixed_length(anchored_favorites: usize) -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + leading_anchor_length(anchored_favorites)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + trailing_length()
}

/// Long-axis length of the full rail (§2.8 of the spec): the fixed chrome
/// plus every tool slot, no scrolling. The favorites section only exists
/// when something is starred, so an empty list costs no length.
pub fn full_length(tool_slot_count: usize, favorite_count: usize) -> f32 {
    fixed_length(favorite_count) + run_length(tool_slot_count)
}

/// Long-axis length at the Scroll stage: the fixed chrome, both chevrons and
/// the band's floor of visible tools.
pub fn scroll_length(anchored_favorites: usize) -> f32 {
    fixed_length(anchored_favorites)
        + 2.0 * BAND_ARROW_BLOCK_PX
        + run_length(BAND_MIN_VISIBLE_ITEMS)
}

/// How many favorites stay anchored outside the band. A pin is only worth
/// anchoring while the band keeps its floor of visible tools; past that the
/// surplus spills into the band's head — still star-badged, still reachable
/// by scrolling — rather than pushing the whole rail into Compact, which is
/// what used to make the fourth star swallow the toolbar.
pub fn anchored_favorites(available: f32, favorite_count: usize) -> usize {
    let mut anchored = favorite_count;
    while anchored > 0 && available < scroll_length(anchored) {
        anchored -= 1;
    }
    anchored
}

/// Long-axis extent the band itself gets, chevrons excluded — snapped down
/// to a whole number of buttons.
///
/// A toolbar button is atomic: half an icon is not readable, not clickable
/// with confidence, and reads as a rendering fault rather than as "there is
/// more below". Sizing the viewport to whole slots makes every offset a
/// chevron can reach a multiple of one slot too, because the surplus
/// `content - viewport` then divides exactly. The leftover px are spent by
/// the flexible gap between the clusters, where nothing is drawn.
pub fn band_viewport(available: f32, anchored_favorites: usize) -> f32 {
    let raw = (available - fixed_length(anchored_favorites) - 2.0 * BAND_ARROW_BLOCK_PX).max(0.0);
    if raw < BUTTON_HIT_PX {
        return raw;
    }
    run_length(band_visible_items(raw))
}

/// How far the band can scroll: zero when everything already fits.
pub fn band_max_offset(viewport: f32, item_count: usize) -> f32 {
    (run_length(item_count) - viewport).max(0.0)
}

/// Whole buttons visible in `viewport` — at least one, so a chevron click
/// always moves.
pub fn band_visible_items(viewport: f32) -> usize {
    let slot = BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX;
    (((viewport + TOOLBOX_ITEM_GAP_PX) / slot).floor() as usize).max(1)
}

/// One chevron click: a page less one button, so the tool the trader was
/// looking at stays on screen to anchor where they landed.
pub fn band_scroll_step(viewport: f32) -> f32 {
    let page = band_visible_items(viewport).saturating_sub(1).max(1);
    page as f32 * (BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX)
}

/// Long-axis length at the Compact stage: the tool run gives way to the
/// armed slot plus More.
pub fn compact_length() -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + GRIP_BLOCK_PX
        + (2.0 * BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX)
        + SEPARATOR_BLOCK_PX
        + (2.0 * BUTTON_HIT_PX + TOOLBOX_ITEM_GAP_PX)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + trailing_length()
}

/// Long-axis length at the Minimal stage: grip, Pointer, the armed tool,
/// More, the cluster gap, one separator and Objects. The spec's 191 px
/// floor — `main.rs` sets a minimum window size that keeps it unreachable.
#[must_use]
pub fn minimal_length() -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + GRIP_BLOCK_PX
        + (3.0 * BUTTON_HIT_PX + 2.0 * TOOLBOX_ITEM_GAP_PX)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + SEPARATOR_BLOCK_PX
        + BUTTON_HIT_PX
}

/// The full trailing cluster: separator, magnet, repeat, hide-all, lock-all,
/// separator, Objects.
pub fn trailing_length() -> f32 {
    SEPARATOR_BLOCK_PX
        + (4.0 * BUTTON_HIT_PX + 3.0 * TOOLBOX_ITEM_GAP_PX)
        + SEPARATOR_BLOCK_PX
        + BUTTON_HIT_PX
}

/// Resolve the stage for an available long-axis extent (margins included).
/// The Scroll boundary is measured with no favorite anchored, because
/// [`anchored_favorites`] gives pins up one at a time before the band is
/// ever asked to give up its floor.
pub fn stage_for(available: f32, tool_slot_count: usize, favorite_count: usize) -> RailStage {
    if available >= full_length(tool_slot_count, favorite_count) {
        RailStage::Full
    } else if available >= scroll_length(0) {
        RailStage::Scroll
    } else if available >= compact_length() {
        RailStage::Compact
    } else {
        RailStage::Minimal
    }
}

#[cfg(test)]
#[path = "tool_rail_tests.rs"]
mod tool_rail_tests;
