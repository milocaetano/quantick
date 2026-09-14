//! The frame's geometry: where a price is, and what is under the pointer.
//!
//! [`PaintCtx`] turns a price into a `y` and paints the lines, gutter chips
//! and tags the chart layer is made of; the free functions beside it are the
//! pure geometry a press has to agree with. They sit together and depend on
//! nothing else in the module, so the ✕ is pressable exactly while it is
//! painted: [`super::paint`] and [`super::input`] ask this file the same
//! question and get the same rectangle.

use eframe::egui;

use super::{
    CHIP_CLEAR_PX, CLOSE_DIVIDER_ALPHA, DRAG_HALO_COLOR, DRAG_HALO_EXTRA_WIDTH_PX, GUTTER_NOTCH_PX,
    HANDLE_CLEAR_PX, HANDLE_SIZE, LINE_DRAG_WIDTH_PX, LINE_GRAB_RADIUS_PX, LINE_HOVER_WIDTH_PX,
    ORDER_DASH_PX, ORDER_GAP_PX, TAG_BUTTON_PX, TAG_GAP_PX, TAG_HEIGHT_PX, TAG_HOVER_SLACK_PX,
    TAG_PAD_X,
};
use crate::chart::PriceScale;
use crate::theme;

/// The frame geometry every paper paint helper reads: one struct so a tag,
/// a chip and a line can never disagree about where the plot ends.
pub(super) struct PaintCtx<'a> {
    pub(super) painter: &'a egui::Painter,
    pub(super) chart_rect: egui::Rect,
    /// Right edge of the interactive plot (the lane divider when a live
    /// lane is up, the chart's edge otherwise) — tags anchor inside it.
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
                egui::Stroke::new(width + DRAG_HALO_EXTRA_WIDTH_PX, DRAG_HALO_COLOR),
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
        let chip_y = dodged_chip_y(
            y,
            self.reserved_chip_y,
            self.chart_rect.top(),
            self.chart_rect.bottom(),
        );
        let galley = self.painter.layout_no_wrap(
            text.to_owned(),
            egui::FontId::monospace(11.0),
            theme::CHIP_INK,
        );
        let text_pos = egui::pos2(self.axis_x + 6.0, chip_y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        self.painter
            .rect_filled(bg, egui::Rounding::same(2.0), color);
        self.painter.galley(text_pos, galley, theme::CHIP_INK);
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
        let ink = if ghost { fill } else { theme::CHIP_INK };
        let galley =
            self.painter
                .layout_no_wrap(text.to_owned(), egui::FontId::monospace(11.0), ink);
        let half = TAG_HEIGHT_PX / 2.0;
        let center_y = clamp_tag_center(y, self.chart_rect.top(), self.chart_rect.bottom());
        let right = self.tag_right - TAG_GAP_PX;
        let button_w = if with_close { TAG_BUTTON_PX } else { 0.0 };
        let content_w = galley.size().x + 2.0 * TAG_PAD_X + button_w;
        let full = egui::Rect::from_min_max(
            egui::pos2(right - content_w, center_y - half),
            egui::pos2(right, center_y + half),
        );
        if ghost {
            self.painter
                .rect_filled(full, egui::Rounding::same(3.0), theme::INSET);
            self.painter.rect_stroke(
                full,
                egui::Rounding::same(3.0),
                egui::Stroke::new(1.0_f32, fill),
            );
        } else {
            self.painter
                .rect_filled(full, egui::Rounding::same(3.0), fill);
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
            egui::FontId::monospace(11.0),
            ink,
        );
        // A hairline of ink between the words and the ✕, so the zone reads
        // as a button rather than a longer label.
        self.painter.line_segment(
            [
                egui::pos2(button.left(), full.top() + 4.0),
                egui::pos2(button.left(), full.bottom() - 4.0),
            ],
            egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgba_unmultiplied(
                    ink.r(),
                    ink.g(),
                    ink.b(),
                    CLOSE_DIVIDER_ALPHA,
                ),
            ),
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
        let font = egui::FontId::monospace(11.0);
        let side_galley =
            self.painter
                .layout_no_wrap(side_text.to_owned(), font.clone(), side_color);
        let points_galley = points.map(|(text, color)| {
            (
                self.painter.layout_no_wrap(text, font.clone(), color),
                color,
            )
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
        self.painter
            .rect_filled(full, egui::Rounding::same(3.0), theme::INSET);
        self.painter.rect_stroke(
            full,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, theme::BORDER),
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
        self.painter.galley(
            egui::pos2(x, center_y - side_size.y / 2.0),
            side_galley,
            side_color,
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
            egui::FontId::monospace(11.0),
            if over_button {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
        // A hairline between the words and the ✕ zone.
        self.painter.line_segment(
            [
                egui::pos2(button.left(), full.top() + 3.0),
                egui::pos2(button.left(), full.bottom() - 3.0),
            ],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
        full
    }

    /// A labelled `SL`/`TP` handle on the entry line: quiet control fill,
    /// the leg's own colour on hover.
    pub(super) fn bracket_handle(&self, rect: egui::Rect, label: &str, leg_color: egui::Color32) {
        let hovered = self.pointer.is_some_and(|pointer| rect.contains(pointer));
        self.painter
            .rect_filled(rect, egui::Rounding::same(3.0), theme::CONTROL);
        self.painter.rect_stroke(
            rect,
            egui::Rounding::same(3.0),
            egui::Stroke::new(1.0_f32, if hovered { leg_color } else { theme::BORDER }),
        );
        self.painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(9.0),
            if hovered {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
    }
}

/// A tag's vertical center: the line's row, kept fully inside the plot.
pub(crate) fn clamp_tag_center(y: f32, top: f32, bottom: f32) -> f32 {
    let half = TAG_HEIGHT_PX / 2.0;
    // A band too short to hold a tag has nothing to clamp into, and
    // `f32::clamp` does not merely saturate there — it panics outright the
    // moment its bounds cross. That band is reachable: `plot_split` floors
    // the plot at 20 px, but `indicators::split_panes` then carves the
    // indicator strips out of it with no floor of its own, so a squeezed
    // window with enough panes really does leave the candles a few pixels.
    // Centre the tag in what there is rather than taking a live session
    // down.
    if bottom - top <= TAG_HEIGHT_PX {
        return f32::midpoint(top, bottom);
    }
    y.clamp(top + half, bottom - half)
}

/// The ✕ zone every closable tag reserves at its right edge — a fixed
/// position derivable without measuring text, which is what lets the
/// paint and the press-time geometric hit-test share one truth.
pub(crate) fn close_button_rect(tag_right: f32, center_y: f32) -> egui::Rect {
    let right = tag_right - TAG_GAP_PX;
    egui::Rect::from_min_max(
        egui::pos2(right - TAG_BUTTON_PX, center_y - TAG_HEIGHT_PX / 2.0),
        egui::pos2(right, center_y + TAG_HEIGHT_PX / 2.0),
    )
}

/// Whether a bracket owner's `SL`/`TP` handles paint this frame.
///
/// The pointer clause is the one that is easy to miss. A pane that is not
/// feeding paper input has no pointer here, and `reveal` can still be true
/// over there — an order's tag opens on *every* pane at once, by design, so
/// one hover reads on both charts. Without the clause the other pane drew a
/// pressable-looking handle beside an order whose presses it does not take:
/// the exact inversion of the layer rule that an invisible control is not a
/// control, and no better.
pub(super) fn handles_visible(
    pointer: Option<egui::Pos2>,
    reveal: bool,
    over_handle: bool,
) -> bool {
    pointer.is_some() && (reveal || over_handle)
}

/// A bracket handle's rect: the ✕ column, one clear step above or below
/// the entry line so it never overlaps the position tag between them.
pub(super) fn bracket_handle_rect(tag_right: f32, entry_y: f32, above: bool) -> egui::Rect {
    let right = tag_right - TAG_GAP_PX;
    let y = if above {
        entry_y - HANDLE_CLEAR_PX - HANDLE_SIZE.y
    } else {
        entry_y + HANDLE_CLEAR_PX
    };
    egui::Rect::from_min_size(egui::pos2(right - HANDLE_SIZE.x, y), HANDLE_SIZE)
}

/// Keep a gutter chip legible when it would land on the last-price chip:
/// push it just clear of the reserved row, towards its own side of the
/// price, clamped into the pane. At the exact fill price (no distance at
/// all) the chip steps down, below the last-price chip. When the reserved
/// row itself hugs a pane edge the clamp can land the chip back inside the
/// band — accepted: a chip pinned at the edge beats one pushed out of the
/// pane, and the next print separates them.
pub(super) fn dodged_chip_y(y: f32, reserved: Option<f32>, top: f32, bottom: f32) -> f32 {
    let Some(reserved) = reserved else {
        return y;
    };
    let delta = y - reserved;
    if delta.abs() >= CHIP_CLEAR_PX {
        return y;
    }
    let dodged = if delta >= 0.0 {
        reserved + CHIP_CLEAR_PX
    } else {
        reserved - CHIP_CLEAR_PX
    };
    dodged.clamp(top, bottom)
}

/// The row around a line where its in-plot tag counts as hovered: the
/// line's own grab band, plus the row a tag was clamped into near a chart
/// edge (where the two part company). Text-free geometry on purpose — a
/// press-time hit-test has no painter to measure a galley with, and the
/// paint must be able to ask the same question.
pub(super) fn tag_row_hit(pointer: egui::Pos2, y: f32, chart: egui::Rect) -> bool {
    if !chart.contains(pointer) {
        return false;
    }
    let center = clamp_tag_center(y, chart.top(), chart.bottom());
    (pointer.y - y).abs() <= LINE_GRAB_RADIUS_PX
        || (pointer.y - center).abs() <= TAG_HEIGHT_PX / 2.0 + TAG_HOVER_SLACK_PX
}
