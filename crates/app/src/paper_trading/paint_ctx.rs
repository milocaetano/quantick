//! The frame's geometry: where a price is, and what is under the pointer.
//!
//! [`PaintCtx`] turns a price into a `y` and paints the lines, gutter chips
//! and tags the chart layer is made of. The geometry a press has to agree
//! with is the desk's (`quantick_paper::desk::geometry`); the functions at
//! the bottom of this file only carry it across the window's edge, so the ✕
//! is pressable exactly while it is painted: the paint here and the desk's
//! hit-test ask one function the same question and get the same rectangle.

use std::cmp::Ordering;
use std::sync::Arc;

use eframe::egui;
use eframe::egui::{Color32, Stroke};
use quantick_appearance::Token;
use quantick_appearance::paper::{self, ChartLevel};
use quantick_paper::desk::geometry::{self, Bounds, Point};
use quantick_paper::desk::geometry::{
    LINE_GRAB_RADIUS_PX, TAG_BUTTON_PX, TAG_GAP_PX, TAG_HEIGHT_PX,
};
pub(crate) use quantick_paper::desk::geometry::{clamp_tag_center, dodged_chip_y};

use super::{
    CLOSE_DIVIDER_ALPHA, DRAG_HALO_EXTRA_WIDTH_PX, GUTTER_NOTCH_PX, LINE_DRAG_WIDTH_PX,
    LINE_HOVER_WIDTH_PX, LINE_WIDTH_PX, ORDER_DASH_PX, ORDER_GAP_PX, TAG_PAD_X,
};
use crate::chart::PriceScale;
use crate::theme;

/// How one priced level is drawn: its line and price chip in `color` and,
/// on a theme that plates its chart, its words in the plate's ink.
#[derive(Debug, Clone, Copy)]
pub(super) struct LevelPaint {
    pub(super) color: Color32,
    pub(super) plate_ink: Option<Color32>,
}

/// The frame geometry every paper paint helper reads: one struct so a tag,
/// a chip and a line can never disagree about where the plot ends.
pub(super) struct PaintCtx<'a> {
    pub(super) palette: std::sync::Arc<theme::Palette>,
    pub(super) painter: &'a egui::Painter,
    pub(super) chart_rect: egui::Rect,
    /// Right edge of the tag band (`tag_band`: the candles' band, or the
    /// whole plot in tape only) — tags anchor inside it.
    pub(super) tag_right: f32,
    /// Left edge of the price axis — lines run to it, gutter chips sit past
    /// it.
    pub(super) axis_x: f32,
    pub(super) scale: &'a PriceScale,
    /// The last-price chip's row, which every gutter chip dodges.
    pub(super) reserved_chip_y: Option<f32>,
    /// The pointer, `Some` only on the pane that owns paper input.
    pub(super) pointer: Option<egui::Pos2>,
}

impl PaintCtx<'_> {
    /// A level's paint: the plate `paper::level_plate` lays under it, or
    /// `fallback` (the accent, side or leg colour) where the chart has none.
    pub(super) fn level(&self, level: ChartLevel, fallback: Color32) -> LevelPaint {
        match paper::level_plate(&self.palette.resolved, level) {
            Some(plate) => LevelPaint {
                color: *self.palette.get(plate.fill),
                plate_ink: Some(*self.palette.get(plate.ink)),
            },
            None => LevelPaint {
                color: fallback,
                plate_ink: None,
            },
        }
    }

    /// Words in `ink` on a plate, each signed figure in the plate's gain or
    /// loss ink.
    fn plate_text(&self, text: &str, ink: Color32) -> Arc<egui::Galley> {
        let palette = &self.palette;
        self.painter.layout_job(crate::paper_chrome::signed_text(
            text,
            palette.mono_font(11.0),
            ink,
            |value| match value.cmp(&rust_decimal::Decimal::ZERO) {
                Ordering::Equal => ink,
                sign => *palette.get(paper::on_plate(sign, Token::OrderPlateInk)),
            },
        ))
    }

    /// [`Self::gutter_chip`] for a level: its plate's ink where it has one.
    pub(super) fn level_chip(&self, y: f32, paint: LevelPaint, text: &str) {
        let ink = paint.plate_ink.unwrap_or_else(|| self.palette.chip_ink());
        self.chip(y, paint.color, text, (paint.color, ink, egui::Stroke::NONE));
    }

    /// [`Self::chip_tag`] for a level: on a plated chart a resting tag is the
    /// plate itself, its words in the level's ink. A ghost keeps its outline.
    pub(super) fn level_tag(
        &self,
        y: f32,
        paint: LevelPaint,
        text: &str,
        with_close: bool,
        ghost: bool,
    ) {
        match paint.plate_ink {
            Some(ink) if !ghost => {
                let galley = self.plate_text(text, ink);
                self.tag_body(y, galley, (paint.color, None), ink, with_close);
            }
            _ => self.chip_tag(y, paint.color, text, with_close, ghost),
        }
    }

    pub(super) fn in_range(&self, y: f32) -> bool {
        y >= self.chart_rect.top() && y <= self.chart_rect.bottom()
    }

    /// Whether the pointer is within grab range of the line at `y`.
    pub(super) fn hovers_line(&self, y: f32) -> bool {
        self.pointer.is_some_and(|pointer| {
            self.chart_rect.contains(pointer) && (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        })
    }

    /// The line itself: hover thickens it, a drag thickens it further and
    /// paints the drawings' halo treatment beneath, so a grabbed stop feels
    /// identical to a grabbed drawing.
    pub(super) fn level_line(
        &self,
        y: f32,
        color: egui::Color32,
        dashed: bool,
        base_width: f32,
        hovered: bool,
        dragged: bool,
    ) {
        let width = if dragged {
            LINE_DRAG_WIDTH_PX
        } else if hovered {
            LINE_HOVER_WIDTH_PX.max(base_width)
        } else {
            base_width
        };
        let points = [
            egui::pos2(self.chart_rect.left(), y),
            egui::pos2(self.axis_x, y),
        ];
        if dragged {
            self.painter.line_segment(
                points,
                egui::Stroke::new(
                    width + DRAG_HALO_EXTRA_WIDTH_PX,
                    self.palette.trade_selection_halo(),
                ),
            );
        }
        let stroke = egui::Stroke::new(width, color);
        if dashed {
            self.painter.extend(egui::Shape::dashed_line(
                &points,
                stroke,
                ORDER_DASH_PX,
                ORDER_GAP_PX,
            ));
        } else {
            self.painter.line_segment(points, stroke);
        }
    }

    /// The gutter chip: the price and nothing else, on the last-price
    /// chip's geometry. The line stays at its true price; only the chip
    /// dodges the reserved last-price row.
    pub(super) fn gutter_chip(&self, y: f32, color: egui::Color32, text: &str) {
        let palette = &self.palette;
        self.chip(
            y,
            color,
            text,
            (color, palette.chip_ink(), egui::Stroke::NONE),
        );
    }

    /// Hollow: a `color` border and ink on the chart's ground, for a level
    /// that is no order (the aim's echo).
    pub(super) fn hollow_gutter_chip(&self, y: f32, color: egui::Color32, text: &str) {
        let palette = &self.palette;
        let border = egui::Stroke::new(LINE_WIDTH_PX, color);
        self.chip(y, color, text, (palette.canvas(), color, border));
    }

    fn chip(&self, y: f32, color: Color32, text: &str, look: (Color32, Color32, Stroke)) {
        let (fill, ink, border) = look;
        let chip_y = dodged_chip_y(
            y,
            self.reserved_chip_y,
            self.chart_rect.top(),
            self.chart_rect.bottom(),
        );
        let galley =
            self.painter
                .layout_no_wrap(text.to_owned(), self.palette.mono_font(11.0), ink);
        let text_pos = egui::pos2(self.axis_x + 6.0, chip_y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        self.painter
            .rect(bg, egui::Rounding::same(2.0), fill, border);
        self.painter.galley(text_pos, galley, ink);
        // A chip dodged away from its own line is a price with no arrow back
        // to it, and near a chart edge every chip is dodged. The notch is
        // drawn at the *line's* height, not the chip's, so the two are only
        // ever read together.
        self.gutter_notch(y, color);
    }

    /// A small triangle on the axis edge, pointing into the plot at `y`.
    ///
    /// The dashed line says where across the width; this says where on the
    /// axis, which is the half a trader reads when the line is behind the
    /// tape band or the heat map. Drawn for every level the aim carries —
    /// the entry and both its legs — so one glance at the axis answers
    /// "where does this order sit" without following three lines back.
    fn gutter_notch(&self, y: f32, color: egui::Color32) {
        if !self.in_range(y) {
            return;
        }
        let x = self.axis_x;
        self.painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x - GUTTER_NOTCH_PX, y),
                egui::pos2(x, y - GUTTER_NOTCH_PX * 0.75),
                egui::pos2(x, y + GUTTER_NOTCH_PX * 0.75),
            ],
            color,
            egui::Stroke::NONE,
        ));
    }

    /// A solid tag on a line, right-anchored inside the plot. When it is
    /// closable (`with_close` — everything but a mid-drag preview) its ✕
    /// occupies the tag's right edge, painted on `close_button_rect`'s own
    /// geometry — the exact rect the press-time hit-test computes, so the
    /// two can never disagree. Overlay ✕s carry no tooltip of their own —
    /// each has a full-size, fully labelled twin in the chrome. A `ghost`
    /// tag is outlined instead — dark fill, `fill` as its stroke and ink —
    /// for a leg that has not armed yet (see `leg_tag`).
    pub(super) fn chip_tag(
        &self,
        y: f32,
        fill: egui::Color32,
        text: &str,
        with_close: bool,
        ghost: bool,
    ) {
        let palette = &self.palette;
        let financial = !palette.resolved.dark
            && text.split_whitespace().any(|word| {
                word.starts_with(['+', '-']) && word.parse::<rust_decimal::Decimal>().is_ok()
            });
        let ink = if financial {
            palette.text_primary()
        } else if ghost {
            fill
        } else {
            crate::paper_chrome::trade_action_ink(palette, fill)
        };
        let galley = if financial {
            self.painter.layout_job(crate::paper_chrome::financial_text(
                palette,
                text,
                palette.mono_font(11.0),
                ink,
            ))
        } else {
            self.painter
                .layout_no_wrap(text.to_owned(), palette.mono_font(11.0), ink)
        };
        let body = if ghost || financial {
            (palette.inset(), Some(fill))
        } else {
            (fill, None)
        };
        self.tag_body(y, galley, body, ink, with_close);
    }

    /// A tag's body, words and ✕, right-anchored inside the plot: `body` is
    /// its fill and the outline it wears, if any.
    fn tag_body(
        &self,
        y: f32,
        galley: Arc<egui::Galley>,
        (fill, outline): (Color32, Option<Color32>),
        ink: Color32,
        with_close: bool,
    ) {
        let palette = &self.palette;
        let half = TAG_HEIGHT_PX / 2.0;
        let center_y = clamp_tag_center(y, self.chart_rect.top(), self.chart_rect.bottom());
        let right = self.tag_right - TAG_GAP_PX;
        let button_w = if with_close { TAG_BUTTON_PX } else { 0.0 };
        let content_w = galley.size().x + 2.0 * TAG_PAD_X + button_w;
        let full = egui::Rect::from_min_max(
            egui::pos2(right - content_w, center_y - half),
            egui::pos2(right, center_y + half),
        );
        self.painter
            .rect_filled(full, egui::Rounding::same(3.0), fill);
        if let Some(outline) = outline {
            self.painter.rect_stroke(
                full,
                egui::Rounding::same(3.0),
                egui::Stroke::new(1.0_f32, outline),
            );
        }
        self.painter.galley(
            egui::pos2(full.left() + TAG_PAD_X, center_y - galley.size().y / 2.0),
            galley,
            ink,
        );
        if !with_close {
            return;
        }
        let button = close_button_rect(self.tag_right, center_y);
        self.painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            palette.mono_font(11.0),
            ink,
        );
        // A hairline of ink between the words and the ✕, so the zone reads
        // as a button rather than a longer label.
        self.painter.line_segment(
            [
                egui::pos2(button.left(), full.top() + 4.0),
                egui::pos2(button.left(), full.bottom() - 4.0),
            ],
            egui::Stroke::new(1.0_f32, theme::with_opacity(ink, CLOSE_DIVIDER_ALPHA)),
        );
    }

    /// The position's tag wears the card grammar, not a chip: a position is
    /// a fact about the account, not an order that will fire. Its ✕ sits at
    /// the right edge on `close_button_rect`'s own geometry, like every
    /// chart close. Returns the tag's rect (the handle-reveal hover zone).
    pub(super) fn position_tag(
        &self,
        y: f32,
        side_color: egui::Color32,
        side_text: &str,
        points: Option<(String, egui::Color32)>,
    ) -> egui::Rect {
        let palette = &self.palette;
        let font = palette.mono_font(11.0);
        // A plated chart writes the label on the order plate; Dark keeps the
        // card, its words in the side colour.
        let plate = paper::position_label(&palette.resolved)
            .map(|plate| (*palette.get(plate.fill), *palette.get(plate.ink)));
        let side_galley = self.painter.layout_no_wrap(
            side_text.to_owned(),
            font.clone(),
            match plate {
                Some((_, ink)) => ink,
                None if palette.resolved.dark => side_color,
                None => palette.text_primary(),
            },
        );
        let points_galley = points.map(|(text, color)| match plate {
            Some((_, ink)) => (self.plate_text(&text, ink), ink),
            None => (
                self.painter.layout_job(crate::paper_chrome::financial_text(
                    palette,
                    &text,
                    font.clone(),
                    color,
                )),
                color,
            ),
        });
        let rail = 3.0;
        let mut content_w = rail + TAG_PAD_X + side_galley.size().x + TAG_PAD_X + TAG_BUTTON_PX;
        if let Some((galley, _)) = &points_galley {
            content_w += galley.size().x + TAG_PAD_X;
        }
        let half = TAG_HEIGHT_PX / 2.0;
        let center_y = clamp_tag_center(y, self.chart_rect.top(), self.chart_rect.bottom());
        let right = self.tag_right - TAG_GAP_PX;
        let full = egui::Rect::from_min_max(
            egui::pos2(right - content_w, center_y - half),
            egui::pos2(right, center_y + half),
        );
        let (body, edge) = plate.map_or((palette.inset(), palette.border()), |(fill, _)| {
            (fill, fill)
        });
        self.painter
            .rect_filled(full, egui::Rounding::same(3.0), body);
        self.painter.rect_stroke(
            full,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, edge),
        );
        // The side rail rides the card's left edge.
        self.painter.rect_filled(
            egui::Rect::from_min_max(
                full.min + egui::vec2(1.0, 1.0),
                egui::pos2(full.min.x + 1.0 + rail, full.max.y - 1.0),
            ),
            egui::Rounding {
                nw: 2.0,
                sw: 2.0,
                ne: 0.0,
                se: 0.0,
            },
            side_color,
        );
        let mut x = full.left() + rail + TAG_PAD_X;
        let side_size = side_galley.size();
        let side_ink = plate.map_or(side_color, |(_, ink)| ink);
        self.painter.galley(
            egui::pos2(x, center_y - side_size.y / 2.0),
            side_galley,
            side_ink,
        );
        x += side_size.x + TAG_PAD_X;
        if let Some((galley, color)) = points_galley {
            let size = galley.size();
            self.painter
                .galley(egui::pos2(x, center_y - size.y / 2.0), galley, color);
        }
        let button = close_button_rect(self.tag_right, center_y);
        let over_button = self.pointer.is_some_and(|pointer| button.contains(pointer));
        self.painter.text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            palette.mono_font(11.0),
            match plate {
                Some((_, ink)) => ink,
                None if over_button => palette.text_primary(),
                None => palette.text_muted(),
            },
        );
        // A hairline between the words and the ✕ zone.
        self.painter.line_segment(
            [
                egui::pos2(button.left(), full.top() + 3.0),
                egui::pos2(button.left(), full.bottom() - 3.0),
            ],
            egui::Stroke::new(
                1.0_f32,
                plate.map_or(palette.border(), |(_, ink)| {
                    theme::with_opacity(ink, CLOSE_DIVIDER_ALPHA)
                }),
            ),
        );
        full
    }

    /// A labelled `SL`/`TP` handle on the entry line: quiet control fill,
    /// the leg's own colour on hover.
    pub(super) fn bracket_handle(&self, rect: egui::Rect, label: &str, leg_color: egui::Color32) {
        let palette = &self.palette;
        let hovered = self.pointer.is_some_and(|pointer| rect.contains(pointer));
        self.painter
            .rect_filled(rect, egui::Rounding::same(3.0), palette.control());
        self.painter.rect_stroke(
            rect,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, if hovered { leg_color } else { palette.border() }),
        );
        self.painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            palette.mono_font(9.0),
            if hovered {
                palette.text_primary()
            } else {
                palette.text_muted()
            },
        );
    }
}

/// A window point, in the desk's plain values.
pub(super) fn point(pos: egui::Pos2) -> Point {
    Point::new(pos.x, pos.y)
}

/// A desk point, back in the window's type.
pub(super) fn pos(point: Point) -> egui::Pos2 {
    egui::pos2(point.x, point.y)
}

/// A window rectangle, in the desk's plain values.
pub(super) fn bounds(rect: egui::Rect) -> Bounds {
    Bounds::from_min_max(point(rect.min), point(rect.max))
}

/// A desk rectangle, back in the window's type.
pub(super) fn rect(bounds: Bounds) -> egui::Rect {
    egui::Rect::from_min_max(pos(bounds.min), pos(bounds.max))
}

/// Where the paper tags and their controls anchor; see `geometry::tag_band`.
pub(crate) fn tag_band(candles: egui::Rect, plot: egui::Rect) -> egui::Rect {
    rect(geometry::tag_band(bounds(candles), bounds(plot)))
}

/// The ✕ zone every closable tag reserves at its right edge; see
/// `geometry::close_button_rect`.
pub(crate) fn close_button_rect(tag_right: f32, center_y: f32) -> egui::Rect {
    rect(geometry::close_button_rect(tag_right, center_y))
}

/// Whether a bracket owner's `SL`/`TP` handles paint this frame; see
/// `geometry::handles_visible`.
pub(super) fn handles_visible(
    pointer: Option<egui::Pos2>,
    reveal: bool,
    over_handle: bool,
) -> bool {
    geometry::handles_visible(pointer.map(point), reveal, over_handle)
}

/// A bracket handle's rect; see `geometry::bracket_handle_rect`.
pub(super) fn bracket_handle_rect(tag_right: f32, entry_y: f32, above: bool) -> egui::Rect {
    rect(geometry::bracket_handle_rect(tag_right, entry_y, above))
}

/// The cmd preview's line and label; see `desk::cmd::cmd_preview_layout`.
pub(super) fn cmd_preview_layout(
    band: egui::Rect,
    axis_x: f32,
    pointer: egui::Pos2,
) -> (egui::Pos2, egui::Pos2, egui::Rect) {
    let (start, end, label) =
        quantick_paper::desk::cmd::cmd_preview_layout(bounds(band), axis_x, point(pointer));
    (pos(start), pos(end), rect(label))
}
