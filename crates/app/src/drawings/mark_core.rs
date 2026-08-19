//! The shared body of the trade marks: a filled arrowhead stamped on one
//! bar, clear of the candle it belongs to.
//!
//! A mark is not the [`super::arrow`] tool with fewer clicks. That one is a
//! *vector* — tail here, point there, "look at this" — and it takes two
//! anchors because both ends carry meaning. A mark is a *stamp*: one click,
//! no direction, "I acted on this bar". The two coexist because they answer
//! different questions, and folding them together would cost the stamp the
//! single click that is its entire reason to exist.
//!
//! Two rules make it read as a mark rather than as a shape:
//!
//! * **It sits outside the candle.** The anchor snaps to the bar's low (up)
//!   or high (down) through [`super::AnchorSnap`], and the glyph is offset
//!   from there in *pixels*. An offset in price would drift off the candle
//!   the moment the vertical zoom changed.
//! * **It does not grow with the zoom.** Its size is screen pixels, chosen
//!   by the trader. A note about the chart that inflates with the chart has
//!   quietly become a second series.

use eframe::egui;

use super::{DrawContext, DrawingPayload, DrawingStyle, GlyphSize, ToolFamily};

/// The rail family the marks share. They are not Lines: "mark my entry" and
/// "draw a trend" are not looked for in the same place.
pub(super) const MARKS_FAMILY: ToolFamily = ToolFamily {
    id: "marks",
    title: "Marks",
    icon: egui_phosphor::regular::ARROW_FAT_UP,
    icon_strokes: &[],
    icon_dots: &[],
};

/// Gap between the candle's extreme and the tip of the mark, in pixels, so
/// the mark never touches the bar it is about.
const MARK_GAP_PX: f32 = 6.0;
pub(super) const DEFAULT_MARK_PX: f32 = 16.0;
pub(super) const MIN_MARK_PX: f32 = 10.0;
pub(super) const MAX_MARK_PX: f32 = 32.0;
/// Ratio of the arrowhead's half-width to its height.
const HEAD_HALF_WIDTH: f32 = 0.42;
/// Ratio of the shaft's half-width to the head's height.
const SHAFT_HALF_WIDTH: f32 = 0.16;
/// How much of the total height the head takes.
const HEAD_SHARE: f32 = 0.55;

/// Which way the mark points, which is also which extreme it hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    /// Points up, sits below the bar's low: a buy.
    Up,
    /// Points down, sits above the bar's high: a sell.
    Down,
}

impl Direction {
    /// Screen-space sign: down the screen is +y, so an up mark grows -y.
    const fn sign(self) -> f32 {
        match self {
            Self::Up => 1.0,
            Self::Down => -1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct MarkPayload {
    pub size_px: f32,
}

impl Default for MarkPayload {
    fn default() -> Self {
        Self {
            size_px: DEFAULT_MARK_PX,
        }
    }
}

impl DrawingPayload for MarkPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(self.clone())
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| other == self)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    /// A preset carries the size. It does not carry the colour — that is the
    /// shared style's job — which is what lets "my big buy arrow" apply to
    /// both directions without either of them turning green.
    fn export_preset(&self) -> Option<toml::Value> {
        let mut table = toml::value::Table::new();
        table.insert(
            "size_px".to_owned(),
            toml::Value::Float(f64::from(self.size_px)),
        );
        Some(toml::Value::Table(table))
    }
    fn import_preset(&mut self, value: &toml::Value) -> bool {
        let Some(size) = value.get("size_px").and_then(toml::Value::as_float) else {
            return false;
        };
        self.size_px = (size as f32).clamp(MIN_MARK_PX, MAX_MARK_PX);
        true
    }
}

pub(super) fn glyph_size(payload: &dyn DrawingPayload) -> Option<GlyphSize> {
    payload
        .as_any()
        .downcast_ref::<MarkPayload>()
        .map(|payload| GlyphSize {
            px: payload.size_px,
            min: MIN_MARK_PX,
            max: MAX_MARK_PX,
        })
}

pub(super) fn set_glyph_size(payload: &mut dyn DrawingPayload, px: f32) {
    if let Some(payload) = payload.as_any_mut().downcast_mut::<MarkPayload>() {
        payload.size_px = px.clamp(MIN_MARK_PX, MAX_MARK_PX);
    }
}

fn size_of(ctxt: &DrawContext<'_>) -> f32 {
    ctxt.payload
        .as_any()
        .downcast_ref::<MarkPayload>()
        .map_or(DEFAULT_MARK_PX, |payload| payload.size_px)
}

/// The seven points of the arrow, tip first, in screen space.
///
/// `anchor` is the bar's extreme; the whole shape hangs off it in pixels, so
/// this is also the function that decides "outside the candle" and the one a
/// test can ask about without a chart. `inverted` mirrors the hang: the
/// anchor is a *price* extreme, and upside down the outside of the candle is
/// the other screen direction.
pub(super) fn outline(
    anchor: egui::Pos2,
    direction: Direction,
    size_px: f32,
    inverted: bool,
) -> Vec<egui::Pos2> {
    let sign = direction.sign() * if inverted { -1.0 } else { 1.0 };
    let head = size_px * HEAD_SHARE;
    let head_half = size_px * HEAD_HALF_WIDTH;
    let shaft_half = size_px * SHAFT_HALF_WIDTH;
    // Tip nearest the candle, tail pointing away from it: the mark reads as
    // an arrow *into* the bar it marks.
    let tip = egui::pos2(anchor.x, anchor.y + sign * MARK_GAP_PX);
    let shoulder = tip.y + sign * head;
    let tail = tip.y + sign * size_px;
    vec![
        tip,
        egui::pos2(anchor.x - head_half, shoulder),
        egui::pos2(anchor.x - shaft_half, shoulder),
        egui::pos2(anchor.x - shaft_half, tail),
        egui::pos2(anchor.x + shaft_half, tail),
        egui::pos2(anchor.x + shaft_half, shoulder),
        egui::pos2(anchor.x + head_half, shoulder),
    ]
}

pub(super) fn paint(
    painter: &egui::Painter,
    style: DrawingStyle,
    points: &[egui::Pos2],
    direction: Direction,
    ctxt: &DrawContext<'_>,
) {
    let Some(anchor) = points.first() else {
        return;
    };
    let outline = outline(*anchor, direction, size_of(ctxt), ctxt.scale.is_inverted());
    if ctxt.halo {
        // The halo pass paints stroke geometry only, like every other tool.
        painter.add(egui::Shape::closed_line(
            outline,
            egui::Stroke::new(style.width_px, style.color),
        ));
        return;
    }
    // Solid, not outlined: a filled silhouette reads as a mark placed on the
    // chart, where a thin outline reads as something drawn by hand.
    painter.add(egui::Shape::convex_polygon(
        outline,
        style.color,
        egui::Stroke::NONE,
    ));
}

pub(super) fn hit_test(
    points: &[egui::Pos2],
    position: egui::Pos2,
    radius_px: f32,
    direction: Direction,
    ctxt: &DrawContext<'_>,
) -> bool {
    let Some(anchor) = points.first() else {
        return false;
    };
    let size = size_of(ctxt);
    let outline = outline(*anchor, direction, size, ctxt.scale.is_inverted());
    // The bounding box of the shape, grown by the grab radius. A mark is
    // small and solid; asking a trader to hit a triangle's true edge on a
    // moving chart would make it the hardest object on the canvas to grab.
    let mut bounds = egui::Rect::from_min_max(outline[0], outline[0]);
    for point in &outline {
        bounds.extend_with(*point);
    }
    bounds.expand(radius_px).contains(position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_up_mark_hangs_below_its_anchor_and_a_down_mark_above() {
        let anchor = egui::pos2(100.0, 200.0);
        let up = outline(anchor, Direction::Up, DEFAULT_MARK_PX, false);
        let down = outline(anchor, Direction::Down, DEFAULT_MARK_PX, false);
        assert!(
            up.iter().all(|point| point.y > anchor.y),
            "an up mark clears the low it hangs from: {up:?}"
        );
        assert!(
            down.iter().all(|point| point.y < anchor.y),
            "a down mark clears the high it hangs from: {down:?}"
        );
    }

    /// Upside down the anchor's price extreme sits on the other side of the
    /// candle, so the whole glyph mirrors — without this the stamp paints
    /// straight across the bar it marks.
    #[test]
    fn an_inverted_chart_mirrors_the_hang() {
        let anchor = egui::pos2(100.0, 200.0);
        let up = outline(anchor, Direction::Up, DEFAULT_MARK_PX, true);
        let down = outline(anchor, Direction::Down, DEFAULT_MARK_PX, true);
        assert!(
            up.iter().all(|point| point.y < anchor.y),
            "inverted, the up mark hangs above the low's pixel: {up:?}"
        );
        assert!(
            down.iter().all(|point| point.y > anchor.y),
            "and the down mark below the high's: {down:?}"
        );
    }

    #[test]
    fn the_mark_never_touches_the_candle_it_is_about() {
        for direction in [Direction::Up, Direction::Down] {
            let anchor = egui::pos2(0.0, 0.0);
            let nearest = outline(anchor, direction, MIN_MARK_PX, false)
                .into_iter()
                .map(|point| (point.y - anchor.y).abs())
                .fold(f32::INFINITY, f32::min);
            assert!(
                nearest >= MARK_GAP_PX,
                "{direction:?} came within {nearest} px of the bar"
            );
        }
    }

    #[test]
    fn the_mark_is_measured_in_screen_pixels_only() {
        // The trader's rule: it does not inflate with the zoom. The only
        // input to its geometry is the chosen size, so there is nowhere for
        // a scale factor to enter.
        let anchor = egui::pos2(50.0, 50.0);
        let small = outline(anchor, Direction::Up, MIN_MARK_PX, false);
        let large = outline(anchor, Direction::Up, MAX_MARK_PX, false);
        let height = |shape: &[egui::Pos2]| {
            shape
                .iter()
                .map(|point| point.y)
                .fold(f32::NEG_INFINITY, f32::max)
                - anchor.y
        };
        assert!(height(&large) > height(&small));
        assert!((height(&small) - (MIN_MARK_PX + MARK_GAP_PX)).abs() < f32::EPSILON);
    }

    #[test]
    fn the_arrow_is_a_head_on_a_shaft() {
        let outline = outline(egui::pos2(0.0, 0.0), Direction::Up, DEFAULT_MARK_PX, false);
        assert_eq!(outline.len(), 7, "tip, two shoulders, two waists, two feet");
        let widest = outline
            .iter()
            .map(|point| point.x.abs())
            .fold(0.0_f32, f32::max);
        let narrowest = outline
            .iter()
            .skip(1)
            .map(|point| point.x.abs())
            .fold(f32::INFINITY, f32::min);
        assert!(
            widest > narrowest * 2.0,
            "the head has to read as a head against the shaft"
        );
    }
}
