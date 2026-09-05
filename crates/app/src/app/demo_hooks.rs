//! The harness demo appliers: the launch hooks that stage a state a
//! screenshot needs and no click can reach.
//!
//! Each is one `QUANTICK_*` hook's other half. The hook is read at launch —
//! in [`super::QuantickApp::new_with_workspace`] for the evidence bundle, in
//! [`crate::harness`] for the rest — and parked on the application; the
//! method here spends it on the first frames, once the panes have a viewport
//! to place objects against. They are grouped because they share that shape
//! and the constants below, and because one caller runs the whole group:
//! `draw_frame` applies them in order and moves on.
//!
//! No hook is declared here. Every `QUANTICK_*` these bodies name, they name
//! in a doc comment or a log message; the reads themselves stay beside the
//! other launch hooks in [`super`], so the registry's owner table is
//! unchanged by their living here.

use eframe::egui;

use crate::drawings;
use crate::harness::{DrawingDraft, DrawingsDemo, FrvpDemo, StrategyDemoMode, VenueHistoryDemo};
use crate::pane::{self, ChartPane};
use crate::tab::CanvasLayout;

use super::{DEMO_VISIBLE_SLOTS, QuantickApp};

/// How many demo objects wide the visible window is — the reciprocal of how
/// far a multi-anchor object reaches. Four keeps a rectangle big enough to
/// read while still leaving the tools distinguishable from each other.
const DEMO_SPANS_PER_WINDOW: usize = 4;
/// How far apart, as a fraction of the visible price band, the demo places
/// the successive anchors of one object and the successive rows of objects.
/// Both are small enough that the widest object still lands inside the band
/// the chart is showing.
const DEMO_ANCHOR_BAND_STEP: f64 = 0.12;
const DEMO_ROW_BAND_STEP: f64 = 0.22;
/// Points the demo hook gives a freehand tool, which declares no anchor
/// count of its own. Enough to read as a path rather than as a line.
const DEMO_FREEHAND_POINTS: usize = 4;
/// How much of the visible window and of the visible price band the
/// `QUANTICK_DRAWING_DRAFT` hook's anchors span. Wide enough that the
/// half-made object reads at a glance, and centred, so the segment its
/// anchors describe passes through the parked pointer at the chart's middle.
///
/// Much wider than it is tall, because a trend line a trader would actually
/// draw runs *along* the tape. A near-vertical draft photographs the
/// mechanism but nothing about whether the shape reads, which is what a QA
/// reference image is for.
const DEMO_DRAFT_SPAN: f32 = 0.6;
const DEMO_DRAFT_BAND_SPAN: f64 = 0.12;
/// Where the parked pointer stands when a single anchor is down, as a
/// fraction of the chart from its centre.
///
/// A lone anchor sits at that centre, so parking the pointer there too gives
/// a rubber band of no length — an invisible draft, which is the one thing
/// this hook exists not to photograph. Offset, there is a line to see, and it
/// is a *sloped* one, which is what makes the levelled state worth its own
/// capture.
const DEMO_DRAFT_POINTER_OFFSET: egui::Vec2 = egui::vec2(0.2, -0.15);
/// The band to spread across before the pane has an auto-range to read (no
/// bars yet): a fraction of price, since there is nothing better to ask.
const DEMO_FALLBACK_BAND_FRACTION: f64 = 0.004;
/// How far before the loaded history the re-cut demo anchors its off-series
/// mark. Any distance the tab cannot possibly hold would do; an hour is
/// unambiguous at every timeframe the chart offers.
const DEMO_OFF_SERIES_LEAD_MS: i64 = 3_600_000;

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
    /// [`crate::harness::CONTROL_EVIDENCE_HOOK_FRAMES`] rather than hanging a capture run on a
    /// surface that never presents.
    pub(super) fn apply_control_evidence_hook(&mut self, ctx: &egui::Context) {
        if self.control.pending_control_evidence.is_none() {
            return;
        }
        // Access is taken *before* the request is, so a frame that finds it
        // borrowed leaves the hook pending rather than dropping it silently.
        let Some(mut access) = self.control.control_access.take() else {
            return;
        };
        let request = self
            .control
            .pending_control_evidence
            .take()
            .expect("the hook was pending one line above");
        let mut wants_screenshot = false;
        // A set, not a list: `all,scene.controls` is a reasonable thing to
        // type, and the capability refuses a scope named twice, so the tokens
        // are folded rather than concatenated.
        let mut scopes = std::collections::BTreeSet::new();
        for token in request.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            match token {
                "screenshot" => wants_screenshot = true,
                "all" | "1" => scopes.extend(
                    access
                        .readable_scopes()
                        .into_iter()
                        .map(|scope| scope.to_string()),
                ),
                scope => {
                    scopes.insert(scope.to_owned());
                }
            }
        }
        if scopes.is_empty() {
            scopes.extend(
                access
                    .readable_scopes()
                    .into_iter()
                    .map(|scope| scope.to_string()),
            );
        }
        // The scope is checked before anything is armed, not after. Arming is
        // what eventually raises the screenshot notice, and telling the trader
        // their window was captured on the way to refusing the capture would
        // make the one indicator `visual-qa` asserts on say something untrue.
        if wants_screenshot && !access.grants_screenshot() {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_HOOK_SCREENSHOT_NOT_GRANTED",
                "QUANTICK_CONTROL_EVIDENCE asked for an image without observe.screenshot; \
                 capturing without one"
            );
            wants_screenshot = false;
        }
        if wants_screenshot {
            // Harvests as well as arms: the frame service that normally takes
            // the pixels runs only while the gateway is enabled, and this hook
            // is meant to work without a client on the socket.
            access.service_screenshot(self, ctx);
        }
        if wants_screenshot && !access.has_screenshot() {
            if self.harness.evidence_frame_waited() {
                // Every waiting frame asks for the next one. Without this a
                // quiescent window — a paused replay, no feed, exactly the
                // headless validation run the hook exists for — would never
                // repaint, the counter would never advance, and the hook would
                // neither complete nor give up.
                ctx.request_repaint();
                self.control.pending_control_evidence = Some(request);
                self.control.control_access = Some(access);
                return;
            }
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_EVIDENCE_HOOK_GAVE_UP_ON_IMAGE",
                frames = crate::harness::CONTROL_EVIDENCE_HOOK_FRAMES,
                "the window never delivered a frame to rasterise; capturing without one"
            );
            // The request stays true on purpose. Clearing it would send
            // `screenshot: false`, and the bundle would then record
            // `screenshot/not_requested` — a lie about a run that asked for a
            // picture and waited two seconds for one. Left true, the capture
            // finds no image and records `frame_not_delivered`, which is what
            // actually happened.
        }
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

    /// The `QUANTICK_DRAWINGS_DEMO` hook: one of every registered drawing on
    /// the flow pane, spread across the visible bars, the last one selected
    /// so the inspector is on screen too.
    ///
    /// Waits for bars: anchors are placed on real slots, so every anchor
    /// carries a real market time and the shared-drawing path is exercised
    /// rather than faked. Consumed once, whether or not it placed anything on
    /// this attempt — an env var is a request for this run, and it must never
    /// keep re-placing objects the user then deletes.
    pub(super) fn apply_drawing_demo(&mut self) {
        if !self.harness.drawings_demo_armed() {
            return;
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        let slots = pane.slots();
        // Enough bars for every tool to get its own stretch of chart.
        if slots < 8 * drawings::DRAWING_TOOLS.len() {
            return;
        }
        let Some(demo) = self.harness.take_drawings_demo() else {
            return;
        };
        let DrawingsDemo {
            bands,
            shared: share,
            select_tool,
        } = demo;
        if share {
            // A shared drawing has nothing to be shared *with* on a single
            // pane, so the hook that asks for one opens the split too — the
            // surface under test is the projection onto the other chart.
            self.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        // Anchored inside the window the chart actually opens on, not at slot
        // zero: a demo whose objects sit 300 bars off the left edge shows a
        // screenshot of an empty chart, which is exactly the evidence this
        // hook exists to produce. `visible` is the newest stretch, and the
        // objects are laid across it left to right in registry order.
        let visible = DEMO_VISIBLE_SLOTS.min(slots);
        let first = slots - visible;
        // Two different spacings on purpose. `stride` walks the *starts*
        // apart so the objects are distinguishable; `span` sets how far a
        // multi-anchor object reaches, and it has to be wide or a rectangle
        // lands two bars across and photographs as a sliver. The objects
        // overlap, which is fine — a QA screen wants every tool legible, not
        // a tidy row.
        let stride = (visible / drawings::DRAWING_TOOLS.len()).max(1);
        let span = (visible / DEMO_SPANS_PER_WINDOW).max(2);
        let close = pane
            .closed_bar(slots.saturating_sub(1))
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        // Spread the objects across the band the chart is actually showing,
        // never across a fixed percentage of price. A tick chart's window is
        // a few tenths of a percent tall, so a fixed ±0.2 % dropped a third
        // of the demo below the visible range — objects placed off screen
        // photograph as an empty chart, which is the one failure this hook
        // exists to prevent.
        let (centre, band) = pane
            .last_auto_range
            .filter(|(lo, hi)| hi > lo)
            .map_or((close, close * DEMO_FALLBACK_BAND_FRACTION), |(lo, hi)| {
                ((lo + hi) / 2.0, hi - lo)
            });
        let mut requested_selection = None;
        for (index, tool) in drawings::DRAWING_TOOLS.into_iter().enumerate() {
            // A freehand tool declares no anchor count, so the demo gives it
            // a short path of its own. A stroke no screenshot can reach is a
            // tool that ships unvalidated.
            let anchors = if tool.freehand() {
                DEMO_FREEHAND_POINTS
            } else {
                tool.required_points()
            };
            for anchor in 0..anchors {
                let slot = (first + index * stride + anchor * span).min(slots.saturating_sub(1));
                let point = drawings::ChartPoint::at_time(
                    slot as f32 + 0.5,
                    centre + (f64::from(anchor as i32) - 1.0) * band * DEMO_ANCHOR_BAND_STEP
                        - (f64::from(index as i32 % 3) - 1.0) * band * DEMO_ROW_BAND_STEP,
                    pane.slot_open_time(slot),
                );
                let mut completed =
                    pane.drawings
                        .place_with(tool, &drawings::DrawingBand::Price, point, |tool| {
                            drawings::NewDrawing {
                                style: tool.default_style(),
                                payload: tool.default_payload(),
                            }
                        });
                // The release, for the tool whose gesture has one.
                if tool.freehand() && anchor + 1 == anchors {
                    completed = pane.drawings.finish_draft();
                }
                // Placement selects what it completed, so this reaches the
                // object just made — no separate index bookkeeping.
                if completed
                    && share
                    && let Some(drawing) = pane.drawings.selected_mut()
                    && drawing.shareable()
                {
                    drawing.scope = drawings::DrawingScope::AllCharts;
                }
                if completed && select_tool.as_deref() == Some(tool.id()) {
                    requested_selection = pane.drawings.selected();
                }
            }
        }
        if let Some(index) = requested_selection {
            pane.drawings.select(Some(index));
            // Selecting an object the viewport is not looking at proves
            // nothing: the handles are on screen or they are not photographed
            // at all. Centre on the object's bar span, the object manager's
            // own "select and centre".
            if let Some(chart) = pane.last_chart_area {
                let points = &pane.drawings.items()[index].points;
                if !points.is_empty() {
                    let mid =
                        points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                    pane.viewport.center_on_bar(mid, chart.width(), slots);
                }
            }
        }
        if bands {
            Self::seed_band_demo(pane, first, visible, slots);
        }
        self.carry_inspector_across_selection();
        self.apply_drawing_demo_recut();
    }

    /// The `QUANTICK_DRAWING_DRAFT=<anchors>` hook: the tool armed by
    /// `QUANTICK_DRAWING_TOOL`, part-placed, with the pointer parked where
    /// the next anchor would go.
    ///
    /// The live preview of a half-made object is a surface like any other,
    /// and for a multi-anchor tool it is the *whole* feedback of the gesture:
    /// it is what tells the trader that a drag has fixed the trend line and a
    /// click is owed for the width. It is also the one surface no click-free
    /// launch could reach, because it exists only between two clicks and only
    /// while a pointer is over the chart — so this hook does both halves.
    ///
    /// The anchors go down through `place_with`, the same call the click path
    /// makes; nothing here is a parallel placement path. They straddle the
    /// middle of the visible window so the line they draw runs through the
    /// parked pointer — which is the exact spot the reported defect lived at,
    /// the pointer standing on the trend line it just drew. A screenshot of
    /// this state is a screenshot of the fix.
    ///
    /// Waits for bars, and is consumed once, for the same reasons
    /// [`Self::apply_drawing_demo`] is.
    pub(super) fn apply_drawing_draft(&mut self) {
        let Some(DrawingDraft { anchors, constrain }) = self.harness.drawing_draft() else {
            return;
        };
        let Some(tool) = self.toolrail.tool().drawing_tool() else {
            return;
        };
        // Bars first, and a chart rectangle to park a pointer in: both are
        // answers only a drawn frame has.
        let pane = &self.active_tab().flow_pane;
        let slots = pane.slots();
        let (Some(chart), true) = (pane.last_chart_area, slots > 0) else {
            return;
        };
        self.harness.drawing_draft_staged();
        let pane = &mut self.active_tab_mut().flow_pane;
        // One short of the count, whatever was asked for: a draft that
        // completed itself is not a draft, and photographs as a finished
        // object rather than as the gesture under test.
        let anchors = anchors.min(tool.required_points().saturating_sub(1));
        let visible = DEMO_VISIBLE_SLOTS.min(slots);
        let first = slots - visible;
        let close = pane
            .closed_bar(slots.saturating_sub(1))
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        let (centre, band) = pane
            .last_auto_range
            .filter(|(lo, hi)| hi > lo)
            .map_or((close, close * DEMO_FALLBACK_BAND_FRACTION), |(lo, hi)| {
                ((lo + hi) / 2.0, hi - lo)
            });
        for anchor in 0..anchors {
            // Symmetric about the middle of the window, so the segment the
            // anchors describe has the chart's own centre as its midpoint.
            let offset = (anchor as f32 + 0.5) / anchors as f32 - 0.5;
            let slot = ((first as f32 + visible as f32 * (0.5 + offset * DEMO_DRAFT_SPAN))
                as usize)
                .min(slots.saturating_sub(1));
            let point = drawings::ChartPoint::at_time(
                slot as f32 + 0.5,
                centre + f64::from(offset) * band * DEMO_DRAFT_BAND_SPAN,
                pane.slot_open_time(slot),
            );
            pane.drawings
                .place_with(tool, &drawings::DrawingBand::Price, point, |tool| {
                    drawings::NewDrawing {
                        style: tool.default_style(),
                        payload: tool.default_payload(),
                    }
                });
        }
        // The hand this run does not have. Parked on the line the anchors
        // drew, which is where a real hand is sitting the instant a drag lets
        // go — and where a channel used to be born with no width at all.
        //
        // `QUANTICK_DRAWING_CONSTRAIN=1` presses Shift for it. The levelled
        // draft is a state of its own — a corridor held flat looks different
        // from one following the pointer — and a modifier is the one input a
        // capture run cannot supply any other way.
        // With two anchors down the pointer belongs *on* the line they drew —
        // the exact spot the reported defect lived at. With one, that spot is
        // the anchor itself, so it steps aside far enough to leave a rubber
        // band worth looking at.
        let parked = if anchors >= 2 {
            chart.center()
        } else {
            chart.center()
                + egui::vec2(
                    chart.width() * DEMO_DRAFT_POINTER_OFFSET.x,
                    chart.height() * DEMO_DRAFT_POINTER_OFFSET.y,
                )
        };
        pane.parked_hand = Some(pane::ParkedHand {
            position: parked,
            constrain: if constrain {
                drawings::Constrain::Level
            } else {
                drawings::Constrain::Free
            },
        });
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
    fn deliver_synthetic_prefix(&mut self, candles: i64, slice: quantick_feed::OhlcvSlice) -> bool {
        let tab = self.active_tab_mut();
        let Some(first) = tab.flow_pane.state.bars().first() else {
            return false;
        };
        let (first_open, anchor) = (first.open_time, first.open);
        let interval = quantick_feed::OHLCV_BASE_INTERVAL_MS;
        let bars: Vec<quantick_engine::Bar> = (-candles..0)
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
            .collect();
        tab.deliver_ohlcv_slice(interval, bars, slice);
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
                let _ = self.arm_strategy_instance(
                    pane::PaneSide::Flow,
                    drawing_id,
                    &form,
                    "demo BF".to_owned(),
                );
            }
            StrategyDemoMode::AlarmBadge => {
                form.alarm_only = true;
                let _ = self.arm_strategy_instance(
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
                if let Some(instance) = pane.strategies.for_drawing_mut(drawing_id) {
                    instance.mark = crate::strategy_anchors::AlarmMark::Preview;
                }
                // Placing a drawing selects it, and a selected drawing raises
                // the context bar across its own top edge — which is where
                // the badge this scene exists to photograph sits. Drop the
                // selection so the badge is the thing on screen.
                pane.drawings.select(None);
            }
            StrategyDemoMode::EndedBadge | StrategyDemoMode::PausedBadge => {
                let _ = self.arm_strategy_instance(
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
                let tab = self.active_tab().id;
                self.surfaces
                    .strategy_popup
                    .open(tab, pane::PaneSide::Flow, drawing_id, form);
            }
        }
        // This demo places its rectangle, which selects it, which closes a
        // panel the launch may have asked for. Same door as the others.
        self.carry_inspector_across_selection();
    }

    /// The `QUANTICK_FRVP_DEMO` hook: one fixed-range volume profile on the
    /// flow pane. When the pane carries a venue history prefix the range
    /// starts inside it, so the partial-coverage honesty label ("profile
    /// from N of M bars") is on screen — the surface this hook exists to
    /// photograph. Consumed once, like the drawings demo.
    pub(super) fn apply_frvp_demo(&mut self) {
        /// One-minute venue candles the `=stress` scene installs behind the
        /// tape — a time chart's worth of history, and far more than one fold
        /// pass spends, so the range folds across frames rather than in the one
        /// that placed it. The time pane folds them to whatever interval it is
        /// showing, so the slot count is this number only at 1m — the scene is
        /// about the fold surviving a long history, not about an exact count.
        const FRVP_STRESS_CANDLES: i64 = 25_000;
        // `=compare` places two adjacent profiles over the same stretch of
        // map, one in each over-heatmap mode — the before/after of the
        // silhouette decision in a single frame, which no toggle-and-wait
        // pair of screenshots can prove as cleanly.
        let Some(demo) = self.harness.frvp_demo() else {
            return;
        };
        let FrvpDemo {
            compare,
            stress,
            select,
        } = demo;
        // The venue's candles land on the **time** pane — that is the chart
        // whose history runs to tens of thousands of bars, and the one a
        // trader drops a session profile on. Every other scene stays on the
        // flow pane, where the tape and the map are.
        let side = if stress {
            pane::PaneSide::Time(0)
        } else {
            pane::PaneSide::Flow
        };
        // The time pane is built the first time the split is shown, and
        // `pane_mut` answers with the flow pane until it is. Waiting is the
        // only honest option here: a stress scene photographed on the tape
        // pane would be a picture of twenty bars captioned as twenty-five
        // thousand.
        if stress && self.active_tab_mut().time_panes.is_empty() {
            return;
        }
        let slots = self.active_tab_mut().pane_mut(side).slots();
        if slots < 12 {
            return;
        }
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        else {
            self.harness.frvp_demo_placed();
            return;
        };
        // `=stress` is the scene this object used to freeze the app on: a
        // venue history longer than any single fold pass, with one profile
        // over the whole of it. What it photographs is the *filling* state —
        // a partial histogram and its `loading N of M bars` line — which is
        // only a state at all because the fold is resumable. Pair it with
        // QUANTICK_FRVP_FOLD_BUDGET=1 to hold that frame indefinitely.
        //
        // The delivery has to land before the range is placed: a scene that
        // drew "the whole chart" over the handful of bars a pane starts with
        // would be captioned as twenty-five thousand and be a picture of
        // twenty. It stays armed and tries again next frame instead.
        if stress
            && !self.deliver_synthetic_prefix(
                FRVP_STRESS_CANDLES,
                quantick_feed::OhlcvSlice::Last { complete: true },
            )
        {
            return;
        }
        self.harness.frvp_demo_placed();
        // Re-read after the delivery: the prefix that just landed is part of
        // the chart the range is about to span.
        let slots = self.active_tab_mut().pane_mut(side).slots();
        let prefix = self.active_tab_mut().pane_mut(side).history_prefix.len();
        // The compare scene exists to photograph profiles *over the map*, and
        // the map only covers bars closed after book capture began — the
        // session's earliest bars never get cells. So it waits for enough
        // freshly-built bars and anchors on those, where the coverage is.
        if compare && slots.saturating_sub(prefix) < 60 {
            self.harness.rearm_frvp_demo(demo);
            return;
        }
        let pane = self.active_tab_mut().pane_mut(side);
        // Straddle the seam when there is one; else the newest stretch.
        let start = if prefix > 0 && prefix < slots {
            prefix.saturating_sub(5)
        } else {
            slots.saturating_sub(30)
        };
        let end = (start + 29).min(slots - 1);
        let close = pane
            .closed_bar(end)
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        let newest = slots - 1;
        let ranges: &[(usize, usize, bool)] = if stress {
            // The whole chart, oldest slot to newest: the range a trader
            // drags when they want the session's own volume-by-price, and the
            // one that used to take the window with it.
            &[(0, newest, true)]
        } else if compare {
            // Two adjacent 25-bar ranges over the newest (map-covered) tape.
            // Left object keeps the honest default; right one is forced to
            // "always fill", the composed-into-the-map look under review.
            &[
                (newest.saturating_sub(49), newest.saturating_sub(25), true),
                (newest.saturating_sub(24), newest, false),
            ]
        } else {
            &[(start, end, true)]
        };
        // Selecting what the demo drew is what makes the *context bar* — the
        // strip a trader edits a profile from — reachable without a click.
        // Without it a validation run can photograph a profile but never the
        // controls over it, which is exactly the surface whose placement this
        // change is about (`ui-harness`: every surface owes a hook).
        for &(from, to, outline) in ranges {
            for slot in [from, to] {
                pane.drawings.place_with(
                    tool,
                    &drawings::DrawingBand::Price,
                    // On the slot's centre, where a trader's click lands —
                    // anchoring on its trailing edge drew a box one candle
                    // wider than the range it folded.
                    drawings::ChartPoint::at_time(slot as f32, close, pane.slot_open_time(slot)),
                    |tool| {
                        let mut payload = tool.default_payload();
                        if let Some(frvp) =
                            payload.as_any_mut().downcast_mut::<drawings::FrvpPayload>()
                        {
                            frvp.outline_over_heatmap = outline;
                        }
                        drawings::NewDrawing {
                            style: drawings::DrawingStyle::default(),
                            payload,
                        }
                    },
                );
            }
            if select {
                let last = pane.drawings.items().len().saturating_sub(1);
                pane.drawings.select(Some(last));
            }
        }
        if select {
            self.carry_inspector_across_selection();
        }
    }

    /// The `QUANTICK_AVWAP_DEMO` hook: one anchored VWAP on the flow pane,
    /// anchored a stretch back with the first two band pairs on — the band
    /// stack, the fills and the anchor marker in a single deterministic
    /// frame. Consumed once, like the drawings demo.
    pub(super) fn apply_avwap_demo(&mut self) {
        /// Fewest bars the demo waits for before anchoring — enough for the
        /// average and one band pair to visibly develop.
        const DEMO_AVWAP_MIN_SLOTS: usize = 12;
        /// How far back of the newest bar the demo drops its anchor.
        const DEMO_AVWAP_LOOKBACK_BARS: usize = 40;
        if !self.harness.avwap_demo() {
            return;
        }
        let slots = self.active_tab_mut().flow_pane.slots();
        if slots < DEMO_AVWAP_MIN_SLOTS {
            return;
        }
        self.harness.avwap_demo_placed();
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == crate::avwap::TOOL_ID)
        else {
            return;
        };
        let pane = &mut self.active_tab_mut().flow_pane;
        // Far enough back for the average to develop, on a bar that exists.
        let anchor = slots
            .saturating_sub(DEMO_AVWAP_LOOKBACK_BARS)
            .min(slots - 1);
        let close = pane
            .closed_bar(anchor)
            .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
            .unwrap_or(1.0);
        pane.drawings.place_with(
            tool,
            &drawings::DrawingBand::Price,
            drawings::ChartPoint::at_time(anchor as f32, close, pane.slot_open_time(anchor)),
            |tool| {
                let mut payload = tool.default_payload();
                if let Some(avwap) = payload
                    .as_any_mut()
                    .downcast_mut::<drawings::AvwapPayload>()
                {
                    // 1σ ships on; the demo also opens 2σ so the stack and
                    // its fill layering are on screen.
                    avwap.bands[1].on = true;
                }
                drawings::NewDrawing {
                    style: tool.default_style(),
                    payload,
                }
            },
        );
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
        self.surfaces.drawing_chrome.carry_across_selection();
    }

    /// The `bands` half of the demo hook: on every indicator pane, a level on
    /// the band's own value and a diagonal across it.
    ///
    /// Two objects, not seventeen: a pane is a fifth of the chart's height,
    /// and a screenshot of every tool stacked in one would prove nothing
    /// about the projection it exists to check. The level is placed *at a
    /// value the series actually holds*, so a drawing that has drifted off
    /// its curve is visible at a glance.
    fn seed_band_demo(pane: &mut ChartPane, first: usize, visible: usize, slots: usize) {
        let level_slot = (first + visible / 2).min(slots.saturating_sub(1));
        let (left, right) = (
            (first + visible / 8).min(slots.saturating_sub(1)),
            (first + visible * 3 / 4).min(slots.saturating_sub(1)),
        );
        for (band, value) in pane.indicator_band_samples(level_slot) {
            for tool in drawings::DRAWING_TOOLS {
                let anchors: &[(usize, f64)] = match tool.id() {
                    "horizontal-line" => &[(0, 0.0)],
                    "trend-line" => &[(1, 0.0), (2, 0.0)],
                    _ => continue,
                };
                for (which, _) in anchors {
                    let (slot, value) = match which {
                        // The level sits on the sampled value itself.
                        0 => (level_slot, value),
                        // The diagonal spans the window around it, so its
                        // ends are inside the band without being on the curve.
                        1 => (left, value * 0.5),
                        _ => (right, value * 1.5),
                    };
                    let point = drawings::ChartPoint::at_time(
                        slot as f32 + 0.5,
                        value,
                        pane.slot_open_time(slot),
                    );
                    pane.drawings
                        .place_with(tool, &band, point, |tool| drawings::NewDrawing {
                            style: drawings::DrawingStyle::default(),
                            payload: tool.default_payload(),
                        });
                }
            }
        }
    }

    /// The `QUANTICK_DRAWINGS_DEMO_RECUT` hook: re-cut the bars under the demo
    /// objects, so a screenshot shows what a timeframe switch does to them.
    ///
    /// It is the only way to reach the two surfaces this behaviour added
    /// without a human touching the BARS selector: marks that survived a
    /// re-cut and are still on their own instants, and a mark the new series
    /// cannot reach, faded and labelled off-series. One extra object is placed
    /// an hour before the first bar to produce the second.
    fn apply_drawing_demo_recut(&mut self) {
        if !self.harness.drawings_demo_recut() {
            return;
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        // An anchor before anything the tab has loaded: honest input for the
        // off-series path, not a flag set by hand.
        if let Some(first) = pane.slot_open_time(0) {
            let base = pane
                .closed_bar(0)
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .unwrap_or(1.0);
            // A one-anchor tool by name, not `DRAWING_TOOLS[0]` — that is the
            // trend line, which needs two, so a single `place_with` left a
            // half-finished draft and no object at all. The whole point here
            // is to produce one *completed* off-series mark.
            let single_anchor = drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == "horizontal-line");
            if let Some(tool) = single_anchor {
                let placed = pane.drawings.place_with(
                    tool,
                    &drawings::DrawingBand::Price,
                    drawings::ChartPoint::at_time(0.5, base, Some(first - DEMO_OFF_SERIES_LEAD_MS)),
                    |tool| drawings::NewDrawing {
                        style: drawings::DrawingStyle::default(),
                        payload: tool.default_payload(),
                    },
                );
                debug_assert!(placed, "a horizontal line completes on one anchor");
            }
        }
        // Half the bars, same trades — the plainest re-cut there is. Two
        // settle frames because a spec change waits for the selector to hold
        // still for one (`Tab::apply_spec_change`).
        pane.tick_n = pane.tick_n.saturating_mul(2).max(2);
        self.active_tab_mut().apply_spec_changes();
        self.active_tab_mut().apply_spec_changes();
    }
}
