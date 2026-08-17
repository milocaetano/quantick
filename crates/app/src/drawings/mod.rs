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
const UNDO_HISTORY_LIMIT: usize = 64;
const SELECTED_ANCHOR_RADIUS_PX: f32 = 3.5;
/// Handles read as hollow rings, not solid discs: the core is the chart's own
/// backdrop, so the handle marks the anchor without adding a bright blob over
/// the candles (`docs/ux/drawing-tools-2026-08.md` §D2).
const SELECTED_ANCHOR_FILL: egui::Color32 = theme::CANVAS;
const SELECTED_ANCHOR_RING_WIDTH_PX: f32 = 1.25;
/// Selection never repaints the object white: it keeps the configured colour
/// and paints this soft halo underneath instead, plus ring anchor handles.
/// Premultiplied ~11% white — enough to find the object under the pointer,
/// not enough to double its visual weight.
const SELECTION_HALO_COLOR: egui::Color32 = egui::Color32::from_rgba_premultiplied(28, 28, 28, 28);
/// How much wider than the object's own stroke the halo pass paints.
const SELECTION_HALO_EXTRA_WIDTH_PX: f32 = 2.5;
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
}

/// A vector icon: polylines in the unit square (x right, y down), scaled to
/// the glyph box at paint time. `&[]` means "use the font glyph". A tool
/// declares one when no Phosphor glyph draws its meaning — a slanted
/// channel, Fibonacci levels — so the icon is registry data, not a special
/// case in the chrome.
pub type IconStrokes = &'static [&'static [(f32, f32)]];

/// The implementation port every drawing plugs into. Selection visuals (halo
/// and anchor handles) are common chrome painted by the wrapper, so a tool
/// only ever paints its own geometry in the style it is given. Capability
/// methods drive which inspector sections exist for the tool — an
/// unsupported property is absent, never disabled.
trait DrawingToolImpl: Sync {
    fn id(&self) -> &'static str;
    /// Human name shown in the inspector header and the object manager.
    fn name(&self) -> &'static str;
    fn settings_title(&self) -> &'static str;
    fn icon(&self) -> &'static str;
    /// Vector strokes painted in place of [`Self::icon`] when non-empty —
    /// see [`IconStrokes`].
    fn icon_strokes(&self) -> IconStrokes {
        &[]
    }
    fn hover_text(&self) -> &'static str;
    fn required_points(&self) -> usize;
    /// What the *next* click will do, with `placed` anchors already down.
    ///
    /// A multi-anchor tool that stops following the pointer looks broken:
    /// the trader dragged, let go, and the object sat there waiting for a
    /// click nobody told them about. The rail's `2/3` badge is true but it
    /// is on the far side of the screen from where the eye is. A tool that
    /// can say what it wants next says it here, and the draft prints it by
    /// the cursor.
    fn placement_hint(&self, _placed: usize) -> Option<&'static str> {
        None
    }
    /// Where the anchor the trader is still shaping really lands, given the
    /// anchors already down and the pointer — both in screen space.
    ///
    /// Default: the pointer itself, which is every tool whose anchors mean
    /// exactly where they were dropped.
    ///
    /// It exists because a tool of three anchors has a *shaping* phase the
    /// raw pointer describes badly. A drag fixes a channel's trend line and
    /// lets go with the pointer still sitting **on** that line, and a
    /// channel's width is measured across the line and nowhere else — so the
    /// width the pointer implies at that instant is exactly zero. The preview
    /// draws a corridor of no width, which is a straight line, and the click
    /// that looks like it confirms the shape commits one: a three-anchor
    /// object that *is* a line. A tool that knows what it is refuses to be
    /// born degenerate, and says so here rather than leaving the host to
    /// special-case it by id.
    ///
    /// The host runs the preview *and* the commit through this, so the object
    /// a click creates is always the one that was on screen when it was
    /// clicked.
    ///
    /// `constrain` is what the trader is holding — see [`Constrain`]. A tool
    /// that has an axis worth holding to says so here; the rest ignore it.
    ///
    /// Rate: per frame while a draft is in flight, over a handful of anchors.
    fn pending_anchor(
        &self,
        _placed: &[egui::Pos2],
        cursor: egui::Pos2,
        _constrain: Constrain,
    ) -> egui::Pos2 {
        cursor
    }
    /// The key that arms this tool from the chart, if it has one.
    fn shortcut(&self) -> Option<ToolShortcut> {
        None
    }
    /// Where this tool's anchors land on the bar under the pointer.
    fn anchor_snap(&self) -> AnchorSnap {
        AnchorSnap::Pointer
    }
    /// Whether a freshly placed object of this tool needs its settings panel
    /// opened straight away.
    ///
    /// `false` for every tool whose object is complete the moment it is
    /// drawn — which is all of them but one. A text note is placed *empty*,
    /// and the field that gives it words is in the panel: without this it
    /// arrives as a grey placeholder with no visible way to write in it.
    fn opens_settings_on_place(&self) -> bool {
        false
    }
    /// Whether this tool is placed by a held drag instead of by N clicks.
    ///
    /// A freehand tool answers `0` from [`Self::required_points`], because
    /// the count is whatever the gesture gave: the host starts its draft on
    /// the press, feeds it the path, and finishes it on the release.
    fn freehand(&self) -> bool {
        false
    }
    /// The rectangle this tool actually *paints*, given the box its anchors
    /// span and the pane it is drawn in.
    ///
    /// The anchor box is the default and is right for most tools: a trend
    /// line, a rectangle, a triangle all end where their anchors do. It is
    /// badly wrong for the ones that do not. A fixed-range profile carries two
    /// anchors at a single price and paints a histogram across the whole price
    /// axis; a vertical line has one anchor and paints floor to ceiling.
    ///
    /// Anything that has to keep clear of an object — the settings inspector,
    /// the context bar — asks for this, not for the anchors. Placing against
    /// the anchors of a profile means walking around a thin horizontal sliver
    /// and landing in the middle of the figure, which is precisely the bug
    /// this exists to make impossible.
    fn painted_bounds(&self, anchors: egui::Rect, _chart: egui::Rect) -> egui::Rect {
        anchors
    }
    /// The colour a fresh object of this tool is born in, when the stock
    /// blue would be the wrong answer. `None` — almost every tool — takes
    /// [`DEFAULT_DRAWING_COLOR`].
    ///
    /// It exists for the tools whose colour *is* their meaning: a buy mark
    /// that arrives blue is one the trader repaints every single time. The
    /// trader's own saved default still wins over this, because that one was
    /// chosen rather than assumed.
    fn default_color(&self) -> Option<egui::Color32> {
        None
    }
    /// The stroke width a fresh object of this tool is born with, when the
    /// stock hairline would be the wrong answer. The hairline rule (§D2) is
    /// written for *annotations*; a tool that paints a derived **series** —
    /// one value per bar competing with candle bodies — declares its weight
    /// here instead of asking every trader to fix it by hand.
    fn default_width_px(&self) -> Option<f32> {
        None
    }
    /// The fill alpha a fresh object opens with, when the stock value would
    /// read as "the fill is broken" for this tool's geometry.
    fn default_fill_alpha(&self) -> Option<u8> {
        None
    }
    /// The chart's right-click menu entry that places this tool at the
    /// clicked bar, if the tool wants one. The pane sweeps the registry —
    /// the next series tool docks with a declaration, not a pane.rs edit.
    fn context_menu_label(&self) -> Option<&'static str> {
        None
    }
    /// The rail family this tool belongs to, if any. Consecutive registry
    /// entries with the same family id share one rail slot.
    fn family(&self) -> Option<ToolFamily> {
        None
    }
    /// Whether the tool paints an interior that the fill controls affect.
    fn supports_fill(&self) -> bool {
        false
    }
    /// Whether this tool's anchors carry a meaningful *value*.
    ///
    /// Almost every tool's second coordinate means something on the axis it
    /// was drawn against, which is what binds it to one band. A vertical line
    /// and a date range mark instants: they belong to no band, so they are
    /// placed as [`DrawingBand::AllBands`] and painted through every band as
    /// one object (`docs/ux/drawing-tools-2026-08.md` §D10).
    fn value_axis(&self) -> bool {
        true
    }
    /// Whether this tool only ever means something on the candles' price
    /// axis. The mirror of [`Self::value_axis`]'s `false`: where a time-only
    /// tool belongs to *every* band, a price-only tool belongs to the price
    /// band whatever band it was started over — a volume profile's rows are
    /// prices, and one drawn against a CVD axis would be the data-honesty
    /// failure this repo refuses.
    fn price_band_only(&self) -> bool {
        false
    }
    /// Whether the tool paints a stroke the width control affects. Almost
    /// every tool does; a text note has glyphs and no stroke at all, and a
    /// width slider on its Style tab would move nothing.
    fn supports_stroke_width(&self) -> bool {
        true
    }
    /// The object's own glyph size, for a tool drawn as a glyph rather than
    /// a stroke — a text note, a trade mark. `None`, the answer for almost
    /// every tool, means the object has no such size and the context bar
    /// offers stroke width in that slot instead.
    ///
    /// The size is in screen pixels and stays in screen pixels: a note about
    /// the chart that inflates with the zoom has quietly become a second
    /// series.
    fn glyph_size(&self, _payload: &dyn DrawingPayload) -> Option<GlyphSize> {
        None
    }
    /// Write a glyph size back. A tool that answers [`Self::glyph_size`]
    /// must implement this, or its own control would move nothing.
    fn set_glyph_size(&self, _payload: &mut dyn DrawingPayload, _px: f32) {}
    /// Fresh tool-owned state for a newly placed object.
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(NoPayload)
    }
    /// Title of the tool-owned inspector tab, if the tool brings one.
    fn extra_tab(&self) -> Option<&'static str> {
        None
    }
    /// Draw the tool-owned inspector tab. Returns whether anything was
    /// edited (the caller folds it into the shared undo coalescing).
    fn draw_extra_tab(
        &self,
        _ui: &mut egui::Ui,
        _drawing: &mut Drawing,
        _host: &mut dyn PresetHost,
    ) -> bool {
        false
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    );
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool;
    /// Where the trader grabs this object, in screen space. `None` — the
    /// answer for almost every tool — means the raw anchors: the point you
    /// clicked is the point you drag.
    ///
    /// A tool overrides this when its anchors are not where the gesture
    /// belongs. A channel is the case that forced the port open: its third
    /// anchor is a corner of the corridor, so the only way to widen one was
    /// to find a lone dot off in the distance, and only ever that one edge.
    /// The handle for a rail belongs at the centre of that rail, and there is
    /// one per rail.
    fn handles(
        &self,
        _chart_rect: egui::Rect,
        _points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        None
    }
    /// Apply a drag of handle `handle` to screen position `to`, answering the
    /// object's new screen anchors — the tool decides which anchors a handle
    /// moves, and a handle may well move more than one.
    ///
    /// `None` (the default) means the plain anchor move the host already
    /// does. A tool that overrides [`DrawingToolImpl::handles`] must override
    /// this too: a handle that is not an anchor has no default meaning.
    fn drag_handle(
        &self,
        _chart_rect: egui::Rect,
        _points: &[egui::Pos2],
        _handle: usize,
        _to: egui::Pos2,
        _ctxt: &DrawContext<'_>,
        _constrain: Constrain,
    ) -> Option<Handles> {
        None
    }
    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2);
}

/// A cheap, copyable reference to one registered implementation.
#[derive(Clone, Copy)]
pub struct DrawingTool(&'static dyn DrawingToolImpl);

impl DrawingTool {
    #[must_use]
    pub fn id(self) -> &'static str {
        self.0.id()
    }

    /// Look up a registered tool by its stable id — how the saved favorites
    /// list and the env hooks name tools. `None` for an id no registered
    /// tool carries (a stale file survives a removed tool).
    #[must_use]
    pub fn by_id(id: &str) -> Option<Self> {
        DRAWING_TOOLS.into_iter().find(|tool| tool.id() == id)
    }

    /// Vector icon strokes, `&[]` when the tool paints a font glyph.
    #[must_use]
    pub fn icon_strokes(self) -> IconStrokes {
        self.0.icon_strokes()
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        self.0.name()
    }

    #[must_use]
    pub fn settings_title(self) -> &'static str {
        self.0.settings_title()
    }

    #[must_use]
    pub fn supports_fill(self) -> bool {
        self.0.supports_fill()
    }

    #[must_use]
    pub fn supports_stroke_width(self) -> bool {
        self.0.supports_stroke_width()
    }

    #[must_use]
    pub fn anchor_snap(self) -> AnchorSnap {
        self.0.anchor_snap()
    }

    #[must_use]
    pub fn freehand(self) -> bool {
        self.0.freehand()
    }

    /// See [`DrawingToolImpl::painted_bounds`].
    #[must_use]
    pub fn painted_bounds(self, anchors: egui::Rect, chart: egui::Rect) -> egui::Rect {
        self.0.painted_bounds(anchors, chart)
    }

    #[must_use]
    pub fn opens_settings_on_place(self) -> bool {
        self.0.opens_settings_on_place()
    }

    /// The stock look of a fresh object of this tool, before the trader's
    /// own saved default is consulted.
    #[must_use]
    pub fn default_style(self) -> DrawingStyle {
        let stock = DrawingStyle::default();
        DrawingStyle {
            color: self.0.default_color().unwrap_or(DEFAULT_DRAWING_COLOR),
            width_px: self.0.default_width_px().unwrap_or(stock.width_px),
            fill_alpha: self.0.default_fill_alpha().unwrap_or(stock.fill_alpha),
        }
    }

    #[must_use]
    pub fn context_menu_label(self) -> Option<&'static str> {
        self.0.context_menu_label()
    }

    #[must_use]
    pub fn glyph_size(self, drawing: &Drawing) -> Option<GlyphSize> {
        self.0.glyph_size(drawing.payload.as_ref())
    }

    pub fn set_glyph_size(self, drawing: &mut Drawing, px: f32) {
        self.0.set_glyph_size(drawing.payload.as_mut(), px);
    }

    #[must_use]
    pub fn value_axis(self) -> bool {
        self.0.value_axis()
    }

    /// The band a fresh object of this tool is placed on when the pointer is
    /// over `band`. A time-only tool ignores the band it was drawn in — it
    /// crosses all of them, and a band picker on it would be a control with
    /// one correct setting. A price-only tool ignores it the other way: its
    /// values are prices, so it lands on the price band wherever it started.
    #[must_use]
    pub fn band_for(self, band: &DrawingBand) -> DrawingBand {
        if self.0.price_band_only() {
            DrawingBand::Price
        } else if self.value_axis() {
            band.clone()
        } else {
            DrawingBand::AllBands
        }
    }

    #[must_use]
    pub fn default_payload(self) -> Box<dyn DrawingPayload> {
        self.0.default_payload()
    }

    #[must_use]
    pub fn shortcut(self) -> Option<ToolShortcut> {
        self.0.shortcut()
    }

    #[must_use]
    pub fn family(self) -> Option<ToolFamily> {
        self.0.family()
    }

    #[must_use]
    pub fn extra_tab(self) -> Option<&'static str> {
        self.0.extra_tab()
    }

    /// Draw the tool-owned inspector tab; returns whether anything changed.
    pub fn draw_extra_tab(
        self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        host: &mut dyn PresetHost,
    ) -> bool {
        self.0.draw_extra_tab(ui, drawing, host)
    }

    #[must_use]
    pub fn icon(self) -> &'static str {
        self.0.icon()
    }

    #[must_use]
    pub fn hover_text(self) -> &'static str {
        self.0.hover_text()
    }

    #[must_use]
    pub fn required_points(self) -> usize {
        self.0.required_points()
    }

    #[must_use]
    pub fn placement_hint(self, placed: usize) -> Option<&'static str> {
        self.0.placement_hint(placed)
    }

    /// Where the anchor under the pointer really lands while the object is
    /// still being shaped — see [`DrawingToolImpl::pending_anchor`].
    #[must_use]
    pub fn pending_anchor(
        self,
        placed: &[egui::Pos2],
        cursor: egui::Pos2,
        constrain: Constrain,
    ) -> egui::Pos2 {
        self.0.pending_anchor(placed, cursor, constrain)
    }

    /// Paint the object. Selection adds a halo *under* the geometry and, when
    /// `show_handles` (not locked), white anchor handles on top — the object's
    /// configured colour keeps carrying meaning either way.
    pub fn paint(
        self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
        show_handles: bool,
    ) {
        if ctxt.selected {
            let halo_style = DrawingStyle {
                color: SELECTION_HALO_COLOR,
                width_px: style.width_px + SELECTION_HALO_EXTRA_WIDTH_PX,
                fill_alpha: 0,
            };
            let halo_ctxt = DrawContext {
                halo: true,
                ..*ctxt
            };
            self.0
                .paint(painter, chart_rect, halo_style, points, &halo_ctxt);
        }
        self.0.paint(painter, chart_rect, style, points, ctxt);
        if ctxt.selected && show_handles {
            let ring = egui::Stroke::new(SELECTED_ANCHOR_RING_WIDTH_PX, theme::ACCENT);
            for point in self.handles(chart_rect, points, ctxt) {
                painter.circle_filled(point, SELECTED_ANCHOR_RADIUS_PX, SELECTED_ANCHOR_FILL);
                painter.circle_stroke(point, SELECTED_ANCHOR_RADIUS_PX, ring);
            }
        }
    }

    /// The grab points of this object — the tool's own when it declares them,
    /// the raw anchors otherwise. Paint, hit-test and drag all ask here, so
    /// what the trader sees is exactly what they can grab.
    #[must_use]
    pub fn handles(
        self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) -> Handles {
        self.0
            .handles(chart_rect, points, ctxt)
            .unwrap_or_else(|| points.iter().copied().collect())
    }

    /// Whether this tool's handles *are* its anchors — true for almost every
    /// tool. A host that can only express "move anchor N" (the cross-pane
    /// shared edit) asks this before offering a handle at all, rather than
    /// leaving a grab point that moves something other than what it sits on.
    #[must_use]
    pub fn handles_are_anchors(
        self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) -> bool {
        self.0.handles(chart_rect, points, ctxt).is_none()
    }

    /// New screen anchors after dragging `handle` to `to`, when the tool owns
    /// the gesture. `None` means the host's plain "this handle is anchor
    /// `handle`" move.
    #[must_use]
    pub fn drag_handle(
        self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        handle: usize,
        to: egui::Pos2,
        ctxt: &DrawContext<'_>,
        constrain: Constrain,
    ) -> Option<Handles> {
        self.0
            .drag_handle(chart_rect, points, handle, to, ctxt, constrain)
    }

    #[must_use]
    pub fn hit_test(
        self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        self.handles(chart_rect, points, ctxt)
            .iter()
            .any(|point| point.distance_sq(position) <= radius_px * radius_px)
            || self
                .0
                .hit_test(chart_rect, points, position, radius_px, ctxt)
    }

    #[cfg(test)]
    fn test_geometry(self) -> (Vec<egui::Pos2>, egui::Pos2) {
        self.0.test_geometry()
    }
}

impl PartialEq for DrawingTool {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for DrawingTool {}

impl fmt::Debug for DrawingTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DrawingTool")
            .field(&self.id())
            .finish()
    }
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

#[derive(Debug, Clone)]
pub struct Drawing {
    /// See [`DrawingId`]: identity, where the index is only position.
    pub id: DrawingId,
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

impl Drawings {
    #[must_use]
    pub fn items(&self) -> &[Drawing] {
        &self.items
    }

    /// Mutable access for **derived-state refresh only** — `frvp::refresh`
    /// bringing cached profiles up to date. Never a user edit: nothing
    /// reached through here may participate in payload equality, or a
    /// refresh would register as an edit against the undo snapshots
    /// (`Self::record` compares them). The cache exclusion in
    /// `FrvpPayload::eq` is the other half of this contract.
    #[must_use]
    pub(crate) fn items_mut(&mut self) -> &mut [Drawing] {
        &mut self.items
    }

    /// The in-flight draft, under the same derived-state-only contract as
    /// [`Self::items_mut`] — the profile refresh folds it so the histogram
    /// is live while the range is still being placed.
    #[must_use]
    pub(crate) fn draft_mut(&mut self) -> Option<&mut Drawing> {
        self.draft.as_mut()
    }

    /// How many objects are painted on the tab's other panes as well — the
    /// per-frame reprojection cost, in the health summary.
    #[must_use]
    pub fn shared_count(&self) -> usize {
        self.items.iter().filter(|item| item.shared()).count()
    }

    fn alloc_id(&mut self) -> DrawingId {
        self.next_id += 1;
        DrawingId(self.next_id)
    }

    /// Where the object with this identity currently sits, if it still
    /// exists. The answer is only good for this frame: every reorder or
    /// delete moves it, which is the whole reason callers hold the id.
    #[must_use]
    pub fn index_of(&self, id: DrawingId) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }

    /// Rename the object at `index` as one undo step. Whitespace-only input
    /// clears the name back to the derived label.
    pub fn rename_at(&mut self, index: usize, name: &str) {
        if index >= self.items.len() {
            return;
        }
        let trimmed = name.trim();
        let name = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        let before = self.snapshot();
        self.items[index].name = name;
        self.record(before);
    }

    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, selected: Option<usize>) {
        self.selected = selected.filter(|&index| index < self.items.len());
    }

    #[must_use]
    pub fn selected_mut(&mut self) -> Option<&mut Drawing> {
        self.selected.and_then(|index| self.items.get_mut(index))
    }

    #[must_use]
    pub fn draft(&self) -> Option<&Drawing> {
        self.draft.as_ref()
    }

    #[must_use]
    pub fn draft_len(&self) -> usize {
        self.draft.as_ref().map_or(0, |draft| draft.points.len())
    }

    #[must_use]
    pub fn all_hidden(&self) -> bool {
        self.all_hidden
    }

    /// Whether the object at `index` paints and hit-tests this frame.
    #[must_use]
    pub fn is_visible(&self, index: usize) -> bool {
        !self.all_hidden && self.items.get(index).is_some_and(|item| !item.hidden)
    }

    fn snapshot(&self) -> UndoEntry {
        UndoEntry {
            items: self.items.clone(),
            all_hidden: self.all_hidden,
        }
    }

    /// Push `before` as one undo step if the store actually changed since.
    fn record(&mut self, before: UndoEntry) {
        if before == self.snapshot() {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > UNDO_HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Start coalescing: the next [`Self::commit_gesture`] records everything
    /// mutated in between as a single undo entry. Idempotent within a gesture.
    pub fn begin_gesture(&mut self) {
        if self.gesture_baseline.is_none() {
            self.gesture_baseline = Some(self.snapshot());
        }
    }

    /// End coalescing. A gesture that changed nothing records nothing.
    pub fn commit_gesture(&mut self) {
        if let Some(baseline) = self.gesture_baseline.take() {
            self.record(baseline);
        }
    }

    /// Record an already-applied edit to one object, given its pre-edit
    /// state. Used by the inspector to coalesce slider/color gestures.
    pub fn record_edit_of(&mut self, index: usize, before_drawing: Drawing) {
        let mut before = self.snapshot();
        let Some(slot) = before.items.get_mut(index) else {
            return;
        };
        *slot = before_drawing;
        self.record(before);
    }

    #[cfg(test)]
    pub(crate) fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    fn restore(&mut self, entry: UndoEntry) {
        self.items = entry.items;
        self.all_hidden = entry.all_hidden;
        self.draft = None;
        self.selected = self.selected.filter(|&index| index < self.items.len());
    }

    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(entry);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(entry);
        true
    }

    /// [`Self::place_with`] with the tool's stock look — the test-side
    /// shorthand; the app always goes through `place_with` to honour the
    /// trader's saved defaults.
    #[cfg(test)]
    pub fn place(&mut self, tool: DrawingTool, point: ChartPoint) -> bool {
        self.place_on(tool, &DrawingBand::Price, point)
    }

    /// [`Self::place`] on a named band.
    #[cfg(test)]
    pub fn place_on(&mut self, tool: DrawingTool, band: &DrawingBand, point: ChartPoint) -> bool {
        self.place_with(tool, band, point, |tool| NewDrawing {
            style: DrawingStyle::default(),
            payload: tool.default_payload(),
        })
    }

    /// [`Self::place`] with a caller-chosen look for a new draft — how the
    /// app applies the trader's saved defaults to newly created objects only.
    /// Objects that already exist are never touched by a default changing.
    pub fn place_with(
        &mut self,
        tool: DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        new_drawing: impl FnOnce(DrawingTool) -> NewDrawing,
    ) -> bool {
        if self.draft.as_ref().is_none_or(|draft| draft.tool != tool) {
            let fresh = new_drawing(tool);
            let id = self.alloc_id();
            self.draft = Some(Drawing {
                id,
                name: None,
                tool,
                points: Vec::with_capacity(tool.required_points()),
                // The band the *first* anchor landed in owns the whole
                // object: an in-flight draft that changed axis halfway
                // through would have anchors in two value spaces.
                band: tool.band_for(band),
                style: fresh.style,
                locked: false,
                hidden: false,
                scope: DrawingScope::default(),
                foreign_market: false,
                off_series: false,
                payload: fresh.payload,
            });
        }
        let draft = self.draft.as_mut().expect("draft was installed above");
        draft.points.push(point);
        // A freehand tool declares no anchor count: its draft is finished by
        // the release, through `finish_draft`, never by arithmetic here.
        if tool.required_points() > 0 && draft.points.len() == tool.required_points() {
            let before = self.snapshot();
            self.items
                .push(self.draft.take().expect("draft has points"));
            self.selected = Some(self.items.len() - 1);
            // Drawing while hide-all is engaged releases it (audit M8): the
            // act of placing a new object is the strongest possible request
            // to see drawings, and a mark that vanishes on the click that
            // finished it reads as a broken tool. One undo entry with the
            // placement — undoing the object restores the hidden state too.
            self.all_hidden = false;
            self.record(before);
            true
        } else {
            false
        }
    }

    pub fn cancel_draft(&mut self) {
        self.draft = None;
    }

    /// Finish a freehand draft: what the release does for a tool whose
    /// anchor count is whatever the hand gave.
    ///
    /// A stroke of fewer than two points is a click that missed, not a
    /// drawing — it is dropped rather than stored as an invisible object the
    /// trader can neither see nor select to delete.
    pub fn finish_draft(&mut self) -> bool {
        let Some(draft) = self.draft.take() else {
            return false;
        };
        if draft.points.len() < 2 {
            return false;
        }
        let before = self.snapshot();
        self.items.push(draft);
        self.selected = Some(self.items.len() - 1);
        // Same rule as a clicked placement: drawing releases hide-all, and
        // it does so inside the one undo entry the gesture records.
        self.all_hidden = false;
        self.record(before);
        true
    }

    /// Backspace during placement: drop the last placed anchor; dropping the
    /// only one cancels the draft.
    pub fn remove_last_draft_anchor(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.points.pop();
            if draft.points.is_empty() {
                self.draft = None;
            }
        }
    }

    /// Duplicate the selected object as one undo entry: the copy lands
    /// `offset_bars` to the right, unlocked, and becomes the selection.
    pub fn duplicate_selected(&mut self, offset_bars: f32) {
        let Some(index) = self.selected.filter(|&index| index < self.items.len()) else {
            return;
        };
        let before = self.snapshot();
        let mut copy = self.items[index].clone();
        // A copy is a new object: its own identity, and never the
        // original's name — two drawings answering to "congestão 108k"
        // would make every reference ambiguous.
        copy.id = self.alloc_id();
        copy.name = None;
        for point in &mut copy.points {
            point.bar += offset_bars;
        }
        copy.locked = false;
        self.items.push(copy);
        self.selected = Some(self.items.len() - 1);
        self.record(before);
    }

    /// Re-express every anchor against a series that was cut again — a
    /// timeframe or bar-kind switch, a replay seek, a reconnect, a symbol
    /// change.
    ///
    /// A bar index means nothing across two cuts of the tape; the market
    /// instant each anchor captured at placement does, and it is the same
    /// coordinate that already carries a drawing between the panes of a tab
    /// (`docs/ux/drawing-tools-2026-08.md` §D7). So the object is not
    /// discarded and not left pointing at a stale index: it is asked where
    /// its own timestamps landed.
    ///
    /// `slot_of` answers where a market instant sits on the new series, or
    /// `None` when the series does not reach it — before its first bar, or on
    /// an instrument that never traded at that moment. Those anchors clamp to
    /// the nearest edge and the object is flagged [`Drawing::off_series`], so
    /// it fades and says so rather than pretending to sit on data.
    ///
    /// An anchor with no timestamp at all is one dropped past the newest bar,
    /// where the tape has written nothing (see `ChartPoint::time_ms`). It has
    /// no instant to look up, so it keeps its distance past the end of the
    /// series instead — which is exactly where the trader put it.
    ///
    /// Rate: once per re-cut, never per frame or per trade, over a handful of
    /// objects. The undo stacks travel with the live items for the same
    /// reason [`Self::shift_bars`] moves them — undoing later must not
    /// resurrect coordinates from a series that no longer exists.
    pub fn reanchor(
        &mut self,
        old_slots: usize,
        new_slots: usize,
        slot_of: impl Fn(i64) -> Option<f32>,
    ) {
        #[allow(clippy::cast_precision_loss)]
        let past_end = new_slots as f32 - old_slots as f32;
        let reanchor_all = |items: &mut [Drawing]| {
            for drawing in items {
                let mut off_series = false;
                for point in &mut drawing.points {
                    let Some(time) = point.time_ms else {
                        point.bar += past_end;
                        continue;
                    };
                    match slot_of(time) {
                        Some(slot) => point.bar = slot,
                        None => {
                            off_series = true;
                            point.bar = 0.0;
                        }
                    }
                }
                drawing.off_series = off_series;
            }
        };
        reanchor_all(&mut self.items);
        if let Some(draft) = self.draft.as_mut() {
            reanchor_all(std::slice::from_mut(draft));
        }
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            reanchor_all(&mut entry.items);
        }
        if let Some(baseline) = &mut self.gesture_baseline {
            reanchor_all(&mut baseline.items);
        }
    }

    /// Mark every object as belonging to a market this tab no longer shows.
    ///
    /// Called on the one transition that changes the instrument under the
    /// marks. Everything present at that moment was drawn on the old market;
    /// anything placed afterwards is on the new one and starts clean.
    ///
    /// The undo stacks travel with the live items, for the same reason
    /// [`Self::reanchor`] moves them: undoing back to a state from before the
    /// switch must not restore a mark that claims to be a level on this
    /// instrument.
    pub fn mark_market_changed(&mut self) {
        let mark = |items: &mut [Drawing]| {
            for drawing in items {
                drawing.foreign_market = true;
            }
        };
        mark(&mut self.items);
        if let Some(draft) = self.draft.as_mut() {
            mark(std::slice::from_mut(draft));
        }
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            mark(&mut entry.items);
        }
        if let Some(baseline) = &mut self.gesture_baseline {
            mark(&mut baseline.items);
        }
    }

    /// Rewrite the market instants behind one object's anchors, after a move
    /// that changed their bar positions.
    ///
    /// [`Self::translate_selected`] and the keyboard nudge shift bar indices
    /// directly; the timestamp behind each anchor is what every *other* pane
    /// reads, so leaving it stale would drag a mark on one chart and leave
    /// its shared twin standing where it used to be. Only the pane knows how
    /// to name the instant under a slot, so it hands the answers back here.
    pub fn set_times(&mut self, index: usize, times: &[Option<i64>]) {
        let Some(drawing) = self.items.get_mut(index) else {
            return;
        };
        for (point, time) in drawing.points.iter_mut().zip(times) {
            point.time_ms = *time;
        }
    }

    // There is deliberately no `clear`. A re-cut of the bars used to wipe the
    // store, on the reasoning that a bar index cannot survive one; the anchors
    // carry market time, so they are re-expressed instead
    // ([`Self::reanchor`]). The only way a drawing leaves is the trader
    // removing it — [`Self::delete_selected`] or [`Self::delete_all`], both of
    // which are undoable.

    pub fn shift_bars(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }
        let delta = delta as f32;
        let shift = |items: &mut Vec<Drawing>| {
            for drawing in items {
                for point in &mut drawing.points {
                    point.bar += delta;
                }
            }
        };
        shift(&mut self.items);
        if let Some(draft) = &mut self.draft {
            for point in &mut draft.points {
                point.bar += delta;
            }
        }
        // History snapshots hold the same bar-index coordinates, so a prepend
        // shifts them too — undoing later must not re-anchor objects to bars
        // that moved underneath them.
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            shift(&mut entry.items);
        }
        if let Some(baseline) = &mut self.gesture_baseline {
            shift(&mut baseline.items);
        }
    }

    /// Remove every drawing as one undoable step, and report how many went.
    /// Locked objects go too: the caller gates this behind a count-bearing
    /// confirmation, and a lock protects against a stray click, not against
    /// an explicit clear-everything — `Ctrl+Z` brings the whole set back.
    pub fn delete_all(&mut self) -> usize {
        let count = self.items.len();
        if count == 0 {
            return 0;
        }
        let before = self.snapshot();
        self.items.clear();
        self.selected = None;
        self.record(before);
        count
    }

    /// One delete command for every trigger (button, manager, keyboard).
    /// A locked object is never deleted without `force`.
    pub fn delete_selected(&mut self, force: bool) -> DeleteOutcome {
        let Some(index) = self.selected.filter(|&index| index < self.items.len()) else {
            return DeleteOutcome::NothingSelected;
        };
        if self.items[index].locked && !force {
            return DeleteOutcome::NeedsConfirmation;
        }
        let before = self.snapshot();
        self.items.remove(index);
        self.selected = None;
        self.record(before);
        DeleteOutcome::Deleted
    }

    pub fn set_selected_locked(&mut self, locked: bool) {
        if let Some(index) = self.selected {
            self.set_locked_at(index, locked);
        }
    }

    pub fn set_selected_hidden(&mut self, hidden: bool) {
        if let Some(index) = self.selected {
            self.set_hidden_at(index, hidden);
        }
    }

    pub fn set_locked_at(&mut self, index: usize, locked: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.items.get_mut(index) {
            drawing.locked = locked;
            self.record(before);
        }
    }

    pub fn set_hidden_at(&mut self, index: usize, hidden: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.items.get_mut(index) {
            drawing.hidden = hidden;
            self.record(before);
        }
    }

    /// Z-order: painting walks the list front-to-back, hit-testing walks it
    /// back-to-front, so the last item is the topmost object.
    pub fn bring_to_front(&mut self, index: usize) {
        if index >= self.items.len() || index + 1 == self.items.len() {
            return;
        }
        let before = self.snapshot();
        let drawing = self.items.remove(index);
        self.items.push(drawing);
        // Selection follows the object, not the slot it used to occupy.
        self.selected = self.selected.map(|selected| {
            if selected == index {
                self.items.len() - 1
            } else if selected > index {
                selected - 1
            } else {
                selected
            }
        });
        self.record(before);
    }

    pub fn set_all_hidden(&mut self, hidden: bool) {
        let before = self.snapshot();
        self.all_hidden = hidden;
        self.record(before);
    }

    /// Whether every drawing is individually locked (used by the toolbox's
    /// lock-all toggle). An empty collection is not "all locked".
    #[must_use]
    pub fn all_locked(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|item| item.locked)
    }

    /// Reversible bulk protection: locks (or unlocks) every drawing as one
    /// undo entry. Never deletes anything.
    pub fn set_all_locked(&mut self, locked: bool) {
        let before = self.snapshot();
        for item in &mut self.items {
            item.locked = locked;
        }
        self.record(before);
    }

    /// Rigid translation of the selected object. Locked geometry stays put.
    pub fn translate_selected(&mut self, delta_bar: f32, delta_price: f64) {
        let Some(drawing) = self.selected_mut() else {
            return;
        };
        if drawing.locked {
            return;
        }
        for point in &mut drawing.points {
            point.bar += delta_bar;
            point.price += delta_price;
        }
    }

    /// Move one anchor of one object. Locked geometry stays put.
    pub fn move_anchor(
        &mut self,
        drawing_index: usize,
        point_index: usize,
        point: ChartPoint,
    ) -> bool {
        let Some(drawing) = self.items.get_mut(drawing_index) else {
            return false;
        };
        if drawing.locked {
            return false;
        }
        let Some(anchor) = drawing.points.get_mut(point_index) else {
            return false;
        };
        *anchor = point;
        true
    }

    /// Replace every anchor of one object at once — what a tool-owned handle
    /// drag produces, because a handle that moves a rail moves the anchors
    /// that define it together. Locked geometry stays put, and an anchor
    /// count that does not match is refused rather than reshaping the object
    /// into something the tool cannot paint.
    pub fn set_points(&mut self, drawing_index: usize, points: &[ChartPoint]) -> bool {
        let Some(drawing) = self.items.get_mut(drawing_index) else {
            return false;
        };
        if drawing.locked || drawing.points.len() != points.len() {
            return false;
        }
        drawing.points.clear();
        drawing.points.extend_from_slice(points);
        true
    }
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
mod tests {
    use super::*;

    fn tool(id: &str) -> DrawingTool {
        DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == id)
            .expect("registered test tool")
    }

    /// The rectangle a popup must keep clear of is what the tool *paints*,
    /// not where its anchors sit. A fixed-range profile carries two anchors at
    /// one price and covers the axis; placing against the anchors walks around
    /// a thin sliver and lands in the middle of the figure.
    #[test]
    fn a_tool_that_paints_past_its_anchors_says_so() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
        let anchors = egui::Rect::from_min_max(egui::pos2(300.0, 290.0), egui::pos2(500.0, 310.0));

        let profile = tool("fixed-range-profile");
        let painted = profile.painted_bounds(anchors, chart);
        assert_eq!(
            painted.x_range(),
            anchors.x_range(),
            "its time span is its own"
        );
        assert_eq!(
            (painted.top(), painted.bottom()),
            (chart.top(), chart.bottom()),
            "and it covers every price on screen"
        );

        let vertical = tool("vertical-line");
        assert_eq!(
            (
                vertical.painted_bounds(anchors, chart).top(),
                vertical.painted_bounds(anchors, chart).bottom()
            ),
            (chart.top(), chart.bottom()),
        );

        let horizontal = tool("horizontal-line");
        let painted = horizontal.painted_bounds(anchors, chart);
        assert_eq!(
            (painted.left(), painted.right()),
            (chart.left(), chart.right()),
            "edge to edge"
        );

        // Everything else ends where its anchors do, unchanged.
        let trend = tool("trend-line");
        assert_eq!(trend.painted_bounds(anchors, chart), anchors);
        let rect = tool("rectangle");
        assert_eq!(rect.painted_bounds(anchors, chart), anchors);
    }

    /// The vector-icon port's contract: every declared polyline has at
    /// least two points and stays inside the unit square, so the paint
    /// helper never has to clamp. The tools that replaced a lying glyph
    /// must actually declare strokes — an accidental `&[]` would silently
    /// bring the lozenge back.
    #[test]
    fn icon_strokes_have_two_points_each_and_stay_in_the_unit_square() {
        for tool in DRAWING_TOOLS {
            for polyline in tool.icon_strokes() {
                assert!(
                    polyline.len() >= 2,
                    "{}: an icon stroke needs at least two points",
                    tool.id()
                );
                for (x, y) in *polyline {
                    assert!(
                        (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y),
                        "{}: icon point ({x}, {y}) leaves the unit square",
                        tool.id()
                    );
                }
            }
        }
        for id in ["parallel-channel", "fib-retracement", "fib-extension"] {
            assert!(
                !tool(id).icon_strokes().is_empty(),
                "{id} replaced its glyph and must keep vector strokes"
            );
        }
    }

    #[test]
    fn every_registered_tool_has_metadata_and_a_valid_point_count() {
        let mut ids = Vec::with_capacity(DRAWING_TOOLS.len());
        for tool in DRAWING_TOOLS {
            assert!(!tool.id().is_empty());
            assert!(!ids.contains(&tool.id()), "duplicate tool id {}", tool.id());
            ids.push(tool.id());
            assert!(!tool.icon().is_empty());
            assert!(!tool.settings_title().is_empty());
            assert!(!tool.hover_text().is_empty());
            // Zero anchors is legal for exactly one shape of tool: a
            // freehand one, whose count is whatever the gesture gave. Any
            // other tool answering zero would never complete a draft.
            assert_eq!(
                tool.required_points() == 0,
                tool.freehand(),
                "{} must declare an anchor count unless it is freehand",
                tool.id()
            );
            assert!(
                !tool.freehand() || tool.placement_hint(0).is_some(),
                "{} is placed by a gesture nobody has seen before; it has to say so",
                tool.id()
            );
        }
    }

    /// A tool of three or more anchors says what every step wants, in words.
    ///
    /// Two anchors need no words: the drag that starts the object also
    /// finishes it, so the trader is never left holding one. The third anchor
    /// is where they get stranded — the gesture ends, the object sits there,
    /// and the `n/N` badge is on the far side of the screen from the cursor.
    /// A new tool that stays silent mid-placement fails here rather than in
    /// the running app.
    #[test]
    fn a_tool_that_outlasts_its_drag_says_what_each_step_wants() {
        for tool in DRAWING_TOOLS
            .into_iter()
            .filter(|tool| tool.required_points() >= 3)
        {
            for placed in 1..tool.required_points() {
                assert!(
                    tool.placement_hint(placed).is_some(),
                    "{} says nothing with {placed} anchors down",
                    tool.id()
                );
            }
        }
    }

    /// The shaping port is opt-in: it changed nothing for a tool that did not
    /// ask for it. Only the two whose third anchor gives a shape its
    /// thickness — and which are therefore the two that can collapse into a
    /// line — move the pointer at all.
    #[test]
    fn the_shaping_port_defaults_to_the_pointer_the_trader_is_holding() {
        let placed = [egui::pos2(10.0, 10.0), egui::pos2(110.0, 60.0)];
        // Exactly on the line between the two anchors: the collapsed case,
        // where a tool that shapes has to move and one that does not must not.
        let cursor = egui::pos2(60.0, 35.0);
        let mut shaping: Vec<&'static str> = DRAWING_TOOLS
            .into_iter()
            .filter(|tool| tool.pending_anchor(&placed, cursor, Constrain::Free) != cursor)
            .map(DrawingTool::id)
            .collect();
        shaping.sort_unstable();
        assert_eq!(shaping, vec!["parallel-channel", "triangle"]);
    }

    /// Which tools Shift means something to, named exactly. Everything else
    /// answers with the pointer, so a trader holding the modifier out of
    /// habit on some other tool gets no surprise — and a tool that *should*
    /// answer to it fails here until it is listed.
    #[test]
    fn shift_reaches_exactly_the_tools_with_an_angle_to_hold() {
        // One anchor down: the far end of a line, which is the step the
        // modifier is about for every tool that has one.
        let placed = [egui::pos2(10.0, 10.0)];
        let cursor = egui::pos2(110.0, 60.0);
        let mut levelled: Vec<&'static str> = DRAWING_TOOLS
            .into_iter()
            .filter(|tool| tool.pending_anchor(&placed, cursor, Constrain::Level) != cursor)
            .map(DrawingTool::id)
            .collect();
        levelled.sort_unstable();
        assert_eq!(levelled, vec!["parallel-channel", "trend-line"]);
    }

    /// The host skips the shaping port for a freehand draft, and the reason
    /// is runtime: a pencil stroke holds hundreds of anchors, and projecting
    /// every one of them up to three times a frame just to consult the port
    /// would cost far more than the gesture is worth — see
    /// `ChartPane::shaped_placement`.
    ///
    /// That skip is only sound while no freehand tool actually wants its
    /// anchors shaped; otherwise the host would be ignoring a tool that
    /// asked, in silence. This is the test that holds the two facts together,
    /// so a pencil that one day wants shaping fails here instead of being
    /// quietly disobeyed.
    #[test]
    fn no_freehand_tool_asks_to_shape_its_anchors() {
        let placed = [egui::pos2(10.0, 10.0), egui::pos2(110.0, 60.0)];
        let cursor = egui::pos2(60.0, 35.0);
        for tool in DRAWING_TOOLS.into_iter().filter(|tool| tool.freehand()) {
            assert_eq!(
                tool.pending_anchor(&placed, cursor, Constrain::Free),
                cursor,
                "{} is freehand, and the host does not consult the port for one",
                tool.id()
            );
        }
    }

    /// The shared half of the port: push off the line, keep the position
    /// along it.
    #[test]
    fn off_line_by_moves_across_the_line_and_never_along_it() {
        let start = egui::pos2(0.0, 0.0);
        let end = egui::pos2(100.0, 0.0);
        let pushed = off_line_by(start, end, egui::pos2(40.0, 1.0), 10.0);
        assert!(
            (pushed.x - 40.0).abs() < 1e-3,
            "kept its place along the line"
        );
        assert!((pushed.y - 10.0).abs() < 1e-3, "stands the floor off it");
        let clear = egui::pos2(40.0, -25.0);
        assert_eq!(
            off_line_by(start, end, clear, 10.0),
            clear,
            "a point already clear is untouched"
        );
    }

    /// A "line" whose ends are one point has no direction of its own. It gets
    /// the vertical, which is the axis a chart measures in — the alternative
    /// is a NaN anchor, which is an object that can never be drawn or deleted.
    #[test]
    fn a_line_of_no_length_still_pushes_off_itself() {
        let point = egui::pos2(50.0, 50.0);
        let pushed = off_line_by(point, point, point, 10.0);
        assert!(pushed.is_finite());
        assert!((pushed.y - 60.0).abs() < 1e-3, "pushed along the vertical");
    }

    /// A price-only tool started over an indicator band still lands on the
    /// candles' price axis: profile rows are prices, and a profile hanging
    /// off a CVD axis would read its rows as CVD values.
    #[test]
    fn a_price_only_tool_lands_on_the_price_band_wherever_it_started() {
        let mut drawings = Drawings::default();
        let frvp = tool("fixed-range-profile");
        let band = DrawingBand::Indicator(PaneKey {
            kind: "native.cvd".into(),
            ordinal: 0,
        });
        drawings.place_on(frvp, &band, ChartPoint::at(1.0, 5.0));
        drawings.place_on(frvp, &band, ChartPoint::at(4.0, 9.0));
        assert_eq!(drawings.items()[0].band, DrawingBand::Price);
    }

    #[test]
    fn horizontal_line_completes_with_one_point() {
        let mut drawings = Drawings::default();
        assert!(drawings.place(tool("horizontal-line"), ChartPoint::at(3.0, 100.0)));
    }

    #[test]
    fn channel_needs_three_points() {
        let mut drawings = Drawings::default();
        let channel = tool("parallel-channel");
        for bar in [1.0, 2.0] {
            assert!(!drawings.place(channel, ChartPoint::at(bar, 1.0)));
        }
        assert!(drawings.place(channel, ChartPoint::at(3.0, 1.0)));
    }

    #[test]
    fn moving_a_selected_drawing_preserves_its_shape() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
        drawings.place(rectangle, ChartPoint::at(3.0, 110.0));
        drawings.translate_selected(2.0, -5.0);
        assert_eq!(
            drawings.items()[0].points,
            [ChartPoint::at(3.0, 95.0), ChartPoint::at(5.0, 105.0)]
        );
    }

    #[test]
    fn moving_one_anchor_does_not_move_the_other_points() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
        drawings.place(rectangle, ChartPoint::at(3.0, 110.0));
        let opposite = drawings.items()[0].points[1];
        let replacement = ChartPoint::at(0.5, 95.0);

        assert!(drawings.move_anchor(0, 0, replacement));

        assert_eq!(drawings.items()[0].points[0], replacement);
        assert_eq!(drawings.items()[0].points[1], opposite);
        assert!(!drawings.move_anchor(5, 0, replacement));
        assert!(!drawings.move_anchor(0, 5, replacement));
    }

    #[test]
    fn deleting_the_selected_drawing_clears_both_object_and_selection() {
        let mut drawings = Drawings::default();
        let line = tool("horizontal-line");
        assert!(drawings.place(line, ChartPoint::at(1.0, 100.0)));
        assert_eq!(drawings.selected(), Some(0));

        assert_eq!(drawings.delete_selected(false), DeleteOutcome::Deleted);

        assert!(drawings.items().is_empty());
        assert_eq!(drawings.selected(), None);
    }

    /// The id is identity where the index is only position: reorders and
    /// deletes move objects around the `Vec`, and the id keeps naming the
    /// same object through all of it.
    #[test]
    fn ids_survive_reorder_and_delete_where_indices_do_not() {
        let mut drawings = Drawings::default();
        let line = tool("horizontal-line");
        for price in [100.0, 110.0, 120.0] {
            drawings.place(line, ChartPoint::at(1.0, price));
        }
        let second = drawings.items()[1].id;
        assert_eq!(drawings.index_of(second), Some(1));

        drawings.bring_to_front(0);
        drawings.select(Some(drawings.index_of(drawings.items()[0].id).unwrap()));
        assert_eq!(
            drawings.index_of(second),
            Some(0),
            "the reorder moved it; the id still finds it"
        );

        drawings.select(drawings.index_of(second));
        drawings.delete_selected(false);
        assert_eq!(
            drawings.index_of(second),
            None,
            "deleted is gone, not renumbered"
        );
    }

    #[test]
    fn an_undone_delete_restores_the_object_under_its_old_id() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
        let id = drawings.items()[0].id;
        drawings.delete_selected(false);
        assert_eq!(drawings.index_of(id), None);
        drawings.undo();
        assert_eq!(
            drawings.index_of(id),
            Some(0),
            "an undo restores identity, not a lookalike"
        );

        // And a fresh object after the undo still gets an id never seen
        // before — the counter does not rewind with the history.
        drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 105.0));
        assert!(drawings.items()[1].id > id);
    }

    #[test]
    fn a_duplicate_is_a_new_object_with_no_name() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
        drawings.rename_at(0, "congestão 108k");
        drawings.duplicate_selected(2.0);
        let [original, copy] = drawings.items() else {
            panic!("one original and one copy");
        };
        assert_ne!(original.id, copy.id);
        assert_eq!(original.name.as_deref(), Some("congestão 108k"));
        assert_eq!(copy.name, None);
    }

    #[test]
    fn rename_is_one_undo_step_and_whitespace_clears_it() {
        let mut drawings = Drawings::default();
        let line = tool("horizontal-line");
        drawings.place(line, ChartPoint::at(1.0, 100.0));
        assert_eq!(
            drawings.items()[0].display_label(0),
            format!("{} 1", line.name()),
            "no name falls back to the derived label"
        );

        drawings.rename_at(0, "  zona de venda  ");
        assert_eq!(drawings.items()[0].name.as_deref(), Some("zona de venda"));
        assert_eq!(drawings.items()[0].display_label(0), "zona de venda");

        drawings.rename_at(0, "   ");
        assert_eq!(drawings.items()[0].name, None, "whitespace clears the name");

        drawings.undo();
        assert_eq!(drawings.items()[0].name.as_deref(), Some("zona de venda"));
        drawings.undo();
        assert_eq!(drawings.items()[0].name, None);
    }

    #[test]
    fn a_locked_drawing_ignores_geometry_edits_until_unlocked() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 100.0));
        drawings.set_selected_locked(true);

        drawings.translate_selected(3.0, 5.0);
        assert!(!drawings.move_anchor(0, 0, ChartPoint::at(9.0, 50.0)));
        assert_eq!(
            drawings.items()[0].points[0],
            ChartPoint::at(2.0, 100.0),
            "locked geometry must not move"
        );

        drawings.set_selected_locked(false);
        drawings.translate_selected(3.0, 5.0);
        assert_eq!(drawings.items()[0].points[0], ChartPoint::at(5.0, 105.0));
    }

    #[test]
    fn deleting_a_locked_drawing_requires_explicit_force() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
        drawings.set_selected_locked(true);

        assert_eq!(
            drawings.delete_selected(false),
            DeleteOutcome::NeedsConfirmation
        );
        assert_eq!(drawings.items().len(), 1, "unforced delete must not land");

        assert_eq!(drawings.delete_selected(true), DeleteOutcome::Deleted);
        assert!(drawings.items().is_empty());

        assert!(drawings.undo(), "a forced delete is still undoable");
        assert_eq!(drawings.items().len(), 1);
        assert!(drawings.items()[0].locked, "undo restores the lock too");
    }

    /// Placing a drawing releases hide-all (audit M8): the finished object
    /// must land on screen, never silently invisible behind the rail's eye —
    /// and undo restores the hidden state along with the store.
    #[test]
    fn placing_a_drawing_releases_hide_all() {
        let mut drawings = Drawings::default();
        drawings.set_all_hidden(true);
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
        assert!(!drawings.all_hidden(), "the new mark is visible");
        assert!(drawings.undo(), "placement is one undoable step");
        assert!(drawings.items().is_empty());
        assert!(
            drawings.all_hidden(),
            "undoing the placement restores hide-all too"
        );
    }

    /// Delete-all is one command, one undo entry, locked objects included —
    /// the caller's count-bearing confirmation is the gate, not the lock.
    #[test]
    fn delete_all_takes_everything_in_one_undoable_step() {
        let mut drawings = Drawings::default();
        assert_eq!(drawings.delete_all(), 0, "an empty store deletes nothing");
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
        drawings.set_selected_locked(true);
        drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 105.0));
        let undo_before = drawings.undo_depth();

        assert_eq!(drawings.delete_all(), 2, "locked and unlocked both go");
        assert!(drawings.items().is_empty());
        assert_eq!(
            drawings.undo_depth(),
            undo_before + 1,
            "one command, one entry"
        );
        assert!(drawings.undo(), "and it comes back whole");
        assert_eq!(drawings.items().len(), 2);
        assert!(
            drawings.items().iter().any(|drawing| drawing.locked),
            "the lock survives the round trip"
        );
    }

    #[test]
    fn creating_dragging_and_deleting_are_one_undo_entry_each() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        // Creation: two anchors, one entry.
        drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
        drawings.place(rectangle, ChartPoint::at(3.0, 110.0));
        assert_eq!(drawings.undo_depth(), 1);

        // One drag over many frames: one entry.
        drawings.begin_gesture();
        for _ in 0..5 {
            drawings.translate_selected(0.5, 1.0);
        }
        drawings.commit_gesture();
        assert_eq!(drawings.undo_depth(), 2);

        // A gesture that changes nothing records nothing.
        drawings.begin_gesture();
        drawings.commit_gesture();
        assert_eq!(drawings.undo_depth(), 2);

        drawings.delete_selected(false);
        assert_eq!(drawings.undo_depth(), 3);

        assert!(drawings.undo(), "undo the delete");
        assert_eq!(drawings.items().len(), 1);
        assert!(drawings.undo(), "undo the drag");
        assert_eq!(drawings.items()[0].points[0], ChartPoint::at(1.0, 100.0));
        assert!(drawings.undo(), "undo the creation");
        assert!(drawings.items().is_empty());
        assert!(!drawings.undo(), "history is exhausted");
    }

    #[test]
    fn redo_replays_an_undone_edit_until_a_new_command_clears_it() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0));
        drawings.undo();
        assert!(drawings.items().is_empty());
        assert!(drawings.redo());
        assert_eq!(drawings.items().len(), 1);

        drawings.undo();
        drawings.place(tool("horizontal-line"), ChartPoint::at(4.0, 90.0));
        assert!(!drawings.redo(), "a new command clears the redo stack");
    }

    #[test]
    fn hide_all_is_a_layer_over_each_drawings_own_eye() {
        let mut drawings = Drawings::default();
        for price in [100.0, 105.0] {
            drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, price));
        }
        drawings.select(Some(0));
        drawings.set_selected_hidden(true);
        assert!(!drawings.is_visible(0));
        assert!(drawings.is_visible(1));

        drawings.set_all_hidden(true);
        assert!(!drawings.is_visible(0));
        assert!(!drawings.is_visible(1));

        drawings.set_all_hidden(false);
        assert!(
            !drawings.is_visible(0),
            "show-all must preserve the individual eye"
        );
        assert!(drawings.is_visible(1));

        drawings.select(Some(0));
        drawings.set_selected_hidden(false);
        assert!(drawings.is_visible(0));
    }

    #[test]
    fn duplicate_lands_offset_unlocked_and_selected_as_one_entry() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(4.0, 100.0));
        drawings.set_selected_locked(true);
        let depth = drawings.undo_depth();

        drawings.duplicate_selected(2.0);

        assert_eq!(drawings.items().len(), 2);
        assert_eq!(drawings.selected(), Some(1), "the copy becomes selected");
        assert_eq!(drawings.items()[1].points[0].bar, 6.0, "the copy is offset");
        assert!(
            !drawings.items()[1].locked,
            "a copy starts unlocked even when the source was locked"
        );
        assert_eq!(drawings.undo_depth(), depth + 1);
        drawings.undo();
        assert_eq!(drawings.items().len(), 1);
    }

    #[test]
    fn backspace_steps_back_one_draft_anchor_at_a_time() {
        let mut drawings = Drawings::default();
        let channel = tool("parallel-channel");
        for bar in [1.0, 2.0] {
            drawings.place(channel, ChartPoint::at(bar, 1.0));
        }
        assert_eq!(drawings.draft_len(), 2);

        drawings.remove_last_draft_anchor();
        assert_eq!(drawings.draft_len(), 1);
        drawings.remove_last_draft_anchor();
        assert!(
            drawings.draft().is_none(),
            "dropping the only anchor cancels the draft"
        );
    }

    #[test]
    fn rectangle_interior_hit_tests_only_while_the_fill_is_visible() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let rectangle = tool("rectangle");
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        let payload = rectangle.default_payload();
        let points = [egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)];
        let anchors = anchors_for(&points, &scale);
        let center = egui::pos2(150.0, 150.0);
        let border = egui::pos2(100.0, 150.0);

        let filled = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: false,
            halo: false,
        };
        assert!(rectangle.hit_test(chart, &points, center, 5.0, &filled));

        let outline_only = DrawContext {
            style: DrawingStyle {
                fill_alpha: 0,
                ..DrawingStyle::default()
            },
            ..filled
        };
        assert!(
            !rectangle.hit_test(chart, &points, center, 5.0, &outline_only),
            "with no visible fill the interior belongs to the chart"
        );
        assert!(
            rectangle.hit_test(chart, &points, border, 5.0, &outline_only),
            "the border stays selectable without a fill"
        );
    }

    #[test]
    fn lock_all_is_one_reversible_undo_entry_and_never_deletes() {
        let mut drawings = Drawings::default();
        for price in [100.0, 105.0] {
            drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, price));
        }
        let depth_before = drawings.undo_depth();

        drawings.set_all_locked(true);
        assert!(drawings.all_locked());
        assert_eq!(drawings.items().len(), 2);
        assert_eq!(drawings.undo_depth(), depth_before + 1);

        assert!(drawings.undo());
        assert!(!drawings.all_locked(), "undo releases the bulk lock");
        assert_eq!(drawings.items().len(), 2);
    }

    #[test]
    fn undo_snapshots_shift_with_prepended_history() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(2.0, 100.0));
        drawings.begin_gesture();
        drawings.translate_selected(1.0, 0.0);
        drawings.commit_gesture();

        drawings.shift_bars(3);
        assert_eq!(drawings.items()[0].points[0].bar, 6.0);

        drawings.undo();
        assert_eq!(
            drawings.items()[0].points[0].bar,
            5.0,
            "the undone position must sit on the shifted bars, not the stale ones"
        );
    }

    #[test]
    fn selection_preserves_the_drawing_color_and_adds_white_handles() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let color = egui::Color32::from_rgb(0xFF, 0x9F, 0x43);
        let style = DrawingStyle {
            color,
            ..DrawingStyle::default()
        };
        let line = tool("horizontal-line");
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(chart),
            ..Default::default()
        };
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        let payload = line.default_payload();
        let anchors = [ChartPoint::at(0.0, scale.price_at(120.0))];
        let output = ctx.run(input, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("selection-test"),
            ));
            let ctxt = DrawContext {
                payload: payload.as_ref(),
                anchors: &anchors,
                scale: &scale,
                px_per_bar: 20.0,
                unit: ValueUnit::Price,
                primary_band: true,
                style,
                selected: true,
                halo: false,
            };
            line.paint(
                &painter,
                chart,
                style,
                &[egui::pos2(100.0, 120.0)],
                &ctxt,
                true,
            );
        });

        let mut kept_color = false;
        let mut ring_handle = false;
        for clipped in &output.shapes {
            match &clipped.shape {
                egui::Shape::LineSegment { stroke, .. } => {
                    kept_color |= stroke.color == egui::epaint::ColorMode::Solid(color);
                }
                egui::Shape::Circle(circle) => {
                    ring_handle |= circle.fill == SELECTED_ANCHOR_FILL;
                }
                _ => {}
            }
        }
        assert!(
            kept_color,
            "selection must keep painting the configured colour"
        );
        assert!(ring_handle, "selection must add ring anchor handles");
    }

    /// Nothing that exists changes: every object still opens on the chart it
    /// was drawn on, and sharing is something the trader asks for.
    #[test]
    fn a_new_drawing_belongs_to_the_chart_it_was_drawn_on() {
        let mut drawings = Drawings::default();
        assert!(drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0)));
        assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
        assert!(!drawings.items()[0].shared());
    }

    /// Market time is the only coordinate two panes agree on, so an anchor
    /// without one cannot be re-expressed — and none is invented for it.
    #[test]
    fn an_anchor_without_a_market_time_cannot_be_shared() {
        let mut untimed = Drawings::default();
        assert!(untimed.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0)));
        assert!(
            !untimed.items()[0].shareable(),
            "an anchor past the newest bar has no instant behind it"
        );

        let mut timed = Drawings::default();
        assert!(timed.place(
            tool("horizontal-line"),
            ChartPoint::at_time(1.0, 100.0, Some(1_700_000_000_000))
        ));
        assert!(timed.items()[0].shareable());
    }

    /// A shared object that is switched off shows nowhere, including on the
    /// other pane: one eye, one answer.
    #[test]
    fn hiding_a_shared_drawing_hides_it_everywhere() {
        let mut drawings = Drawings::default();
        assert!(drawings.place(
            tool("horizontal-line"),
            ChartPoint::at_time(1.0, 100.0, Some(1_700_000_000_000))
        ));
        drawings.selected_mut().expect("just placed").scope = DrawingScope::AllCharts;
        assert!(drawings.items()[0].shared());
        drawings.set_hidden_at(0, true);
        assert!(!drawings.items()[0].shared());
    }

    /// A multi-anchor object shares only when *every* anchor can be placed:
    /// half a channel on the other chart would be a lie about where it is.
    #[test]
    fn one_untimed_anchor_blocks_sharing_the_whole_object() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        assert!(!drawings.place(
            rectangle,
            ChartPoint::at_time(1.0, 100.0, Some(1_700_000_000_000))
        ));
        assert!(drawings.place(rectangle, ChartPoint::at(4.0, 110.0)));
        assert!(!drawings.items()[0].shareable());
    }

    /// A re-cut used to throw the whole store away, draft included. Nothing is
    /// thrown away now, and a half-placed object is no exception: its first
    /// anchor moves onto the new bars like any other, so the trader finishes
    /// the shape they started rather than starting over.
    #[test]
    fn reanchoring_carries_an_unfinished_draft_too() {
        let mut drawings = Drawings::default();
        assert!(!drawings.place(
            tool("rectangle"),
            ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000))
        ));

        drawings.reanchor(10, 5, halved);

        let draft = drawings.draft().expect("the draft survives the re-cut");
        assert_eq!(draft.points[0].bar, 3.0);
        assert_eq!(draft.points[0].time_ms, Some(1_700_000_006_000));
    }

    #[test]
    fn prepending_history_shifts_completed_and_draft_bar_anchors() {
        let mut drawings = Drawings::default();
        drawings.place(tool("horizontal-line"), ChartPoint::at(2.5, 100.0));
        drawings.place(tool("rectangle"), ChartPoint::at(4.0, 101.0));

        drawings.shift_bars(3);

        assert_eq!(drawings.items()[0].points[0].bar, 5.5);
        assert_eq!(
            drawings.draft().expect("rectangle draft").points[0].bar,
            7.0
        );
    }

    /// A series re-cut so that every instant lands on half its old slot —
    /// what doubling a timeframe does to the bars under a drawing.
    fn halved(time: i64) -> Option<f32> {
        Some((time - 1_700_000_000_000) as f32 / 2_000.0)
    }

    #[test]
    fn reanchoring_puts_an_anchor_on_the_slot_its_timestamp_now_lands_on() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000)),
        );

        drawings.reanchor(10, 5, halved);

        assert_eq!(drawings.items()[0].points[0].bar, 3.0);
        assert_eq!(drawings.items()[0].points[0].price, 100.0);
        assert!(!drawings.items()[0].off_series);
    }

    #[test]
    fn reanchoring_keeps_an_undated_anchor_past_the_end_of_the_new_series() {
        let mut drawings = Drawings::default();
        // Two bars past the newest of a ten-bar series, where the tape has
        // written nothing and there is no instant to look up.
        drawings.place(tool("horizontal-line"), ChartPoint::at(12.0, 100.0));

        drawings.reanchor(10, 5, halved);

        // Still two bars past the newest, now that the newest is bar 5.
        assert_eq!(drawings.items()[0].points[0].bar, 7.0);
    }

    #[test]
    fn reanchoring_flags_an_anchor_the_new_series_cannot_reach() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000)),
        );

        // A symbol switch: the new instrument's series does not cover the
        // instant this mark was drawn at.
        drawings.reanchor(10, 5, |_| None);

        assert!(drawings.items()[0].off_series, "the mark must say so");
        assert_eq!(
            drawings.items()[0].points[0].time_ms,
            Some(1_700_000_006_000),
            "the instant it was placed at is never rewritten"
        );
    }

    #[test]
    fn reanchoring_travels_with_the_undo_history() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint::at_time(6.0, 100.0, Some(1_700_000_006_000)),
        );
        drawings.begin_gesture();
        drawings.translate_selected(4.0, 0.0);
        drawings.commit_gesture();

        drawings.reanchor(10, 5, halved);
        drawings.undo();

        // Undo must not resurrect a coordinate from the series that is gone.
        assert_eq!(drawings.items()[0].points[0].bar, 3.0);
    }

    #[test]
    fn setting_times_rewrites_the_instants_behind_the_anchors() {
        let mut drawings = Drawings::default();
        let rectangle = tool("rectangle");
        drawings.place(
            rectangle,
            ChartPoint::at_time(1.0, 100.0, Some(1_700_000_001_000)),
        );
        drawings.place(
            rectangle,
            ChartPoint::at_time(3.0, 110.0, Some(1_700_000_003_000)),
        );

        drawings.translate_selected(2.0, 0.0);
        drawings.set_times(0, &[Some(1_700_000_003_000), Some(1_700_000_005_000)]);

        let times: Vec<_> = drawings.items()[0]
            .points
            .iter()
            .map(|point| point.time_ms)
            .collect();
        assert_eq!(
            times,
            [Some(1_700_000_003_000), Some(1_700_000_005_000)],
            "a moved mark carries its new instants, or its shared twin stays behind"
        );
    }

    /// The honesty hole a symbol switch opens, and the one `off_series`
    /// cannot close: both instruments traded at the same instants, so every
    /// anchor resolves onto a real bar and only the *price* is meaningless.
    /// A mark left painting at full strength there reads as a level on a
    /// market it was never drawn on.
    #[test]
    fn a_market_change_marks_every_object_as_belonging_to_the_old_one() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint::at_time(6.0, 118_000.0, Some(1_700_000_006_000)),
        );

        drawings.mark_market_changed();

        assert!(drawings.items()[0].foreign_market);
        assert!(
            !drawings.items()[0].off_series,
            "the instants are still on this series - which is exactly why the \
             time-based flag cannot catch this case"
        );

        // Anything drawn after the switch is on the market now showing.
        drawings.place(
            tool("horizontal-line"),
            ChartPoint::at_time(7.0, 138_000.0, Some(1_700_000_007_000)),
        );
        assert!(!drawings.items()[1].foreign_market);
    }

    #[test]
    fn undo_cannot_restore_a_mark_that_claims_the_new_market() {
        let mut drawings = Drawings::default();
        drawings.place(
            tool("horizontal-line"),
            ChartPoint::at_time(6.0, 118_000.0, Some(1_700_000_006_000)),
        );
        drawings.begin_gesture();
        drawings.translate_selected(1.0, 0.0);
        drawings.commit_gesture();

        drawings.mark_market_changed();
        drawings.undo();

        assert!(
            drawings.items()[0].foreign_market,
            "stepping back through history must not undo the market change"
        );
    }

    /// Chart anchors consistent with `points` under `scale` — the identity
    /// projection the tool tests run with.
    fn anchors_for(points: &[egui::Pos2], scale: &PriceScale) -> Vec<ChartPoint> {
        points
            .iter()
            .enumerate()
            .map(|(index, point)| ChartPoint::at(index as f32, scale.price_at(point.y)))
            .collect()
    }

    #[test]
    fn horizontal_line_is_selectable_from_anywhere_on_its_stroke() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let line = tool("horizontal-line");
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        let payload = line.default_payload();
        let points = [egui::pos2(100.0, 120.0)];
        let anchors = anchors_for(&points, &scale);
        let ctxt = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: false,
            halo: false,
        };
        assert!(line.hit_test(chart, &points, egui::pos2(450.0, 123.0), 5.0, &ctxt));
    }

    /// The handle port with a second implementation on it — one implementer
    /// never proves a trait is a port. This fake's handle is nowhere near its
    /// anchor, so every host path that must go through the tool shows up:
    /// what gets painted, what can be grabbed, and what a grab does.
    #[test]
    fn a_tool_may_own_its_handles_and_the_host_paints_hits_and_drags_them() {
        /// One anchor, one handle floating a fixed distance above it.
        struct PivotTool;
        const PIVOT_HANDLE_OFFSET_PX: f32 = 20.0;
        impl DrawingToolImpl for PivotTool {
            fn id(&self) -> &'static str {
                "pivot"
            }
            fn name(&self) -> &'static str {
                "Pivot"
            }
            fn settings_title(&self) -> &'static str {
                "Pivot settings"
            }
            fn icon(&self) -> &'static str {
                "P"
            }
            fn hover_text(&self) -> &'static str {
                "A fake tool whose handle is not its anchor"
            }
            fn required_points(&self) -> usize {
                1
            }
            fn paint(
                &self,
                _painter: &egui::Painter,
                _chart_rect: egui::Rect,
                _style: DrawingStyle,
                _points: &[egui::Pos2],
                _ctxt: &DrawContext<'_>,
            ) {
            }
            fn hit_test(
                &self,
                _chart_rect: egui::Rect,
                _points: &[egui::Pos2],
                _position: egui::Pos2,
                _radius_px: f32,
                _ctxt: &DrawContext<'_>,
            ) -> bool {
                false
            }
            fn handles(
                &self,
                _chart_rect: egui::Rect,
                points: &[egui::Pos2],
                _ctxt: &DrawContext<'_>,
            ) -> Option<Handles> {
                Some(Handles::from_slice(&[
                    points[0] - egui::vec2(0.0, PIVOT_HANDLE_OFFSET_PX)
                ]))
            }
            fn drag_handle(
                &self,
                _chart_rect: egui::Rect,
                _points: &[egui::Pos2],
                _handle: usize,
                to: egui::Pos2,
                _ctxt: &DrawContext<'_>,
                _constrain: Constrain,
            ) -> Option<Handles> {
                Some(Handles::from_slice(&[
                    to + egui::vec2(0.0, PIVOT_HANDLE_OFFSET_PX)
                ]))
            }
            #[cfg(test)]
            fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
                (vec![egui::pos2(50.0, 50.0)], egui::pos2(50.0, 30.0))
            }
        }
        static PIVOT: PivotTool = PivotTool;
        let tool = DrawingTool(&PIVOT);

        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 200.0));
        let scale = PriceScale::from_range(0.0, 200.0, 0.0, 200.0);
        let points = vec![egui::pos2(50.0, 50.0)];
        let anchors = anchors_for(&points, &scale);
        let payload = tool.default_payload();
        let ctxt = DrawContext {
            payload: payload.as_ref(),
            anchors: &anchors,
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style: DrawingStyle::default(),
            selected: true,
            halo: false,
        };
        let handle = egui::pos2(50.0, 30.0);
        assert_eq!(tool.handles(chart, &points, &ctxt).as_slice(), &[handle]);
        assert!(
            tool.hit_test(chart, &points, handle, 4.0, &ctxt),
            "the declared handle is what the trader grabs"
        );
        assert!(
            !tool.hit_test(chart, &points, points[0], 4.0, &ctxt),
            "the anchor is not a grab point unless the tool says so"
        );
        assert_eq!(
            tool.drag_handle(
                chart,
                &points,
                0,
                egui::pos2(80.0, 10.0),
                &ctxt,
                Constrain::Free
            )
            .expect("the tool owns the drag")
            .as_slice(),
            &[egui::pos2(80.0, 30.0)],
            "the tool decides which anchors a handle moves"
        );

        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(chart),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("pivot-handles"),
            ));
            tool.paint(
                &painter,
                chart,
                DrawingStyle::default(),
                &points,
                &ctxt,
                true,
            );
        });
        let rings: Vec<egui::Pos2> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle) if circle.fill == SELECTED_ANCHOR_FILL => {
                    Some(circle.center)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            rings,
            vec![handle],
            "the ring the trader sees is the point they can grab, not the anchor"
        );
    }

    /// The style-default port is additive: a tool that declares nothing is
    /// born in the stock look, and the one that declares (the anchored VWAP,
    /// a series) is born in its own — colour, weight and fill together.
    #[test]
    fn style_defaults_are_per_tool_and_additive() {
        let stock = DrawingStyle::default();
        let mut declaring = 0;
        for tool in DRAWING_TOOLS {
            let style = tool.default_style();
            if tool.id() == "anchored-vwap" {
                assert_eq!(style.color, crate::theme::DRAW_CYAN);
                assert!(style.width_px > stock.width_px, "a series outweighs a note");
                assert!(style.fill_alpha > stock.fill_alpha);
                declaring += 1;
            } else {
                assert_eq!(
                    (style.width_px, style.fill_alpha),
                    (stock.width_px, stock.fill_alpha),
                    "{} must keep the stock weight and fill",
                    tool.id()
                );
            }
        }
        assert_eq!(declaring, 1, "exactly one tool declares today");
    }

    /// The context-menu port is additive too: exactly the tools that declare
    /// a label appear in the pane's right-click sweep.
    #[test]
    fn the_context_menu_port_is_declared_not_hardcoded() {
        let labelled: Vec<&str> = DRAWING_TOOLS
            .into_iter()
            .filter_map(|tool| tool.context_menu_label())
            .collect();
        assert_eq!(labelled, vec!["Anchor VWAP here"]);
    }

    /// The handle port is additive: a tool that does not declare handles is
    /// grabbed by its raw anchors, exactly as before the port existed. Only
    /// the channel opts out, and it opts out on purpose.
    #[test]
    fn a_tool_that_declares_no_handles_is_still_grabbed_by_its_anchors() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        for drawing_tool in DRAWING_TOOLS {
            let (points, _) = drawing_tool.test_geometry();
            let payload = drawing_tool.default_payload();
            let anchors = anchors_for(&points, &scale);
            let ctxt = DrawContext {
                payload: payload.as_ref(),
                anchors: &anchors,
                scale: &scale,
                px_per_bar: 20.0,
                unit: ValueUnit::Price,
                primary_band: true,
                style: DrawingStyle::default(),
                selected: true,
                halo: false,
            };
            let handles = drawing_tool.handles(chart, &points, &ctxt);
            if drawing_tool.id() == "parallel-channel" {
                assert_eq!(
                    handles.len(),
                    6,
                    "the channel adds a corner and a centre per rail"
                );
                continue;
            }
            if drawing_tool.id() == "fixed-range-profile" {
                // The profile opts out too: its anchors' prices are not data
                // (the extent comes from the profile), so the grab points
                // ride the drawn object — anchor x's, visible-extent middle.
                assert_eq!(handles.len(), 2);
                assert_eq!(
                    handles.iter().map(|handle| handle.x).collect::<Vec<_>>(),
                    points.iter().map(|point| point.x).collect::<Vec<_>>(),
                    "profile handles keep their anchors' x"
                );
                continue;
            }
            if drawing_tool.id() == "brush" {
                // The one tool that answers "none", on purpose: a ring on
                // every captured point is a cloud nobody can aim at, over a
                // shape whose individual points mean nothing.
                assert!(
                    handles.is_empty(),
                    "a scribble is moved whole, never point by point"
                );
                continue;
            }
            assert_eq!(
                handles.as_slice(),
                points.as_slice(),
                "{} must keep being grabbed by its anchors",
                drawing_tool.id()
            );
            assert!(
                drawing_tool
                    .drag_handle(
                        chart,
                        &points,
                        0,
                        egui::pos2(1.0, 1.0),
                        &ctxt,
                        Constrain::Free
                    )
                    .is_none(),
                "{} leaves the drag to the host",
                drawing_tool.id()
            );
        }
    }

    #[test]
    fn every_registered_tool_paints_and_hits_its_finished_geometry() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(500.0, 300.0));
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        for drawing_tool in DRAWING_TOOLS {
            let (points, hit) = drawing_tool.test_geometry();
            if drawing_tool.freehand() {
                // A captured path has no declared length; two points is the
                // shortest thing that is still a stroke.
                assert!(points.len() >= 2, "{} needs a path", drawing_tool.id());
            } else {
                assert_eq!(points.len(), drawing_tool.required_points());
            }
            let payload = drawing_tool.default_payload();
            let anchors = anchors_for(&points, &scale);
            let ctxt = DrawContext {
                payload: payload.as_ref(),
                anchors: &anchors,
                scale: &scale,
                px_per_bar: 20.0,
                unit: ValueUnit::Price,
                primary_band: true,
                style: DrawingStyle::default(),
                selected: false,
                halo: false,
            };
            assert!(
                drawing_tool.hit_test(chart, &points, hit, 5.0, &ctxt),
                "{} cannot be selected from its visible geometry",
                drawing_tool.id()
            );

            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(chart),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new(drawing_tool.id()),
                ));
                drawing_tool.paint(
                    &painter,
                    chart,
                    DrawingStyle::default(),
                    &points,
                    &ctxt,
                    false,
                );
            });
            assert!(
                !output.shapes.is_empty(),
                "{} rendered no geometry",
                drawing_tool.id()
            );
        }
    }

    /// The extension port, proven end to end: a fake tool with a property no
    /// other tool has (`ray_count`) docks through the registry surface alone.
    /// Its payload rides the envelope, survives undo snapshots and renders
    /// its own inspector tab — with zero edits to the shared model or the
    /// central inspector code.
    #[test]
    fn a_fake_tool_with_its_own_payload_docks_without_touching_the_model() {
        #[derive(Debug, Clone, PartialEq)]
        struct RayPayload {
            ray_count: u32,
        }
        impl DrawingPayload for RayPayload {
            fn clone_box(&self) -> Box<dyn DrawingPayload> {
                Box::new(self.clone())
            }
            fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
                other
                    .as_any()
                    .downcast_ref::<Self>()
                    .is_some_and(|other| self == other)
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
        struct RayTool;
        impl DrawingToolImpl for RayTool {
            fn id(&self) -> &'static str {
                "test-ray"
            }
            fn name(&self) -> &'static str {
                "Ray fan"
            }
            fn settings_title(&self) -> &'static str {
                "Ray fan settings"
            }
            fn icon(&self) -> &'static str {
                "R"
            }
            fn hover_text(&self) -> &'static str {
                "Ray fan - test tool"
            }
            fn required_points(&self) -> usize {
                1
            }
            fn default_payload(&self) -> Box<dyn DrawingPayload> {
                Box::new(RayPayload { ray_count: 2 })
            }
            fn extra_tab(&self) -> Option<&'static str> {
                Some("Rays")
            }
            fn draw_extra_tab(
                &self,
                ui: &mut egui::Ui,
                drawing: &mut Drawing,
                _host: &mut dyn PresetHost,
            ) -> bool {
                let payload = drawing
                    .payload
                    .as_any_mut()
                    .downcast_mut::<RayPayload>()
                    .expect("a ray tool always carries a ray payload");
                ui.add(egui::Slider::new(&mut payload.ray_count, 1..=8).text("rays"))
                    .changed()
            }
            fn paint(
                &self,
                painter: &egui::Painter,
                _chart_rect: egui::Rect,
                style: DrawingStyle,
                points: &[egui::Pos2],
                ctxt: &DrawContext<'_>,
            ) {
                let payload = ctxt
                    .payload
                    .as_any()
                    .downcast_ref::<RayPayload>()
                    .expect("ray payload");
                if let Some(origin) = points.first() {
                    for ray in 0..payload.ray_count {
                        let target = *origin + egui::vec2(40.0, 10.0 * (ray as f32 + 1.0));
                        painter.line_segment([*origin, target], drawing_stroke(style));
                    }
                }
            }
            fn hit_test(
                &self,
                _chart_rect: egui::Rect,
                points: &[egui::Pos2],
                position: egui::Pos2,
                radius_px: f32,
                _ctxt: &DrawContext<'_>,
            ) -> bool {
                points
                    .first()
                    .is_some_and(|point| point.distance(position) <= radius_px * 10.0)
            }
            #[cfg(test)]
            fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
                (vec![egui::pos2(50.0, 50.0)], egui::pos2(60.0, 60.0))
            }
        }
        static RAY: RayTool = RayTool;
        let ray_tool = DrawingTool(&RAY);

        let mut drawings = Drawings::default();
        assert!(drawings.place(ray_tool, ChartPoint::at(1.0, 100.0)));
        // The unique property lives in the payload and rides the undo
        // history exactly like the shared fields do.
        drawings.begin_gesture();
        drawings
            .selected_mut()
            .expect("placement selects")
            .payload
            .as_any_mut()
            .downcast_mut::<RayPayload>()
            .expect("ray payload")
            .ray_count = 5;
        drawings.commit_gesture();
        assert!(drawings.undo(), "the payload edit is one undo entry");
        let restored = drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<RayPayload>()
            .expect("ray payload")
            .ray_count;
        assert_eq!(restored, 2, "undo restores the tool-owned property");

        // The tool-owned inspector tab renders through the same port every
        // tool uses — no central match, no central form edit.
        assert_eq!(ray_tool.extra_tab(), Some("Rays"));
        let ctx = egui::Context::default();
        let mut host = NullPresetHost;
        let mut painted = Vec::new();
        let input = egui::RawInput::default();
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let drawing = drawings.selected_mut().expect("still selected");
                ray_tool.draw_extra_tab(ui, drawing, &mut host);
            });
        });
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                painted.push(text.galley.text().to_owned());
            }
        }
        assert!(
            painted.iter().any(|text| text.contains("rays")),
            "the fake tool's own section rendered: {painted:?}"
        );
    }
}
