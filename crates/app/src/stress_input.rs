//! `QUANTICK_STRESS_INPUT=<period_ms>`: a stress run's scripted hand.
//!
//! Every period a pan, a wheel notch or a double click goes into the active
//! tab's flow pane as real pointer events, through `raw_input_hook` — the
//! same path a trader's mouse takes. The schedule, the answer ledger and the
//! frame percentiles are [`InputProbe`]'s; this file only turns a gesture into
//! egui events and reads the pane's view back. `tools/stress/replay_stress.py`
//! launches the run and grades the `STRESS_INPUT_SUMMARY` it logs.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eframe::egui;
use quantick_chart_interaction::input_probe::{Gesture, InputProbe};

use crate::pane::ChartPane;

/// How often the running totals are logged.
const SUMMARY_EVERY: Duration = Duration::from_secs(5);
/// How far a scripted pan drags, in points.
const PAN_POINTS: f32 = 160.0;
/// One wheel notch, in points.
const WHEEL_POINTS: f32 = 60.0;

/// The scripted hand and its ledger.
pub(crate) struct StressInput {
    probe: InputProbe,
    /// Events still owed to the gesture in flight, one entry per frame.
    script: VecDeque<Vec<egui::Event>>,
    frame: u64,
    started: Instant,
    last_frame: Option<Instant>,
    last_summary: Instant,
}

impl StressInput {
    /// Parse the hook's value: a period in milliseconds, at least one.
    pub(crate) fn from_env_value(value: &str) -> Option<Self> {
        let period_ms = value.trim().parse::<u64>().ok().filter(|ms| *ms > 0)?;
        let now = Instant::now();
        Some(Self {
            probe: InputProbe::new(period_ms),
            script: VecDeque::new(),
            frame: 0,
            started: now,
            last_frame: None,
            last_summary: now,
        })
    }

    /// This frame's scripted events, before egui reads the input.
    pub(crate) fn push_input(&mut self, flow: &ChartPane, raw_input: &mut egui::RawInput) {
        if self.script.is_empty() {
            let Some(area) = candle_area(flow) else {
                return;
            };
            let now_ms = self.started.elapsed().as_millis() as u64;
            let before = flow.model.viewport.gesture_fingerprint();
            if let Some(gesture) = self.probe.due(now_ms, self.frame, before) {
                self.script = script(gesture, area);
            }
        }
        if let Some(events) = self.script.pop_front() {
            raw_input.events.extend(events);
            if self.script.is_empty() {
                self.probe.delivered(self.frame);
            }
        }
    }

    /// After the frame drew: pace it, see whether the view answered, and log.
    pub(crate) fn after_frame(&mut self, flow: &ChartPane, now: Instant) {
        if let Some(last) = self.last_frame {
            self.probe.record_frame((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);
        if self.script.is_empty()
            && let Some(outcome) = self
                .probe
                .observe(self.frame, flow.model.viewport.gesture_fingerprint())
        {
            match outcome.frames {
                Some(frames) => tracing::info!(
                    target: "quantick::app",
                    event_code = "STRESS_INPUT_ANSWERED",
                    gesture = outcome.gesture.name(),
                    frames,
                    "a scripted gesture moved the chart"
                ),
                None => tracing::warn!(
                    target: "quantick::app",
                    event_code = "STRESS_INPUT_STALL",
                    gesture = outcome.gesture.name(),
                    frames = quantick_chart_interaction::input_probe::STALL_FRAMES,
                    "a scripted gesture never moved the chart"
                ),
            }
        }
        self.frame += 1;
        if now - self.last_summary >= SUMMARY_EVERY {
            self.last_summary = now;
            self.log_summary();
        }
    }

    fn log_summary(&self) {
        let s = self.probe.summary();
        tracing::info!(
            target: "quantick::app",
            event_code = "STRESS_INPUT_SUMMARY",
            sent = s.sent,
            answered = s.answered,
            on_time = s.on_time,
            stalls = s.stalls,
            slowest_answer_frames = s.slowest_answer_frames,
            frames = s.frames,
            freezes = s.freezes,
            worst_ms = s.worst_ms,
            p99_ms = s.p99_ms,
            "stress input totals"
        );
    }
}

/// The flow pane's candle area, once the pane has drawn.
fn candle_area(flow: &ChartPane) -> Option<egui::Rect> {
    Some(crate::bands::drawing_area(
        flow.frame.chart_rect?,
        flow.frame.lane_divider_x,
    ))
}

/// The gesture as pointer events, one entry per frame.
fn script(gesture: Gesture, area: egui::Rect) -> VecDeque<Vec<egui::Event>> {
    let at = area.center();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    match gesture {
        Gesture::Pan => {
            let from = egui::pos2(area.left() + area.width() * 0.3, at.y);
            let to = from + egui::vec2(PAN_POINTS, 0.0);
            VecDeque::from([
                vec![egui::Event::PointerMoved(from), button(from, true)],
                vec![egui::Event::PointerMoved(to)],
                vec![button(to, false)],
            ])
        }
        Gesture::Zoom => VecDeque::from([vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, WHEEL_POINTS),
                modifiers: egui::Modifiers::default(),
            },
        ]]),
        Gesture::DoubleClick => VecDeque::from([
            vec![
                egui::Event::PointerMoved(at),
                button(at, true),
                button(at, false),
            ],
            vec![button(at, true), button(at, false)],
        ]),
    }
}
