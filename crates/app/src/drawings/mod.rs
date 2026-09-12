//! Modular user-authored chart drawings.
//!
//! Each drawing tool implements [`DrawingToolImpl`] in its own file. The
//! registry macro is the only docking point: add a module name there and the
//! toolbox, placement state, renderer and hit-testing all see the new tool.
//! Market data remains immutable and the deterministic engine never learns
//! about UI marks.

pub mod action_bar;
pub mod context_bar;
pub mod fib;
pub mod presets;

// Geometry shared by a family of tools. Not tools themselves, so they are not
// in the registry — a family core exists so its members stay declarations.
mod line_core;
mod mark_core;
mod measure_core;
mod shape_core;

// The subsystem's three other owners. This module keeps the object model, the
// vocabulary every tool speaks and the registry; a tool's port and handle live
// in `tool`, the collection and its undo history in `collection`, and the
// rules that draft or re-anchor an object in `placement`.
mod collection;
mod placement;
mod tool;

pub use tool::DrawingTool;
use tool::DrawingToolImpl;

use std::any::Any;
use std::fmt;

use eframe::egui;
use smallvec::SmallVec;

use crate::chart::PriceScale;
use crate::theme;

/// The screen-space grab points of one selected object. Six covers every tool
/// in the registry — the channel is the widest, with a corner and a centre on
/// each of its two rails — so the handle pass of the selected object
/// allocates nothing per frame.
pub type Handles = SmallVec<[egui::Pos2; 6]>;

/// The price-axis heights one drawing asks to be tagged at, in screen `y`.
///
/// Two, because the tools that declare a level today name exactly one and the
/// obvious next candidates (a price range's two edges) name two — so the pass
/// over every visible object allocates nothing per frame, the same reason
/// [`Handles`] is sized the way it is.
pub type AxisLevels = SmallVec<[f32; 2]>;

pub const DEFAULT_DRAWING_COLOR: egui::Color32 = egui::Color32::from_rgb(138, 180, 248);
/// A drawing is an annotation *on* the chart, never a second series: the
/// stock stroke is a hairline, thinner than the candle bodies it sits over
/// (`docs/ux/drawing-tools-2026-08.md` §D2). The width slider keeps its full
/// range — this is the default, not the ceiling.
pub const DEFAULT_DRAWING_WIDTH_PX: f32 = 1.0;
pub const DEFAULT_DRAWING_FILL_ALPHA: u8 = 14;
pub const MIN_DRAWING_WIDTH_PX: f32 = 0.5;
pub const MAX_DRAWING_WIDTH_PX: f32 = 6.0;
pub const MAX_DRAWING_FILL_ALPHA: u8 = 160;
/// Undo history depth. One entry per committed command (a whole drag or
/// slider gesture is one command), so this bounds memory without cutting a
/// working session short.
pub(super) const UNDO_HISTORY_LIMIT: usize = 64;
pub(super) const SELECTED_ANCHOR_RADIUS_PX: f32 = 3.5;
/// Handles read as hollow rings, not solid discs: the core is the chart's own
/// backdrop, so the handle marks the anchor without adding a bright blob over
/// the candles (`docs/ux/drawing-tools-2026-08.md` §D2).
pub(super) const SELECTED_ANCHOR_FILL: egui::Color32 = theme::CANVAS;
pub(super) const SELECTED_ANCHOR_RING_WIDTH_PX: f32 = 1.25;
/// Selection never repaints the object white: it keeps the configured colour
/// and paints this soft halo underneath instead, plus ring anchor handles.
/// Premultiplied ~11% white — enough to find the object under the pointer,
/// not enough to double its visual weight.
pub(super) const SELECTION_HALO_COLOR: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(28, 28, 28, 28);
/// How much wider than the object's own stroke the halo pass paints.
pub(super) const SELECTION_HALO_EXTRA_WIDTH_PX: f32 = 2.5;
pub(super) const FIB_LABEL_OFFSET_PX: f32 = 3.0;
pub(super) const FIB_LABEL_SIZE_PX: f32 = 10.0;

/// Tool-owned state beyond anchors and common style. A property unique to
/// one tool lives in that tool's payload, never in the shared envelope, so
/// the next tool cannot force the model or the central inspector open.
pub trait DrawingPayload: fmt::Debug {
    fn clone_box(&self) -> Box<dyn DrawingPayload>;
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Serialize the payload for a named preset. Coordinates, lock and
    /// visibility never travel with a preset, only the tool-owned config.
    fn export_preset(&self) -> Option<toml::Value> {
        None
    }
    /// Apply a previously exported preset. `false` leaves the payload alone.
    fn import_preset(&mut self, _value: &toml::Value) -> bool {
        false
    }
}

impl Clone for Box<dyn DrawingPayload> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Payload of tools whose whole state is anchors + common style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoPayload;

impl DrawingPayload for NoPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(Self)
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other.as_any().downcast_ref::<Self>().is_some()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Named-preset storage a tool's inspector tab can talk to without knowing
/// where presets live. Presets carry an opaque payload export; the host
/// stores them per tool id, versioned, surviving restarts.
pub trait PresetHost {
    fn custom_preset_names(&self, tool_id: &str) -> Vec<String>;
    fn load_custom_preset(&self, tool_id: &str, name: &str) -> Option<toml::Value>;
    /// `false` means the name exists and `overwrite` was not set — the
    /// caller asks the user before trying again.
    fn save_custom_preset(
        &mut self,
        tool_id: &str,
        name: &str,
        value: toml::Value,
        overwrite: bool,
    ) -> bool;
    fn delete_custom_preset(&mut self, tool_id: &str, name: &str);
    fn default_preset(&self, tool_id: &str) -> Option<String>;
    fn set_default_preset(&mut self, tool_id: &str, name: Option<String>);
    /// The colour / width / fill new objects of this tool open with, when the
    /// trader has saved one. Separate from the named presets above because it
    /// answers a different question: not "apply this look now" but "stop
    /// asking me for this look every single time".
    fn default_style(&self, tool_id: &str) -> Option<DrawingStyle>;
    fn set_default_style(&mut self, tool_id: &str, style: Option<DrawingStyle>);
    /// Everything else a new object of this tool opens with — whatever the
    /// tool's own payload exports, which for a Fib is the level list, the
    /// per-level colours, the labels, the band and the span.
    ///
    /// The style pair above could not answer this: a Fib's colours are *per
    /// level*, and they live in the payload, so "remember my look" was only
    /// ever remembering the outline. Reaching for a named preset instead
    /// meant inventing a name and then setting it as the default — two
    /// dialogs to answer "like this one, from now on".
    fn default_config(&self, tool_id: &str) -> Option<toml::Value>;
    fn set_default_config(&mut self, tool_id: &str, value: Option<toml::Value>);
    /// Whether one is stored, without building it. The inspector asks this
    /// every frame it is open, purely to decide whether a button exists —
    /// answering it through [`Self::default_config`] would deep-clone a whole
    /// level list per frame and drop it on the next line.
    fn has_default_config(&self, tool_id: &str) -> bool;
}

/// What a new object of `tool` should open with, given everything the trader
/// has told the app to remember. The one place that answer is assembled, so
/// the click path, the scripted hooks and the tests can never open different
/// objects from the same saved defaults.
///
/// Order is precedence, weakest first: the built-in look, then the saved
/// default configuration, then the explicitly *named* default preset — a
/// name the trader chose beats one they saved by pressing a button.
#[must_use]
pub fn new_drawing_from_defaults(host: &dyn PresetHost, tool: DrawingTool) -> NewDrawing {
    let style = host
        .default_style(tool.id())
        .unwrap_or_else(|| tool.default_style());
    let mut payload = tool.default_payload();
    if let Some(value) = host.default_config(tool.id()) {
        payload.import_preset(&value);
    }
    if let Some(name) = host.default_preset(tool.id())
        && let Some(value) = host.load_custom_preset(tool.id(), &name)
    {
        payload.import_preset(&value);
    }
    NewDrawing { style, payload }
}

/// Remember this object's whole configuration as what new objects of its tool
/// open with — the named call behind the inspector's "save as default", so a
/// script or the future assistant can do it without the button.
///
/// Objects already on the chart are never touched: a default is a statement
/// about the *next* object, and repainting the marks a trader has placed is a
/// bulk edit nobody asked for.
pub fn save_tool_default(host: &mut dyn PresetHost, drawing: &Drawing) {
    let tool_id = drawing.tool.id();
    host.set_default_style(tool_id, Some(drawing.style));
    host.set_default_config(tool_id, drawing.payload.export_preset());
}

/// Forget everything saved for this tool, so new objects open the way they
/// did out of the box — style, configuration and the named default preset.
///
/// The named preset itself is kept: this restores the factory *start*, it
/// does not delete work the trader saved under a name.
pub fn reset_tool_default(host: &mut dyn PresetHost, tool: DrawingTool) {
    let tool_id = tool.id();
    host.set_default_style(tool_id, None);
    host.set_default_config(tool_id, None);
    host.set_default_preset(tool_id, None);
}

/// Whether this tool has anything saved to forget — what the reset control
/// reads, so it is absent rather than inert when there is nothing to undo.
#[must_use]
pub fn has_saved_default(host: &dyn PresetHost, tool: DrawingTool) -> bool {
    let tool_id = tool.id();
    host.default_style(tool_id).is_some()
        || host.has_default_config(tool_id)
        || host.default_preset(tool_id).is_some()
}

/// A host with no storage: custom presets are absent, saving reports success
/// and drops the value. For contexts without a store (tests, previews).
#[cfg(test)]
#[derive(Debug, Default)]
pub struct NullPresetHost;

#[cfg(test)]
impl PresetHost for NullPresetHost {
    fn custom_preset_names(&self, _tool_id: &str) -> Vec<String> {
        Vec::new()
    }
    fn load_custom_preset(&self, _tool_id: &str, _name: &str) -> Option<toml::Value> {
        None
    }
    fn save_custom_preset(
        &mut self,
        _tool_id: &str,
        _name: &str,
        _value: toml::Value,
        _overwrite: bool,
    ) -> bool {
        true
    }
    fn delete_custom_preset(&mut self, _tool_id: &str, _name: &str) {}
    fn default_preset(&self, _tool_id: &str) -> Option<String> {
        None
    }
    fn set_default_preset(&mut self, _tool_id: &str, _name: Option<String>) {}
    fn default_style(&self, _tool_id: &str) -> Option<DrawingStyle> {
        None
    }
    fn set_default_style(&mut self, _tool_id: &str, _style: Option<DrawingStyle>) {}
    fn default_config(&self, _tool_id: &str) -> Option<toml::Value> {
        None
    }
    fn set_default_config(&mut self, _tool_id: &str, _value: Option<toml::Value>) {}
    fn has_default_config(&self, _tool_id: &str) -> bool {
        false
    }
}

/// What an anchor's second coordinate means in the band being painted.
///
/// A tool that only projects never asks; a tool that *reads a number back to
/// the trader* has to, because `pts` and `%` are price words. A percent over
/// a signed cumulative series is not a smaller truth, it is a false one: a
/// move from -100 to +100 is not "-200%".
#[derive(Clone, Copy)]
pub enum ValueUnit<'a> {
    /// The instrument's price.
    Price,
    /// An indicator's own value, named by that pane's label.
    Indicator(&'a str),
}

/// Everything a tool may need beyond raw screen anchors when painting or
/// hit-testing: its own payload, the chart-space anchors and the price scale
/// that projected them (log-scaled tools compute prices, then project).
#[derive(Clone, Copy)]
pub struct DrawContext<'a> {
    pub payload: &'a dyn DrawingPayload,
    pub anchors: &'a [ChartPoint],
    pub scale: &'a PriceScale,
    /// Screen pixels one bar slot occupies — the time axis's own scale, the
    /// way `scale` is the value axis's. Almost every tool ignores it: their
    /// anchors are already projected. A tool that paints a *series* (one
    /// value per bar, like the anchored VWAP's line) needs it to place slots
    /// its anchors never touched; `<= 0` means the pane could not say, and
    /// such a tool draws nothing rather than guessing.
    pub px_per_bar: f32,
    /// What `scale` measures on this band — see [`ValueUnit`].
    pub unit: ValueUnit<'a>,
    /// Whether this is the first band painting an object that crosses all of
    /// them. A vertical line's stroke belongs in every band; its readout
    /// plate and its selection handles belong in one, or a date range's
    /// "17 bars 4m 21s" is stamped three times down the screen.
    pub primary_band: bool,
    /// The object's own style — hit-testing reads it too (an invisible fill
    /// takes no part in the interior hit-test).
    pub style: DrawingStyle,
    pub selected: bool,
    /// True while the wrapper paints the selection halo pass: tools draw
    /// only their stroke geometry then — no fills, no labels.
    pub halo: bool,
    /// True while this object's *content* is being edited somewhere else on
    /// screen — the on-chart note editor holding the words while they are
    /// typed. A tool whose content is words paints none of it then: the
    /// field is showing the same thing, one line lower, and an empty note
    /// showed its "Note" placeholder stacked over the field's "Add text",
    /// which reads as two objects.
    pub content_editing: bool,
}

/// What the trader is holding down while they shape an object.
///
/// Shift is free to take during a chart drag, and that is worth stating
/// because Shift is otherwise the trading modifier: every paper-trading
/// hotkey is Shift **plus a letter** (`docs/ux/paper-trading.md` §9) and the
/// rail's tool keys are letters too, so the modifier held on its own cannot
/// fire an order, flatten a position or arm a tool. Holding it while the hand
/// is on the mouse costs nothing and collides with nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Constrain {
    /// The pointer means exactly where it is.
    #[default]
    Free,
    /// Shift is down: hold the shape level.
    Level,
}

/// Hold `cursor` level with `anchor` — the same height, free to slide along
/// the tape.
///
/// Level, and not "the nearest of 0°/45°/90°", because a chart's two axes are
/// not the same kind of thing: one is a price and the other is time, their
/// ratio changes with every zoom, and a 45° line drawn today is a different
/// line after one scroll. Horizontal is the only angle that survives a zoom,
/// and it is the one that means something — a level *is* a price a trader is
/// holding constant. Vertical is an instant, which is what the vertical-line
/// tool is for.
pub(super) fn level_with(anchor: egui::Pos2, cursor: egui::Pos2) -> egui::Pos2 {
    egui::pos2(cursor.x, anchor.y)
}

/// Where a tool wants its anchor to land on the bar under the pointer.
///
/// Almost every tool answers [`AnchorSnap::Pointer`]: the trader chose the
/// price by pointing at it, and the OHLC magnet is theirs to switch on. A
/// *mark* is the exception — it is a note about a bar, not a level, and it
/// only reads as a mark when it sits clear of the candle it belongs to. It
/// snaps whether or not the magnet is on, because a mark floating inside a
/// candle body is the failure the tool exists to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnchorSnap {
    #[default]
    Pointer,
    BarLow,
    BarHigh,
    /// Glued to the nearest of the bar's OHLC, whatever the distance and
    /// whether or not the magnet is on — and the bar itself clamps to the
    /// tape. For a tool whose anchor *means a bar* (the anchored VWAP): its
    /// price is presentation, and a ball floating in empty space far above
    /// any candle reads as a bug, not as a choice.
    NearestOhlc,
}

/// A tool's arming shortcut, declared by the tool itself so the keyboard
/// map never becomes a central match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolShortcut {
    pub key: egui::Key,
    pub shift: bool,
}

/// A family of related tools sharing one rail slot. Declared by each member,
/// never listed centrally — the rail folds consecutive registry entries with
/// equal `id` into a single split button. `PartialEq` only: the stroke
/// coordinates are `f32`, and nothing orders or hashes families.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolFamily {
    pub id: &'static str,
    /// Header of the family flyout.
    pub title: &'static str,
    /// Slot icon before any member has been armed.
    pub icon: &'static str,
    /// Vector icon for the slot, painted instead of `icon` when non-empty —
    /// same contract as [`DrawingTool::icon_strokes`].
    pub icon_strokes: IconStrokes,
    /// Anchor dots for the slot icon — same contract as
    /// [`DrawingTool::icon_dots`].
    pub icon_dots: IconDots,
    /// Letter set into the slot icon — same contract as
    /// [`DrawingTool::icon_letter`]. A family whose slot borrows one
    /// member's picture declares that member's letter, or the slot would be
    /// the only place that drawing appears unnamed.
    pub icon_letter: Option<IconLetter>,
}

/// A vector icon: polylines in the unit square (x right, y down), scaled to
/// the glyph box at paint time. `&[]` means "use the font glyph". A tool
/// declares one when no Phosphor glyph draws its meaning — a slanted
/// channel, Fibonacci levels — so the icon is registry data, not a special
/// case in the chrome.
pub type IconStrokes = &'static [&'static [(f32, f32)]];

/// The dots of a vector icon: points in the same unit square, painted as
/// small filled circles over the strokes.
///
/// They exist because the icons that needed them are icons *of a gesture*.
/// A Fib is not a stack of lines — it is a leg the trader dragged, with the
/// levels hung off it, and the two ends of that drag are what tells it apart
/// from the extension's three. Every drawing platform draws these tools with
/// their anchors marked for that reason. Registry data like the strokes, so
/// the chrome never learns which tool it is painting.
pub type IconDots = &'static [(f32, f32)];

/// A letter set into a vector icon: the character, where its centre sits in
/// the same unit square as the strokes, and its size as a fraction of the
/// glyph box.
///
/// It exists for the tools that draw the *same picture*. The two Fib tools
/// are one ladder of levels hung off one leg, and only the number of anchors
/// separates them — a difference that is two dots wide on a 32 px rail. `R`
/// and `P` say retracement and projection outright, so a trader picks the
/// one they meant without hovering for the tooltip first. Registry data like
/// the strokes: the chrome paints the letter it is handed and never learns
/// which tool it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconLetter {
    /// The letter itself, painted in the icon's own colour.
    pub text: &'static str,
    /// Centre of the letter in the unit square (x right, y down).
    pub at: (f32, f32),
    /// Font size as a fraction of the glyph box's height.
    pub height: f32,
}

macro_rules! register_drawing_tools {
    ($($module:ident),+ $(,)?) => {
        $(mod $module;)+
        pub const DRAWING_TOOLS: [DrawingTool; [$(stringify!($module)),+].len()] = [
            $(DrawingTool(&$module::TOOL)),+
        ];
    };
}

// The extension port: a new tool is one implementation file plus one name
// here. Order is rail order, and consecutive entries declaring the same
// family fold into one rail slot — so the grouping below is the grouping the
// trader sees, and adding a tool cannot silently reorder the rail.
register_drawing_tools!(
    // Lines
    trend_line,
    ray,
    extended_line,
    horizontal_line,
    horizontal_ray,
    vertical_line,
    arrow,
    // Channels
    parallel_channel,
    // Marks
    arrow_mark_up,
    arrow_mark_down,
    // Freehand
    brush,
    // Shapes
    rectangle,
    ellipse,
    triangle,
    // Fib
    fib_retracement,
    fib_extension,
    // Measure
    measure,
    price_range,
    date_range,
    fixed_range_profile,
    // Series
    anchored_vwap,
    // Annotation
    text,
);

// The rectangle's registry id, re-exported for the strategy seat — the one
// gate that names a specific shape (two anchors honestly bound a price
// region), in the `frvp::TOOL_ID` idiom.
pub use rectangle::TOOL_ID as RECTANGLE_TOOL_ID;

// The rectangle's payload, re-exported for the strategy seat too: whether
// the drawn band extends right decides whether an armed region ever
// expires off its right anchor.
pub use rectangle::RectanglePayload;

// The profile drawing's payload types, re-exported for `crate::frvp` — the
// refresh pass that folds engine ladders into the cache the paint reads.
pub use fixed_range_profile::{FrvpCache, FrvpCacheKey, FrvpEmpty, FrvpPayload};

// The anchored VWAP's payload types, re-exported for `crate::avwap` — the
// refresh pass that replays the indicators-crate kernel into the cache.
pub use anchored_vwap::{
    AVWAP_BAND_PAIRS, AVWAP_ROW_WIDTH, AvwapBand, AvwapCache, AvwapCacheKey, AvwapPartialSig,
    AvwapPayload,
};

/// One anchor of a drawing.
///
/// `bar` is the pane's own fractional slot — the coordinate the chart draws
/// from, and the one pan, zoom and history prepends keep meaningful.
/// `time_ms` is the same instant said in market time, captured when the
/// anchor was placed. Two panes of one symbol disagree completely about bar
/// indices and agree exactly about market time, which is what lets a drawing
/// cross from the timeframe chart to the tick chart
/// (`docs/ux/drawing-tools-2026-08.md` §D7).
///
/// It is an `Option` because a pane cannot always name the time: an anchor
/// dropped past the newest bar, or on a pane with no bars yet, has no instant
/// behind it. A drawing whose anchors have no time simply cannot be shared —
/// it is never guessed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartPoint {
    pub bar: f32,
    pub price: f64,
    pub time_ms: Option<i64>,
}

impl ChartPoint {
    /// An anchor with no market time behind it. Test-only on purpose: every
    /// production anchor comes from a pane, and a pane always knows whether
    /// the slot under the pointer has an instant behind it. A production
    /// caller reaching for this would be dropping that answer on the floor.
    #[cfg(test)]
    #[must_use]
    pub const fn at(bar: f32, price: f64) -> Self {
        Self {
            bar,
            price,
            time_ms: None,
        }
    }

    #[must_use]
    pub const fn at_time(bar: f32, price: f64, time_ms: Option<i64>) -> Self {
        Self {
            bar,
            price,
            time_ms,
        }
    }
}

/// A glyph tool's own type size, and the range it accepts. The range travels
/// with the value so a host offering sizes never has to know which tool it
/// is talking to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphSize {
    pub px: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawingStyle {
    pub color: egui::Color32,
    pub width_px: f32,
    pub fill_alpha: u8,
}

impl Default for DrawingStyle {
    fn default() -> Self {
        Self {
            color: DEFAULT_DRAWING_COLOR,
            width_px: DEFAULT_DRAWING_WIDTH_PX,
            fill_alpha: DEFAULT_DRAWING_FILL_ALPHA,
        }
    }
}

/// Which charts a drawing appears on
/// (`docs/ux/drawing-tools-2026-08.md` §D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawingScope {
    /// The pane it was drawn on, and only that one. Today's behaviour, and
    /// the default, so nothing that exists changes.
    #[default]
    ThisChart,
    /// Every pane of the same tab — one symbol, one feed, two bar types. The
    /// anchors are re-expressed through each pane's own market clock.
    ///
    /// Never across tabs: a price level drawn on BTC means nothing on a WIN
    /// chart, and a mark that says otherwise is the data-honesty failure this
    /// repo refuses.
    AllCharts,
}

/// Durable identity of one indicator pane inside a chart pane.
///
/// `kind` is the constructor the indicator was added through (`native.cvd`,
/// `script.zigzag.pine`), `ordinal` distinguishes two instances of the same
/// kind in add order. Deliberately *not* the `SlotId`: slots are a monotonic
/// counter, so removing an indicator and adding it back always yields a new
/// one, and every drawing on that pane would orphan on the most common
/// indicator action there is. Ordinal-within-kind is also why the key can
/// never re-adopt a drawing onto a *different* indicator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneKey {
    /// Shared, not cloned: a key is copied on every band carve, which runs
    /// twice per chart pane per frame.
    pub kind: std::sync::Arc<str>,
    pub ordinal: u8,
}

/// Which value axis of the chart pane an object's anchors live on.
///
/// A *band* is a region of one chart pane owning a value axis: the candles'
/// price band, plus one per expanded indicator pane. A drawing belongs to
/// exactly one band, or — for the time-only tools — to none of them and
/// therefore to all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DrawingBand {
    /// The candles' price axis. Today's behaviour, and the default, so
    /// nothing that exists changes.
    #[default]
    Price,
    /// One indicator pane's own value axis. Painted, hit-tested and dragged
    /// only there: a CVD level drawn through the candles would read as a
    /// price, which is the data-honesty failure this repo refuses.
    Indicator(PaneKey),
    /// No value axis at all: the object marks an instant, so it paints as a
    /// clipped segment in every band while remaining one object.
    AllBands,
}

/// Stable identity of one drawn object, unique within its pane's store for
/// the life of the session.
///
/// The `Vec` index is a *position* — `bring_to_front` and deletes reorder
/// it under anything that remembers it. Everything that must keep pointing
/// at "that drawing" across frames (an armed strategy on a rectangle, a
/// future alert) holds this id instead and resolves it through
/// [`Drawings::index_of`] each time. Ids are never reused; an undone delete
/// restores the object under the id it always had, so a reference held
/// across the undo keeps meaning the same object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DrawingId(pub u64);

/// What [`Drawings::duplicate_selected`] made: the object it copied, and the
/// copy.
///
/// The pair is returned rather than acted on because everything that rides a
/// drawing without living in it — an armed strategy today, whatever docks
/// next — is owned a layer up. `Drawings` stays a store of marks and learns
/// nothing about strategies; the pane that owns both reads this and carries
/// the passengers across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duplicated {
    pub source: DrawingId,
    pub copy: DrawingId,
}

/// Who placed an object, when it was not the trader's own hand.
///
/// Data honesty, at the level the eye works: an object an assistant put on
/// the chart must never be indistinguishable from one the trader drew. The
/// two strings are the control plane's own vocabulary — the actor kind and
/// the client's name from its handshake — carried as plain text so the
/// drawings layer stays free of control types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingAuthor {
    /// `agent`, `automation` — the wire's actor kind.
    pub actor_kind: String,
    /// The client's own name, as it introduced itself.
    pub client_name: String,
}

impl DrawingAuthor {
    /// One line for a panel: "Claude Code (agent)".
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} ({})", self.client_name, self.actor_kind)
    }
}

#[derive(Debug, Clone)]
pub struct Drawing {
    /// See [`DrawingId`]: identity, where the index is only position.
    pub id: DrawingId,
    /// Set when something other than the trader's hand placed this object.
    /// `None` is the trader's own; anything else is shown as its author's
    /// wherever the object is named, and is the only thing the annotate tier
    /// of the control plane is allowed to remove.
    pub author: Option<DrawingAuthor>,
    /// The trader's own name for the object ("congestão 108k"). `None`
    /// falls back to the derived `"<tool> <n>"` label everywhere a label is
    /// shown; empty strings are normalised to `None` on edit.
    pub name: Option<String>,
    pub tool: DrawingTool,
    pub points: Vec<ChartPoint>,
    /// The value axis the anchors were placed against.
    pub band: DrawingBand,
    pub style: DrawingStyle,
    /// A locked drawing keeps rejecting geometry edits and unforced deletes;
    /// its style stays editable.
    pub locked: bool,
    /// A hidden drawing neither paints nor hit-tests, and stays recoverable.
    pub hidden: bool,
    /// Whether the other panes of this tab show it too.
    pub scope: DrawingScope,
    /// Set when the tab changed the instrument under this mark.
    ///
    /// Time survives a symbol switch and price does not: BTC traded at the
    /// same instants the index did, so the anchors resolve perfectly and the
    /// level lands at a price that means nothing on the chart it is now over.
    /// `off_series` cannot catch it — the series *does* reach those instants.
    ///
    /// Marks are never deleted by a state change, so this is what keeps that
    /// honest: the object stays, and it says it belongs to another market
    /// rather than pretending to be a level on this one.
    pub foreign_market: bool,
    /// Set by [`Drawings::reanchor`] when this pane's series does not reach
    /// the market instant an anchor was placed at — the mark survived a
    /// re-cut, a rewind or a symbol switch, but it is no longer sitting on
    /// the data it was drawn against.
    ///
    /// Derived state, never edited: it is what the honesty fade and the
    /// object manager's off-series badge read, and it is deliberately absent
    /// from [`PartialEq`] so re-anchoring can never look like a user edit to
    /// the undo history.
    pub off_series: bool,
    /// Tool-owned state (Fib levels, a future tool's own properties). The
    /// registry creates it; the shared envelope never learns its fields.
    pub payload: Box<dyn DrawingPayload>,
}

impl Drawing {
    /// The label every list and menu shows: the trader's name when one was
    /// given, the tool name plus the 1-based position otherwise.
    #[must_use]
    pub fn display_label(&self, index: usize) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("{} {}", self.tool.name(), index + 1),
        }
    }

    /// Whether this object *can* be shared: every anchor has to name a market
    /// instant, because that is the only coordinate two panes agree on. An
    /// anchor dropped past the newest bar has none, and no time is invented
    /// to make the checkbox available.
    #[must_use]
    pub fn shareable(&self) -> bool {
        !self.points.is_empty() && self.points.iter().all(|point| point.time_ms.is_some())
    }

    /// Whether the other panes of this tab paint it this frame.
    #[must_use]
    pub fn shared(&self) -> bool {
        self.scope == DrawingScope::AllCharts && !self.hidden && self.shareable()
    }
}

impl PartialEq for Drawing {
    fn eq(&self, other: &Self) -> bool {
        // `id` stays out: identity is not content, and an undo snapshot
        // holding the same objects under the same ids must compare equal to
        // the live store by their *edits* alone. `name` is content — a
        // rename is an edit the undo history records.
        self.name == other.name
            && self.author == other.author
            && self.tool == other.tool
            && self.points == other.points
            && self.band == other.band
            && self.style == other.style
            && self.locked == other.locked
            && self.hidden == other.hidden
            && self.scope == other.scope
            && self.payload.eq_dyn(other.payload.as_ref())
    }
}

/// The look a freshly placed object opens with: the trader's saved default
/// for the tool when there is one, the built-in start otherwise. Both halves
/// travel together because a saved look is one thing to a trader, not a style
/// and a payload.
pub struct NewDrawing {
    pub style: DrawingStyle,
    pub payload: Box<dyn DrawingPayload>,
}

/// What a delete request did. Locked objects demand an explicit `force`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    NeedsConfirmation,
    NothingSelected,
}

/// One undo step: the whole collection plus the global-hide layer. Selection,
/// viewport and inspector state deliberately stay out, so undo never yanks
/// the camera or the UI around.
#[derive(Debug, Clone, PartialEq)]
struct UndoEntry {
    items: Vec<Drawing>,
    all_hidden: bool,
}

#[derive(Debug, Default)]
pub struct Drawings {
    /// Bumped on every change to the collection — a placement, an edit, a
    /// delete, an undo, a load. What the layout store compares against to
    /// know a pane's drawings need writing, at the cost of one integer per
    /// pane per frame rather than a walk of every object.
    revision: u64,
    items: Vec<Drawing>,
    draft: Option<Drawing>,
    selected: Option<usize>,
    /// Source of [`DrawingId`]s: incremented on every allocation and never
    /// rewound — not by undo, not by delete — so an id can never be reborn
    /// as a different object.
    next_id: u64,
    /// Global hide layer. Independent from each drawing's own eye, so
    /// "show all" restores exactly the per-object visibility it found.
    all_hidden: bool,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    /// Snapshot taken when a pointer gesture starts; committed (as one undo
    /// entry) on release, so a whole drag coalesces into one command.
    gesture_baseline: Option<UndoEntry>,
}

pub(super) fn drawing_stroke(style: DrawingStyle) -> egui::Stroke {
    egui::Stroke::new(style.width_px, style.color)
}

pub(super) fn drawing_fill(style: DrawingStyle) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        style.color.r(),
        style.color.g(),
        style.color.b(),
        style.fill_alpha,
    )
}

pub(super) fn distance_to_segment(position: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_sq = segment.length_sq();
    if length_sq <= f32::EPSILON {
        return position.distance(start);
    }
    let projection = ((position - start).dot(segment) / length_sq).clamp(0.0, 1.0);
    position.distance(start + segment * projection)
}

/// The unit normal of `direction` — the axis a shape's thickness is measured
/// along. A direction of no length has no normal of its own, so it is given
/// the vertical, which is the axis a chart measures in.
pub(super) fn unit_normal(direction: egui::Vec2) -> egui::Vec2 {
    let length = direction.length();
    if length <= f32::EPSILON {
        return egui::vec2(0.0, 1.0);
    }
    egui::vec2(-direction.y, direction.x) / length
}

/// Push `cursor` off the line through `start`–`end` until it stands at least
/// `floor_px` away from it, keeping exactly where it sits *along* that line.
///
/// The shared half of [`DrawingToolImpl::pending_anchor`]: a tool whose third
/// anchor gives a shape its thickness is degenerate when that anchor lands on
/// the line the first two drew — a channel of no width, a triangle of no
/// area — and that is precisely where the pointer is standing the instant a
/// drag lets go. Sliding *along* the line is left alone, so the gesture still
/// means what it always meant; only the collapsed case is refused.
///
/// A cursor exactly on the line opens the shape on the normal's own side, so
/// the same gesture always produces the same object.
pub(super) fn off_line_by(
    start: egui::Pos2,
    end: egui::Pos2,
    cursor: egui::Pos2,
    floor_px: f32,
) -> egui::Pos2 {
    let normal = unit_normal(end - start);
    let offset = (cursor - start).dot(normal);
    if offset.abs() >= floor_px {
        return cursor;
    }
    let side = if offset < 0.0 { -1.0 } else { 1.0 };
    cursor + normal * (side * floor_px - offset)
}

#[cfg(test)]
mod tests;
