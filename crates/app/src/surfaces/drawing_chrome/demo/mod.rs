//! Default-off drawing scenarios, consumed at their original frame phases.
#![cfg(any(feature = "drawing-harness", test))]
mod gallery;
#[cfg(test)]
mod tests;

use crate::drawings::{DRAWING_TOOLS, DrawingTool};
use eframe::egui;
use gallery::{Anchor, DrawingRecipient, Look};
use std::ffi::OsString;

pub(crate) use gallery::{DraftPlan, apply_recut_marker, demo_recut_count};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct DrawingsDemo {
    pub bands: bool,
    pub shared: bool,
    pub select_tool: Option<String>,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrvpDemo {
    pub compare: bool,
    pub stress: bool,
    pub select: bool,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawingDraft {
    pub anchors: usize,
    pub constrain: bool,
}

#[derive(Default)]
pub(crate) struct DrawingDemoLaunch {
    gallery: Option<DrawingsDemo>,
    recut: bool,
    profile: Option<FrvpDemo>,
    avwap: bool,
    draft: Option<DrawingDraft>,
}
impl DrawingDemoLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut read = |name| lookup(name).and_then(|value| value.into_string().ok());
        let gallery = read("QUANTICK_DRAWINGS_DEMO")
            .filter(|value| matches!(value.as_str(), "1" | "bands"))
            .map(|value| DrawingsDemo {
                bands: value == "bands",
                shared: read("QUANTICK_DRAWINGS_DEMO_SHARED").is_some_and(|value| value == "1"),
                select_tool: read("QUANTICK_DRAWINGS_DEMO_SELECT"),
            });
        let recut = read("QUANTICK_DRAWINGS_DEMO_RECUT").is_some_and(|value| value == "1");
        let profile = read("QUANTICK_FRVP_DEMO")
            .filter(|value| matches!(value.trim(), "1" | "compare" | "stress"))
            .map(|value| FrvpDemo {
                compare: value.trim() == "compare",
                stress: value.trim() == "stress",
                select: read("QUANTICK_FRVP_DEMO_SELECT").is_some_and(|value| value.trim() == "1"),
            });
        let avwap = read("QUANTICK_AVWAP_DEMO").is_some_and(|value| value.trim() == "1");
        let draft = read("QUANTICK_DRAWING_DRAFT")
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|anchors| *anchors > 0)
            .map(|anchors| DrawingDraft {
                anchors,
                constrain: read("QUANTICK_DRAWING_CONSTRAIN").is_some_and(|value| value == "1"),
            });
        Self {
            gallery,
            recut,
            profile,
            avwap,
            draft,
        }
    }
}

#[derive(Default)]
pub(crate) struct DrawingDemoState {
    input: DrawingDemoLaunch,
}
impl DrawingDemoState {
    pub fn install(&mut self, input: DrawingDemoLaunch) {
        self.input = input;
    }
    pub fn gallery_requested(&self) -> bool {
        self.input.gallery.is_some()
    }
    pub fn take_gallery(&mut self, slots: usize) -> Option<DrawingsDemo> {
        if slots < 8 * DRAWING_TOOLS.len() {
            return None;
        }
        self.input.gallery.take()
    }
    pub fn recut_requested(&self) -> bool {
        self.input.recut
    }
    pub fn profile_requested(&self) -> Option<FrvpDemo> {
        self.input.profile
    }
    pub fn avwap_requested(&self) -> bool {
        self.input.avwap
    }
    pub fn draft_requested(&self) -> Option<DrawingDraft> {
        self.input.draft
    }
    pub fn take_draft(
        &mut self,
        tool: Option<DrawingTool>,
        chart: Option<egui::Rect>,
        facts: SeriesFacts,
    ) -> Option<DraftPlan> {
        let request = self.input.draft?;
        let tool = tool?;
        let chart = chart?;
        if facts.slots == 0 {
            return None;
        }
        self.input.draft = None;
        Some(request.plan(tool, chart, facts))
    }
    #[cfg(test)]
    pub fn arm_drawings_demo(&mut self, demo: DrawingsDemo) {
        self.input.gallery = Some(demo);
    }
}

/// Fresh geometry facts; timestamp projection happens only for chosen slots.
#[derive(Clone, Copy)]
pub(crate) struct SeriesFacts {
    pub slots: usize,
    pub last_close: Option<f64>,
    pub auto_range: Option<(f64, f64)>,
}
impl SeriesFacts {
    fn window(self) -> (usize, usize, f64, f64) {
        let visible = 90.min(self.slots);
        let close = self.last_close.unwrap_or(1.0);
        let (center, band) = self
            .auto_range
            .filter(|(lo, hi)| hi > lo)
            .map_or((close, close * 0.004), |(lo, hi)| {
                ((lo + hi) / 2.0, hi - lo)
            });
        (visible, self.slots - visible, center, band)
    }
}

pub(crate) enum ProfilePreparation {
    Wait,
    Ready,
    Prefix { candles: i64 },
}
pub(crate) struct ProfileFacts {
    pub slots: usize,
    pub prefix: usize,
}
pub(crate) struct ProfilePlan {
    anchors: Vec<Anchor>,
    select: bool,
}

impl DrawingDemoState {
    pub fn prepare_profile(&mut self, slots: Option<usize>) -> ProfilePreparation {
        self.prepare_profile_with_tool(
            slots,
            DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == crate::frvp::TOOL_ID),
        )
    }
    fn prepare_profile_with_tool(
        &mut self,
        slots: Option<usize>,
        tool: Option<DrawingTool>,
    ) -> ProfilePreparation {
        let Some(request) = self.input.profile else {
            return ProfilePreparation::Wait;
        };
        if slots.is_none_or(|slots| slots < 12) {
            return ProfilePreparation::Wait;
        }
        if tool.is_none() {
            self.input.profile = None;
            return ProfilePreparation::Wait;
        }
        if request.stress {
            ProfilePreparation::Prefix { candles: 25_000 }
        } else {
            ProfilePreparation::Ready
        }
    }
    pub fn finish_profile(&mut self, facts: ProfileFacts) -> Option<ProfileGeometry> {
        let request = self.input.profile.take()?;
        if request.compare && facts.slots.saturating_sub(facts.prefix) < 60 {
            self.input.profile = Some(request);
            return None;
        }
        Some(ProfileGeometry::new(request, facts))
    }
    pub fn take_avwap(&mut self, slots: usize) -> Option<AvwapGeometry> {
        self.take_avwap_with_lookup(slots, || {
            DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == crate::avwap::TOOL_ID)
        })
    }
    fn take_avwap_with_lookup(
        &mut self,
        slots: usize,
        lookup: impl FnOnce() -> Option<DrawingTool>,
    ) -> Option<AvwapGeometry> {
        if !self.input.avwap || slots < 12 {
            return None;
        }
        self.input.avwap = false;
        let tool = lookup()?;
        Some(AvwapGeometry {
            tool,
            slot: slots.saturating_sub(40).min(slots - 1),
        })
    }
}
pub(crate) struct ProfileGeometry {
    request: FrvpDemo,
    ranges: Vec<(usize, usize, bool)>,
    pub close_slot: usize,
}
impl ProfileGeometry {
    fn new(request: FrvpDemo, facts: ProfileFacts) -> Self {
        let start = if facts.prefix > 0 && facts.prefix < facts.slots {
            facts.prefix.saturating_sub(5)
        } else {
            facts.slots.saturating_sub(30)
        };
        let end = (start + 29).min(facts.slots - 1);
        let newest = facts.slots - 1;
        let ranges = if request.stress {
            vec![(0, newest, true)]
        } else if request.compare {
            vec![
                (newest.saturating_sub(49), newest.saturating_sub(25), true),
                (newest.saturating_sub(24), newest, false),
            ]
        } else {
            vec![(start, end, true)]
        };
        Self {
            request,
            ranges,
            close_slot: end,
        }
    }
    pub fn plan(self, close: Option<f64>) -> ProfilePlan {
        let tool = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == crate::frvp::TOOL_ID)
            .expect("profile tool checked before history delivery");
        let anchors = self
            .ranges
            .into_iter()
            .flat_map(|(from, to, outline)| {
                [from, to].map(|slot| {
                    Anchor::new(
                        tool,
                        slot,
                        slot as f32,
                        close.unwrap_or(1.0),
                        Look::Profile { outline },
                    )
                })
            })
            .collect();
        ProfilePlan {
            anchors,
            select: self.request.select,
        }
    }
}
pub(crate) struct AvwapGeometry {
    tool: DrawingTool,
    pub slot: usize,
}
impl AvwapGeometry {
    pub fn plan(self, close: Option<f64>) -> ProfilePlan {
        ProfilePlan {
            anchors: vec![Anchor::new(
                self.tool,
                self.slot,
                self.slot as f32,
                close.unwrap_or(1.0),
                Look::Avwap,
            )],
            select: false,
        }
    }
}
impl ProfilePlan {
    pub fn project_times(&mut self, mut time: impl FnMut(usize) -> Option<i64>) {
        for anchor in &mut self.anchors {
            anchor.project(&mut time);
        }
    }
    pub fn apply(&self, target: &mut impl DrawingRecipient) {
        for (index, anchor) in self.anchors.iter().enumerate() {
            anchor.apply(target);
            if self.select && index % 2 == 1 {
                target.select(target.len().saturating_sub(1));
            }
        }
    }
    pub fn carries_inspector(&self) -> bool {
        self.select
    }
}
