//! The stress run's hand: the pointer events each gesture is made of, one
//! frame at a time, and the [`InputProbe`] that judges whether the chart
//! answered.
//!
//! The host calls [`ScriptedHand::frame`] once per frame, before the window
//! reads its input, with the interval since the last call and a fingerprint
//! of what the gestures move. It gets back the events to inject and, when one
//! is decided, the outcome of the gesture in flight. Points are window
//! points; nothing here knows the UI library.

use std::collections::VecDeque;

use crate::input_probe::{Gesture, InputProbe, Outcome, Summary};

/// How far a scripted pan drags, in points.
pub const PAN_POINTS: f32 = 160.0;
/// One wheel notch, in points.
pub const WHEEL_POINTS: f32 = 60.0;

/// One pointer event of a gesture, in window points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HandEvent {
    Move { x: f32, y: f32 },
    Press { x: f32, y: f32 },
    Release { x: f32, y: f32 },
    Wheel { dy: f32 },
}

/// A rectangle on screen, in window points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Area {
    fn center(self) -> (f32, f32) {
        (
            (self.left + self.right) / 2.0,
            (self.top + self.bottom) / 2.0,
        )
    }
}

/// Where the gestures land: the candles, and the time axis under them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Targets {
    pub candles: Area,
    pub time_axis: Area,
}

/// What one frame of the hand comes to.
#[derive(Debug, Default, PartialEq)]
pub struct HandFrame {
    /// The events to inject this frame, in order.
    pub events: Vec<HandEvent>,
    /// The gesture decided as of the last frame drawn, if one was.
    pub outcome: Option<Outcome>,
}

/// The gesture as pointer events, one entry per frame.
///
/// Each opens with a frame that only moves the pointer there, as a hand
/// arrives before it presses: the window decides what a press grabs from
/// where the pointer already was. The double click lands on the time axis,
/// where it has one meaning — the live overview — and not on the candles,
/// where it may mean "open this candle's settings" and leave the view alone.
#[must_use]
pub fn script(gesture: Gesture, targets: Targets) -> VecDeque<Vec<HandEvent>> {
    match gesture {
        Gesture::Pan => {
            let (_, y) = targets.candles.center();
            let x = targets.candles.left + (targets.candles.right - targets.candles.left) * 0.3;
            let to = x + PAN_POINTS;
            VecDeque::from([
                vec![HandEvent::Move { x, y }],
                vec![HandEvent::Press { x, y }],
                vec![HandEvent::Move { x: to, y }],
                vec![HandEvent::Release { x: to, y }],
            ])
        }
        Gesture::Zoom => {
            let (x, y) = targets.candles.center();
            VecDeque::from([
                vec![HandEvent::Move { x, y }],
                vec![HandEvent::Wheel { dy: WHEEL_POINTS }],
            ])
        }
        Gesture::DoubleClick => {
            let (x, y) = targets.time_axis.center();
            let click = vec![HandEvent::Press { x, y }, HandEvent::Release { x, y }];
            VecDeque::from([vec![HandEvent::Move { x, y }], click.clone(), click])
        }
    }
}

/// The scripted hand and the probe judging it.
#[derive(Debug, Clone)]
pub struct ScriptedHand {
    probe: InputProbe,
    script: VecDeque<Vec<HandEvent>>,
    /// The frame about to be drawn.
    frame: u64,
}

impl ScriptedHand {
    /// A hand sending one gesture every `period_ms`.
    #[must_use]
    pub fn new(period_ms: u64) -> Self {
        Self {
            probe: InputProbe::new(period_ms),
            script: VecDeque::new(),
            frame: 0,
        }
    }

    /// Close the frame just drawn and open the next. `interval_ms` is the
    /// time since the last call, `fingerprint` what the gestures move as the
    /// last frame left it, `targets` where they land (`None` until drawn).
    pub fn frame(
        &mut self,
        now_ms: u64,
        interval_ms: Option<f32>,
        fingerprint: u64,
        targets: Option<Targets>,
    ) -> HandFrame {
        if let Some(interval_ms) = interval_ms {
            self.probe.record_frame(interval_ms);
        }
        let mut step = HandFrame::default();
        if self.script.is_empty() {
            if let Some(drawn) = self.frame.checked_sub(1) {
                step.outcome = self.probe.observe(drawn, fingerprint);
            }
            if let Some(targets) = targets
                && let Some(gesture) = self.probe.due(now_ms, self.frame, fingerprint)
            {
                self.script = script(gesture, targets);
            }
        }
        if let Some(events) = self.script.pop_front() {
            step.events = events;
            if self.script.is_empty() {
                self.probe.delivered(self.frame);
            }
        }
        self.frame += 1;
        step
    }

    /// The totals so far.
    #[must_use]
    pub fn summary(&self) -> Summary {
        self.probe.summary()
    }
}

#[cfg(test)]
mod scripted_hand_tests {
    use super::*;

    const TARGETS: Targets = Targets {
        candles: Area {
            left: 100.0,
            top: 0.0,
            right: 1100.0,
            bottom: 400.0,
        },
        time_axis: Area {
            left: 100.0,
            top: 400.0,
            right: 1100.0,
            bottom: 420.0,
        },
    };

    #[test]
    fn a_pan_arrives_presses_drags_and_lets_go_one_frame_each() {
        let pan = script(Gesture::Pan, TARGETS);
        assert_eq!(
            Vec::from(pan),
            vec![
                vec![HandEvent::Move { x: 400.0, y: 200.0 }],
                vec![HandEvent::Press { x: 400.0, y: 200.0 }],
                vec![HandEvent::Move { x: 560.0, y: 200.0 }],
                vec![HandEvent::Release { x: 560.0, y: 200.0 }],
            ]
        );
        let double = script(Gesture::DoubleClick, TARGETS);
        assert_eq!(double.len(), 3);
        assert!(double.iter().flatten().all(|event| match event {
            HandEvent::Move { y, .. }
            | HandEvent::Press { y, .. }
            | HandEvent::Release { y, .. } => *y == 410.0,
            HandEvent::Wheel { .. } => false,
        }));
    }

    #[test]
    fn the_hand_waits_for_targets_then_judges_each_gesture_after_its_last_event() {
        let mut hand = ScriptedHand::new(100);
        assert_eq!(hand.frame(0, None, 7, None), HandFrame::default());
        assert!(
            hand.frame(50, Some(16.0), 7, Some(TARGETS))
                .events
                .is_empty()
        );
        let frames: Vec<HandFrame> = (0..4)
            .map(|n| hand.frame(150 + n, Some(16.0), 7, Some(TARGETS)))
            .collect();
        assert!(frames.iter().all(|frame| frame.outcome.is_none()));
        assert_eq!(
            frames[3].events,
            vec![HandEvent::Release { x: 560.0, y: 200.0 }]
        );
        let answered = hand.frame(160, Some(16.0), 8, Some(TARGETS));
        assert_eq!(
            answered.outcome,
            Some(Outcome {
                gesture: Gesture::Pan,
                frames: Some(0)
            })
        );
        let summary = hand.summary();
        assert_eq!((summary.sent, summary.on_time), (1, 1));
    }
}
