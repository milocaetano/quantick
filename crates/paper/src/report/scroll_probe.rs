//! Bounded report-scroll scenario, driven by frames supplied by its host.

use std::collections::VecDeque;

const MAX_STAGES: usize = 4;
const MAX_NOTCHES: u16 = 120;
const SETTLE_FRAMES: u16 = 120;
const INTERVAL_FRAMES: u16 = 12;
const NOTCH_POINTS: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollTarget {
    Report,
    Rows,
}

#[derive(Clone, Debug)]
pub struct ScrollProbe {
    pub target: ScrollTarget,
    remaining: u16,
    delay: u16,
    delta: f32,
}

/// Painted rectangles supplied by a UI adapter; no target exists before paint.
#[derive(Default)]
pub struct ReportScroll {
    plan: VecDeque<ScrollProbe>,
    armed: bool,
    report: Option<[f32; 4]>,
    rows: Option<[f32; 4]>,
}
impl ReportScroll {
    pub fn arm(&mut self, value: &str) -> bool {
        let plans = value
            .split(',')
            .map(ScrollProbe::parse)
            .collect::<Option<VecDeque<_>>>();
        self.plan = plans
            .filter(|plans| {
                plans.len() <= MAX_STAGES
                    && plans.iter().map(|p| p.remaining).sum::<u16>() <= MAX_NOTCHES
            })
            .unwrap_or_default();
        self.armed = !self.plan.is_empty();
        self.armed
    }
    pub fn active(&self) -> bool {
        self.armed
    }
    pub fn report(&mut self, rect: [f32; 4]) {
        self.report = Some(rect);
        self.rows = None;
    }
    pub fn rows(&mut self, rect: [f32; 4]) {
        self.rows = Some(rect);
    }
    /// A wheel event only over the selected, actually painted report surface.
    pub fn input(&mut self, open: bool) -> Option<(f32, f32, f32)> {
        if self.plan.front().is_some_and(|plan| plan.remaining == 0) {
            self.plan.pop_front();
        }
        let plan = self.plan.front_mut()?;
        let rect = match plan.target {
            ScrollTarget::Report => self.report,
            ScrollTarget::Rows => self.rows,
        };
        let rect = rect.filter(|r| open && r[2] > r[0] && r[3] > r[1]);
        let delta = plan.frame(rect.is_some())?;
        let [left, top, right, bottom] = rect?;
        let x = match plan.target {
            ScrollTarget::Report => right - 1.0,
            ScrollTarget::Rows => (left + right) / 2.0,
        };
        Some((x, (top + bottom) / 2.0, delta))
    }
}

impl ScrollProbe {
    /// `report:-20` or `rows:20`: signed wheel notches, at most 120.
    /// Negative moves down, matching an ordinary wheel event.
    pub fn parse(value: &str) -> Option<Self> {
        let (target, count) = value.trim().split_once(':')?;
        let target = match target {
            "report" => ScrollTarget::Report,
            "rows" => ScrollTarget::Rows,
            _ => return None,
        };
        let count = count
            .parse::<i16>()
            .ok()
            .filter(|n| *n != 0 && n.unsigned_abs() <= MAX_NOTCHES)?;
        Some(Self {
            target,
            remaining: count.unsigned_abs(),
            delay: SETTLE_FRAMES,
            delta: NOTCH_POINTS * f32::from(count.signum()),
        })
    }

    /// Wait for painted geometry, then deliver one notch every twelve frames.
    /// A missing target consumes the finite wait budget but never emits input.
    pub fn frame(&mut self, ready: bool) -> Option<f32> {
        if self.remaining == 0 {
            return None;
        }
        if self.delay > 0 {
            self.delay -= 1;
            return None;
        }
        if !ready {
            self.remaining = 0;
            return None;
        }
        self.remaining -= 1;
        self.delay = INTERVAL_FRAMES;
        Some(self.delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_or_unbounded_requests_do_not_arm() {
        for value in [
            "report:0",
            "report:121",
            "rows:-32768",
            "chart:3",
            "rows:NaN",
        ] {
            assert!(ScrollProbe::parse(value).is_none(), "{value}");
        }
    }
    #[test]
    fn wheel_delivery_is_bounded_and_waits_for_real_geometry() {
        let mut probe = ScrollProbe::parse("report:-2").unwrap();
        let steps: Vec<_> = (0..1000).filter_map(|_| probe.frame(true)).collect();
        assert_eq!(steps, [-120.0, -120.0]);
        let mut absent = ScrollProbe::parse("rows:2").unwrap();
        assert!((0..1000).all(|_| absent.frame(false).is_none()));
        assert!(absent.frame(true).is_none());
    }

    #[test]
    fn sequence_targets_painted_surfaces_with_separate_settling() {
        let mut probe = ReportScroll::default();
        assert!(probe.arm("report:-2,rows:1"));
        probe.report([10.0, 20.0, 100.0, 80.0]);
        probe.rows([30.0, 40.0, 70.0, 60.0]);
        let events: Vec<_> = (0..1000)
            .filter_map(|frame| probe.input(true).map(|event| (frame, event)))
            .collect();
        assert_eq!(
            events,
            [
                (120, (99.0, 50.0, -120.0)),
                (133, (99.0, 50.0, -120.0)),
                (254, (50.0, 50.0, 120.0)),
            ]
        );
    }

    #[test]
    fn stage_and_total_notch_budgets_reject_the_entire_plan() {
        let mut probe = ReportScroll::default();
        assert!(probe.arm("report:30,rows:-30,report:30,rows:-30"));
        for invalid in [
            "report:1,rows:1,report:1,rows:1,report:1",
            "report:60,rows:-61",
            "report:1,rows:0",
            "report:1,",
            "",
        ] {
            assert!(!probe.arm(invalid), "{invalid}");
            assert!(!probe.active());
            probe.report([0.0, 0.0, 100.0, 100.0]);
            assert!((0..2000).all(|_| probe.input(true).is_none()));
        }
    }

    #[test]
    fn closed_or_missing_targets_expire_and_cannot_replay_later() {
        for open in [false, true] {
            let mut probe = ReportScroll::default();
            assert!(probe.arm("report:-120"));
            if !open {
                probe.report([0.0, 0.0, 100.0, 100.0]);
            }
            assert!((0..122).all(|_| probe.input(open).is_none()));
            probe.report([0.0, 0.0, 100.0, 100.0]);
            assert!((0..2000).all(|_| probe.input(true).is_none()));
        }
    }

    #[test]
    fn missing_rows_do_not_block_later_report_stage() {
        let mut probe = ReportScroll::default();
        assert!(!probe.arm("rows:-120,report:1"));
        assert!(!probe.active());
        assert!(probe.arm("rows:-1,report:1"));
        probe.rows([0.0, 0.0, 100.0, 100.0]);
        // A new report paint invalidates the previous nested viewport.
        probe.report([0.0, 0.0, 100.0, 100.0]);
        let events: Vec<_> = (0..1000).filter_map(|_| probe.input(true)).collect();
        assert_eq!(events, [(99.0, 50.0, 120.0)]);
    }
}
