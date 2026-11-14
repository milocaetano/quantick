//! Opt-in drawing-chrome launch scenarios and the note placement policy.
//! Normal editing stays on the ordinary owner; only scripted requests live here.
#![cfg(any(feature = "drawing-harness", test))]

use std::ffi::OsString;

use eframe::egui;

use super::{Chrome, InspectorTab};
use quantick_ui_model::drawing_chrome::scenario;
use scenario::{DrawingDraft, DrawingsDemo, FrvpDemo};

#[derive(Default)]
pub(crate) struct DrawingChromeLaunch {
    demos: super::demo::DrawingDemoLaunch,
    manager: bool,
    inspector: bool,
    note: bool,
    tab: Option<InspectorTab>,
    rejected_tab: Option<String>,
    inspector_position: Option<egui::Pos2>,
    bar_position: Option<egui::Pos2>,
}

impl DrawingChromeLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut read = |name| lookup(name).and_then(|value| value.into_string().ok());
        let manager = read("QUANTICK_DRAWINGS_MANAGER").is_some_and(|value| value == "1");
        let inspector = read("QUANTICK_DRAWING_INSPECTOR").is_some_and(|value| value == "1");
        let note = read("QUANTICK_TEXT_NOTE").is_some_and(|value| value == "1");
        let (tab, rejected_tab) = match read("QUANTICK_DRAWING_INSPECTOR_TAB") {
            None => (None, None),
            Some(value) => match value.trim() {
                "style" => (Some(InspectorTab::Style), None),
                "extra" => (Some(InspectorTab::Extra), None),
                "coordinates" => (Some(InspectorTab::Coordinates), None),
                other => (None, Some(other.to_owned())),
            },
        };
        let inspector_position =
            read("QUANTICK_DRAWING_INSPECTOR_POS").and_then(|value| parse_point(&value));
        let bar_position = read("QUANTICK_CONTEXT_BAR_POS").and_then(|value| parse_point(&value));
        Self {
            demos: super::demo::DrawingDemoLaunch::capture(lookup),
            manager,
            inspector,
            note,
            tab,
            rejected_tab,
            inspector_position,
            bar_position,
        }
    }
}
impl Chrome<'_> {
    pub fn queue_launch(&mut self, input: DrawingChromeLaunch) {
        self.view.rejected_tab = input.rejected_tab;
        self.emit(super::Intent::Scenario(scenario::Intent::QueueLaunch(
            scenario::Launch {
                demos: input.demos.into(),
                manager: input.manager,
                inspector: input.inspector,
                note: input.note,
                tab: input.tab,
                inspector_position: input.inspector_position.map(|p| [p.x, p.y]),
                bar_position: input.bar_position.map(|p| [p.x, p.y]),
            },
        )));
    }
    pub fn apply_launch(&mut self) {
        let mut effects = self.emit(super::Intent::Scenario(scenario::Intent::ApplyLaunch));
        if let Some(scenario::Effect::Launch {
            inspector_position,
            bar_position,
        }) = effects.take_scenario()
        {
            if inspector_position.is_some() {
                self.view.inspector.max_width = None;
            }
            if let Some([x, y]) = bar_position {
                self.view.bar.bar.set_manual(egui::pos2(x, y));
            }
        }
        if let Some(tab) = self.view.rejected_tab.take() {
            tracing::warn!(tab, "unknown drawing inspector tab");
        }
    }
}
/// Binds the model's note plan to the host's timestamp vocabulary.
pub(crate) struct NotePlacement;
impl NotePlacement {
    pub fn point(plan: scenario::NotePlacement, time: Option<i64>) -> crate::drawings::ChartPoint {
        crate::drawings::ChartPoint::at_time(plan.slot as f32 + 0.5, plan.price, time)
    }
    pub fn tool() -> Option<crate::drawings::DrawingTool> {
        crate::drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
    }
}

/// Screen points: malformed or nonfinite input leaves automatic placement.
fn parse_point(raw: &str) -> Option<egui::Pos2> {
    let (x, y) = raw.split_once(',')?;
    let x: f32 = x.trim().parse().ok()?;
    let y: f32 = y.trim().parse().ok()?;
    (x.is_finite() && y.is_finite()).then_some(egui::pos2(x, y))
}

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct DrawingDemoLaunch {
    pub(super) gallery: Option<DrawingsDemo>,
    pub(super) recut: bool,
    pub(super) profile: Option<FrvpDemo>,
    pub(super) avwap: bool,
    pub(super) draft: Option<DrawingDraft>,
}
impl DrawingDemoLaunch {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut read = |name| lookup(name).and_then(|value| value.into_string().ok());
        let gallery = read("QUANTICK_DRAWINGS_DEMO")
            .filter(|value| matches!(value.as_str(), "1" | "bands"))
            .map(|value| DrawingsDemo {
                bands: value == "bands",
                shared: read("QUANTICK_DRAWINGS_DEMO_SHARED")
                    .filter(|value| value == "0" || value == "1")
                    .map(|value| value == "1"),
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

impl From<DrawingDemoLaunch> for scenario::DrawingDemoLaunch {
    fn from(input: DrawingDemoLaunch) -> Self {
        Self {
            gallery: input.gallery,
            recut: input.recut,
            profile: input.profile,
            avwap: input.avwap,
            draft: input.draft,
        }
    }
}
