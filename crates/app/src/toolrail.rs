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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }

    #[must_use]
    pub fn dock(&self) -> ToolboxDock {
        self.dock
    }

    /// Dock the rail against `dock` — the menu path beside dragging.
    pub fn set_dock(&mut self, dock: ToolboxDock) {
        self.dock = dock;
    }

    /// Show or hide the rail outright, for a saved workspace restoring the
    /// state it recorded rather than toggling from whatever this launch
    /// happens to be in.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.toggle_visible();
        }
    }

    /// The controls the rail painted last frame, in rail order.
    ///
    /// Folded through the very `tool_slots()` the draw folds through, and cut
    /// by the stage and the band window the draw recorded, so this can never
    /// name a button that is behind a flyout, off the scrolled run, or inside
    /// the More menu. A rail nobody can see, and one that has not been drawn
    /// yet, both report nothing: the guarantee is made here rather than left
    /// to whichever caller remembers to ask [`Self::visible`] first.
    #[must_use]
    pub(crate) fn painted_controls(&self) -> Vec<RailControl> {
        if !self.visible {
            return Vec::new();
        }
        let Some(stage) = self.last_stage else {
            return Vec::new();
        };
        let armed_drawing = self.tool.drawing_tool();
        let mut controls = vec![RailControl {
            kind: RailControlKind::Tool,
            id: Tool::Pointer.id(),
            label: Tool::Pointer.name(),
            armed: self.tool == Tool::Pointer,
        }];
        // Minimal drops the crosshair; every wider stage keeps it.
        if stage != RailStage::Minimal {
            controls.push(RailControl {
                kind: RailControlKind::Tool,
                id: Tool::Crosshair.id(),
                label: Tool::Crosshair.name(),
                armed: self.tool == Tool::Crosshair,
            });
        }
        match stage {
            // The whole run has a button, and so does every star: nothing is
            // clipped and nothing is folded away at this width.
            RailStage::Full => {
                for tool in &self.favorites {
                    controls.push(self.favorite_control(*tool));
                }
                for slot in tool_slots() {
                    controls.push(Self::slot_control(slot, armed_drawing));
                }
            }
            // The band clips. Only the anchored stars stay outside it; the
            // rest of the stars and the whole tool run scroll behind two
            // chevrons, and only the window the draw recorded is on screen.
            RailStage::Scroll => {
                let Some(band) = self.last_band else {
                    return controls;
                };
                let anchored = band.anchored.min(self.favorites.len());
                for tool in &self.favorites[..anchored] {
                    controls.push(self.favorite_control(*tool));
                }
                let slots = tool_slots();
                let spilled = self.favorites.len() - anchored;
                let content = spilled + slots.len();
                let first = band.first.min(content);
                let last = first.saturating_add(band.visible).min(content);
                for index in first..last {
                    controls.push(match index.checked_sub(spilled) {
                        Some(slot) => Self::slot_control(&slots[slot], armed_drawing),
                        None => self.favorite_control(self.favorites[anchored + index]),
                    });
                }
            }
            // Only the armed tool keeps a button of its own; the rest are
            // behind More, which is not a control the scene can name yet.
            // The stars go with them — these stages draw no pinned section.
            RailStage::Compact | RailStage::Minimal => {
                if let Some(tool) = armed_drawing {
                    controls.push(RailControl {
                        kind: RailControlKind::Tool,
                        id: tool.id(),
                        label: tool.name(),
                        armed: true,
                    });
                }
            }
        }
        controls
    }

    /// One entry of the folded tool run, as the draw paints it.
    fn slot_control(slot: &RailSlot, armed_drawing: Option<DrawingTool>) -> RailControl {
        match slot {
            RailSlot::Single(tool) => RailControl {
                kind: RailControlKind::Tool,
                id: tool.id(),
                label: tool.name(),
                armed: armed_drawing == Some(*tool),
            },
            RailSlot::Family { family, members } => RailControl {
                kind: RailControlKind::Family,
                id: family.id,
                label: family.title,
                armed: armed_drawing.is_some_and(|tool| members.contains(&tool)),
            },
        }
    }

    /// One pinned button. A star is a second button for a tool that also has
    /// a slot in the run, so it is named in its own namespace rather than
    /// answering to the slot's name.
    fn favorite_control(&self, tool: DrawingTool) -> RailControl {
        RailControl {
            kind: RailControlKind::Favorite,
            id: tool.id(),
            label: tool.name(),
            armed: self.tool == Tool::Drawing(tool),
        }
    }

    /// Whether a grip drag is live this frame — the app's escape stack must
    /// yield Esc to the drag while it is.
    #[must_use]
    pub fn drag_active(&self) -> bool {
        self.dragging
    }

    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.tool = Tool::Pointer;
            // Nothing is painted while the rail is away, so what the last
            // draw settled on stops describing the screen. Forgotten rather
            // than kept: control captures are served before the rail draws,
            // so a stale stage would answer the first capture after a hide
            // with the buttons of a rail nobody can see.
            self.last_stage = None;
            self.last_band = None;
        }
    }

    pub fn arm(&mut self, tool: Tool) {
        if let Tool::Drawing(drawing_tool) = tool
            && let Some(family) = drawing_tool.family()
        {
            self.last_family_member.insert(family.id, drawing_tool);
        }
        if self.tool != tool {
            self.reveal_armed = true;
        }
        self.tool = tool;
    }

    /// The starred tools, in the order they were starred.
    #[must_use]
    pub fn favorites(&self) -> &[DrawingTool] {
        &self.favorites
    }

    #[must_use]
    pub fn is_favorite(&self, tool: DrawingTool) -> bool {
        self.favorites.contains(&tool)
    }

    /// Star or unstar a tool. Starring appends — the pinned section grows at
    /// its far end, and the tools already pinned never move.
    pub fn toggle_favorite(&mut self, tool: DrawingTool) {
        if let Some(index) = self.favorites.iter().position(|entry| *entry == tool) {
            self.favorites.remove(index);
        } else {
            self.favorites.push(tool);
        }
        self.favorites_changed = true;
    }

    /// Whether the trader starred or unstarred something since this was last
    /// asked, clearing the flag.
    ///
    /// The rail curates the list; it does not know where lists are kept. The
    /// app reads this on the frame the star was clicked and writes the choice
    /// down — the same hand-off the replay browser's folder pick uses, and for
    /// the same reason: "it forgot my tools again" must not be one crash away.
    pub fn take_favorites_change(&mut self) -> bool {
        std::mem::take(&mut self.favorites_changed)
    }

    /// Restore the starred list from saved tool ids — the workspace file
    /// path. An id no registered tool carries is dropped, a duplicate keeps
    /// its first position, and saved order is kept: it is the order the
    /// trader starred in.
    ///
    /// Returns the ids it did not recognise, so the caller can say so. The
    /// prune used to be private and harmless — it only reached the disk on an
    /// explicit save. Now the trader's next star click writes this list back,
    /// which makes the loss permanent, and losing a saved id without a word is
    /// exactly the silent patching `CLAUDE.md` rules out. The sibling restore
    /// logs `UI_STATE_TAB_DROPPED` for a tab it cannot open, for the same
    /// reason.
    pub fn set_favorites(&mut self, ids: &[String]) -> Vec<String> {
        self.favorites.clear();
        let mut unknown = Vec::new();
        for id in ids {
            match DrawingTool::by_id(id) {
                Some(tool) if !self.favorites.contains(&tool) => self.favorites.push(tool),
                // A duplicate is not a loss: the first position stands.
                Some(_) => {}
                None => unknown.push(id.clone()),
            }
        }
        unknown
    }

    /// Park the scrolling tool band at `offset` px along the rail — the
    /// validation hook for a state that otherwise takes a chevron click.
    /// The band clamps it on the next frame, so `f32::INFINITY` means "the
    /// far end" and a rail that does not scroll ignores it entirely.
    pub fn set_band_offset(&mut self, offset: f32) {
        self.band_target = Some(offset.max(0.0));
    }

    /// Whether the repeat pin keeps the tool armed after an object completes.
    #[must_use]
    pub fn repeat(&self) -> bool {
        self.repeat
    }

    #[cfg(test)]
    pub(crate) fn set_repeat(&mut self, repeat: bool) {
        self.repeat = repeat;
    }

    /// Whether placed anchors snap to the bar's open / high / low / close.
    #[must_use]
    pub fn magnet(&self) -> bool {
        self.magnet
    }

    /// Arm the magnet without a click — the `QUANTICK_DRAWING_MAGNET` hook
    /// and the tests both come through here, so neither can drift from what
    /// the button does.
    pub(crate) fn set_magnet(&mut self, magnet: bool) {
        self.magnet = magnet;
    }

    /// Ask for a family flyout without a click — the
    /// `QUANTICK_TOOLBOX_FLYOUT` hook. The slot honours it on its next draw,
    /// when the anchor rect exists; an unknown family id is simply never
    /// matched and stays pending, which draws nothing.
    pub(crate) fn request_flyout(&mut self, family_id: String) {
        self.hook_flyout = Some(family_id);
    }

    #[cfg(test)]
    pub(crate) fn button_rect(&self, tool: Tool) -> Option<egui::Rect> {
        self.button_rects
            .iter()
            .flatten()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }

    #[cfg(test)]
    pub(crate) fn objects_button_rect(&self) -> Option<egui::Rect> {
        self.objects_rect
    }

    #[cfg(test)]
    pub(crate) fn flyout_row_rect(&self, tool: DrawingTool) -> Option<egui::Rect> {
        self.flyout_rects
            .iter()
            .find_map(|(candidate, rect)| (*candidate == tool).then_some(*rect))
    }

    /// Tool-arming keys. Escape lives in the app's escape stack (rail drag →
    /// input → draft → selection → Pointer), not here. Each drawing tool
    /// declares its own shortcut through the registry.
    pub fn handle_keys(&mut self, ctx: &egui::Context) {
        // A hidden rail arms nothing (audit M9): with no rail on screen an
        // armed tool has no indication anywhere, and the next chart click
        // would draw instead of pan — the keyboard twin of the invariant
        // `hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed`.
        if !self.visible {
            return;
        }
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let armed = ctx.input(|input| {
            if input.modifiers.command || input.modifiers.alt {
                return None;
            }
            if input.key_pressed(egui::Key::Num1) {
                return Some(Tool::Pointer);
            }
            if input.key_pressed(egui::Key::Num2) {
                return Some(Tool::Crosshair);
            }
            DRAWING_TOOLS.into_iter().find_map(|tool| {
                tool.shortcut()
                    .filter(|shortcut| {
                        // A tool's shortcut is a *bare* key (with Shift where
                        // the tool asks for it). Ctrl+M, Cmd+M and Alt+M
                        // belong to whoever claims them — the mark hotkey does
                        // — and must not also arm a tool out from under the
                        // trader's hand.
                        input.key_pressed(shortcut.key)
                            && input.modifiers.shift == shortcut.shift
                            && !input.modifiers.ctrl
                            && !input.modifiers.command
                            && !input.modifiers.alt
                    })
                    .map(|_| Tool::Drawing(tool))
            })
        });
        if let Some(tool) = armed {
            self.arm(tool);
        }
    }

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

    /// The pinned section at the tool end of the rail: a separator, then one
    /// button per starred tool in star order. One click arms; unstarring
    /// lives only on the star in the tool's own flyout row, so a pinned
    /// button can never be destroyed by the click meant to use it.
    fn draw_favorites_section(
        &mut self,
        ui: &mut egui::Ui,
        vertical: bool,
        drawings: &Drawings,
        range: std::ops::Range<usize>,
    ) {
        if range.is_empty() {
            return;
        }
        self.draw_separator(ui, vertical);
        self.draw_favorite_buttons(ui, drawings, range);
    }

    /// The pinned buttons themselves, without the section separator — the
    /// band reuses this for favorites that spilled past the anchored head.
    fn draw_favorite_buttons(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &Drawings,
        range: std::ops::Range<usize>,
    ) {
        // Indexed so the loop never clones the list on the per-frame path;
        // `DrawingTool` is `Copy` and arming cannot reorder favorites.
        for index in range {
            let tool = self.favorites[index];
            let armed = self.tool == Tool::Drawing(tool);
            let response = IconButton::new(tool.icon(), TOOLRAIL_ICON)
                .vector_icon(tool.icon_strokes(), tool.icon_dots(), tool.icon_letter())
                .active(armed)
                .active_marker(self.dock.marker_edge())
                .hover_text(tool.hover_text())
                .show(ui);
            self.paint_draft_badge(ui, &response, Tool::Drawing(tool), drawings);
            if ui.is_rect_visible(response.rect) {
                // The corner star names the section: this button is a pin,
                // not a second registry slot.
                ui.painter().text(
                    egui::pos2(
                        response.rect.right() - FAVORITE_BADGE_INSET_PX,
                        response.rect.top() + FAVORITE_BADGE_INSET_PX,
                    ),
                    egui::Align2::RIGHT_TOP,
                    icons::STAR,
                    egui::FontId::proportional(FAVORITE_BADGE_PX),
                    theme::ACCENT,
                );
            }
            #[cfg(test)]
            self.favorite_rects.push((tool, response.rect));
            if response.clicked() {
                self.arm(Tool::Drawing(tool));
            }
        }
    }

    /// The scrolling tool band: a chevron at each end and, between them, the
    /// favorites that spilled past the anchored head followed by every tool
    /// slot. Nothing leaves the inventory — the band is a window onto it, so
    /// a fourth star can no longer swallow the toolbar.
    fn draw_band(
        &mut self,
        ui: &mut egui::Ui,
        vertical: bool,
        drawings: &Drawings,
        slots: &'static [RailSlot],
        anchored: usize,
        available: f32,
    ) {
        let viewport = band_viewport(available, anchored);
        let spilled = self.favorites.len() - anchored;
        let max_offset = band_max_offset(viewport, spilled + slots.len());
        // Arming a tool the band has scrolled past pulls it back into view,
        // so the rail keeps the spec's promise that the armed tool always
        // has a real slot. Only on the frame it was armed: past that, the
        // band stays wherever the trader put it.
        if self.reveal_armed {
            self.reveal_armed = false;
            if let Some(index) = self.armed_band_index(slots, spilled) {
                let span = TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX;
                let visible = band_visible_items(viewport);
                let first = (self.band_offset / span).round() as usize;
                if index < first {
                    self.band_target = Some(index as f32 * span);
                } else if index >= first + visible {
                    self.band_target = Some((index + 1 - visible) as f32 * span);
                }
            }
        }
        // A pending chevron click is resolved before anything is drawn, so
        // both chevrons and the band read one offset. Taking it from last
        // frame instead would leave the way-back arrow a frame stale — dead
        // on the very click that opened it.
        let target = self.band_target.take().map(|at| at.clamp(0.0, max_offset));
        let offset = target.unwrap_or(self.band_offset).clamp(0.0, max_offset);
        self.band_offset = offset;
        let step = band_scroll_step(viewport);

        if self.draw_chevron(ui, vertical, true, offset > 0.0) {
            self.band_target = Some(offset - step);
        }

        let mut area = egui::ScrollArea::new([!vertical, vertical])
            .id_salt("toolrail_band")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false]);
        area = if vertical {
            area.max_height(viewport)
        } else {
            area.max_width(viewport)
        };
        if let Some(target) = target {
            area = if vertical {
                area.vertical_scroll_offset(target)
            } else {
                area.horizontal_scroll_offset(target)
            };
        }
        let inner = if vertical {
            egui::Layout::top_down(egui::Align::Center)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        };
        // The band is handed its extent rather than left to ask for one:
        // told only a maximum, a scroll area still claims whatever the
        // parent has left, and the far cluster needs that room. Its bar is
        // also denied a lane across the rail's short axis, because a hidden
        // bar still books one and 44 px has none to spare.
        let band_size = if vertical {
            egui::vec2(ui.available_width(), viewport)
        } else {
            egui::vec2(viewport, ui.available_height())
        };
        let output = ui
            .allocate_ui_with_layout(band_size, inner, |ui| {
                let scroll = &mut ui.spacing_mut().scroll;
                scroll.floating = true;
                scroll.bar_width = 0.0;
                scroll.floating_allocated_width = 0.0;
                area.show(ui, |ui| {
                    ui.with_layout(inner, |ui| {
                        ui.spacing_mut().item_spacing =
                            egui::vec2(TOOLBOX_ITEM_GAP_PX, TOOLBOX_ITEM_GAP_PX);
                        let favorites = self.favorites.len();
                        self.draw_favorite_buttons(ui, drawings, anchored..favorites);
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
                    });
                })
            })
            .inner;
        #[cfg(test)]
        {
            self.band_rect = Some(output.inner_rect);
        }
        // The wheel moves the band without asking us, so the offset is read
        // back rather than assumed.
        let scrolled = if vertical {
            output.state.offset.y
        } else {
            output.state.offset.x
        };
        self.band_offset = scrolled.clamp(0.0, max_offset);
        // The window this draw actually showed, written down for the same
        // reason the stage is: a reader that is not looking at the screen
        // cannot re-derive it, because it depends on the extent the layout
        // handed the rail and on wherever the trader last scrolled to.
        let span = TOOLRAIL_ICON.hit + TOOLBOX_ITEM_GAP_PX;
        self.last_band = Some(BandWindow {
            anchored,
            first: (self.band_offset / span).round() as usize,
            visible: band_visible_items(viewport),
        });

        if self.draw_chevron(ui, vertical, false, self.band_offset < max_offset) {
            self.band_target = Some(self.band_offset + step);
        }
        // The trailing chevron sits past the band, so its click can only be
        // honoured next frame. Ask for that frame: without it a click on a
        // still chart reads as a dead button.
        if self.band_target.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// Where the armed tool sits among the band's items: the spilled
    /// favorites first, then the tool slots in registry order. `None` when
    /// nothing is armed, or when the armed tool is anchored outside the band
    /// and therefore already on screen.
    fn armed_band_index(&self, slots: &[RailSlot], spilled: usize) -> Option<usize> {
        let armed = self.tool.drawing_tool()?;
        let anchored = self.favorites.len() - spilled;
        if let Some(offset) = self.favorites[anchored..]
            .iter()
            .position(|pinned| *pinned == armed)
        {
            return Some(offset);
        }
        let slot = slots.iter().position(|slot| match slot {
            RailSlot::Single(tool) => *tool == armed,
            RailSlot::Family { members, .. } => members.contains(&armed),
        })?;
        Some(spilled + slot)
    }

    /// One end of the band's navigation pair. Both ends keep their slot for
    /// as long as the band scrolls: a chevron that vanished at the end of
    /// travel would shift every tool under the pointer by its own length.
    /// A dead end dims and stops sensing clicks instead.
    fn draw_chevron(
        &mut self,
        ui: &mut egui::Ui,
        vertical: bool,
        leading: bool,
        live: bool,
    ) -> bool {
        let size = if vertical {
            egui::vec2(TOOLRAIL_ICON.hit, BAND_ARROW_LENGTH_PX)
        } else {
            egui::vec2(BAND_ARROW_LENGTH_PX, TOOLRAIL_ICON.hit)
        };
        let sense = if live {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(size, sense);
        #[cfg(test)]
        if leading {
            self.band_leading_arrow = Some((rect, live));
        } else {
            self.band_trailing_arrow = Some((rect, live));
        }
        if ui.is_rect_visible(rect) {
            let glyph = match (vertical, leading) {
                (true, true) => icons::CARET_UP,
                (true, false) => icons::CARET_DOWN,
                (false, true) => icons::CARET_LEFT,
                (false, false) => icons::CARET_RIGHT,
            };
            let color = if !live {
                theme::TEXT_FAINT
            } else if response.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                egui::FontId::proportional(BAND_ARROW_GLYPH_PX),
                color,
            );
        }
        // A dimmed control with no reason reads as a bug, so the dead end
        // says which end it is rather than saying nothing.
        let hint = match (vertical, leading, live) {
            (true, true, true) => "Scroll tools up",
            (true, false, true) => "Scroll tools down",
            (false, true, true) => "Scroll tools left",
            (false, false, true) => "Scroll tools right",
            (_, true, false) => "Start of the tool list",
            (_, false, false) => "End of the tool list",
        };
        live && response.on_hover_text(hint).clicked()
    }

    /// A family's shown member: the last-armed one, or `None` before any
    /// member has been used.
    fn family_member(&self, family: ToolFamily, members: &[DrawingTool]) -> Option<DrawingTool> {
        self.last_family_member
            .get(family.id)
            .copied()
            .filter(|member| members.contains(member))
    }

    /// The family split button: left-click arms the shown member; the caret
    /// zone or a right-click opens the member flyout.
    fn draw_family_slot(
        &mut self,
        ui: &mut egui::Ui,
        family: ToolFamily,
        members: &[DrawingTool],
        drawings: &Drawings,
    ) {
        let shown = self.family_member(family, members);
        let armed = self
            .tool
            .drawing_tool()
            .is_some_and(|tool| members.contains(&tool));
        let icon = shown.map_or(family.icon, DrawingTool::icon);
        let strokes = shown.map_or(family.icon_strokes, DrawingTool::icon_strokes);
        let dots = shown.map_or(family.icon_dots, DrawingTool::icon_dots);
        let letter = shown.map_or(family.icon_letter, DrawingTool::icon_letter);
        let hover = shown.map_or(family.title, DrawingTool::hover_text);
        let response = IconButton::new(icon, TOOLRAIL_ICON)
            .vector_icon(strokes, dots, letter)
            .active(armed)
            .active_marker(self.dock.marker_edge())
            .hover_text(hover)
            .show(ui);
        let badge_shown = shown
            .map(Tool::Drawing)
            .is_some_and(|tool| self.paint_draft_badge(ui, &response, tool, drawings));

        // The caret marks the flyout; it yields the corner to a draft badge,
        // because a tool mid-draft is unambiguously armed already.
        let caret_zone = egui::Rect::from_min_max(
            egui::pos2(
                response.rect.right() - TOOLBOX_CARET_ZONE_PX,
                response.rect.bottom() - TOOLBOX_CARET_ZONE_PX,
            ),
            response.rect.max,
        );
        if !badge_shown && ui.is_rect_visible(response.rect) {
            let caret_color = if armed {
                theme::ACCENT
            } else if response.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_FAINT
            };
            let corner = egui::pos2(
                response.rect.right() - CARET_INSET_PX,
                response.rect.bottom() - CARET_INSET_PX,
            );
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(corner.x - CARET_SIDE_PX, corner.y),
                    corner,
                    egui::pos2(corner.x, corner.y - CARET_SIDE_PX),
                ],
                caret_color,
                egui::Stroke::NONE,
            ));
        }

        #[cfg(test)]
        if let Some(slot) = self.button_rects.iter_mut().find(|slot| slot.is_none()) {
            let recorded = shown.unwrap_or(members[0]);
            *slot = Some((Tool::Drawing(recorded), response.rect));
        }

        if self.hook_flyout.as_deref() == Some(family.id) {
            self.hook_flyout = None;
            self.flyout = Some((family.id, response.rect));
        }

        let caret_clicked = response.clicked()
            && response
                .interact_pointer_pos()
                .is_some_and(|position| caret_zone.contains(position));
        if response.secondary_clicked() || caret_clicked {
            self.flyout = Some((family.id, response.rect));
        } else if response.clicked() {
            self.arm(Tool::Drawing(shown.unwrap_or(members[0])));
        }
    }

    /// The open family flyout, on the rail's chart-facing side, first row
    /// aligned with the slot's leading edge.
    fn draw_family_flyout(&mut self, ctx: &egui::Context) {
        let Some((family_id, anchor)) = self.flyout else {
            return;
        };
        let Some((family, members)) = tool_slots().iter().find_map(|slot| match slot {
            RailSlot::Family { family, members } if family.id == family_id => {
                Some((*family, members.as_slice()))
            }
            _ => None,
        }) else {
            self.flyout = None;
            return;
        };

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.flyout = None;
            return;
        }

        let screen = ctx.screen_rect();
        let height =
            TOOLBOX_FLYOUT_ROW_HEIGHT_PX * (members.len() + 1) as f32 + 2.0 * TOOLBOX_ITEM_GAP_PX;
        let position = match self.dock {
            ToolboxDock::Left => egui::pos2(anchor.right() + TOOLBOX_MARGIN_PX, anchor.top()),
            ToolboxDock::Top => egui::pos2(anchor.left(), anchor.bottom() + TOOLBOX_MARGIN_PX),
            ToolboxDock::Bottom => {
                egui::pos2(anchor.left(), anchor.top() - TOOLBOX_MARGIN_PX - height)
            }
        };
        let max_position = egui::pos2(
            (screen.right() - TOOLBOX_FLYOUT_WIDTH_PX).max(screen.left()),
            (screen.bottom() - height).max(screen.top()),
        );
        let position = position.clamp(screen.min, max_position);

        let area = egui::Area::new(egui::Id::new("toolbox_family_flyout"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme::CONTROL)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(egui::Rounding::same(FLYOUT_CORNER_RADIUS_PX))
                    .show(ui, |ui| {
                        ui.set_width(TOOLBOX_FLYOUT_WIDTH_PX - 2.0 * TOOLBOX_ITEM_GAP_PX);
                        ui.label(
                            egui::RichText::new(family.title)
                                .color(theme::TEXT_MUTED)
                                .size(FLYOUT_HEADER_TEXT_PX),
                        );
                        for member in members {
                            match self.draw_flyout_row(ui, *member) {
                                Some(FlyoutClick::Arm) => {
                                    self.arm(Tool::Drawing(*member));
                                    self.flyout = None;
                                }
                                // Starring neither arms nor closes: the
                                // trader is curating the rail, not drawing.
                                Some(FlyoutClick::ToggleFavorite) => {
                                    self.toggle_favorite(*member);
                                }
                                None => {}
                            }
                        }
                    });
            });

        // A press anywhere outside closes the flyout without arming.
        if self.flyout.is_some() {
            let pressed_outside = ctx.input(|input| {
                input.pointer.any_pressed()
                    && input.pointer.interact_pos().is_some_and(|position| {
                        !area.response.rect.contains(position) && !anchor.contains(position)
                    })
            });
            if pressed_outside {
                self.flyout = None;
            }
        }
    }

    /// One flyout row: glyph with its favorite star, name, right-aligned
    /// shortcut. Returns what the click asked for, `None` when nothing was
    /// clicked.
    fn draw_flyout_row(&mut self, ui: &mut egui::Ui, member: DrawingTool) -> Option<FlyoutClick> {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, TOOLBOX_FLYOUT_ROW_HEIGHT_PX),
            egui::Sense::click(),
        );
        // The star holds the row's right edge. Its hit zone outranks the
        // row: a click there curates favorites, everywhere else arms.
        let star_center = egui::pos2(rect.right() - FLYOUT_STAR_RIGHT_INSET_PX, rect.center().y);
        let star_zone =
            egui::Rect::from_center_size(star_center, egui::Vec2::splat(FLYOUT_STAR_HIT_PX));
        let favorite = self.is_favorite(member);
        #[cfg(test)]
        self.flyout_rects.push((member, rect));
        #[cfg(test)]
        self.flyout_star_rects.push((member, star_zone));
        if ui.is_rect_visible(rect) {
            let armed = self.tool == Tool::Drawing(member);
            if armed {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(FLYOUT_ROW_RADIUS_PX),
                    theme::active_tint(theme::ACCENT),
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    egui::Rounding::same(FLYOUT_ROW_RADIUS_PX),
                    theme::BORDER,
                );
            }
            let glyph_color = if armed {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            };
            let glyph_center = egui::pos2(rect.left() + FLYOUT_GLYPH_CENTER_X_PX, rect.center().y);
            if member.icon_strokes().is_empty() {
                ui.painter().text(
                    glyph_center,
                    egui::Align2::CENTER_CENTER,
                    member.icon(),
                    egui::FontId::proportional(FLYOUT_GLYPH_PX),
                    glyph_color,
                );
            } else {
                paint_vector_icon(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        glyph_center,
                        egui::Vec2::splat(FLYOUT_ICON_BOX_PX),
                    ),
                    member.icon_strokes(),
                    member.icon_dots(),
                    member.icon_letter(),
                    glyph_color,
                );
            }
            // Accent when starred; otherwise the star only whispers on row
            // hover, so an unstarred flyout stays as quiet as before.
            if favorite || response.hovered() {
                let star_color = if favorite {
                    theme::ACCENT
                } else {
                    theme::TEXT_FAINT
                };
                ui.painter().text(
                    star_center,
                    egui::Align2::CENTER_CENTER,
                    icons::STAR,
                    egui::FontId::proportional(FLYOUT_STAR_PX),
                    star_color,
                );
            }
            ui.painter().text(
                egui::pos2(rect.left() + FLYOUT_NAME_X_PX, rect.center().y),
                egui::Align2::LEFT_CENTER,
                member.name(),
                egui::FontId::proportional(FLYOUT_NAME_TEXT_PX),
                theme::TEXT_PRIMARY,
            );
            if let Some(shortcut) = Tool::Drawing(member).shortcut_label() {
                ui.painter().text(
                    egui::pos2(
                        rect.right() - FLYOUT_STAR_SLOT_PX - FLYOUT_SHORTCUT_INSET_PX,
                        rect.center().y,
                    ),
                    egui::Align2::RIGHT_CENTER,
                    shortcut,
                    egui::FontId::proportional(FLYOUT_SHORTCUT_TEXT_PX),
                    theme::TEXT_FAINT,
                );
            }
        }
        if response.clicked() {
            let on_star = response
                .interact_pointer_pos()
                .is_some_and(|position| star_zone.contains(position));
            if on_star {
                return Some(FlyoutClick::ToggleFavorite);
            }
            return Some(FlyoutClick::Arm);
        }
        None
    }
}

/// What a click on a flyout row asked for.
enum FlyoutClick {
    /// Arm the row's tool and close the flyout.
    Arm,
    /// Star or unstar the row's tool; the flyout stays open.
    ToggleFavorite,
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
