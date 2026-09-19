//! Opt-in quick-range launch input. The executable captures the value before
//! construction; this owner applies it at the existing drawing-hook phase.
#![cfg(any(feature = "quick-range-harness", test))]

use std::ffi::OsString;

use super::{Owner, QuickRange};
use crate::drawings::{ChartPoint, NewDrawing};
use eframe::egui;
use quantick_chart_interaction::quick_range as core;

#[derive(Default)]
pub(crate) struct QuickRangeLaunch {
    requested: Option<(bool, bool)>,
    rejected: Option<String>,
}

impl QuickRangeLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let Some(value) =
            lookup("QUANTICK_QUICK_RANGE_DEMO").and_then(|value| value.into_string().ok())
        else {
            return Self::default();
        };
        match value.trim() {
            "active" => Self {
                requested: Some((false, false)),
                rejected: None,
            },
            "1" | "ready" => Self {
                requested: Some((true, false)),
                rejected: None,
            },
            "future" => Self {
                requested: Some((true, true)),
                rejected: None,
            },
            other => Self {
                requested: None,
                rejected: Some(other.to_owned()),
            },
        }
    }

    pub fn apply(self, quick: &mut QuickRange) {
        self.deliver(|ready, future| quick.request_demo(ready, future));
    }

    fn deliver(self, request: impl FnOnce(bool, bool)) {
        if let Some((ready, future)) = self.requested {
            request(ready, future);
        }
        if let Some(value) = self.rejected {
            tracing::warn!(value, "unknown quick-range demo state");
        }
    }
}

crate::hooks::declare_hooks!["QUANTICK_QUICK_RANGE_DEMO"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_modes_reach_the_recipient_once_without_another_read() {
        for (value, expected) in [
            ("active", (false, false)),
            ("1", (true, false)),
            (" ready ", (true, false)),
            ("future", (true, true)),
        ] {
            let mut reads = Vec::new();
            let input = QuickRangeLaunch::capture(|name| {
                reads.push(name.to_owned());
                Some(value.into())
            });
            assert_eq!(reads, ["QUANTICK_QUICK_RANGE_DEMO"]);
            let mut calls = Vec::new();
            input.deliver(|ready, future| calls.push((ready, future)));
            assert_eq!(calls, [expected]);
        }
    }

    #[test]
    fn absent_or_unknown_modes_do_not_invent_a_request() {
        for value in [None, Some(""), Some("READY"), Some("unknown")] {
            let input = QuickRangeLaunch::capture(|_| value.map(OsString::from));
            input.deliver(|_, _| panic!("an invalid input must not request a scenario"));
        }
    }
}

impl QuickRange {
    /// Construction queues only the input; the drawing launch phase applies it.
    pub fn queue_launch(&mut self, input: QuickRangeLaunch) {
        self.pending_launch = Some(input);
    }

    pub fn apply_launch(&mut self) {
        if let Some(input) = self.pending_launch.take() {
            input.apply(self);
        }
    }

    pub fn request_demo(&mut self, ready: bool, future: bool) {
        self.demo_requested = Some((ready, future));
    }

    pub fn stage_demo(
        &mut self,
        owner: Owner,
        opening: impl FnOnce(bool) -> Option<([ChartPoint; 2], NewDrawing)>,
    ) {
        let Some((ready, future)) = self.demo_requested else {
            return;
        };
        let Some((anchors, look)) = opening(future) else {
            return;
        };
        self.demo_requested = None;
        self.press(
            owner,
            egui::pos2(0.0, 0.0),
            anchors[0],
            core::GestureEligibility {
                pointer_tool: true,
                unoccluded: true,
                area: core::GestureArea {
                    min: [0.0; 2],
                    max: [20.0; 2],
                },
            },
        );
        self.drag(owner, egui::pos2(10.0, 0.0), anchors[1], 4.0, || look);
        if ready {
            self.release(owner);
        }
    }
}

#[cfg(test)]
mod stage_tests {
    use super::*;
    use crate::drawings;
    use crate::pane::PaneSide;

    #[test]
    fn each_captured_mode_stages_the_real_owner_and_is_consumed_once() {
        for (raw, ready, future) in [
            ("active", false, false),
            ("1", true, false),
            ("ready", true, false),
            ("future", true, true),
        ] {
            let owner = Owner {
                tab: 1,
                pane: 2,
                side: PaneSide::Flow,
                revision: 3,
                layout: Some(4),
            };
            let mut quick = QuickRange::default();
            quick.queue_launch(QuickRangeLaunch::capture(|_| Some(raw.into())));
            assert!(
                quick.demo_requested.is_none(),
                "construction must not stage a demo"
            );
            quick.stage_demo(owner, |_| {
                panic!("the launch phase has not applied its input")
            });
            quick.apply_launch();
            assert!(quick.pending_launch.is_none());
            assert!(!quick.model.present(), "launch waits for the staging phase");
            quick.stage_demo(owner, |requested_future| {
                assert_eq!(requested_future, future);
                let tool = drawings::DrawingTool::by_id("measure").unwrap();
                Some((
                    [ChartPoint::at(1.5, 100.0), ChartPoint::at(10.5, 101.0)],
                    NewDrawing {
                        style: tool.default_style(),
                        payload: tool.default_payload(),
                    },
                ))
            });
            assert_eq!(quick.model.view().unwrap().actionable(), ready);
            quick.dismiss();
            quick.apply_launch();
            quick.stage_demo(owner, |_| panic!("the captured demo was already consumed"));
            assert!(!quick.model.present());
        }
    }

    #[test]
    fn demo_is_one_shot_and_enters_the_production_owner() {
        let owner = Owner {
            tab: 1,
            pane: 2,
            side: PaneSide::Flow,
            revision: 3,
            layout: Some(4),
        };
        let mut quick = QuickRange::default();
        quick.request_demo(true, true);
        quick.stage_demo(owner, |future| {
            assert!(future);
            let tool = drawings::DrawingTool::by_id("measure").unwrap();
            Some((
                [
                    ChartPoint::at_time(1.5, 100.0, Some(1)),
                    ChartPoint::at(10.5, 101.0),
                ],
                NewDrawing {
                    style: tool.default_style(),
                    payload: tool.default_payload(),
                },
            ))
        });
        assert!(quick.model.view().unwrap().actionable());
        quick.dismiss();
        quick.stage_demo(owner, |_| panic!("the demo was consumed"));
        assert!(!quick.model.present());
    }
}
