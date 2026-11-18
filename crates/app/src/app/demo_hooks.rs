//! The harness demo stagers: frame-phase scenarios that stage a state a
//! screenshot needs and no click can reach.
//!
//! Not methods on the app: the frame calls each at its phase over its
//! [`HarnessDesk`], which lends the owners a stager acts on, and each
//! compiles only with its harness family (or under test) — the whole file
//! with any of them — so a default build has no demo stage at all. The gated drawing owner captures its requests at
//! executable startup and chooses geometry and consumption; its adapters
//! below project chosen slot times and invoke normal drawing, canvas and
//! history operations. The venue-history and strategy scenarios read the
//! launch-captured [`crate::harness`], and evidence capture is configured by
//! the control host. The frame keeps each existing phase in order, including
//! the history phases between drawing scenarios.
#![cfg(any(
    feature = "control-harness",
    feature = "drawing-harness",
    feature = "scenario-harness",
    test
))]

#[cfg(any(feature = "drawing-harness", feature = "scenario-harness", test))]
use crate::surfaces::drawing_chrome::DrawingChromeExt as _;
#[cfg(any(feature = "drawing-harness", test))]
use crate::toolrail::ToolRailExt as _;
#[cfg(any(feature = "control-harness", test))]
use eframe::egui;

#[cfg(any(feature = "scenario-harness", test))]
use crate::drawings;
#[cfg(any(feature = "scenario-harness", test))]
use crate::harness::{StrategyDemoMode, VenueHistoryDemo};
#[cfg(any(feature = "scenario-harness", test))]
use crate::pane;

use super::harness_desk::HarnessDesk;
#[cfg(any(feature = "drawing-harness", feature = "scenario-harness", test))]
use super::workspace_runtime::WorkspaceRuntime;
#[cfg(any(feature = "drawing-harness", test))]
use crate::pane::ChartPane;
#[cfg(any(feature = "drawing-harness", test))]
use crate::surfaces::drawing_chrome::demo::{
    self, DraftHost, GalleryHost, ProfileFacts, ProfileHost, ProfilePreparation, SeriesFacts,
};
#[cfg(any(feature = "drawing-harness", test))]
use crate::tab::CanvasLayout;
#[cfg(any(feature = "drawing-harness", test))]
use quantick_ui_model::drawing_chrome::scenario::{
    Effect as ScenarioEffect, Intent as ScenarioIntent,
};

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
pub(super) fn apply_control_evidence_hook<D: HarnessDesk + 'static>(
    desk: &mut D,
    ctx: &egui::Context,
) {
    let control = desk.owners().control;
    if !control.scenarios.has_evidence() {
        return;
    }
    // Access is taken *before* the request is, so a frame that finds it
    // borrowed leaves the hook pending rather than dropping it silently.
    let Some(mut access) = control.control_access.take() else {
        return;
    };
    let capture = control
        .prepare_evidence(&access)
        .expect("the hook was pending one line above");
    if capture.screenshot_not_granted {
        tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_EVIDENCE_HOOK_SCREENSHOT_NOT_GRANTED",
            "QUANTICK_CONTROL_EVIDENCE asked for an image without observe.screenshot; capturing without one");
    }
    if capture.wants_screenshot {
        access.service_screenshot(desk, ctx);
    }
    let control = desk.owners().control;
    let (scopes, wants_screenshot) = match control
        .scenarios
        .finish_evidence(capture, access.has_screenshot())
    {
        super::control_host::EvidenceStep::Waiting => {
            ctx.request_repaint();
            control.control_access = Some(access);
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
        desk,
        "evidence.capture",
        serde_json::json!({ "scopes": scopes, "screenshot": wants_screenshot }),
    );
    desk.owners().control.control_access = Some(access);
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

/// Launch scenarios run the label and notification handlers as a trusted
/// local agent: traced on a replay, never through remote-grant admission.
#[cfg(any(feature = "control-harness", test))]
pub(super) fn apply_control_annotate_hooks<D: HarnessDesk>(desk: &mut D) {
    let remote = crate::control::ActionOrigin::Remote;
    let owners = desk.owners();
    if owners.control.scenarios.has_annotation() {
        let pane = owners.runtime.active_tab().drawing_pane();
        let slot = pane.slots().saturating_sub(1);
        let anchor = match (pane.slot_open_time(slot), pane.closed_bar(slot)) {
            (Some(time), Some(bar)) => Some(serde_json::json!({
                "time_unix_ms": time,
                "price": rust_decimal::prelude::ToPrimitive::to_f64(&bar.close).unwrap_or(1.0).to_string(),
            })),
            _ => None,
        };
        if let Some(input) = owners.control.scenarios.annotation(anchor) {
            desk.run_hook_action("annotate.label.create", input, remote);
        }
    }
    match desk.owners().control.scenarios.notification() {
        Some(super::control_host::NotificationStep::Ready { capability, input }) => {
            desk.run_hook_action(capability, input, remote);
        }
        Some(super::control_host::NotificationStep::Refused { channel }) => tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_NOTIFY_HOOK_REFUSED",
            channel = %channel,
            "QUANTICK_CONTROL_NOTIFY names no notification channel"
        ),
        None => {}
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
#[cfg(any(feature = "scenario-harness", test))]
pub(super) fn apply_venue_history_demo<D: HarnessDesk>(desk: &mut D) {
    /// Candles the venue-history scene installs: enough for the seam and
    /// the divider to read, few enough to stay one screenful of context.
    const DEMO_PREFIX_CANDLES: i64 = 90;
    let owners = desk.owners();
    let Some(demo) = owners.chrome.harness.venue_history_demo() else {
        return;
    };
    // Wait for bars to sit the prefix in front of: a seam needs both sides.
    if owners
        .runtime
        .active_tab()
        .flow_pane
        .series
        .state
        .bars()
        .len()
        < 12
    {
        return;
    }
    owners.chrome.harness.venue_history_demo_staged();
    let slice = match demo {
        VenueHistoryDemo::Complete => quantick_feed::OhlcvSlice::Last { complete: true },
        VenueHistoryDemo::Partial => quantick_feed::OhlcvSlice::More,
    };
    deliver_synthetic_prefix(owners.runtime, DEMO_PREFIX_CANDLES, slice);
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
#[cfg(any(feature = "drawing-harness", feature = "scenario-harness", test))]
pub(super) fn deliver_synthetic_prefix(
    runtime: &mut WorkspaceRuntime,
    candles: i64,
    slice: quantick_feed::OhlcvSlice,
) -> bool {
    let tab_id = runtime.tabs.active_id();
    let tab = runtime.active_tab_mut();
    let Some(first) = tab.flow_pane.series.state.bars().first() else {
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
#[cfg(any(feature = "scenario-harness", test))]
pub(super) fn apply_strategy_demo<D: HarnessDesk>(desk: &mut D) {
    let owners = desk.owners();
    let Some(mode) = owners.chrome.harness.strategy_demo() else {
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
    let closed = owners.runtime.active_tab_mut().flow_pane.closed_slots();
    if closed < DEMO_STRATEGY_MIN_SLOTS {
        return;
    }
    let Some(rectangle) = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
    else {
        // No rectangle in the registry: staging can never succeed, so
        // the flag is consumed rather than retried forever.
        owners.chrome.harness.strategy_demo_staged();
        return;
    };
    let drawing_id = {
        let pane = &mut owners.runtime.active_tab_mut().flow_pane;
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
    owners.chrome.harness.strategy_demo_staged();
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    // The alarm scenes tick the checkbox the trader would tick, and the
    // share gate under it, so the section is unfolded with every control
    // it owns on screen.
    if matches!(
        mode,
        StrategyDemoMode::AlarmPopup | StrategyDemoMode::AlarmSounds | StrategyDemoMode::AlarmBadge
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
    if mode == StrategyDemoMode::AlarmBadge {
        form.alarm_only = true;
    }
    let armed_as = match mode {
        StrategyDemoMode::AlarmBadge => Some("demo alarm"),
        StrategyDemoMode::Armed | StrategyDemoMode::EndedBadge | StrategyDemoMode::PausedBadge => {
            Some("demo BF")
        }
        _ => None,
    };
    if let Some(name) = armed_as {
        let index = owners.runtime.tabs.active_index();
        let _ = owners
            .runtime
            .tabs
            .runtime_mut(index)
            .arm_strategy_instance(
                &mut *owners.audio.alerts,
                pane::PaneSide::Flow,
                drawing_id,
                &form,
                name.to_owned(),
            );
    }
    match mode {
        StrategyDemoMode::Armed => {}
        StrategyDemoMode::AlarmBadge => {
            // Stand a provisional judgement on the badge. The mark is
            // the surface under test, and the tape reaches it only when
            // a force bar happens to be half-formed — so the scene
            // stages the mark itself rather than waiting for a market
            // that may not oblige before the shutter.
            let (pane, strategies) =
                (owners.runtime.active_tab_mut()).pane_with_strategies(pane::PaneSide::Flow);
            if let Some(instance) = strategies.anchors.for_drawing_mut(drawing_id) {
                instance.mark = quantick_strategy::anchors::AlarmMark::Preview;
            }
            // Placing a drawing selects it, and a selected drawing raises
            // the context bar across its own top edge — which is where
            // the badge this scene exists to photograph sits. Drop the
            // selection so the badge is the thing on screen.
            pane.drawings.select(None);
        }
        StrategyDemoMode::EndedBadge | StrategyDemoMode::PausedBadge => {
            let pane = owners
                .runtime
                .active_tab_mut()
                .pane_mut(pane::PaneSide::Flow);
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
        StrategyDemoMode::Popup | StrategyDemoMode::AlarmPopup | StrategyDemoMode::AlarmSounds => {
            let popup = &mut owners.surfaces.strategy_popup;
            if mode == StrategyDemoMode::AlarmSounds {
                popup.update(quantick_ui_model::strategy_popup::Intent::StageSoundPicker);
            }
            let tab = owners.runtime.tabs.active_id();
            popup.update(quantick_ui_model::strategy_popup::Intent::Open {
                tab,
                side: pane::PaneSide::Flow,
                drawing: drawing_id,
                form: Box::new(form),
            });
        }
    }
    // This demo places its rectangle, which selects it, which closes a
    // panel the launch may have asked for. Same door as the others.
    carry_inspector_across_selection(owners.surfaces);
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
#[cfg(any(feature = "scenario-harness", test))]
fn carry_inspector_across_selection(surfaces: &mut crate::surfaces::Surfaces) {
    surfaces.drawing_chrome.carry_across_selection();
}

/// One deterministic prefix generator, shared with the legacy venue scenario.
#[cfg(any(feature = "drawing-harness", feature = "scenario-harness", test))]
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
#[cfg(any(feature = "drawing-harness", test))]
pub(super) fn apply_drawing_demo<D: HarnessDesk>(desk: &mut D) {
    let owners = desk.owners();
    let chrome = &mut owners.surfaces.drawing_chrome;
    if !chrome.demos().gallery_requested() {
        return;
    }
    let slots = owners.runtime.active_tab().flow_pane.slots();
    let tools = crate::drawings::DRAWING_TOOLS;
    if !chrome.demos().gallery_ready(slots, tools.len()) {
        return;
    }

    let pane = &owners.runtime.active_tab().flow_pane;
    let series = facts(pane);
    let samples = chrome
        .demos()
        .gallery_sample_slot(slots)
        .map_or_else(Vec::new, |slot| {
            crate::bands::samples_at(&pane.indicators, slot)
        });
    let Some(ScenarioEffect::Gallery(mut plan)) = chrome.scenario(ScenarioIntent::GalleryReady {
        series,
        tools: tools.to_vec(),
        samples,
    }) else {
        return;
    };
    if plan.shared == Some(true) {
        owners
            .runtime
            .active_tab_mut()
            .set_layout(CanvasLayout::TimeAndFlow);
    }
    let pane = &mut owners.runtime.active_tab_mut().flow_pane;
    plan.project_times(|slot| pane.slot_open_time(slot));
    let center = plan.apply_main(&mut pane.drawings, |intent| chrome.scenario(intent));
    if let (Some(center), Some(chart)) = (center, pane.frame.chart_area) {
        pane.model
            .viewport
            .center_on_bar(center, chart.width(), slots);
    }
    plan.apply_bands(&mut pane.drawings);
    chrome.carry_across_selection();
    // Recut follows taking the ready gallery, even if placement made no object.
    if chrome.demos().recut_requested() {
        let tab = owners.runtime.active_tab_mut();
        let pane = &mut tab.flow_pane;
        let first = pane.slot_open_time(0);
        let base = close(pane, 0);
        demo::apply_recut_marker(&mut pane.drawings, first, base);
        let spec = *pane.spec.retained(crate::state::BarKind::Tick);
        let value = demo::demo_recut_count(spec.parameter());
        pane.spec
            .retain(spec.with_parameter("count", value).expect("positive count"));
        tab.apply_spec_changes();
        tab.apply_spec_changes();
    }
}
#[cfg(any(feature = "drawing-harness", test))]
pub(super) fn apply_drawing_draft<D: HarnessDesk>(desk: &mut D) {
    let owners = desk.owners();
    let surfaces = owners.surfaces;
    if surfaces.drawing_chrome.demos().draft_requested().is_none() {
        return;
    }
    let pane = &owners.runtime.active_tab().flow_pane;
    let (chart, facts) = (pane.frame.chart_area, facts(pane));
    let Some(ScenarioEffect::Draft(mut plan)) =
        surfaces
            .drawing_chrome
            .scenario(ScenarioIntent::DraftReady {
                tool: surfaces.toolrail.tool().drawing_tool(),
                chart_available: chart.is_some(),
                series: facts,
            })
    else {
        return;
    };
    let pane = &mut owners.runtime.active_tab_mut().flow_pane;
    plan.project_times(|slot| pane.slot_open_time(slot));
    plan.apply(&mut pane.drawings);
    pane.gestures.parked_hand = Some(plan.hand(chart.expect("model requires chart")));
}
#[cfg(any(feature = "drawing-harness", test))]
pub(super) fn apply_frvp_demo<D: HarnessDesk>(desk: &mut D) {
    let owners = desk.owners();
    let chrome = &mut owners.surfaces.drawing_chrome;
    let Some(request) = chrome.demos().profile_requested() else {
        return;
    };
    let index = usize::from(request.stress);
    // Optional access proves the target exists; pane_mut's fallback cannot.
    let slots = owners
        .runtime
        .active_tab()
        .pane_at(index)
        .map(ChartPane::slots);
    let tool = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID);
    match chrome.scenario(ScenarioIntent::PrepareProfile {
        slots,
        tool_available: tool.is_some(),
    }) {
        Some(ScenarioEffect::ProfilePreparation(ProfilePreparation::Ready)) => {}
        Some(ScenarioEffect::ProfilePreparation(ProfilePreparation::Prefix { candles })) => {
            let complete = quantick_feed::OhlcvSlice::Last { complete: true };
            if !deliver_synthetic_prefix(owners.runtime, candles, complete) {
                return;
            }
        }
        _ => return,
    }
    // The same actual pane survives delivery. Fresh slots include its prefix.
    let Some(pane) = owners.runtime.active_tab_mut().pane_at_mut(index) else {
        return;
    };
    let facts = ProfileFacts {
        slots: pane.slots(),
        prefix: pane.series.history_prefix.len(),
    };
    let Some(ScenarioEffect::Profile(geometry)) =
        chrome.scenario(ScenarioIntent::ProfileReady(facts))
    else {
        return;
    };
    let price = close(pane, geometry.close_slot);
    let mut plan = geometry.plan(
        tool.expect("profile tool checked before history delivery"),
        price,
    );
    plan.project_times(|slot| pane.slot_open_time(slot));
    plan.apply(&mut pane.drawings);
    if plan.carries_inspector() {
        chrome.carry_across_selection();
    }
}
#[cfg(any(feature = "drawing-harness", test))]
pub(super) fn apply_avwap_demo<D: HarnessDesk>(desk: &mut D) {
    let owners = desk.owners();
    let chrome = &mut owners.surfaces.drawing_chrome;
    if !chrome.demos().avwap_requested() {
        return;
    }
    let slots = owners.runtime.active_tab().flow_pane.slots();
    let effect = chrome.scenario(ScenarioIntent::AvwapReady { slots });
    let Some((geometry, tool)) = demo::resolve_avwap(effect, || {
        crate::drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == quantick_drawings::avwap::TOOL_ID)
    }) else {
        return;
    };
    let pane = &mut owners.runtime.active_tab_mut().flow_pane;
    let price = close(pane, geometry.slot);
    let mut plan = geometry.plan(tool, price);
    plan.project_times(|slot| pane.slot_open_time(slot));
    plan.apply(&mut pane.drawings);
    chrome.carry_across_selection();
}
