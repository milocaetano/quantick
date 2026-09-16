//! The chart's axes and the chrome drawn over them.
//!
//! The time strip and the price gutter say where the eye is in the tape; the
//! crosshair, the compass, the axis tags and the last-price chip say where the
//! pointer is against them; and the dividers, gaps and backfill marks say where
//! the tape itself is not continuous. One file because they all read the same
//! two projections — the viewport's bar-to-x and the band's price-to-y — and a
//! label that disagrees with the mark beside it is the bug this grouping is
//! meant to make visible.
//!
//! The free functions these painters call (`paint_placement_hint`,
//! `snap_bar_to_tape`, `magnet_price_of`) stay at module scope in
//! [`super`]: they sit outside the `impl ChartPane` block this module was cut
//! from, and callers on both sides of the cut still reach them there.

use eframe::egui;

use crate::chart::PriceScale;
use crate::drawings::DrawingBand;
use crate::plot_area::{fmt_time_as, split_time_strip};
use crate::pointer_compass;
use crate::theme;
use crate::toolrail::Tool;
use quantick_layers::ChartLayer;
use quantick_orderflow::{format_window_ms, lane_lag_label};

use super::{
    ChartPane, LANE_AXIS_FONT_PX, LANE_AXIS_GAP_PX, PaneChrome, PointerCompass, PriceAxisClaims,
    PriceAxisLevel, grid_color,
};

impl ChartPane {
    /// Bottom time strip: a top border and a few `HH:MM:SS` labels for the
    /// visible bars. Draggable left/right to zoom the candle spacing.
    ///
    /// The labels stay under the candles' own pane; the segment past the lane's
    /// divider is the tape's time axis and reads its window instead
    /// ([`Self::draw_lane_time_axis`]).
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame's own geometry and window, passed rather than cached: reading a stale copy off the pane is how an axis ends up labelling a window it is no longer showing"
    )]
    pub(super) fn draw_time_strip(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        start: usize,
        end: usize,
        total: usize,
        claims: &pointer_compass::AxisClaims,
        chrome: &PaneChrome<'_>,
    ) {
        painter.line_segment(
            [
                egui::pos2(strip.left(), strip.top()),
                egui::pos2(strip.right(), strip.top()),
            ],
            egui::Stroke::new(1.0_f32, grid_color(chrome.style)),
        );
        let font = egui::FontId::monospace(crate::chart::TIME_LABEL_FONT_PX);
        let y = strip.center().y;
        let visible = end.saturating_sub(start);
        if visible == 0 {
            return;
        }
        let (history_strip, _) = split_time_strip(strip, self.frame.lane_divider_x);

        // Measured, not counted. One layout per format per frame — monospace,
        // so a format's sample answers for every label written in it — and the
        // stride comes out of pixels rather than out of a fixed label count
        // that a narrower strip could not honour.
        let width_of = |format: crate::chart::TimeLabelFormat| {
            painter
                .layout_no_wrap(format.sample().to_owned(), font.clone(), theme::TEXT_MUTED)
                .size()
                .x
        };
        let format = crate::chart::time_label_format(history_strip.width(), width_of);
        let label_width = width_of(format);
        // The pointer's chip is always written in full, whatever this strip
        // thinned its own labels down to, so the two extents are asked for
        // separately: a narrow strip pairs a 30 px label with a 54 px chip.
        let chip_width = width_of(crate::chart::TimeLabelFormat::Full);
        // Per *bar*, not per slot: the walk below steps a bar at a time and
        // labels the bar it lands on, so how far apart two labels end up is
        // how far apart two bars are.
        let stride = crate::chart::time_label_stride(self.viewport.px_per_bar(), label_width);

        let mut index = start;
        while index < end {
            if let Some(bar) = self.closed_bar(index) {
                let x = self.viewport.x_center(index, history_strip.right(), total);
                // The whole label, not just its centre: a label centred a few
                // pixels from the end drew its other half over the gutter.
                if crate::chart::label_fits(
                    x,
                    label_width,
                    history_strip.left(),
                    history_strip.right(),
                ) && !pointer_compass::claimed(
                    x,
                    label_width,
                    chip_width,
                    claims.iter().copied(),
                ) {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        fmt_time_as(bar.open_time, chrome.tz, format),
                        font.clone(),
                        theme::TEXT_MUTED,
                    );
                }
            }
            index = index.saturating_add(stride);
        }
    }

    /// The live lane's own time axis: how much market time the tape is
    /// showing, under the tape.
    ///
    /// The lane has no bar boundaries to label — it is one continuous window —
    /// so its axis reads the window itself. It is also the only readout of what
    /// the lane's zoom is currently worth, which is what makes dragging here
    /// something other than guesswork.
    pub(super) fn draw_lane_time_axis(
        &self,
        painter: &egui::Painter,
        lane_strip: Option<egui::Rect>,
        window_ms: i64,
        tape_age: Option<quantick_orderflow::TapeAge>,
    ) {
        let Some(strip) = lane_strip else {
            return;
        };
        // Clipped to the strip, because both labels are sized from the text
        // rather than from the room: a lane narrow enough to make the warning
        // wider than its own strip would otherwise push it left, over the
        // candles' own time labels. The tape's axis may run out of room; it
        // may not spill into the pane beside it.
        let painter = &painter.with_clip_rect(strip);
        let font = egui::FontId::monospace(LANE_AXIS_FONT_PX);
        // The warning is its own text, pinned to the right end of the strip,
        // and the window keeps the centre it has always had. One label growing
        // a suffix would re-centre itself every time a quiet stretch started
        // and ended — a caption sliding under a tape being read for flow. The
        // right end is also where it belongs: directly under the edge the
        // missing marks should have reached.
        let warning = lane_lag_label(window_ms, tape_age)
            .map(|lag| painter.layout_no_wrap(lag, font.clone(), theme::WARN));
        // Room the warning denies the window label. Doubled, because the window
        // keeps the strip's own centre: a centred label grows by half its
        // width towards each end, so it reaches the warning after only half
        // the distance, and subtracting the warning once would let a
        // mid-width lane pass this check and draw the two on top of each
        // other. Two gaps rather than one for the same reason — one holds the
        // warning off the strip's edge, and the other is the space between
        // the two labels, which is what the constant is for. Reserving a
        // single gap left them legal at zero pixels apart.
        let taken = warning.as_ref().map_or(0.0, |galley| {
            2.0 * (galley.size().x + 2.0 * LANE_AXIS_GAP_PX)
        });
        let window_label = format!("tape · {}", format_window_ms(window_ms));
        let window_galley = painter.layout_no_wrap(window_label, font, theme::TEXT_MUTED);
        // A strip too narrow keeps the urgent label and drops this one. The
        // window is a setting the trader chose and can read from the tape's
        // own menu; how old the newest mark is exists nowhere else.
        //
        // It applies with no warning up too, so a lane narrower than this
        // label draws no axis at all rather than a clipped one. Half a word
        // under a tape is not a shorter way of saying the same thing.
        if window_galley.size().x + taken <= strip.width() {
            painter.galley(
                egui::Align2::CENTER_CENTER
                    .align_size_within_rect(window_galley.size(), strip)
                    .min,
                window_galley,
                theme::TEXT_MUTED,
            );
        }
        if let Some(galley) = warning {
            // Right, under the edge the missing marks should have reached —
            // unless it does not fit, and then hard left instead.
            //
            // The clip decides *which end* gets cut, and for this label that
            // is the difference between a shortened sentence and a wrong
            // number. Right-aligned, a 40 px strip cuts the head off
            // "no print for 1 min 30 s" and leaves "30 s" sitting in warn
            // colour: a ninety-second hole read as three. Left-aligned the cut
            // lands on the tail, where a clipped word is visibly a clipped
            // word. A caption that runs out of room may say less; it may not
            // say something else.
            let fits = galley.size().x + 2.0 * LANE_AXIS_GAP_PX <= strip.width();
            let align = if fits {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            };
            painter.galley(
                align
                    .align_size_within_rect(
                        galley.size(),
                        strip.shrink2(egui::vec2(LANE_AXIS_GAP_PX, 0.0)),
                    )
                    .min,
                galley,
                theme::WARN,
            );
        }
    }

    /// Right-hand price axis: round-number gridlines and labels. `axis_x` is
    /// the gutter's left edge — the chart's right edge normally, the live
    /// strip's right edge while the strip sits between them.
    /// A gridline label landing on a height `claims` has already promised to a
    /// chip stays unwritten, rather than being drawn under one.
    pub(super) fn draw_price_axis(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        claims: &PriceAxisClaims<'_>,
        chrome: &PaneChrome<'_>,
    ) {
        self.layer_renderers
            .grid(&mut super::render_registry::GridPass {
                painter,
                chart_rect,
                axis_x,
                scale,
                claims,
                style: chrome.style,
            });
    }

    /// The current price: a dashed line across the chart and a solid chip on
    /// the price axis, coloured by the direction of the bar carrying it.
    ///
    /// This is the always-on answer to "am I above or below?" — the question
    /// every other mark on the canvas is read against, and the one a wall of
    /// resting liquidity cannot answer on its own.
    fn draw_last_price(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        bar: &quantick_engine::Bar,
        chrome: &PaneChrome<'_>,
    ) {
        self.layer_renderers
            .last_price(&mut super::render_registry::LastPricePass {
                painter,
                chart_rect,
                axis_x,
                scale,
                bar,
                style: chrome.style,
            });
    }

    /// Crosshair following the pointer, with the price shown on the axis.
    /// Drawn only while the Crosshair tool is armed on the rail (§7 — the
    /// hover crosshair is a mode, not an always-on layer).
    pub(super) fn draw_crosshair(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        chrome: &PaneChrome<'_>,
    ) {
        self.layer_renderers
            .crosshair(&mut super::render_registry::CrosshairPass {
                painter,
                chart_rect,
                axis_x,
                scale,
                pointer: self.hover_pos,
                armed: chrome.toolrail.tool() == Tool::Crosshair,
            });
    }

    /// Every price a visible drawing on the price band declares, in the order
    /// they were drawn, handed one at a time to `mark`.
    ///
    /// The read half of the axis tags: what the gutter says about the objects
    /// on the chart is data before it is pixels, so a test asserts on the
    /// levels rather than on a shape count, and anything that later needs to
    /// enumerate a trader's levels — a client, the assistant — asks this
    /// rather than the painter.
    ///
    /// Into a buffer the caller owns, the way the band carve is: the levels
    /// are per-frame by nature — they move with pan, zoom and the price scale
    /// — but the container is not, so after the first frame this allocates
    /// nothing. The frame gathers them once and both the axis and the tags
    /// read that one answer; walking twice would let the coordinate an axis
    /// stood aside for differ from the one a chip landed on.
    ///
    /// Objects on an indicator band are skipped. Their `y` means whatever that
    /// pane's axis means, and writing it on the price gutter would put a CVD
    /// reading where a price goes.
    pub(crate) fn price_axis_levels(
        &self,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
        out: &mut Vec<PriceAxisLevel>,
    ) {
        out.clear();
        for (index, drawing) in self.drawings.items().iter().enumerate() {
            if !self.drawings.is_visible(index) || matches!(drawing.band, DrawingBand::Indicator(_))
            {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            for y in drawing.tool.axis_levels(chart_rect, &points) {
                out.push(PriceAxisLevel {
                    id: drawing.id,
                    y,
                    price: scale.price_at(y),
                    color: Self::painted_color(drawing),
                });
            }
        }
    }

    /// The price gutter's own two marks, in the order they have to be painted.
    ///
    /// A level is a static annotation whose value the trader chose and already
    /// knows; the last price is live market data. The two land on the same
    /// pixel exactly when price arrives at the level — the moment the level
    /// was drawn for — so the annotation goes down first and the market's own
    /// number is the one that stays legible.
    ///
    /// One function rather than two adjacent statements, because the order
    /// *is* the rule: written inline it is a pair of lines any later edit can
    /// swap without noticing, and here it is a decision with a name on it and
    /// a test against it.
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame's geometry plus the bar the chip reports, passed rather than cached: both painters need them and this exists to hold their order, not to shorten their signatures"
    )]
    pub(super) fn draw_axis_marks(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        levels: &[PriceAxisLevel],
        newest: Option<&quantick_engine::Bar>,
        chrome: &PaneChrome<'_>,
    ) {
        Self::draw_drawing_axis_tags(painter, chart_rect, axis_x, levels);
        if self.layer_visible(ChartLayer::LastPrice, chrome.style)
            && let Some(bar) = newest
        {
            self.draw_last_price(painter, chart_rect, axis_x, scale, bar, chrome);
        }
    }

    /// The levels the drawings declare, written on the price axis in each
    /// object's own colour.
    /// The levels the drawings declare, written on the price axis in each
    /// object's own colour.
    ///
    /// Rides the `Drawings` layer rather than carrying a switch of its own:
    /// the tag *is* the object, said on the axis, so hiding the objects has to
    /// take their tags with it — a gutter still marked at a level whose line
    /// is gone would be the chart claiming something it is not drawing. The
    /// caller holds that gate, beside every other layer's.
    fn draw_drawing_axis_tags(
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        levels: &[PriceAxisLevel],
    ) {
        for level in levels {
            if level.y < chart_rect.top() || level.y > chart_rect.bottom() {
                continue;
            }
            // The object's own colour, in the last-price chip's language,
            // because it is the same kind of statement: a price this chart is
            // telling you about, at the height it sits. The ink is *computed*
            // rather than borrowed from that chip — the last price wears one
            // of two saturated colours and a drawing wears whatever the
            // trader picked, dark navy included.
            pointer_compass::paint_price_tag(
                painter,
                axis_x,
                level.y,
                pointer_compass::price_text(level.price),
                level.color,
                theme::ink_on(level.color),
            );
        }
    }

    /// The bar under `x`, as data, with the instant it opened.
    ///
    /// `history_right` is the candles' own right edge: past it lies the live
    /// lane, which is not made of bar slots at all. A pointer out there, or
    /// out in the projection margin past the newest bar, is over no bar — and
    /// this says so rather than naming the nearest one, because a compass that
    /// rounds empty canvas onto the last candle would put a time on a place
    /// where nothing happened.
    #[must_use]
    pub(crate) fn pointer_bar(
        &self,
        x: f32,
        history_right: f32,
        total: usize,
    ) -> Option<pointer_compass::PointerBar> {
        if total == 0 || x > history_right {
            return None;
        }
        let slot = self.viewport.slot_at_x(x, history_right, total)?;
        Some(pointer_compass::PointerBar {
            slot,
            open_time_unix_ms: self.slot_open_time(slot)?,
        })
    }

    /// What the compass will draw this frame.
    ///
    /// Decided once, before either axis labels itself: the axes read it to
    /// know which coordinates are already spoken for, and the paint reads the
    /// same answer several hundred lines later rather than working it out
    /// again. A decision made twice is a decision two surfaces can disagree
    /// about, and here the disagreement would be an axis hiding a label for a
    /// chip that never arrived.
    pub(super) fn pointer_compass(
        &self,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
        chrome: &PaneChrome<'_>,
    ) -> Option<PointerCompass> {
        let price_on = self.layer_visible(ChartLayer::PointerPrice, chrome.style);
        let time_on = self.layer_visible(ChartLayer::PointerTime, chrome.style);
        if !price_on && !time_on {
            return None;
        }
        // Resolved whether or not the time half is switched on. The readout
        // says what is under the pointer, and a switch on a mark is not a
        // statement about the world: gating it here would tell the control
        // plane's cursor scope there is no bar under a candle plainly under
        // the pointer. The lookup is a division and a slot read.
        let bar = self
            .hover_pos
            .and_then(|pointer| self.pointer_bar(pointer.x, history_right, total));
        let readout = pointer_compass::readout(self.hover_pos, chart_rect, scale, bar)?;
        // The armed crosshair already writes a price on this axis. Two chips
        // stacked on one pixel is not two facts, so the mode that draws the
        // cross keeps the tag that belongs to it — the compass still supplies
        // the time half, which the crosshair has never drawn.
        //
        // The tool alone decides it: arming the crosshair turns its layer back
        // on through `unhide_layer_for_armed_tool`, so a second conjunct
        // asking whether the layer is visible could never be false and would
        // read as a condition that can be met.
        // The paper aim writes a price on this axis for the very pixel the
        // pointer is on, and while it is up it owns that chip for the same
        // reason the crosshair does.
        let crosshair_owns_the_price =
            chrome.toolrail.tool() == Tool::Crosshair || chrome.paper.aiming();
        Some(PointerCompass {
            price: price_on && !crosshair_owns_the_price,
            time: time_on && readout.bar.is_some(),
            readout,
        })
    }

    /// The pointer's compass: its price on the price axis, and the time of the
    /// bar it is over on the time axis.
    ///
    /// Two switches, one per axis ([`ChartLayer::PointerPrice`] and
    /// [`ChartLayer::PointerTime`]), each reached from that axis's own
    /// right-click menu. See [`crate::pointer_compass`] for why this exists
    /// and why it is not a crosshair.
    pub(super) fn draw_pointer_compass(
        &self,
        painter: &egui::Painter,
        compass: &PointerCompass,
        axis_x: f32,
        time_strip: egui::Rect,
        chrome: &PaneChrome<'_>,
    ) {
        self.layer_renderers
            .pointer(&mut super::render_registry::PointerPass {
                painter,
                compass,
                axis_x,
                time_strip,
                divider_x: self.frame.lane_divider_x,
                tz: chrome.tz,
            });
    }

    /// A vertical marker where venue candles give way to bars this app built
    /// from prints.
    ///
    /// Dashed, and in a nearly transparent white rather than the backfill
    /// divider's amber. Both mark provenance, but they are read differently:
    /// the backfill divider answers a question asked once, while this one sits
    /// on the chart the entire session, so it is drawn to be *found* rather
    /// than noticed (see [`theme::SEAM_LINE`]). The dash still says it is a
    /// different kind of boundary. Left of it the bars are the venue's own
    /// summaries — one price per interval, with the aggressor split only where
    /// the venue publishes one. Right of it every bar was cut from prints this
    /// app saw.
    pub(super) fn draw_seam_divider(
        &self,
        painter: &egui::Painter,
        pane: egui::Rect,
        total: usize,
        candle_width: f32,
    ) {
        self.layer_renderers
            .seam(&mut super::render_registry::DividerPass {
                painter,
                pane,
                total,
                candle_width,
                viewport: &self.viewport,
                seam: self.seam_slot(),
                boundary: self.state.backfill_boundary(),
                bars: self.state.bars(),
                gaps: &[],
            });
    }

    /// Vertical markers where the tape has a hole no print covers.
    ///
    /// A reconnect that keeps the timeline is the whole point of having a
    /// reconnect beside a reload — but the market that traded while nobody was
    /// listening cannot be recovered, and butting the two halves of the session
    /// against each other would draw one continuous tape that never existed.
    /// So the hole is drawn: dashed, amber-ish, with the silence named beside
    /// it, at the bar the stream resumed into.
    pub(super) fn draw_feed_gaps(
        &self,
        painter: &egui::Painter,
        pane: egui::Rect,
        total: usize,
        candle_width: f32,
        gaps: &[quantick_feed::FeedGap],
    ) {
        self.layer_renderers
            .feed_gaps(&mut super::render_registry::DividerPass {
                painter,
                pane,
                total,
                candle_width,
                viewport: &self.viewport,
                seam: self.seam_slot(),
                boundary: self.state.backfill_boundary(),
                bars: self.state.bars(),
                gaps,
            });
    }

    /// A vertical marker separating backfilled history (left) from live (right),
    /// drawn only when the boundary falls inside the candles' pane.
    ///
    /// `pane` is the candles' own rect — the chart minus the live lane — since
    /// that is the space the viewport maps bar indices into.
    pub(super) fn draw_backfill_divider(
        &self,
        painter: &egui::Painter,
        pane: egui::Rect,
        total: usize,
        candle_width: f32,
    ) {
        self.layer_renderers
            .backfill(&mut super::render_registry::DividerPass {
                painter,
                pane,
                total,
                candle_width,
                viewport: &self.viewport,
                seam: self.seam_slot(),
                boundary: self.state.backfill_boundary(),
                bars: self.state.bars(),
                gaps: &[],
            });
    }
}
