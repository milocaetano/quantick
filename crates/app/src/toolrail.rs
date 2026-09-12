//! Drawing-tool selection and the four-edge toolbar rail.
//!
//! The rail owns only chrome state. Drawing definitions and metadata live in
//! [`crate::drawings`], so registering a new drawing does not require another
//! matching list in this module. Geometry, states and docking behaviour
//! follow `docs/drawing-toolbar-ux.md`.

use std::collections::BTreeMap;

use eframe::egui;
use egui_phosphor::regular as icons;

use crate::drawings::{
    DRAWING_TOOLS, DrawingTool, Drawings, IconDots, IconLetter, IconStrokes, ToolFamily,
};
use crate::theme;
use crate::widgets::{IconButton, MarkerEdge, TOOLRAIL_ICON, paint_vector_icon};

mod band;
mod flyout;
mod state;

/// Rail cross axis, all four docks: `44 = 6 + 32 + 6`.
const TOOLBOX_THICKNESS_PX: f32 = 44.0;
/// Outer margin, both axes.
const TOOLBOX_MARGIN_PX: f32 = 6.0;
/// Gap between buttons inside a cluster.
const TOOLBOX_ITEM_GAP_PX: f32 = 4.0;
/// Inset of a separator hairline from each rail edge, so the rule floats.
const TOOLBOX_SEPARATOR_INSET_PX: f32 = 8.0;
/// Grip extent along the rail.
const TOOLBOX_GRIP_LENGTH_PX: f32 = 18.0;
/// Grip glyph font size.
const TOOLBOX_GRIP_GLYPH_PX: f32 = 14.0;
/// Smallest allowed gap between the leading and trailing clusters.
const TOOLBOX_MIN_CLUSTER_GAP_PX: f32 = 12.0;
/// Square corner zone of a family button that opens its flyout.
const TOOLBOX_CARET_ZONE_PX: f32 = 10.0;
/// Family flyout popup width.
const TOOLBOX_FLYOUT_WIDTH_PX: f32 = 208.0;
/// One family flyout row.
const TOOLBOX_FLYOUT_ROW_HEIGHT_PX: f32 = 26.0;
/// Chart-facing accent line of the drop preview band.
const TOOLBOX_DROP_BAND_EDGE_PX: f32 = 3.0;
/// Flyout paint metrics: popup corner radius, row backdrop radius, the
/// glyph centre / name / shortcut columns and their font sizes.
const FLYOUT_CORNER_RADIUS_PX: f32 = 6.0;
const FLYOUT_ROW_RADIUS_PX: f32 = 4.0;
const FLYOUT_GLYPH_CENTER_X_PX: f32 = 12.0;
const FLYOUT_NAME_X_PX: f32 = 26.0;
const FLYOUT_SHORTCUT_INSET_PX: f32 = 6.0;
const FLYOUT_GLYPH_PX: f32 = 18.0;
const FLYOUT_NAME_TEXT_PX: f32 = 12.0;
const FLYOUT_SHORTCUT_TEXT_PX: f32 = 11.0;
const FLYOUT_HEADER_TEXT_PX: f32 = 11.0;
/// The favorite star holds the row's right edge (trader feedback: beside
/// the name, not on the icon), with the shortcut label stepped one slot
/// left so the two never collide. The hit zone is bigger than the glyph —
/// a 9 px star is no hit target — and stays clear of the icon and name, so
/// an arming click can never silently star.
/// Side of the icon box in a flyout row — the smallest box a vector icon is
/// ever painted into, and therefore the size the icon guards measure against.
pub(crate) const FLYOUT_ICON_BOX_PX: f32 = FLYOUT_GLYPH_PX - 4.0;
const FLYOUT_STAR_PX: f32 = 9.0;
const FLYOUT_STAR_RIGHT_INSET_PX: f32 = 10.0;
const FLYOUT_STAR_HIT_PX: f32 = 14.0;
/// Width the star column reserves at the row's right end; the shortcut
/// label right-aligns just left of it.
const FLYOUT_STAR_SLOT_PX: f32 = 20.0;
/// The corner star badge marking a pinned button in the rail's favorites
/// section.
const FAVORITE_BADGE_PX: f32 = 8.0;
const FAVORITE_BADGE_INSET_PX: f32 = 3.0;
/// Side of the caret triangle on a family slot.
const CARET_SIDE_PX: f32 = 5.0;
/// Inset of the caret from the button's trailing-bottom corner.
const CARET_INSET_PX: f32 = 3.0;
/// Badge pill geometry (§2.5 of the spec).
const BADGE_HEIGHT_PX: f32 = 12.0;
const BADGE_RADIUS_PX: f32 = 3.0;
const BADGE_TEXT_PX: f32 = 9.0;
const BADGE_PAD_X_PX: f32 = 3.0;
const BADGE_CORNER_INSET_PX: f32 = 2.0;
/// The band chevron: a slim button at each end of the scrolling tool band.
/// Shorter than a tool button on purpose — it is chrome, not a tool, and a
/// full 32 px slot each end would cost a tool's worth of band.
const BAND_ARROW_LENGTH_PX: f32 = 14.0;
/// Chevron glyph size, and the inset of the band's fade hint from its edge.
const BAND_ARROW_GLYPH_PX: f32 = 12.0;
/// Fewest tool buttons the band must be able to show before the rail gives
/// up scrolling and falls back to Compact. A band showing one icon at a time
/// is worse than the More menu it would replace.
const BAND_MIN_VISIBLE_ITEMS: usize = 4;
/// A separator block along the long axis: the hairline; its 4 px clear space
/// each side comes from the cluster's item spacing.
const SEPARATOR_BLOCK_PX: f32 = 2.0 * TOOLBOX_ITEM_GAP_PX + 1.0;
/// A chevron block along the long axis: the arrow plus the gap after it.
const BAND_ARROW_BLOCK_PX: f32 = BAND_ARROW_LENGTH_PX + TOOLBOX_ITEM_GAP_PX;
/// The grip block: its extent plus the item gap that follows.
const GRIP_BLOCK_PX: f32 = TOOLBOX_GRIP_LENGTH_PX + TOOLBOX_ITEM_GAP_PX;
#[cfg(test)]
const TOOLBOX_BUTTON_COUNT: usize = DRAWING_TOOLS.len() + 4;

/// A chart-acting tool. Only one is armed at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Pointer,
    Crosshair,
    Drawing(DrawingTool),
}

impl Tool {
    /// The stable wire name, for the places that name a tool rather than draw
    /// it. The drawing tools lend their own registered identifier, so a tool
    /// is called the same thing in the rail, in a saved workspace and in the
    /// semantic scene.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Crosshair => "crosshair",
            Self::Drawing(tool) => tool.id(),
        }
    }

    #[must_use]
    pub fn drawing_tool(self) -> Option<DrawingTool> {
        match self {
            Self::Drawing(tool) => Some(tool),
            Self::Pointer | Self::Crosshair => None,
        }
    }

    #[must_use]
    fn icon(self) -> &'static str {
        match self {
            Self::Pointer => icons::CURSOR,
            Self::Crosshair => icons::CROSSHAIR,
            Self::Drawing(tool) => tool.icon(),
        }
    }

    #[must_use]
    fn icon_strokes(self) -> IconStrokes {
        match self {
            Self::Pointer | Self::Crosshair => &[],
            Self::Drawing(tool) => tool.icon_strokes(),
        }
    }

    #[must_use]
    fn icon_dots(self) -> IconDots {
        match self {
            Self::Pointer | Self::Crosshair => &[],
            Self::Drawing(tool) => tool.icon_dots(),
        }
    }

    #[must_use]
    fn icon_letter(self) -> Option<IconLetter> {
        match self {
            Self::Pointer | Self::Crosshair => None,
            Self::Drawing(tool) => tool.icon_letter(),
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer",
            Self::Crosshair => "Crosshair",
            Self::Drawing(tool) => tool.name(),
        }
    }

    #[must_use]
    fn hover_text(self) -> &'static str {
        match self {
            Self::Pointer => "Pointer - pan, zoom, select and move (1, Esc)",
            Self::Crosshair => "Crosshair (2)",
            Self::Drawing(tool) => tool.hover_text(),
        }
    }

    /// The shortcut label shown in menus, `None` when the tool has no key.
    #[must_use]
    fn shortcut_label(self) -> Option<String> {
        match self {
            Self::Pointer => Some("1".to_owned()),
            Self::Crosshair => Some("2".to_owned()),
            Self::Drawing(tool) => tool.shortcut().map(|shortcut| {
                let key = shortcut.key.name();
                if shortcut.shift {
                    format!("Shift+{key}")
                } else {
                    key.to_owned()
                }
            }),
        }
    }
}

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
    fn nearest(pointer: egui::Pos2, screen: egui::Rect) -> Self {
        let pointer = pointer.clamp(screen.min, screen.max);
        let width = screen.width().max(f32::EPSILON);
        let height = screen.height().max(f32::EPSILON);
        let candidates = [
            (Self::Left, (pointer.x - screen.left()) / width),
            (Self::Top, (pointer.y - screen.top()) / height),
            (Self::Bottom, (screen.bottom() - pointer.y) / height),
        ];
        let mut best = candidates[0];
        for candidate in &candidates[1..] {
            if candidate.1 < best.1 {
                best = *candidate;
            }
        }
        best.0
    }

    /// The button edge the active marker hugs: the one facing the window
    /// border this dock sits against.
    #[must_use]
    const fn marker_edge(self) -> MarkerEdge {
        match self {
            Self::Left => MarkerEdge::Left,
            Self::Top => MarkerEdge::Top,
            Self::Bottom => MarkerEdge::Bottom,
        }
    }
}

/// How much of the rail fits along its long axis. Stages are pure functions
/// of the available extent, so a resize is hysteresis-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailStage {
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
pub(crate) struct RailControl {
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
pub(crate) enum RailControlKind {
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
struct BandWindow {
    /// Favorites anchored ahead of the band, outside its clip.
    anchored: usize,
    /// Index into the band's content of its first visible button.
    first: usize,
    /// Whole buttons the viewport shows.
    visible: usize,
}

/// One leading-cluster slot after family folding: a lone tool, or a family
/// of consecutive registry entries sharing the slot.
enum RailSlot {
    Single(DrawingTool),
    Family {
        family: ToolFamily,
        members: Vec<DrawingTool>,
    },
}

/// Fold the registry into rail slots. Consecutive entries with the same
/// family id share one slot — consecutive, not sorted, so rail order stays
/// registry order and adding a tool cannot silently reorder the rail.
/// Folded once per process: the registry is `const`, so rebuilding this
/// every frame would be per-frame allocation of stable data.
fn tool_slots() -> &'static [RailSlot] {
    static SLOTS: std::sync::OnceLock<Vec<RailSlot>> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| {
        let mut slots: Vec<RailSlot> = Vec::new();
        for tool in DRAWING_TOOLS {
            match tool.family() {
                Some(family) => {
                    if let Some(RailSlot::Family {
                        family: previous,
                        members,
                    }) = slots.last_mut()
                        && previous.id == family.id
                    {
                        members.push(tool);
                    } else {
                        slots.push(RailSlot::Family {
                            family,
                            members: vec![tool],
                        });
                    }
                }
                None => slots.push(RailSlot::Single(tool)),
            }
        }
        slots
    })
}

/// Long-axis length of a run of `count` rail buttons, gaps between them.
fn run_length(count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let n = count as f32;
    n * TOOLRAIL_ICON.hit + (n - 1.0) * TOOLBOX_ITEM_GAP_PX
}

/// The anchored head of the leading cluster: grip, Pointer, Crosshair, the
/// separator, and the pinned favorites behind a separator of their own. This
/// never scrolls — the trader starred these to keep them under the pointer,
/// and the two navigation tools are the way out of any drawing mode.
fn leading_anchor_length(anchored_favorites: usize) -> f32 {
    let favorites = if anchored_favorites == 0 {
        0.0
    } else {
        SEPARATOR_BLOCK_PX + anchored_favorites as f32 * (TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX)
    };
    GRIP_BLOCK_PX + (2.0 * TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX) + SEPARATOR_BLOCK_PX + favorites
}

/// Everything the tool band cannot spend: margins, the anchored head, the
/// minimum cluster gap and the trailing cluster.
fn fixed_length(anchored_favorites: usize) -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + leading_anchor_length(anchored_favorites)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + trailing_length()
}

/// Long-axis length of the full rail (§2.8 of the spec): the fixed chrome
/// plus every tool slot, no scrolling. The favorites section only exists
/// when something is starred, so an empty list costs no length.
fn full_length(tool_slot_count: usize, favorite_count: usize) -> f32 {
    fixed_length(favorite_count) + run_length(tool_slot_count)
}

/// Long-axis length at the Scroll stage: the fixed chrome, both chevrons and
/// the band's floor of visible tools.
fn scroll_length(anchored_favorites: usize) -> f32 {
    fixed_length(anchored_favorites)
        + 2.0 * BAND_ARROW_BLOCK_PX
        + run_length(BAND_MIN_VISIBLE_ITEMS)
}

/// How many favorites stay anchored outside the band. A pin is only worth
/// anchoring while the band keeps its floor of visible tools; past that the
/// surplus spills into the band's head — still star-badged, still reachable
/// by scrolling — rather than pushing the whole rail into Compact, which is
/// what used to make the fourth star swallow the toolbar.
fn anchored_favorites(available: f32, favorite_count: usize) -> usize {
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
fn band_viewport(available: f32, anchored_favorites: usize) -> f32 {
    let raw = (available - fixed_length(anchored_favorites) - 2.0 * BAND_ARROW_BLOCK_PX).max(0.0);
    if raw < TOOLRAIL_ICON.hit {
        return raw;
    }
    run_length(band_visible_items(raw))
}

/// How far the band can scroll: zero when everything already fits.
fn band_max_offset(viewport: f32, item_count: usize) -> f32 {
    (run_length(item_count) - viewport).max(0.0)
}

/// Whole buttons visible in `viewport` — at least one, so a chevron click
/// always moves.
fn band_visible_items(viewport: f32) -> usize {
    let slot = TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX;
    (((viewport + TOOLBOX_ITEM_GAP_PX) / slot).floor() as usize).max(1)
}

/// One chevron click: a page less one button, so the tool the trader was
/// looking at stays on screen to anchor where they landed.
fn band_scroll_step(viewport: f32) -> f32 {
    let page = band_visible_items(viewport).saturating_sub(1).max(1);
    page as f32 * (TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX)
}

/// Long-axis length at the Compact stage: the tool run gives way to the
/// armed slot plus More.
fn compact_length() -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + GRIP_BLOCK_PX
        + (2.0 * TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX)
        + SEPARATOR_BLOCK_PX
        + (2.0 * TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + trailing_length()
}

/// Long-axis length at the Minimal stage: grip, Pointer, the armed tool,
/// More, the cluster gap, one separator and Objects. The spec's 191 px
/// floor — `main.rs` sets a minimum window size that keeps it unreachable.
#[cfg(test)]
fn minimal_length() -> f32 {
    2.0 * TOOLBOX_MARGIN_PX
        + GRIP_BLOCK_PX
        + (3.0 * TOOLRAIL_ICON.hit + 2.0 * TOOLBOX_ITEM_GAP_PX)
        + TOOLBOX_MIN_CLUSTER_GAP_PX
        + SEPARATOR_BLOCK_PX
        + TOOLRAIL_ICON.hit
}

/// The full trailing cluster: separator, magnet, repeat, hide-all, lock-all,
/// separator, Objects.
fn trailing_length() -> f32 {
    SEPARATOR_BLOCK_PX
        + (4.0 * TOOLRAIL_ICON.hit + 3.0 * TOOLBOX_ITEM_GAP_PX)
        + SEPARATOR_BLOCK_PX
        + TOOLRAIL_ICON.hit
}

/// Resolve the stage for an available long-axis extent (margins included).
/// The Scroll boundary is measured with no favorite anchored, because
/// [`anchored_favorites`] gives pins up one at a time before the band is
/// ever asked to give up its floor.
fn stage_for(available: f32, tool_slot_count: usize, favorite_count: usize) -> RailStage {
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

/// Toolbar chrome state. The panel is always outside `CentralPanel`, so the
/// chart never renders behind it.
#[derive(Debug)]
pub struct ToolRail {
    tool: Tool,
    visible: bool,
    /// The stage the last draw settled on, or `None` before the first — and
    /// again once the rail is hidden, because nothing is painted then.
    ///
    /// Read by the semantic scene, never by the draw: the stage is a pure
    /// function of the extent and is recomputed every frame regardless.
    last_stage: Option<RailStage>,
    /// Which slice of the scrolling band the last draw actually showed.
    ///
    /// Only the Scroll stage has one; every other stage clears it. The band
    /// clips, so this is the difference between the buttons the rail owns and
    /// the buttons a trader can see — the scene reports the second.
    last_band: Option<BandWindow>,
    dock: ToolboxDock,
    /// The repeat pin: `true` keeps a drawing tool armed after it completes
    /// an object; the default is one-shot back to Pointer.
    repeat: bool,
    /// The magnet: anchors snap to the nearest OHLC of the bar under the
    /// pointer. Off by default — a magnet nobody asked for moves marks the
    /// trader placed deliberately.
    magnet: bool,
    /// Last-armed member of each tool family, keyed by family id.
    last_family_member: BTreeMap<&'static str, DrawingTool>,
    /// Starred tools, in the order the trader starred them — the pinned
    /// section at the rail's tool end. Star order, not registry order, so a
    /// favorite keeps the position the trader learned.
    favorites: Vec<DrawingTool>,
    /// A star was clicked and the choice has not been written down yet. Read
    /// and cleared by [`ToolRail::take_favorites_change`]; set by the toggle
    /// only, never by [`ToolRail::set_favorites`] — restoring the saved list
    /// is not the trader making a choice, and saving it straight back would
    /// rewrite the file on every launch for nothing.
    favorites_changed: bool,
    /// Scroll offset of the tool band along the rail's long axis, in px.
    /// Only the Scroll stage spends it; every other stage clamps it back to
    /// zero, so unstarring back down to a rail that fits leaves no residue.
    band_offset: f32,
    /// An offset a chevron click asked for, handed to the band on the next
    /// frame. `None` leaves the band to the wheel and to drag scrolling.
    band_target: Option<f32>,
    /// The armed tool changed and the band has not yet been asked to show
    /// it. The spec's standing promise is that the armed tool always keeps a
    /// real slot (§2.8); a keyboard shortcut can arm a tool the band has
    /// scrolled past, and a trader who cannot see what is armed does not
    /// know what their next click will draw. Set on arming only, never held,
    /// so scrolling away from the armed tool by hand stays where it was put.
    reveal_armed: bool,
    /// Currently-nearest drop edge while a grip drag is live.
    drag_preview: Option<ToolboxDock>,
    dragging: bool,
    drag_cancelled: bool,
    /// Open family flyout: the family id and the slot rect it anchors to.
    flyout: Option<(&'static str, egui::Rect)>,
    /// A family flyout a validation hook asked for before the first frame —
    /// honoured by the family slot once it knows its rect, then cleared.
    hook_flyout: Option<String>,
    #[cfg(test)]
    button_rects: [Option<(Tool, egui::Rect)>; TOOLBOX_BUTTON_COUNT],
    #[cfg(test)]
    grip_rect: Option<egui::Rect>,
    #[cfg(test)]
    magnet_rect: Option<egui::Rect>,
    #[cfg(test)]
    more_rect: Option<egui::Rect>,
    #[cfg(test)]
    hide_all_rect: Option<egui::Rect>,
    #[cfg(test)]
    lock_all_rect: Option<egui::Rect>,
    #[cfg(test)]
    objects_rect: Option<egui::Rect>,
    #[cfg(test)]
    rail_rect: Option<egui::Rect>,
    #[cfg(test)]
    flyout_rects: Vec<(DrawingTool, egui::Rect)>,
    #[cfg(test)]
    flyout_star_rects: Vec<(DrawingTool, egui::Rect)>,
    #[cfg(test)]
    favorite_rects: Vec<(DrawingTool, egui::Rect)>,
    /// The band chevrons and whether each one had somewhere to go.
    #[cfg(test)]
    band_leading_arrow: Option<(egui::Rect, bool)>,
    #[cfg(test)]
    band_trailing_arrow: Option<(egui::Rect, bool)>,
    /// The band's viewport. A button is only *reachable* if it lands inside
    /// this — allocation alone proves nothing once the band clips.
    #[cfg(test)]
    band_rect: Option<egui::Rect>,
}

impl Default for ToolRail {
    fn default() -> Self {
        Self {
            tool: Tool::Pointer,
            visible: true,
            last_stage: None,
            last_band: None,
            dock: ToolboxDock::Left,
            repeat: false,
            magnet: false,
            last_family_member: BTreeMap::new(),
            favorites: Vec::new(),
            favorites_changed: false,
            band_offset: 0.0,
            band_target: None,
            reveal_armed: false,
            drag_preview: None,
            dragging: false,
            drag_cancelled: false,
            flyout: None,
            hook_flyout: None,
            #[cfg(test)]
            button_rects: [None; TOOLBOX_BUTTON_COUNT],
            #[cfg(test)]
            grip_rect: None,
            #[cfg(test)]
            magnet_rect: None,
            #[cfg(test)]
            more_rect: None,
            #[cfg(test)]
            hide_all_rect: None,
            #[cfg(test)]
            lock_all_rect: None,
            #[cfg(test)]
            objects_rect: None,
            #[cfg(test)]
            rail_rect: None,
            #[cfg(test)]
            flyout_rects: Vec::new(),
            #[cfg(test)]
            flyout_star_rects: Vec::new(),
            #[cfg(test)]
            favorite_rects: Vec::new(),
            #[cfg(test)]
            band_leading_arrow: None,
            #[cfg(test)]
            band_trailing_arrow: None,
            #[cfg(test)]
            band_rect: None,
        }
    }
}

impl ToolRail {
    /// Draw the rail docked against its edge. Drag the grip and release: the
    /// nearest window edge becomes the new dock, previewed live by a band.
    /// The rail also hosts the object-manager entry and the global protection
    /// toggles (hide-all / lock-all), which act on the store.
    pub fn draw(&mut self, ctx: &egui::Context, drawings: &mut Drawings, manager_open: &mut bool) {
        if !self.visible {
            return;
        }

        match self.dock {
            ToolboxDock::Left => egui::SidePanel::left("drawing_toolbox_left")
                .exact_width(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame())
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
            ToolboxDock::Top => egui::TopBottomPanel::top("drawing_toolbox_top")
                .exact_height(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame())
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
            ToolboxDock::Bottom => egui::TopBottomPanel::bottom("drawing_toolbox_bottom")
                .exact_height(TOOLBOX_THICKNESS_PX)
                .resizable(false)
                .frame(rail_frame())
                .show(ctx, |ui| self.draw_contents(ui, drawings, manager_open)),
        };

        if let Some(target) = self.drag_preview.take() {
            paint_drop_preview(ctx, target);
        }
    }

    fn draw_contents(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &mut Drawings,
        manager_open: &mut bool,
    ) {
        #[cfg(test)]
        {
            self.button_rects.fill(None);
            self.grip_rect = None;
            self.magnet_rect = None;
            self.more_rect = None;
            self.hide_all_rect = None;
            self.lock_all_rect = None;
            self.objects_rect = None;
            self.rail_rect = Some(ui.max_rect().expand(TOOLBOX_MARGIN_PX));
            self.flyout_rects.clear();
            self.flyout_star_rects.clear();
            self.favorite_rects.clear();
            self.band_leading_arrow = None;
            self.band_trailing_arrow = None;
            self.band_rect = None;
        }

        let vertical = self.dock.is_vertical();
        let available = if vertical {
            ui.available_height()
        } else {
            ui.available_width()
        } + 2.0 * TOOLBOX_MARGIN_PX;
        let slots = tool_slots();
        let favorite_count = self.favorites.len();
        let stage = stage_for(available, slots.len(), favorite_count);
        // The one thing the rail writes down for a later reader: which stage
        // it drew. A single enum store per frame, and the only way the
        // semantic scene can answer "what has a button right now" without
        // re-deriving a layout it did not measure.
        self.last_stage = Some(stage);

        // Chart-facing hairline: the only stroke the rail paints — a
        // four-sided stroke would draw a seam against the window edge.
        let rail_rect = ui.max_rect().expand(TOOLBOX_MARGIN_PX);
        let edge = match self.dock {
            ToolboxDock::Left => [rail_rect.right_top(), rail_rect.right_bottom()],
            ToolboxDock::Top => [rail_rect.left_bottom(), rail_rect.right_bottom()],
            ToolboxDock::Bottom => [rail_rect.left_top(), rail_rect.right_top()],
        };
        ui.painter()
            .line_segment(edge, egui::Stroke::new(1.0_f32, theme::BORDER));

        let leading = if vertical {
            egui::Layout::top_down(egui::Align::Center)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        };
        let trailing = if vertical {
            egui::Layout::bottom_up(egui::Align::Center)
        } else {
            egui::Layout::right_to_left(egui::Align::Center)
        };

        ui.with_layout(leading, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(TOOLBOX_ITEM_GAP_PX, TOOLBOX_ITEM_GAP_PX);
            ui.set_min_size(egui::Vec2::ZERO);
            self.draw_grip(ui, vertical);
            self.draw_button(ui, Tool::Pointer, drawings);
            if stage != RailStage::Minimal {
                self.draw_button(ui, Tool::Crosshair, drawings);
                self.draw_separator(ui, vertical);
            }
            match stage {
                RailStage::Full => {
                    self.band_offset = 0.0;
                    self.band_target = None;
                    self.last_band = None;
                    self.draw_favorites_section(ui, vertical, drawings, 0..favorite_count);
                    for slot in slots {
                        match slot {
                            RailSlot::Single(tool) => {
                                self.draw_button(ui, Tool::Drawing(*tool), drawings);
                            }
                            RailSlot::Family { family, members } => {
                                self.draw_family_slot(ui, *family, members, drawings);
                            }
                        }
                    }
                }
                RailStage::Scroll => {
                    let anchored = anchored_favorites(available, favorite_count);
                    self.draw_favorites_section(ui, vertical, drawings, 0..anchored);
                    self.draw_band(ui, vertical, drawings, slots, anchored, available);
                }
                RailStage::Compact | RailStage::Minimal => {
                    self.band_offset = 0.0;
                    self.band_target = None;
                    self.last_band = None;
                    if let Some(armed) = self.tool.drawing_tool() {
                        self.draw_button(ui, Tool::Drawing(armed), drawings);
                    }
                    self.draw_more_menu(ui, drawings, stage);
                }
            }
        });

        // The trailing cluster is pinned to the rail's far end — laid from
        // that end backwards, so the flexible gap sits between the clusters.
        ui.with_layout(trailing, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(TOOLBOX_ITEM_GAP_PX, TOOLBOX_ITEM_GAP_PX);
            self.draw_objects_button(ui, drawings, manager_open);
            self.draw_separator(ui, vertical);
            if stage != RailStage::Minimal {
                self.draw_global_buttons(ui, drawings);
                self.draw_repeat_button(ui);
                self.draw_magnet_button(ui);
                self.draw_separator(ui, vertical);
            }
        });

        self.draw_family_flyout(ui.ctx());
    }

    /// The grip: hold and drag to dock the rail against another window edge.
    fn draw_grip(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let size = if vertical {
            egui::vec2(TOOLRAIL_ICON.hit, TOOLBOX_GRIP_LENGTH_PX)
        } else {
            egui::vec2(TOOLBOX_GRIP_LENGTH_PX, TOOLRAIL_ICON.hit)
        };
        let (rect, grip) = ui.allocate_exact_size(size, egui::Sense::drag());
        #[cfg(test)]
        {
            self.grip_rect = Some(rect);
        }
        if ui.is_rect_visible(rect) {
            // The dots always run across the rail, reading as a handle.
            let glyph = if vertical {
                icons::DOTS_SIX
            } else {
                icons::DOTS_SIX_VERTICAL
            };
            let color = if grip.hovered() || self.dragging {
                theme::TEXT_MUTED
            } else {
                theme::TEXT_FAINT
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(TOOLBOX_GRIP_GLYPH_PX),
                color,
            );
        }
        let grip = grip.on_hover_text("Drag to dock the toolbar on another edge");
        if grip.hovered() && !self.dragging {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if grip.drag_started() {
            self.dragging = true;
            self.drag_cancelled = false;
        }
        if self.dragging && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
            // Esc aborts the drag and keeps the current dock — the topmost
            // level of the app's escape stack.
            self.dragging = false;
            self.drag_cancelled = true;
        }
        if self.dragging {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(pointer) = ui.ctx().input(|input| input.pointer.interact_pos()) {
                self.drag_preview = Some(ToolboxDock::nearest(pointer, ui.ctx().screen_rect()));
            }
        }
        if grip.drag_stopped() {
            if self.dragging
                && !self.drag_cancelled
                && let Some(pointer) = ui.ctx().input(|input| input.pointer.interact_pos())
            {
                self.dock = ToolboxDock::nearest(pointer, ui.ctx().screen_rect());
            }
            self.dragging = false;
            self.drag_cancelled = false;
            self.drag_preview = None;
        }
    }

    /// A separator: a 1 px hairline across the rail, floated off both edges.
    /// The 4 px clear space each side comes from the cluster's item spacing.
    fn draw_separator(&self, ui: &mut egui::Ui, vertical: bool) {
        let size = if vertical {
            egui::vec2(ui.available_width(), 1.0)
        } else {
            egui::vec2(1.0, ui.available_height())
        };
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        // The rail edge sits one margin outside the content rect; the spec
        // insets the hairline from the rail edge itself.
        let inset = TOOLBOX_SEPARATOR_INSET_PX - TOOLBOX_MARGIN_PX;
        let (from, to) = if vertical {
            (
                egui::pos2(rect.left() + inset, rect.center().y),
                egui::pos2(rect.right() - inset, rect.center().y),
            )
        } else {
            (
                egui::pos2(rect.center().x, rect.top() + inset),
                egui::pos2(rect.center().x, rect.bottom() - inset),
            )
        };
        ui.painter()
            .line_segment([from, to], egui::Stroke::new(1.0_f32, theme::BORDER));
    }

    /// The repeat pin: keep the armed drawing tool active after it completes
    /// an object, instead of the one-shot return to Pointer.
    fn draw_repeat_button(&mut self, ui: &mut egui::Ui) {
        let response = IconButton::new(icons::ARROW_CLOCKWISE, TOOLRAIL_ICON)
            .active(self.repeat)
            .active_marker(self.dock.marker_edge())
            .hover_text("Keep the drawing tool active after drawing")
            .show(ui);
        if response.clicked() {
            self.repeat = !self.repeat;
        }
    }

    /// The magnet: anchors land on the bar's open / high / low / close when
    /// one is within reach of the pointer. A state, like the repeat pin, so
    /// it reads off the rail without a menu.
    fn draw_magnet_button(&mut self, ui: &mut egui::Ui) {
        let response = IconButton::new(icons::MAGNET, TOOLRAIL_ICON)
            .active(self.magnet)
            .active_marker(self.dock.marker_edge())
            .hover_text("Snap anchors to the bar's open / high / low / close")
            .show(ui);
        #[cfg(test)]
        {
            self.magnet_rect = Some(response.rect);
        }
        if response.clicked() {
            self.magnet = !self.magnet;
        }
    }

    /// The More flyout of a collapsed rail: everything that lost its slot
    /// stays reachable by name with its shortcut, in registry order.
    fn draw_more_menu(&mut self, ui: &mut egui::Ui, drawings: &mut Drawings, stage: RailStage) {
        let response = ui.menu_button(icons::DOTS_THREE, |ui| {
            for tool in self.swallowed_tools(stage) {
                let mut button = egui::Button::new(tool.name());
                if let Some(shortcut) = tool.shortcut_label() {
                    button = button.shortcut_text(shortcut);
                }
                if ui.add(button).clicked() {
                    self.arm(tool);
                    ui.close_menu();
                }
            }
            if stage == RailStage::Minimal {
                ui.separator();
                if ui
                    .add(egui::Button::new("Keep tool active after drawing").selected(self.repeat))
                    .clicked()
                {
                    self.repeat = !self.repeat;
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Snap anchors to OHLC").selected(self.magnet))
                    .clicked()
                {
                    self.magnet = !self.magnet;
                    ui.close_menu();
                }
                let all_hidden = drawings.all_hidden();
                if ui
                    .button(if all_hidden { "Show all" } else { "Hide all" })
                    .clicked()
                {
                    drawings.set_all_hidden(!all_hidden);
                    ui.close_menu();
                }
                let all_locked = drawings.all_locked();
                if ui
                    .button(if all_locked { "Unlock all" } else { "Lock all" })
                    .clicked()
                {
                    drawings.set_all_locked(!all_locked);
                    ui.close_menu();
                }
            }
        });
        #[cfg(test)]
        {
            self.more_rect = Some(response.response.rect);
        }
        response.response.on_hover_text("More tools");
    }

    /// The tools the given stage swallowed into the More flyout — exactly
    /// the ones without a rail slot, in registry order.
    fn swallowed_tools(&self, stage: RailStage) -> Vec<Tool> {
        let armed = self.tool.drawing_tool();
        let mut swallowed = Vec::new();
        if stage == RailStage::Minimal {
            swallowed.push(Tool::Crosshair);
        }
        swallowed.extend(
            DRAWING_TOOLS
                .into_iter()
                .filter(|tool| Some(*tool) != armed)
                .map(Tool::Drawing),
        );
        swallowed
    }

    /// The entry to the drawn-objects manager. A toggle, not a tool: it never
    /// changes which tool is armed. Carries the object count as a badge.
    fn draw_objects_button(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &Drawings,
        manager_open: &mut bool,
    ) {
        let response = IconButton::new(icons::LIST, TOOLRAIL_ICON)
            .active(*manager_open)
            .active_marker(self.dock.marker_edge())
            .hover_text("Drawn objects")
            .show(ui);
        let count = drawings.items().len();
        if count > 0 {
            paint_badge(ui, response.rect, &count.to_string(), theme::TEXT_MUTED);
        }
        #[cfg(test)]
        {
            self.objects_rect = Some(response.rect);
        }
        if response.clicked() {
            *manager_open = !*manager_open;
        }
    }

    /// The reversible global protections. Hide-all is a view layer over each
    /// drawing's own eye; lock-all mutates every lock at once. Neither is a
    /// delete, and both are one undo entry. Drawn lock-first because the
    /// trailing layout lays from the rail's far end backwards.
    fn draw_global_buttons(&mut self, ui: &mut egui::Ui, drawings: &mut Drawings) {
        let all_locked = drawings.all_locked();
        let lock_icon = if all_locked {
            icons::LOCK_SIMPLE
        } else {
            icons::LOCK_SIMPLE_OPEN
        };
        let lock_hover = if all_locked {
            "Unlock all drawings"
        } else {
            "Lock all drawings"
        };
        let lock = IconButton::new(lock_icon, TOOLRAIL_ICON)
            .active(all_locked)
            .active_marker(self.dock.marker_edge())
            .hover_text(lock_hover)
            .show(ui);
        #[cfg(test)]
        {
            self.lock_all_rect = Some(lock.rect);
        }
        if lock.clicked() {
            drawings.set_all_locked(!all_locked);
        }

        let all_hidden = drawings.all_hidden();
        let eye_icon = if all_hidden {
            icons::EYE_SLASH
        } else {
            icons::EYE
        };
        let eye_hover = if all_hidden {
            "Show all drawings"
        } else {
            "Hide all drawings"
        };
        let eye = IconButton::new(eye_icon, TOOLRAIL_ICON)
            .active(all_hidden)
            .active_marker(self.dock.marker_edge())
            .hover_text(eye_hover)
            .show(ui);
        #[cfg(test)]
        {
            self.hide_all_rect = Some(eye.rect);
        }
        if eye.clicked() {
            drawings.set_all_hidden(!all_hidden);
        }
    }

    fn draw_button(&mut self, ui: &mut egui::Ui, tool: Tool, drawings: &Drawings) {
        let response = IconButton::new(tool.icon(), TOOLRAIL_ICON)
            .vector_icon(tool.icon_strokes(), tool.icon_dots(), tool.icon_letter())
            .active(self.tool == tool)
            .active_marker(self.dock.marker_edge())
            .hover_text(tool.hover_text())
            .show(ui);
        self.paint_draft_badge(ui, &response, tool, drawings);
        #[cfg(test)]
        if let Some(slot) = self.button_rects.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((tool, response.rect));
        }
        if response.clicked() {
            self.arm(tool);
        }
    }

    /// Draft progress on the armed tool while a multi-point object is being
    /// placed — answers "how many more clicks?" in place. Returns whether a
    /// badge occupies the button's corner this frame.
    fn paint_draft_badge(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        tool: Tool,
        drawings: &Drawings,
    ) -> bool {
        let Some(drawing_tool) = tool.drawing_tool() else {
            return false;
        };
        if self.tool != tool || drawings.draft().is_none() || drawing_tool.required_points() < 2 {
            return false;
        }
        let text = format!(
            "{}/{}",
            drawings.draft_len(),
            drawing_tool.required_points()
        );
        paint_badge(ui, response.rect, &text, theme::ACCENT);
        true
    }
}

/// The rail's frame: chrome fill, margins, and no stroke — the chart-facing
/// hairline is painted by the rail itself.
fn rail_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(theme::CHROME)
        .inner_margin(egui::Margin::same(TOOLBOX_MARGIN_PX))
}

/// The drop preview: the band the rail will occupy on the candidate edge,
/// with an accent line on its chart-facing side.
fn paint_drop_preview(ctx: &egui::Context, target: ToolboxDock) {
    let screen = ctx.screen_rect();
    let thickness = TOOLBOX_THICKNESS_PX;
    let band = match target {
        ToolboxDock::Left => egui::Rect::from_min_max(
            screen.min,
            egui::pos2(screen.left() + thickness, screen.bottom()),
        ),
        ToolboxDock::Top => egui::Rect::from_min_max(
            screen.min,
            egui::pos2(screen.right(), screen.top() + thickness),
        ),
        ToolboxDock::Bottom => egui::Rect::from_min_max(
            egui::pos2(screen.left(), screen.bottom() - thickness),
            screen.max,
        ),
    };
    let edge = TOOLBOX_DROP_BAND_EDGE_PX;
    let line = match target {
        ToolboxDock::Left => egui::Rect::from_min_max(
            egui::pos2(band.right() - edge, band.top()),
            band.right_bottom(),
        ),
        ToolboxDock::Top => egui::Rect::from_min_max(
            egui::pos2(band.left(), band.bottom() - edge),
            band.right_bottom(),
        ),
        ToolboxDock::Bottom => {
            egui::Rect::from_min_max(band.left_top(), egui::pos2(band.right(), band.top() + edge))
        }
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("toolbox_drop_preview"),
    ));
    painter.rect_filled(band, 0.0, theme::active_tint(theme::ACCENT));
    painter.rect_filled(line, 0.0, theme::ACCENT);
}

/// A pill badge in a button's trailing-bottom corner (§2.5).
fn paint_badge(ui: &egui::Ui, button: egui::Rect, text: &str, color: egui::Color32) {
    let painter = ui.painter();
    let font = egui::FontId::monospace(BADGE_TEXT_PX);
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let size = egui::vec2(galley.size().x + 2.0 * BADGE_PAD_X_PX, BADGE_HEIGHT_PX);
    let rect = egui::Rect::from_min_max(
        egui::pos2(
            button.right() - BADGE_CORNER_INSET_PX - size.x,
            button.bottom() - BADGE_CORNER_INSET_PX - size.y,
        ),
        egui::pos2(
            button.right() - BADGE_CORNER_INSET_PX,
            button.bottom() - BADGE_CORNER_INSET_PX,
        ),
    );
    painter.rect_filled(rect, egui::Rounding::same(BADGE_RADIUS_PX), theme::INSET);
    painter.galley(
        egui::pos2(
            rect.left() + BADGE_PAD_X_PX,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

#[cfg(test)]
mod tests;
