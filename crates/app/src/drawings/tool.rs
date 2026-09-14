//! The implementation port a drawing tool plugs into, and the copyable
//! handle the rest of the app holds it by.
//!
//! The registry that names the implementations stays in the parent module:
//! a new tool is still one implementation file plus one name there.

use std::fmt;

use eframe::egui;

use crate::theme;

use super::{
    AnchorSnap, AxisLevels, Constrain, DEFAULT_DRAWING_COLOR, DRAWING_TOOLS, DrawContext, Drawing,
    DrawingBand, DrawingPayload, DrawingStyle, GlyphSize, Handles, IconDots, IconLetter,
    IconStrokes, NoPayload, PresetHost, SELECTED_ANCHOR_FILL, SELECTED_ANCHOR_RADIUS_PX,
    SELECTED_ANCHOR_RING_WIDTH_PX, SELECTION_HALO_COLOR, SELECTION_HALO_EXTRA_WIDTH_PX, ToolFamily,
    ToolShortcut,
};

/// The implementation port every drawing plugs into. Selection visuals (halo
/// and anchor handles) are common chrome painted by the wrapper, so a tool
/// only ever paints its own geometry in the style it is given. Capability
/// methods drive which inspector sections exist for the tool — an
/// unsupported property is absent, never disabled.
pub(super) trait DrawingToolImpl: Sync {
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
    /// Anchor dots painted over [`Self::icon_strokes`] — see [`IconDots`].
    /// Only meaningful alongside strokes: a font glyph draws its own.
    fn icon_dots(&self) -> IconDots {
        &[]
    }
    /// A letter set into the vector icon — see [`IconLetter`]. Only
    /// meaningful alongside strokes: a font glyph is somebody else's
    /// drawing, with no corner reserved to put a letter in.
    fn icon_letter(&self) -> Option<IconLetter> {
        None
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
    /// The words this object holds, when its content is words rather than
    /// geometry. `None` for every tool but the note.
    ///
    /// Declaring it is what earns the on-chart editor: a tool that holds
    /// text is placed *empty*, so the host puts the caret in the object
    /// itself the moment it lands. That used to be answered by opening the
    /// settings panel, which typed the note in one place and showed it in
    /// another — the eye crossing the screen between keystrokes, and the
    /// object under the pointer reading "Note" in grey the whole time.
    ///
    /// Borrowed, never cloned: this is read on the frame path while the
    /// editor is open, and a note can be a paragraph.
    fn inline_text<'a>(&self, _payload: &'a dyn DrawingPayload) -> Option<&'a str> {
        None
    }
    /// Write the words back. Paired with [`Self::inline_text`]: a tool that
    /// answers one answers both, and the editor is the only caller.
    fn set_inline_text(&self, _payload: &mut dyn DrawingPayload, _text: String) {}
    /// Whether this tool's content is words at all — asked of the *tool*,
    /// with no object in hand, on the placement path. Answering it by
    /// building a payload just to look at it would allocate a box to learn
    /// something that is fixed at compile time. A test holds the two answers
    /// together, so a tool cannot claim one and implement the other.
    fn holds_text(&self) -> bool {
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
    /// The part of this object that belongs *under* the candles, if any.
    ///
    /// Almost every tool draws nothing here, which is why the default is a
    /// no-op: a line, a note, a Fibonacci grid is an annotation *on* the
    /// chart, and burying it under the price would be losing it. A volume
    /// profile is the exception that earns the pass — its histogram is
    /// context the price is read *against*, like the liquidity map, and drawn
    /// over the candles it tints every body it covers.
    ///
    /// Called before the candles, on the same geometry the over-candles pass
    /// gets, with no selection halo and no handles: those are affordances,
    /// and an affordance under the price is not one.
    fn paint_under(
        &self,
        _painter: &egui::Painter,
        _chart_rect: egui::Rect,
        _style: DrawingStyle,
        _points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) {
    }
    /// The heights on the price axis this object *is*, for the axis to tag.
    ///
    /// A horizontal line does not merely cross a price, it names one: the
    /// price is the whole object, and the trader who drew it wants to read it
    /// off the axis without hunting for where the line meets the gutter — the
    /// yellow tag every other platform puts there. So the tool declares its
    /// levels and [`crate::pane::ChartPane::draw_drawing_axis_tags`] writes
    /// them, in the object's own colour.
    ///
    /// Screen `y`, not price, because `points` are already projected and the
    /// projection has exactly one owner: a tool that converted back to price
    /// itself would be a second copy of the price scale, free to drift from
    /// the one the axis labels are drawn with. The axis reads the price back
    /// off the same scale it labels.
    ///
    /// The default is empty, and that is the answer for almost every tool: a
    /// trend line crosses every price between its ends and names none of
    /// them, and an axis tagged with everything drawn on the chart is an axis
    /// nobody can read. Declaring a level is opting *in* to the gutter.
    ///
    /// `chart_rect` is the same rect [`DrawingToolImpl::paint`] gets, and it
    /// is here for one reason: a tool must not declare a level it is not
    /// drawing. A horizontal ray whose anchor projects past the right edge
    /// paints no stroke at all, and a chip on the gutter for a line that is
    /// not there is the axis claiming something the canvas is not.
    fn axis_levels(&self, _chart_rect: egui::Rect, _points: &[egui::Pos2]) -> AxisLevels {
        AxisLevels::new()
    }
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
pub struct DrawingTool(pub(super) &'static dyn DrawingToolImpl);

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

    /// Anchor dots of the vector icon, `&[]` when it has none.
    #[must_use]
    pub fn icon_dots(self) -> IconDots {
        self.0.icon_dots()
    }

    /// The letter set into the vector icon, `None` when it needs none.
    #[must_use]
    pub fn icon_letter(self) -> Option<IconLetter> {
        self.0.icon_letter()
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

    /// See [`DrawingToolImpl::axis_levels`].
    #[must_use]
    pub fn axis_levels(self, chart_rect: egui::Rect, points: &[egui::Pos2]) -> AxisLevels {
        self.0.axis_levels(chart_rect, points)
    }

    /// See [`DrawingToolImpl::inline_text`].
    #[must_use]
    pub fn inline_text(self, payload: &dyn DrawingPayload) -> Option<&str> {
        self.0.inline_text(payload)
    }

    /// See [`DrawingToolImpl::set_inline_text`].
    pub fn set_inline_text(self, payload: &mut dyn DrawingPayload, text: String) {
        self.0.set_inline_text(payload, text);
    }

    /// Whether this tool's content is words — the question the host asks a
    /// freshly placed object to decide whether it needs the caret.
    #[must_use]
    pub fn holds_text(self) -> bool {
        self.0.holds_text()
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

    /// Paint the part of the object that goes under the candles — see
    /// [`DrawingToolImpl::paint_under`]. No halo, no handles.
    pub fn paint_under(
        self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        self.0.paint_under(painter, chart_rect, style, points, ctxt);
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
                content_editing: false,
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
    pub(super) fn test_geometry(self) -> (Vec<egui::Pos2>, egui::Pos2) {
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
