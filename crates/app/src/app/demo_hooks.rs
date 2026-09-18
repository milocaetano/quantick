//! The harness demo appliers: the launch hooks that stage a state a
//! screenshot needs and no click can reach.
//!
//! The gated drawing owner captures its requests at executable startup and
//! chooses geometry and consumption. Its adapters below project chosen slot
//! times and invoke normal drawing, canvas and history operations. Remaining
//! legacy scenarios still read through [`crate::harness`], with evidence
//! capture configured by the app constructor. The frame keeps each existing
//! phase in order, including the history phases between drawing scenarios.

#[cfg(any(feature = "control-harness", test))]
use eframe::egui;

use crate::drawings;
use crate::harness::{StrategyDemoMode, VenueHistoryDemo};
use crate::pane;

use super::QuantickApp;
#[cfg(any(feature = "drawing-harness", test))]
use crate::pane::ChartPane;
#[cfg(any(feature = "drawing-harness", test))]
use crate::surfaces::drawing_chrome::demo::{self, ProfileFacts, ProfilePreparation, SeriesFacts};
#[cfg(any(feature = "drawing-harness", test))]
use crate::tab::CanvasLayout;

impl QuantickApp {
    /// The `QUANTICK_CONTROL_EVIDENCE` hook: capture one evidence bundle
    /// through the very read a connected client calls.
    ///
    /// The value is a comma-separated list of tokens: `all` (or `1`) means
    /// every registered scope the configured grant already reaches,
    /// `screenshot` asks for the window to be rasterised as well, and
    /// anything else is a snapshot scope ID. The manifest is logged, so a
    /// scripted validation run reads what the bundle covered — and what it
    /// did not — without a client on the socket.
    ///
    /// A capture that asked for an image waits for it: the window is asked to
    /// rasterise and the hook takes the next frame, giving up after
    /// [`super::control_host::CONTROL_EVIDENCE_HOOK_FRAMES`] rather than hanging a capture run on a
    /// surface that never presents.
    #[cfg(any(feature = "control-harness", test))]
    pub(super) fn apply_control_evidence_hook(&mut self, ctx: &egui::Context) {
        if !self.control.scenarios.has_evidence() {
            return;
        }
        // Access is taken *before* the request is, so a frame that finds it
        // borrowed leaves the hook pending rather than dropping it silently.
        let Some(mut access) = self.control.control_access.take() else {
            return;
        };
        let capture = self
            .control
            .scenarios
            .prepare_evidence(&access)
            .expect("the hook was pending one line above");
        if capture.screenshot_not_granted {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_HOOK_SCREENSHOT_NOT_GRANTED",
                "QUANTICK_CONTROL_EVIDENCE asked for an image without observe.screenshot; capturing without one");
        }
        if capture.wants_screenshot {
            access.service_screenshot(self, ctx);
        }
        let (scopes, wants_screenshot) = match self
            .control
            .scenarios
            .finish_evidence(capture, access.has_screenshot())
        {
            super::control_host::EvidenceStep::Waiting => {
                ctx.request_repaint();
                self.control.control_access = Some(access);
                return;
            }
            super::control_host::EvidenceStep::Capture {
                scopes,
                screenshot,
                image_timed_out,
            } => {
                if image_timed_out {
                    tracing::warn!(
                        target: "quantick::control",
                        event_code = "CONTROL_EVIDENCE_HOOK_GAVE_UP_ON_IMAGE",
                        frames = super::control_host::CONTROL_EVIDENCE_HOOK_FRAMES,
                        "the window never delivered a frame to rasterise; capturing without one");
                }
                (scopes, screenshot)
            }
        };
        let outcome = access.invoke_local_read(
            self,
            "evidence.capture",
            serde_json::json!({ "scopes": scopes, "screenshot": wants_screenshot }),
        );
        self.control.control_access = Some(access);
        match outcome {
            Ok(manifest) => tracing::info!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_CAPTURED",
                evidence_id = %manifest["evidence_id"].as_str().unwrap_or_default(),
                content_digest = %manifest["content_digest"].as_str().unwrap_or_default(),
                encoded_bytes = %manifest["encoded_bytes"].as_str().unwrap_or_default(),
                chunk_count = manifest["chunk_count"].as_u64().unwrap_or_default(),
                captured_scopes = manifest["source_scopes"].as_array().map_or(0, Vec::len),
                omitted_scopes = manifest["coverage"]["omitted_scopes"].as_array().map_or(0, Vec::len),
                not_captured = manifest["coverage"]["not_captured"].as_array().map_or(0, Vec::len),
                unavailable_fields = manifest["coverage"]["unavailable_fields"].as_array().map_or(0, Vec::len),
                screenshot = !manifest["screenshot"].is_null(),
                "an evidence bundle was captured through the control plane"
            ),
            Err(error) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_HOOK_REFUSED",
                error_code = %error.code,
                error = %error.message,
                "QUANTICK_CONTROL_EVIDENCE could not capture a bundle"
            ),
        }
    }

    /// The `QUANTICK_VENUE_HISTORY_DEMO` hook: a venue candle prefix in front
    /// of the bars cut from prints, delivered through the very path a feed's
    /// reply takes, so what is photographed is the real seam and the real
    /// loading state rather than a picture of them.
    ///
    /// `partial` stops one slice short: the prefix is installed and the run is
    /// left open, which is the mid-load frame progressive delivery exists to
    /// produce and the one no capture could otherwise catch.
    pub(super) fn apply_venue_history_demo(&mut self) {
        /// Candles the venue-history scene installs: enough for the seam and
        /// the divider to read, few enough to stay one screenful of context.
        const DEMO_PREFIX_CANDLES: i64 = 90;
        let Some(demo) = self.harness.venue_history_demo() else {
            return;
        };
        let tab = self.active_tab_mut();
        // Wait for bars to sit the prefix in front of: a seam needs both sides.
        if tab.flow_pane.state.bars().len() < 12 {
            return;
        }
        self.harness.venue_history_demo_staged();
        let slice = match demo {
            VenueHistoryDemo::Complete => quantick_feed::OhlcvSlice::Last { complete: true },
            VenueHistoryDemo::Partial => quantick_feed::OhlcvSlice::More,
        };
        self.deliver_synthetic_prefix(DEMO_PREFIX_CANDLES, slice);
    }

    /// Deliver `candles` synthetic venue candles for the minutes immediately
    /// before the first engine bar, so the prefix meets the tape without
    /// overlapping it.
    ///
    /// The wiggle is a fixed function of the minute — the capture has to be
    /// the same picture every run, or a visual diff means nothing. One
    /// generator, shared by every hook that needs a prefix: a second one would
    /// be a second history to keep honest.
    /// Returns whether the candles were delivered: without a bar to sit them
    /// in front of there is no seam to anchor on, and a caller that promised a
    /// long history in its caption had better wait rather than photograph a
    /// short one.
    pub(super) fn deliver_synthetic_prefix(
        &mut self,
        candles: i64,
        slice: quantick_feed::OhlcvSlice,
    ) -> bool {
        let tab_id = self.tabs.active_id();
        let tab = self.active_tab_mut();
        let Some(first) = tab.flow_pane.state.bars().first() else {
            return false;
        };
        let interval = quantick_feed::OHLCV_BASE_INTERVAL_MS;
        let bars = synthetic_prefix(first.open_time, first.open, candles);
        tab.deliver_ohlcv_slice(tab_id, interval, bars, slice);
        true
    }

    /// The `QUANTICK_STRATEGY_DEMO` hook: a named rectangle over the recent
    /// tape with a force-bar instance armed on it (`1`), the arming dialog
    /// open over it (`popup`), that dialog with the **alarm section
    /// unfolded** (`alarm`), or an **alarm-only** instance wearing a
    /// standing preview mark (`alarm-badge`). The rectangle spans the
    /// visible middle of the chart so `QUANTICK_CONTEXT_MENU=chart`'s centre
    /// click lands on it and opens the per-drawing menu. Consumed once the
    /// chart has bars enough, like the drawings demo.
    pub(super) fn apply_strategy_demo(&mut self) {
        let Some(mode) = self.harness.strategy_demo() else {
            return;
        };
        /// Fewest bars before the demo stages: enough for the shipped
        /// 20-body window to be warm and the rectangle to have tape to span.
        const DEMO_STRATEGY_MIN_SLOTS: usize = 25;
        /// How far back of the newest bar the rectangle's left edge sits.
        const DEMO_STRATEGY_LOOKBACK_BARS: usize = 40;
        /// How far past the newest bar its right edge reaches, keeping the
        /// region alive into the future like a stretched hand-drawn one.
        const DEMO_STRATEGY_AHEAD_BARS: f32 = 6.0;
        /// Half-height of the region, as a fraction of the newest close.
        const DEMO_STRATEGY_BAND_FRACTION: f64 = 0.03;
        // The newest *closed* bar anchors the demo: `slots()` counts the
        // forming partial too, whose `closed_bar` is `None` on almost every
        // frame — bailing on it must keep the flag armed for the next
        // frame, or the hook silently stages nothing.
        let closed = self.active_tab_mut().flow_pane.closed_slots();
        if closed < DEMO_STRATEGY_MIN_SLOTS {
            return;
        }
        let Some(rectangle) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
        else {
            // No rectangle in the registry: staging can never succeed, so
            // the flag is consumed rather than retried forever.
            self.harness.strategy_demo_staged();
            return;
        };
        let drawing_id = {
            let pane = &mut self.active_tab_mut().flow_pane;
            let newest = closed - 1;
            let Some(close) = pane
                .closed_bar(newest)
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            else {
                return;
            };
            let start = newest.saturating_sub(DEMO_STRATEGY_LOOKBACK_BARS);
            #[allow(clippy::cast_precision_loss)]
            let anchors = [
                drawings::ChartPoint::at_time(
                    start as f32,
                    close * (1.0 - DEMO_STRATEGY_BAND_FRACTION),
                    pane.slot_open_time(start),
                ),
                // Past the newest bar no market time exists to name; the
                // anchor carries none, like a hand-dropped one would.
                drawings::ChartPoint::at_time(
                    newest as f32 + DEMO_STRATEGY_AHEAD_BARS,
                    close * (1.0 + DEMO_STRATEGY_BAND_FRACTION),
                    None,
                ),
            ];
            for point in anchors {
                pane.drawings
                    .place_with(rectangle, &drawings::DrawingBand::Price, point, |tool| {
                        drawings::NewDrawing {
                            style: drawings::DrawingStyle::default(),
                            payload: tool.default_payload(),
                        }
                    });
            }
            let index = pane.drawings.items().len().saturating_sub(1);
            pane.drawings.rename_at(index, "demo região");
            pane.drawings.items()[index].id
        };
        // Staged: the rectangle exists, so the hook is consumed.
        self.harness.strategy_demo_staged();
        let mut form =
            crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
        // The alarm scenes tick the checkbox the trader would tick, and the
        // share gate under it, so the section is unfolded with every control
        // it owns on screen.
        if matches!(
            mode,
            StrategyDemoMode::AlarmPopup
                | StrategyDemoMode::AlarmSounds
                | StrategyDemoMode::AlarmBadge
        ) {
            form.alarm = true;
            form.alarm_when = "share".to_owned();
            form.alarm_repeat = "cooldown".to_owned();
            // A library clip with a cut, so the row that exists only for a
            // clip is on screen too. The first standard clip, whatever it
            // is called: a name here would go stale the day a clip is
            // renamed, and the scene would silently photograph the
            // system-sound caveat instead.
            let clip = crate::audio::AlertSound::in_category(crate::audio::SoundCategory::Standard)
                .next()
                .expect("the shipped library has a standard clip");
            form.alarm_sound = clip.token().to_owned();
            form.alarm_play_secs = Some(crate::strategy_presets::DEFAULT_ALARM_PLAY_SECS);
        }
        match mode {
            StrategyDemoMode::Armed => {
                let _ = self
                    .tabs
                    .runtime_mut(self.tabs.active_index())
                    .arm_strategy_instance(
                        &mut *self.audio.alerts,
                        pane::PaneSide::Flow,
                        drawing_id,
                        &form,
                        "demo BF".to_owned(),
                    );
            }
            StrategyDemoMode::AlarmBadge => {
                form.alarm_only = true;
                let _ = self
                    .tabs
                    .runtime_mut(self.tabs.active_index())
                    .arm_strategy_instance(
                        &mut *self.audio.alerts,
                        pane::PaneSide::Flow,
                        drawing_id,
                        &form,
                        "demo alarm".to_owned(),
                    );
                // Stand a provisional judgement on the badge. The mark is
                // the surface under test, and the tape reaches it only when
                // a force bar happens to be half-formed — so the scene
                // stages the mark itself rather than waiting for a market
                // that may not oblige before the shutter.
                let pane = self.active_tab_mut().pane_mut(pane::PaneSide::Flow);
                if let Some(instance) = pane.strategies.anchors.for_drawing_mut(drawing_id) {
                    instance.mark = crate::strategy_anchors::AlarmMark::Preview;
                }
                // Placing a drawing selects it, and a selected drawing raises
                // the context bar across its own top edge — which is where
                // the badge this scene exists to photograph sits. Drop the
                // selection so the badge is the thing on screen.
                pane.drawings.select(None);
            }
            StrategyDemoMode::EndedBadge | StrategyDemoMode::PausedBadge => {
                let _ = self
                    .tabs
                    .runtime_mut(self.tabs.active_index())
                    .arm_strategy_instance(
                        &mut *self.audio.alerts,
                        pane::PaneSide::Flow,
                        drawing_id,
                        &form,
                        "demo BF".to_owned(),
                    );
                let pane = self.active_tab_mut().pane_mut(pane::PaneSide::Flow);
                if mode == StrategyDemoMode::EndedBadge {
                    // End the span the way the trader does — by moving the
                    // rectangle — rather than by stamping a state on. The
                    // shared demo band reaches `DEMO_STRATEGY_AHEAD_BARS`
                    // past the newest bar, which is why arming accepted it;
                    // pulling both anchors behind the tape is the drag that
                    // ends a region. Faking the state instead would
                    // photograph the words over a band that can still fire,
                    // and a reviewer would sign off on a scene the
                    // application cannot produce.
                    if let Some(index) = pane.drawings.index_of(drawing_id) {
                        #[allow(clippy::cast_precision_loss)]
                        let behind = closed.saturating_sub(2) as f32;
                        for point in &mut pane.drawings.items_mut()[index].points {
                            point.bar = point.bar.min(behind);
                        }
                    }
                } else if let Some(index) = pane.drawings.index_of(drawing_id) {
                    // A re-cut stranding an anchor is what sets this on a
                    // real chart; nothing scripted can provoke one on cue.
                    pane.drawings.items_mut()[index].off_series = true;
                }
                // As with the alarm badge: a selected drawing raises the
                // context bar across the very edge the badge sits on.
                pane.drawings.select(None);
            }
            StrategyDemoMode::Popup
            | StrategyDemoMode::AlarmPopup
            | StrategyDemoMode::AlarmSounds => {
                if mode == StrategyDemoMode::AlarmSounds {
                    self.surfaces.strategy_popup.stage_sound_picker();
                }
                let tab = self.tabs.active_id();
                self.surfaces
                    .strategy_popup
                    .open(tab, pane::PaneSide::Flow, drawing_id, form);
            }
        }
        // This demo places its rectangle, which selects it, which closes a
        // panel the launch may have asked for. Same door as the others.
        self.carry_inspector_across_selection();
    }

    /// Keep a `QUANTICK_DRAWING_INSPECTOR=1` request alive across a selection
    /// the *app* made, rather than the trader.
    ///
    /// The context bar closes the panel on every selection change, which is
    /// right for a trader clicking from one object to the next and wrong for a
    /// demo hook that places objects and selects one on the frame it runs:
    /// the pairing the harness table prescribes then photographs a chart with
    /// no panel on it. Re-requested through `pending_open_settings`, the door
    /// a tool that asks for its own settings on placement already uses, which
    /// is applied *after* the clear rather than before it.
    ///
    /// One function rather than the line copied into each hook, because the
    /// omission is silent: the demo that forgets it produces a screenshot that
    /// looks merely uninteresting, and that is how three of these hooks came to
    /// disagree about it.
    fn carry_inspector_across_selection(&mut self) {
        self.drawings.chrome.carry_across_selection();
    }
}

/// One deterministic prefix generator, shared with the legacy venue scenario.
fn synthetic_prefix(
    first_open: i64,
    anchor: rust_decimal::Decimal,
    candles: i64,
) -> Vec<quantick_engine::Bar> {
    let interval = quantick_feed::OHLCV_BASE_INTERVAL_MS;
    (-candles..0)
        .map(|minute| {
            let open_time = first_open + minute * interval;
            let drift = rust_decimal::Decimal::from(minute.rem_euclid(7) - 3);
            let open = anchor + drift;
            quantick_engine::Bar {
                open_time,
                close_time: open_time + interval - 1,
                open,
                high: open + rust_decimal::Decimal::from(2),
                low: open - rust_decimal::Decimal::from(2),
                close: open + rust_decimal::Decimal::from(minute.rem_euclid(3) - 1),
                buy_volume: rust_decimal::Decimal::from(2),
                sell_volume: rust_decimal::Decimal::from(3),
                trade_count: 7,
            }
        })
        .collect()
}

#[cfg(any(feature = "drawing-harness", test))]
fn facts(pane: &ChartPane) -> SeriesFacts {
    SeriesFacts {
        slots: pane.slots(),
        last_close: close(pane, pane.slots().saturating_sub(1)),
        auto_range: pane.frame.auto_range,
    }
}
#[cfg(any(feature = "drawing-harness", test))]
fn close(pane: &ChartPane, slot: usize) -> Option<f64> {
    pane.closed_bar(slot)
        .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
}
impl QuantickApp {
    #[cfg(any(feature = "drawing-harness", test))]
    pub(super) fn apply_drawing_demo(&mut self) {
        if !self.drawings.chrome.demos().gallery_requested() {
            return;
        }
        let slots = self.active_tab().flow_pane.slots();
        let Some(request) = self.drawings.chrome.demos_mut().take_gallery(slots) else {
            return;
        };
        if request.shared {
            self.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        let mut plan = request.plan(facts(pane), &pane.indicators);
        plan.project_times(|slot| pane.slot_open_time(slot));
        let center = plan.apply_main(&mut pane.drawings);
        if let (Some(center), Some(chart)) = (center, pane.frame.chart_area) {
            pane.viewport.center_on_bar(center, chart.width(), slots);
        }
        plan.apply_bands(&mut pane.drawings);
        self.drawings.chrome.carry_across_selection();
        // Recut follows taking the ready gallery, even if placement made no object.
        if self.drawings.chrome.demos().recut_requested() {
            let pane = &mut self.active_tab_mut().flow_pane;
            let first = pane.slot_open_time(0);
            let base = close(pane, 0);
            demo::apply_recut_marker(&mut pane.drawings, first, base);
            let spec = *pane.spec.retained(crate::state::BarKind::Tick);
            let value = demo::demo_recut_count(spec.parameter());
            pane.spec
                .retain(spec.with_parameter("count", value).expect("positive count"));
            self.active_tab_mut().apply_spec_changes();
            self.active_tab_mut().apply_spec_changes();
        }
    }
    #[cfg(any(feature = "drawing-harness", test))]
    pub(super) fn apply_drawing_draft(&mut self) {
        if self.drawings.chrome.demos().draft_requested().is_none() {
            return;
        }
        let pane = &self.active_tab().flow_pane;
        let (chart, facts) = (pane.frame.chart_area, facts(pane));
        let Some(mut plan) = self.drawings.chrome.demos_mut().take_draft(
            self.toolrail.tool().drawing_tool(),
            chart,
            facts,
        ) else {
            return;
        };
        let pane = &mut self.active_tab_mut().flow_pane;
        plan.project_times(|slot| pane.slot_open_time(slot));
        plan.apply(&mut pane.drawings);
        pane.gestures.parked_hand = Some(plan.hand);
    }
    #[cfg(any(feature = "drawing-harness", test))]
    pub(super) fn apply_frvp_demo(&mut self) {
        let Some(request) = self.drawings.chrome.demos().profile_requested() else {
            return;
        };
        let index = usize::from(request.stress);
        // Optional access proves the target exists; pane_mut's fallback cannot.
        let slots = self.active_tab().pane_at(index).map(ChartPane::slots);
        match self.drawings.chrome.demos_mut().prepare_profile(slots) {
            ProfilePreparation::Wait => return,
            ProfilePreparation::Ready => {}
            ProfilePreparation::Prefix { candles } => {
                if !self.deliver_synthetic_prefix(
                    candles,
                    quantick_feed::OhlcvSlice::Last { complete: true },
                ) {
                    return;
                }
            }
        }
        // The same actual pane survives delivery. Fresh slots include its prefix.
        let Some(pane) = self.active_tab_mut().pane_at_mut(index) else {
            return;
        };
        let facts = ProfileFacts {
            slots: pane.slots(),
            prefix: pane.history_prefix.len(),
        };
        let Some(geometry) = self.drawings.chrome.demos_mut().finish_profile(facts) else {
            return;
        };
        let Some(pane) = self.active_tab_mut().pane_at_mut(index) else {
            return;
        };
        let price = close(pane, geometry.close_slot);
        let mut plan = geometry.plan(price);
        plan.project_times(|slot| pane.slot_open_time(slot));
        plan.apply(&mut pane.drawings);
        if plan.carries_inspector() {
            self.drawings.chrome.carry_across_selection();
        }
    }
    #[cfg(any(feature = "drawing-harness", test))]
    pub(super) fn apply_avwap_demo(&mut self) {
        if !self.drawings.chrome.demos().avwap_requested() {
            return;
        }
        let slots = self.active_tab().flow_pane.slots();
        let Some(geometry) = self.drawings.chrome.demos_mut().take_avwap(slots) else {
            return;
        };
        let pane = &mut self.active_tab_mut().flow_pane;
        let price = close(pane, geometry.slot);
        let mut plan = geometry.plan(price);
        plan.project_times(|slot| pane.slot_open_time(slot));
        plan.apply(&mut pane.drawings);
        self.drawings.chrome.carry_across_selection();
    }
}
