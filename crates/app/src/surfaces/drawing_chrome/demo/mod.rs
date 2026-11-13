//! Default-off drawing scenarios, consumed at their original frame phases.
#![cfg(any(feature = "drawing-harness", test))]
mod gallery;
#[cfg(test)]
mod tests;

pub(crate) use super::launch::DrawingDemoLaunch;
#[cfg(test)]
use crate::drawings::{DRAWING_TOOLS, DrawingTool};
#[cfg(test)]
use gallery::Look;
use gallery::{AnchorHost, DrawingRecipient};
#[cfg(test)]
use std::ffi::OsString;

pub(crate) use gallery::{apply_recut_marker, demo_recut_count};

pub(crate) use gallery::{DraftHost, GalleryHost, ProfileHost};
use quantick_ui_model::drawing_chrome::scenario;
pub(crate) use quantick_ui_model::drawing_chrome::scenario::{
    ProfileFacts, ProfilePreparation, SeriesFacts,
};
/// Tool resolution occurs only after the model consumes a ready request.
pub(crate) fn resolve_avwap(
    effect: Option<scenario::Effect>,
    lookup: impl FnOnce() -> Option<crate::drawings::DrawingTool>,
) -> Option<(scenario::AvwapGeometry, crate::drawings::DrawingTool)> {
    let scenario::Effect::Avwap(geometry) = effect? else {
        return None;
    };
    Some((geometry, lookup()?))
}

// The parked draft binds the model's normalized offset to the real chart.
use crate::{drawings, pane::ParkedHand};
use eframe::egui;
use scenario::DraftPlan;
impl DraftHost for DraftPlan {
    fn project_times(&mut self, mut time: impl FnMut(usize) -> Option<i64>) {
        for anchor in &mut self.anchors {
            anchor.project(&mut time);
        }
    }
    fn apply(&self, target: &mut impl DrawingRecipient) {
        for anchor in &self.anchors {
            anchor.apply(target);
        }
    }
    fn hand(&self, chart: egui::Rect) -> ParkedHand {
        ParkedHand {
            position: drawings::px(
                chart.center() + egui::vec2(self.hand[0], self.hand[1]) * chart.size(),
            ),
            constrain: if self.constrain {
                drawings::Constrain::Level
            } else {
                drawings::Constrain::Free
            },
        }
    }
}

#[cfg(test)]
pub(crate) use quantick_ui_model::drawing_chrome::scenario::{
    DrawingDraft, DrawingsDemo, FrvpDemo,
};
