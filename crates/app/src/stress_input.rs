//! `QUANTICK_STRESS_INPUT=<period_ms>`: a stress run's scripted hand.
//!
//! Every period a pan, a wheel notch or a double click goes into the active
//! tab's flow pane as real pointer events, through `raw_input_hook` — the
//! same path a trader's mouse takes. The gestures, their schedule and the
//! judging are [`ScriptedHand`]'s; this file turns its events into egui's and
//! reads the pane back. `tools/stress/replay_stress.py` grades the log.

use std::time::{Duration, Instant};

use eframe::egui;
use quantick_chart_interaction::input_probe::Fingerprint;
use quantick_chart_interaction::scripted_hand::{Area, HandEvent, ScriptedHand, Targets};

use crate::pane::ChartPane;

/// How often the running totals are logged.
const SUMMARY_EVERY: Duration = Duration::from_secs(5);

/// The scripted hand, with the clock it is told.
pub(crate) struct StressInput {
    hand: ScriptedHand,
    started: Instant,
    last_frame: Option<Instant>,
    last_summary: Instant,
}

impl StressInput {
    /// Parse the hook's value: a period in milliseconds, at least one. Any
    /// other value is logged and arms nothing.
    pub(crate) fn from_env_value(value: &str) -> Option<Self> {
        let Some(period_ms) = value.trim().parse::<u64>().ok().filter(|ms| *ms > 0) else {
            tracing::warn!(target: "quantick", value, "STRESS_INPUT_HOOK_REJECTED: expected a period in milliseconds");
            return None;
        };
        let now = Instant::now();
        Some(Self {
            hand: ScriptedHand::new(period_ms),
            started: now,
            last_frame: None,
            last_summary: now,
        })
    }

    /// Once a frame, before egui reads the input: judge the frame just drawn
    /// and inject this one's events.
    pub(crate) fn drive(&mut self, flow: &ChartPane, raw_input: &mut egui::RawInput) {
        let now = Instant::now();
        let interval = self
            .last_frame
            .replace(now)
            .map(|last| (now - last).as_secs_f32() * 1000.0);
        let area =
            |rect: egui::Rect| Area::new(rect.left(), rect.top(), rect.right(), rect.bottom());
        let targets = flow
            .frame
            .chart_rect
            .zip(flow.frame.time_strip)
            .map(|(chart, time)| Targets {
                candles: area(crate::bands::drawing_area(chart, flow.frame.lane_divider_x)),
                time_axis: area(time),
            });
        // What a gesture changes when answered: the view, or a mark a press
        // on an unlocked one dragged instead. A locked one moves neither.
        let fingerprint = Fingerprint {
            view: flow.model.viewport.gesture_fingerprint(),
            marks: flow.drawings.revision(),
        };
        let elapsed_ms = self.started.elapsed().as_millis() as u64;
        let step = self.hand.frame(elapsed_ms, interval, fingerprint, targets);
        raw_input
            .events
            .extend(step.events.into_iter().map(to_egui));
        if let Some(outcome) = step.outcome {
            let event_code = match outcome.frames {
                Some(_) => "STRESS_INPUT_ANSWERED",
                None => "STRESS_INPUT_STALL",
            };
            let gesture = outcome.gesture.name();
            tracing::info!(target: "quantick::app", event_code, gesture, frames = ?outcome.frames, by_mark = outcome.by_mark, "a scripted gesture");
        }
        if now - self.last_summary >= SUMMARY_EVERY {
            self.last_summary = now;
            let summary = self.hand.summary();
            tracing::info!(target: "quantick::app", event_code = "STRESS_INPUT_SUMMARY", ?summary, "stress input totals");
        }
    }
}

fn to_egui(event: HandEvent) -> egui::Event {
    let button = |x, y, pressed| egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    match event {
        HandEvent::Move { x, y } => egui::Event::PointerMoved(egui::pos2(x, y)),
        HandEvent::Press { x, y } => button(x, y, true),
        HandEvent::Release { x, y } => button(x, y, false),
        HandEvent::Wheel { dy } => egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            modifiers: egui::Modifiers::default(),
        },
    }
}
