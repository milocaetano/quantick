//! The strategies armed on this pane's drawings, as the chart sees them: the
//! region a drawing marks and the badge that says what the bot riding it is
//! doing. The gate a drawing must pass to be armed is
//! `quantick_drawings::alert_geometry::check_arm`.
//!
//! The instances themselves are the tab's
//! ([`crate::strategy_anchors::TabStrategies`], keyed by pane id); a pane
//! anchors and paints, the kernel (`quantick-strategy`) judges.
//!
//! The badge says what the bot riding a drawing is actually doing — armed,
//! held, or out of tape. The text is produced as a `String` before it is
//! painted, so the sentence a trader reads is the sentence a test asserts.

use eframe::egui;

use crate::drawings::{self, Drawing, Drawings};
use crate::state::dec_from_f64;
use crate::theme;

use super::region_pause;

/// Resolve the drawing an instance is anchored to into the kernel's
/// terms, for the bar that closed at `slot`: the price band between the
/// rectangle's anchors, and whether that slot falls inside its span of
/// the tape. `None` when the drawing is gone, marks another market, or
/// lost its footing on this series — a region that cannot honestly be
/// tested holds fire.
///
/// The one name both the closed-bar and the forming-bar paths resolve by.
#[must_use]
pub fn drawing_region(
    drawings: &Drawings,
    id: drawings::DrawingId,
    slot: usize,
) -> Option<(quantick_strategy::Region, bool)> {
    let (a, b, active) = quantick_drawings::alert_geometry::region_at(drawings, id, slot)?;
    Some((
        quantick_strategy::Region::new(dec_from_f64(a), dec_from_f64(b)),
        active,
    ))
}

pub(crate) use quantick_drawings::alert_geometry::region_can_fire;

pub(super) fn badge_text_for(
    instance: &crate::strategy_anchors::AnchoredInstance,
    drawing: &Drawing,
    all_hidden: bool,
    closed_slots: usize,
) -> String {
    let mut text = crate::strategy_anchors::badge_text(instance);
    // The region's own state first, and *instead of* the kernel's
    // reason rather than beside it. A paused or expired region makes
    // `PaneStrategies::region` refuse, which the kernel records as "region
    // not active on this bar" — true of the span, and a lie about a band
    // that is merely hidden. Two vocabularies for one fact leave the
    // trader deciding which clause to believe; the specific one wins,
    // and it is the only one carrying a way out.
    let armed = matches!(instance.armed.state(), quantick_strategy::ArmedState::Armed);
    if let Some(pause) = region_pause(drawing, all_hidden) {
        text.push_str(" · ");
        text.push_str(pause);
        return text;
    }
    if armed && !region_can_fire(drawing, closed_slots) {
        text.push_str(" · region ended — stretch it right");
        return text;
    }
    // Otherwise the gate that actually decided, in the words that fit a
    // corner — and never present-tense about a bar it is not about.
    // This is the whole point of the badge and it was reaching only the
    // right-click menu: the trader watches the chart, and "why did
    // nothing happen" is answerable only where they are already looking.
    let held = instance.armed.hold_reason();
    // A gate that refused *this* bar is the whole answer, and it stands
    // alone.
    if let Some(held) = held.filter(|held| held.fresh) {
        text.push_str(" · ");
        text.push_str(held.reason);
        return text;
    }
    // Otherwise the ruler is what decided this bar, and its reading is
    // the only sentence here about the candle in front of the trader.
    // `status_line` has always led with it and the right-click menu
    // prints it; this badge did not, so a bar the ruler held showed an
    // older bar's refusal and nothing about its own — the divergence
    // `region_pause` above exists to end, found again the same way.
    if armed {
        text.push_str(" · ");
        text.push_str(&instance.armed.trigger().status());
    }
    if let Some(held) = held {
        text.push_str(" · last held: ");
        text.push_str(held.reason);
    }
    text
}
pub(super) fn paint_strategy_badge(
    painter: &egui::Painter,
    instance: &crate::strategy_anchors::AnchoredInstance,
    drawing: &Drawing,
    points: &[egui::Pos2],
    all_hidden: bool,
    closed_slots: usize,
) {
    let palette = theme::get(painter.ctx());
    let Some(first) = points.first() else {
        return;
    };
    let anchor = points.iter().fold(*first, |corner, point| {
        egui::pos2(corner.x.min(point.x), corner.y.min(point.y))
    });
    use quantick_strategy::ArmedState;
    let color = match instance.armed.state() {
        ArmedState::Armed => palette.accent(),
        ArmedState::Fired { .. } => palette.amber(),
        ArmedState::InPosition => palette.buy(),
        ArmedState::Done => palette.text_muted(),
        ArmedState::Disarmed { .. } => palette.text_faint(),
    };
    /// Badge label size — the small-annotation size the band chips use.
    const BADGE_FONT_PX: f32 = 11.0;
    /// Ground padding around the label, and the gap that lifts the
    /// badge off the drawing's top-left corner.
    const BADGE_PAD_X_PX: f32 = 3.0;
    const BADGE_PAD_Y_PX: f32 = 2.0;
    const BADGE_LIFT_PX: f32 = 4.0;
    const BADGE_CORNER_PX: f32 = 3.0;
    /// Ground opacity: readable over candles, still a whisper.
    const BADGE_GROUND_ALPHA: f32 = 0.85;
    let text = badge_text_for(instance, drawing, all_hidden, closed_slots);
    let position = anchor + egui::vec2(BADGE_PAD_X_PX - 1.0, -BADGE_LIFT_PX);
    // A whisper of ground behind the label so it stays readable over
    // candles; galley first, box after, text last.
    let galley = painter.layout_no_wrap(text, palette.label_font(BADGE_FONT_PX), color);
    let rect = egui::Rect::from_min_size(
        position - egui::vec2(BADGE_PAD_X_PX, galley.size().y + BADGE_PAD_Y_PX + 1.0),
        galley.size() + egui::vec2(2.0 * BADGE_PAD_X_PX, 2.0 * BADGE_PAD_Y_PX),
    );
    painter.rect_filled(
        rect,
        BADGE_CORNER_PX,
        palette.canvas().gamma_multiply(BADGE_GROUND_ALPHA),
    );
    painter.galley(
        rect.min + egui::vec2(BADGE_PAD_X_PX, BADGE_PAD_Y_PX),
        galley,
        color,
    );
}

#[cfg(test)]
pub(crate) fn strategy_badge_text(
    anchors: &crate::strategy_anchors::StrategyAnchors,
    drawings: &Drawings,
    id: drawings::DrawingId,
    closed_slots: usize,
) -> String {
    let Some(instance) = anchors.for_drawing(id) else {
        return String::new();
    };
    let Some(index) = drawings.index_of(id) else {
        return String::new();
    };
    badge_text_for(
        instance,
        &drawings.items()[index],
        drawings.all_hidden(),
        closed_slots,
    )
}
