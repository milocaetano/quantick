//! App-only timestamp projection, pixel binding and actual drawing writes.
use crate::drawings::{self, ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingTool, Drawings};
use crate::pane::ParkedHand;
use eframe::egui;
pub(super) use quantick_ui_model::drawing_chrome::scenario::Look;
use quantick_ui_model::drawing_chrome::scenario::{Anchor, GalleryPlan, ProfilePlan};
use quantick_ui_model::drawing_chrome::scenario::{Effect, Intent};
const DEMO_OFF_SERIES_LEAD_MS: i64 = 3_600_000;
pub(crate) trait GalleryHost {
    fn project_times(&mut self, time: impl FnMut(usize) -> Option<i64>);
    fn apply_main(
        &self,
        target: &mut impl DrawingRecipient,
        phase: impl FnMut(Intent) -> Option<Effect>,
    ) -> Option<f32>;
    fn apply_bands(&self, target: &mut impl DrawingRecipient);
}
pub(crate) trait DraftHost {
    fn project_times(&mut self, time: impl FnMut(usize) -> Option<i64>);
    fn apply(&self, target: &mut impl DrawingRecipient);
    fn hand(&self, chart: egui::Rect) -> ParkedHand;
}
pub(crate) trait ProfileHost {
    fn project_times(&mut self, time: impl FnMut(usize) -> Option<i64>);
    fn apply(&self, target: &mut impl DrawingRecipient);
    fn carries_inspector(&self) -> bool;
}
pub(super) trait AnchorHost {
    fn project(&mut self, time: &mut impl FnMut(usize) -> Option<i64>);
    fn apply(&self, target: &mut impl DrawingRecipient) -> bool;
}
impl GalleryHost for GalleryPlan {
    fn project_times(&mut self, mut time: impl FnMut(usize) -> Option<i64>) {
        for anchor in self.anchors.iter_mut().chain(&mut self.bands) {
            anchor.project(&mut time);
        }
    }
    fn apply_main(
        &self,
        target: &mut impl DrawingRecipient,
        mut phase: impl FnMut(Intent) -> Option<Effect>,
    ) -> Option<f32> {
        for anchor in &self.anchors {
            let completed = anchor.apply(target);
            if matches!(
                phase(Intent::GalleryPlaced {
                    tool: anchor.tool,
                    completed,
                    selected: target.selected()
                }),
                Some(Effect::KeepSelectedLocal)
            ) {
                target.keep_selected_to_chart();
            }
        }
        if let Some(Effect::GallerySelection(Some(index))) = phase(Intent::GalleryFinished) {
            target.select(index);
            target.selected_center()
        } else {
            None
        }
    }
    fn apply_bands(&self, target: &mut impl DrawingRecipient) {
        for anchor in &self.bands {
            anchor.apply(target);
        }
    }
}

impl ProfileHost for ProfilePlan {
    fn project_times(&mut self, mut time: impl FnMut(usize) -> Option<i64>) {
        for anchor in &mut self.anchors {
            anchor.project(&mut time);
        }
    }
    fn apply(&self, target: &mut impl DrawingRecipient) {
        for anchor in &self.anchors {
            anchor.apply(target);
            if anchor.select_after {
                target.select(target.len().saturating_sub(1));
            }
        }
    }
    fn carries_inspector(&self) -> bool {
        self.select
    }
}
pub(crate) fn apply_recut_marker(
    target: &mut impl DrawingRecipient,
    first: Option<i64>,
    close: Option<f64>,
) {
    if let Some(first) = first
        && let Some(tool) = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "horizontal-line")
    {
        let mut anchor = Anchor::new(tool, 0, 0.5, close.unwrap_or(1.0), Look::Default);
        anchor.point.time_ms = Some(first - DEMO_OFF_SERIES_LEAD_MS);
        let placed = anchor.apply(target);
        debug_assert!(placed, "a horizontal line completes on one anchor");
    }
}
pub(crate) fn demo_recut_count(parameter: rust_decimal::Decimal) -> rust_decimal::Decimal {
    use rust_decimal::prelude::ToPrimitive;
    quantick_ui_model::drawing_chrome::scenario::recut_count(
        parameter.to_u64().expect("registered count"),
    )
    .into()
}
impl AnchorHost for Anchor {
    fn project(&mut self, time: &mut impl FnMut(usize) -> Option<i64>) {
        self.point.time_ms = time(self.slot);
    }
    fn apply(&self, target: &mut impl DrawingRecipient) -> bool {
        let completed = target.place(self.tool, &self.band, self.point, self.look);
        if self.finish {
            target.finish()
        } else {
            completed
        }
    }
}

/// Only normal drawing-store operations; no pane, app or arbitrary callback.
pub(crate) trait DrawingRecipient {
    fn place(
        &mut self,
        tool: DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        look: Look,
    ) -> bool;
    fn finish(&mut self) -> bool;
    fn keep_selected_to_chart(&mut self);
    fn selected(&self) -> Option<usize>;
    fn select(&mut self, index: usize);
    fn selected_center(&self) -> Option<f32>;
    fn len(&self) -> usize;
}

impl DrawingRecipient for Drawings {
    fn place(
        &mut self,
        tool: DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        look: Look,
    ) -> bool {
        self.place_with(tool, band, point, |tool| {
            let mut payload = tool.default_payload();
            match look {
                Look::Profile { outline } => {
                    if let Some(profile) =
                        payload.as_any_mut().downcast_mut::<drawings::FrvpPayload>()
                    {
                        profile.outline_over_heatmap = outline;
                    }
                }
                Look::Avwap => {
                    if let Some(avwap) = payload
                        .as_any_mut()
                        .downcast_mut::<quantick_drawings::AvwapPayload>()
                    {
                        avwap.bands[1].on = true;
                    }
                }
                Look::Tool | Look::Default => {}
            }
            drawings::NewDrawing {
                style: match look {
                    Look::Tool | Look::Avwap => tool.default_style(),
                    Look::Default | Look::Profile { .. } => drawings::DrawingStyle::default(),
                },
                payload,
            }
        })
    }
    fn finish(&mut self) -> bool {
        self.finish_draft()
    }
    fn keep_selected_to_chart(&mut self) {
        if let Some(drawing) = self.selected_mut() {
            drawing.set_scope(drawings::DrawingScope::ThisChart);
        }
    }
    fn selected(&self) -> Option<usize> {
        self.selected()
    }
    fn select(&mut self, index: usize) {
        self.select(Some(index));
    }
    fn selected_center(&self) -> Option<f32> {
        let points = &self.items().get(self.selected()?)?.points;
        (!points.is_empty())
            .then(|| points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32)
    }
    fn len(&self) -> usize {
        self.items().len()
    }
}
