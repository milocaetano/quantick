//! The strategies armed on this pane's drawings: the owner, the lifecycle
//! that keeps its badge honest, and the two views that say so.
//!
//! A pane anchors and paints; the kernel (`quantick-strategy`) judges. That
//! division is why three of [`PaneStrategies`]' four fields are queues rather
//! than state: the pane sees the closed bar and the click, but cannot reach
//! the simulator or the dialog from inside its own input pass, so it parks
//! what it saw and the tab drains it on the same frame.
//!
//! The owner never reaches into the pane. What it needs from the chart — the
//! drawing an instance rides, the closed bars a ruler warms on — arrives as a
//! borrowed [`Drawings`] or a [`PaneSeriesRead`], so the same transitions
//! serve the menu, the keyboard, the tab's ingestion sweep and a test that
//! never lays a pane out.
//!
//! The badge says what the bot riding a drawing is actually doing — armed,
//! held, or out of tape — and the lifecycle here is what makes that sentence
//! true: re-arming when the trader drags the shape back over the future,
//! sweeping the instance when the drawing dies, and queueing the cleanup so
//! no resting order outlives the object that placed it. The text is produced
//! as a `String` before it is painted, so the sentence a trader reads is the
//! sentence a test asserts.

use eframe::egui;

use crate::drawings::{self, Drawing, DrawingBand, Drawings};
use crate::state::dec_from_f64;
use crate::theme;

use super::drawing_projection::PaneSeriesRead;
use super::{PaneContextMenu, region_pause};

/// The armed strategies and the work they park for the tab. See the module
/// docs.
#[derive(Default)]
pub struct PaneStrategies {
    /// Armed strategy instances riding this pane's drawings. The kernel
    /// (`quantick-strategy`) judges; this pane only anchors and paints.
    pub anchors: crate::strategy_anchors::StrategyAnchors,
    /// Closed bars awaiting strategy evaluation, each with the slot it
    /// closed at. Pushed by `ingest_live_trade` only while instances
    /// exist, drained by the tab in the same ingestion sweep — the slot
    /// and the drawings' anchors are therefore read against one cut of
    /// the series.
    pub(super) pending: Vec<(quantick_engine::Bar, usize)>,
    /// The drawing whose "Add strategy…" was clicked; the app drains it
    /// and opens the arming dialog over this pane.
    pub(crate) popup_request: Option<drawings::DrawingId>,
    /// Simulator commands the drawing menu owes the paper host — cancelling
    /// a resting retest limit on disarm/removal. The pane cannot reach the
    /// tab's simulator from inside the menu, so the tab drains this on the
    /// same frame ([`crate::tab::TabState::apply_strategy_cleanup`]).
    pub(super) cleanup: Vec<quantick_sim::Command>,
}

impl PaneStrategies {
    /// The closed bars awaiting strategy evaluation, slot each. Drained by
    /// the tab right after the ingestion sweep that queued them.
    #[must_use]
    pub fn take_bars(&mut self) -> Vec<(quantick_engine::Bar, usize)> {
        std::mem::take(&mut self.pending)
    }

    /// Resolve the drawing an instance is anchored to into the kernel's
    /// terms, for the bar that closed at `slot`: the price band between the
    /// rectangle's anchors, and whether that slot falls inside its span of
    /// the tape. `None` when the drawing is gone, marks another market, or
    /// lost its footing on this series — a region that cannot honestly be
    /// tested holds fire.
    #[must_use]
    pub fn region(
        &self,
        drawings: &Drawings,
        id: drawings::DrawingId,
        slot: usize,
    ) -> Option<(quantick_strategy::Region, bool)> {
        let index = drawings.index_of(id)?;
        let drawing = drawings.items().get(index)?;
        // Every reason a region cannot honestly be tested — another market,
        // a lost series, a drawing nobody can see — is one rule, shared with
        // the badge that has to say so ([`region_pause`]). An order fired
        // from a region nobody can see is an invisible bot; showing the
        // drawing resumes it.
        if region_pause(drawing, drawings.all_hidden()).is_some() {
            return None;
        }
        let [a, b] = drawing.points.as_slice() else {
            return None;
        };
        let region = quantick_strategy::Region::new(dec_from_f64(a.price), dec_from_f64(b.price));
        // An extended rectangle runs to the chart's right edge until
        // further notice, and its region does too — otherwise the bot
        // silently expires at the drawn end while the band visibly keeps
        // going (the replay trap this option exists to close).
        let extend_right = drawing
            .payload
            .as_any()
            .downcast_ref::<drawings::RectanglePayload>()
            .is_some_and(|payload| payload.extend_right);
        #[allow(clippy::cast_precision_loss)]
        let slot = slot as f32;
        let active = slot >= a.bar.min(b.bar) && (extend_right || slot <= a.bar.max(b.bar));
        Some((region, active))
    }

    /// Re-arm the instance riding `drawing`, re-warming its ruler when the
    /// disarm named a *rebuilt series* (a replay seek, a bar-spec change, a
    /// market switch reset the trigger's window). Without the re-warm,
    /// "re-armed" silently means "warming up for another twenty bars" — the
    /// replay-seek trap where force bars right after a seek never fire.
    pub(crate) fn rearm(&mut self, drawing: drawings::DrawingId, series: PaneSeriesRead<'_>) {
        use quantick_strategy::ArmedState;
        let Some(instance) = self.anchors.for_drawing(drawing) else {
            return;
        };
        let series_changed = matches!(
            instance.armed.state(),
            ArmedState::Disarmed { reason } if reason.resets_series()
        );
        if let Some(instance) = self.anchors.for_drawing_mut(drawing) {
            instance.armed.rearm();
        }
        if series_changed {
            self.rewarm_trigger(drawing, series);
        }
    }

    /// Feed the last `warmup_bars` closed bars of the live series back into
    /// an instance's trigger — the arm-time warmup, repeated after a rearm
    /// whose disarm reset the ruler. Venue-prefix candles are excluded for
    /// the same reason as at arm time: they measure another ruler entirely.
    fn rewarm_trigger(&mut self, id: drawings::DrawingId, series: PaneSeriesRead<'_>) {
        let Some(instance) = self.anchors.for_drawing(id) else {
            return;
        };
        let bars = series.warmup_bars(instance.armed.trigger().warmup_bars());
        let Some(instance) = self.anchors.for_drawing_mut(id) else {
            return;
        };
        instance.armed.warm(&bars);
    }

    /// Sweep instances whose drawing no longer exists — for the deletion
    /// paths that cannot call [`Self::remove_for_drawing`] with an id in
    /// hand (delete-all, undo, redo), so no path leaves a resting bot order
    /// with no badge over it. Cleanup is queued for the tab's same-frame
    /// drain like the menu's.
    pub(crate) fn sweep_orphans(&mut self, drawings: &Drawings) {
        if self.anchors.is_empty() {
            return;
        }
        let alive: Vec<drawings::DrawingId> = self
            .anchors
            .instances
            .iter()
            .map(|instance| instance.drawing)
            .filter(|id| drawings.index_of(*id).is_some())
            .collect();
        let cleanup = self.anchors.drop_orphans(|id| alive.contains(&id));
        self.cleanup.extend(cleanup);
    }

    /// Drain the cleanup commands the drawing menu queued; the tab applies
    /// them to the paper host on this same frame.
    #[must_use]
    pub fn take_cleanup(&mut self) -> Vec<quantick_sim::Command> {
        std::mem::take(&mut self.cleanup)
    }

    /// Remove the instance riding `drawing` and queue the sweep of its
    /// pending entry — every "the bot dies with its drawing" path funnels
    /// through here so none of them can orphan a resting retest limit.
    pub(crate) fn remove_for_drawing(&mut self, drawing: drawings::DrawingId) {
        let cleanup = self.anchors.remove_for_drawing(drawing);
        self.cleanup.extend(cleanup);
    }

    /// The strategy seat of the per-drawing menu: arm a bot on this region,
    /// or manage the one riding it. Price-band rectangles only — the one
    /// shape whose two anchors honestly bound a price region today.
    ///
    /// `menu` is the context menu the entries are drawn into; under test it
    /// records where each button landed so a test can click the real widget.
    pub(super) fn draw_menu_entries(
        &mut self,
        ui: &mut egui::Ui,
        drawings: &Drawings,
        index: usize,
        series: PaneSeriesRead<'_>,
        menu: &mut PaneContextMenu,
    ) {
        #[cfg(not(test))]
        let _ = &menu;
        let drawing = &drawings.items()[index];
        if drawing.tool.id() != drawings::RECTANGLE_TOOL_ID || drawing.band != DrawingBand::Price {
            return;
        }
        let id = drawing.id;
        let Some(instance) = self.anchors.for_drawing(id) else {
            let add = ui.button("Add strategy…").on_hover_text(
                "arm a strategy on this region: it fires on the trigger bar, in paper trading",
            );
            #[cfg(test)]
            menu.menu_rects.push(("Add strategy", add.rect));
            if add.clicked() {
                self.popup_request = Some(id);
                ui.close_menu();
            }
            return;
        };
        // One line of truth about the bot on this drawing, then its verbs.
        ui.label(
            egui::RichText::new(instance.armed.status_line())
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        let state = instance.armed.state().clone();
        use quantick_strategy::{ArmedState, DisarmReason};
        match state {
            // One Disarm arm for every state that can be called off — a
            // resting retest limit included (it can wait for hours). Only
            // the hover varies; the cleanup plumbing must never fork.
            ArmedState::Armed | ArmedState::InPosition | ArmedState::Fired { retest: true, .. } => {
                let hover = if matches!(state, ArmedState::Fired { .. }) {
                    "cancel the resting retest limit and stop watching"
                } else {
                    "stop watching; an open operation keeps its position and bracket — yours to manage"
                };
                let disarm = ui.button("Disarm").on_hover_text(hover);
                #[cfg(test)]
                menu.menu_rects.push(("Disarm", disarm.rect));
                if disarm.clicked()
                    && let Some(instance) = self.anchors.for_drawing_mut(id)
                {
                    let cleanup = instance.armed.disarm(DisarmReason::User);
                    self.cleanup.extend(cleanup);
                    ui.close_menu();
                }
            }
            ArmedState::Done | ArmedState::Disarmed { .. } => {
                // A drawing with no footing on this market/series cannot be
                // honestly re-armed — and neither can a region whose drawn
                // span already ended: the instance would show "armed" while
                // the region test refuses it forever, the silent halt the
                // named disarms exist to prevent.
                let footed = region_pause(drawing, drawings.all_hidden()).is_none();
                let span_alive = region_can_fire(drawing, series.closed_slots());
                let rearm = ui
                    .add_enabled(footed && span_alive, egui::Button::new("Re-arm"))
                    .on_hover_text("watch this region again with the same parameters")
                    .on_disabled_hover_text(if footed {
                        "the region ends before the next bar — stretch it right, or turn on \
                         \"extend right\" in its Region settings"
                    } else {
                        "this drawing belongs to another market or lost its series — redraw the \
                         region here first"
                    });
                #[cfg(test)]
                menu.menu_rects.push(("Re-arm", rearm.rect));
                if rearm.clicked() {
                    self.rearm(id, series);
                    ui.close_menu();
                }
            }
            // A market entry lives for exactly one print; nothing to offer.
            ArmedState::Fired { retest: false, .. } => {}
        }
        let remove = ui.button("Remove strategy").on_hover_text(
            "detach the bot from this drawing; an open operation keeps its position and bracket",
        );
        #[cfg(test)]
        menu.menu_rects.push(("Remove strategy", remove.rect));
        if remove.clicked() {
            self.remove_for_drawing(id);
            ui.close_menu();
        }
    }
}

pub(crate) fn region_can_fire(drawing: &Drawing, closed_slots: usize) -> bool {
    let extend_right = drawing
        .payload
        .as_any()
        .downcast_ref::<drawings::RectanglePayload>()
        .is_some_and(|payload| payload.extend_right);
    if extend_right {
        return true;
    }
    let [a, b] = drawing.points.as_slice() else {
        return false;
    };
    #[allow(clippy::cast_precision_loss)]
    let next_slot = closed_slots as f32;
    a.bar.max(b.bar) >= next_slot
}
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
    let Some(first) = points.first() else {
        return;
    };
    let anchor = points.iter().fold(*first, |corner, point| {
        egui::pos2(corner.x.min(point.x), corner.y.min(point.y))
    });
    use quantick_strategy::ArmedState;
    let color = match instance.armed.state() {
        ArmedState::Armed => theme::ACCENT,
        ArmedState::Fired { .. } => theme::AMBER,
        ArmedState::InPosition => theme::BUY,
        ArmedState::Done => theme::TEXT_MUTED,
        ArmedState::Disarmed { .. } => theme::TEXT_FAINT,
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
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(BADGE_FONT_PX), color);
    let rect = egui::Rect::from_min_size(
        position - egui::vec2(BADGE_PAD_X_PX, galley.size().y + BADGE_PAD_Y_PX + 1.0),
        galley.size() + egui::vec2(2.0 * BADGE_PAD_X_PX, 2.0 * BADGE_PAD_Y_PX),
    );
    painter.rect_filled(
        rect,
        BADGE_CORNER_PX,
        theme::CANVAS.gamma_multiply(BADGE_GROUND_ALPHA),
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
