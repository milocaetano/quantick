//! The badge over a strategy's drawing, and the lifecycle that keeps it
//! honest.
//!
//! Two halves of one promise. The badge says what the bot riding a drawing is
//! actually doing — armed, held, or out of tape — and the lifecycle below is
//! what makes that sentence true: re-arming when the trader drags the shape
//! back over the future, sweeping the instance when the drawing dies, and
//! queueing the cleanup so no resting order outlives the object that placed
//! it. A badge is only worth painting if the state behind it is swept, which
//! is why the painter and the sweep share a file.
//!
//! The text is produced as a `String` before it is painted, so the sentence a
//! trader reads is the sentence a test asserts.

use eframe::egui;

use crate::drawings::{self, DrawingBand};
use crate::theme;

use super::{ChartPane, region_pause};

impl ChartPane {
    /// What the badge over `drawing` says, as a value.
    ///
    /// The instance's own half ([`crate::strategy_anchors::badge_text`])
    /// plus the two things it cannot know, because they are facts about the
    /// *drawing* rather than about the strategy: a region nobody can
    /// honestly test ([`region_pause`]), and a drawn span that no longer
    /// reaches the next bar. Both shut the order and the alarm together, so
    /// both owe the trader a word — a badge reading a bare "armed" over a
    /// bot that has been held for an hour is the chart lying about the one
    /// thing this badge exists to say.
    ///
    /// Neither is a disarm. The trader moves the rectangle all session; a
    /// band dragged back over the future starts firing again on the next
    /// bar, with no button to press, and the alarm never went quiet.
    ///
    /// A `String` rather than paint, so the sentence a trader reads is the
    /// sentence a test asserts and a reader that is not looking at the
    /// screen can obtain. The painter below is one consumer of it.
    #[must_use]
    pub(crate) fn badge_text_for(
        &self,
        instance: &crate::strategy_anchors::AnchoredInstance,
        drawing: &drawings::Drawing,
    ) -> String {
        let mut text = crate::strategy_anchors::badge_text(instance);
        // The region's own state first, and *instead of* the kernel's
        // reason rather than beside it. A paused or expired region makes
        // `strategy_region` refuse, which the kernel records as "region not
        // active on this bar" — true of the span, and a lie about a band
        // that is merely hidden. Two vocabularies for one fact leave the
        // trader deciding which clause to believe; the specific one wins,
        // and it is the only one carrying a way out.
        let armed = matches!(instance.armed.state(), quantick_strategy::ArmedState::Armed);
        if let Some(pause) = region_pause(drawing, self.drawings.all_hidden()) {
            text.push_str(" · ");
            text.push_str(pause);
            return text;
        }
        if armed && !self.strategy_region_can_fire(drawing.id) {
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

    /// The badge over the drawing with this id — the lookup half of
    /// [`Self::badge_text_for`], which the painter reaches directly because
    /// it already holds both.
    ///
    /// Test-only, and gated so it cannot drift into the shipped binary: the
    /// sentence the trader reads is worth asserting, and the id is what a
    /// test has in hand. The production path a reader that is not looking
    /// at the screen would need is the control plane's scene, which does
    /// not carry armed instances yet — filed rather than widened here.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn strategy_badge_text(&self, id: drawings::DrawingId) -> String {
        let Some(instance) = self.strategies.for_drawing(id) else {
            return String::new();
        };
        let Some(index) = self.drawings.index_of(id) else {
            return String::new();
        };
        self.badge_text_for(instance, &self.drawings.items()[index])
    }

    /// The armed instance's badge, pinned to its drawing's top-left corner:
    /// state at a glance, in the state's colour. Per frame this is one
    /// bounding-box fold and one text draw per *armed* drawing — a handful
    /// at most, and nothing at all on a chart with no instances.
    pub(super) fn paint_strategy_badge(
        &self,
        painter: &egui::Painter,
        instance: &crate::strategy_anchors::AnchoredInstance,
        drawing: &drawings::Drawing,
        points: &[egui::Pos2],
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
        let text = self.badge_text_for(instance, drawing);
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

    /// The strategy seat of the per-drawing menu: arm a bot on this region,
    /// or manage the one riding it. Price-band rectangles only — the one
    /// shape whose two anchors honestly bound a price region today.
    pub(super) fn draw_strategy_menu_entries(&mut self, ui: &mut egui::Ui, index: usize) {
        let drawing = &self.drawings.items()[index];
        if drawing.tool.id() != drawings::RECTANGLE_TOOL_ID || drawing.band != DrawingBand::Price {
            return;
        }
        let id = drawing.id;
        let Some(instance) = self.strategies.for_drawing(id) else {
            let add = ui.button("Add strategy…").on_hover_text(
                "arm a strategy on this region: it fires on the trigger bar, in paper trading",
            );
            #[cfg(test)]
            self.gestures.menu_rects.push(("Add strategy", add.rect));
            if add.clicked() {
                self.strategy_popup_request = Some(id);
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
                self.gestures.menu_rects.push(("Disarm", disarm.rect));
                if disarm.clicked()
                    && let Some(instance) = self.strategies.for_drawing_mut(id)
                {
                    let cleanup = instance.armed.disarm(DisarmReason::User);
                    self.strategy_cleanup.extend(cleanup);
                    ui.close_menu();
                }
            }
            ArmedState::Done | ArmedState::Disarmed { .. } => {
                // A drawing with no footing on this market/series cannot be
                // honestly re-armed — and neither can a region whose drawn
                // span already ended: the instance would show "armed" while
                // the region test refuses it forever, the silent halt the
                // named disarms exist to prevent.
                let footed = {
                    let drawing = &self.drawings.items()[index];
                    region_pause(drawing, self.drawings.all_hidden()).is_none()
                };
                let span_alive = self.strategy_region_can_fire(id);
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
                self.gestures.menu_rects.push(("Re-arm", rearm.rect));
                if rearm.clicked() {
                    self.rearm_strategy_for_drawing(id);
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
        self.gestures
            .menu_rects
            .push(("Remove strategy", remove.rect));
        if remove.clicked() {
            self.remove_strategy_for_drawing(id);
            ui.close_menu();
        }
    }

    /// Re-arm the instance riding `drawing`, re-warming its ruler when the
    /// disarm named a *rebuilt series* (a replay seek, a bar-spec change, a
    /// market switch reset the trigger's window). Without the re-warm,
    /// "re-armed" silently means "warming up for another twenty bars" — the
    /// replay-seek trap where force bars right after a seek never fire.
    pub(crate) fn rearm_strategy_for_drawing(&mut self, drawing: drawings::DrawingId) {
        use quantick_strategy::ArmedState;
        let Some(instance) = self.strategies.for_drawing(drawing) else {
            return;
        };
        let series_changed = matches!(
            instance.armed.state(),
            ArmedState::Disarmed { reason } if reason.resets_series()
        );
        if let Some(instance) = self.strategies.for_drawing_mut(drawing) {
            instance.armed.rearm();
        }
        if series_changed {
            self.rewarm_strategy_trigger(drawing);
        }
    }

    /// Feed the last `warmup_bars` closed bars of the live series back into
    /// an instance's trigger — the arm-time warmup, repeated after a rearm
    /// whose disarm reset the ruler. Venue-prefix candles are excluded for
    /// the same reason as at arm time: they measure another ruler entirely.
    fn rewarm_strategy_trigger(&mut self, id: drawings::DrawingId) {
        let Some(instance) = self.strategies.for_drawing(id) else {
            return;
        };
        let bars = self.strategy_warmup_bars(instance.armed.trigger().warmup_bars());
        let Some(instance) = self.strategies.for_drawing_mut(id) else {
            return;
        };
        instance.armed.warm(&bars);
    }

    /// Whether the drawing's drawn span can still cover a future closed
    /// bar — the liveness half of [`Self::strategy_region`]'s `active`
    /// test, shared by arming, re-arming and the menu so the three cannot
    /// drift. The next bar to close lands at slot `closed_slots()`, so an
    /// unextended region needs its right anchor at or past that slot; an
    /// extended one never expires right.
    pub(crate) fn strategy_region_can_fire(&self, id: drawings::DrawingId) -> bool {
        let Some(index) = self.drawings.index_of(id) else {
            return false;
        };
        let drawing = &self.drawings.items()[index];
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
        let next_slot = self.closed_slots() as f32;
        a.bar.max(b.bar) >= next_slot
    }

    /// Sweep instances whose drawing no longer exists — for the deletion
    /// paths that cannot call [`Self::remove_strategy_for_drawing`] with an
    /// id in hand (delete-all, undo, redo), so no path leaves a resting bot
    /// order with no badge over it. Cleanup is queued for the tab's
    /// same-frame drain like the menu's.
    pub(crate) fn sweep_strategy_orphans(&mut self) {
        if self.strategies.is_empty() {
            return;
        }
        let alive: Vec<drawings::DrawingId> = self
            .strategies
            .instances
            .iter()
            .map(|instance| instance.drawing)
            .filter(|id| self.drawings.index_of(*id).is_some())
            .collect();
        let cleanup = self.strategies.drop_orphans(|id| alive.contains(&id));
        self.strategy_cleanup.extend(cleanup);
    }

    /// The last `want` closed bars of the live series — never venue-prefix
    /// candles, whose bodies measure another ruler — for warming a strategy
    /// trigger at arm or re-arm time.
    pub fn strategy_warmup_bars(&self, want: usize) -> Vec<quantick_engine::Bar> {
        let slots = self.slots();
        let first_live = self.seam_slot();
        (slots.saturating_sub(want).max(first_live)..slots)
            .filter_map(|slot| self.closed_bar(slot).cloned())
            .collect()
    }

    /// Drain the cleanup commands the drawing menu queued; the tab applies
    /// them to the paper host on this same frame.
    #[must_use]
    pub fn take_strategy_cleanup(&mut self) -> Vec<quantick_sim::Command> {
        std::mem::take(&mut self.strategy_cleanup)
    }

    /// Remove the instance riding `drawing` and queue the sweep of its
    /// pending entry — every "the bot dies with its drawing" path funnels
    /// through here so none of them can orphan a resting retest limit.
    pub(crate) fn remove_strategy_for_drawing(&mut self, drawing: drawings::DrawingId) {
        let cleanup = self.strategies.remove_for_drawing(drawing);
        self.strategy_cleanup.extend(cleanup);
    }
}
