//! Gallery, band, recut and parked-draft policy over narrow drawing inputs.
use super::{DrawingDraft, DrawingsDemo, SeriesFacts};
use crate::drawings::{self, ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingTool, Drawings};
use crate::pane::ParkedHand;
use eframe::egui;

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
/// How far before the loaded history the re-cut demo anchors its off-series
/// mark. Any distance the tab cannot possibly hold would do; an hour is
/// unambiguous at every timeframe the chart offers.
const DEMO_OFF_SERIES_LEAD_MS: i64 = 3_600_000;

pub(crate) struct GalleryPlan {
    anchors: Vec<Anchor>,
    bands: Vec<Anchor>,
    shared: bool,
    selected_tool: Option<String>,
}
impl DrawingsDemo {
    pub fn plan(
        self,
        facts: SeriesFacts,
        indicators: &crate::indicators::IndicatorViews,
    ) -> GalleryPlan {
        let samples = self
            .band_sample_slot(facts.slots)
            .map_or_else(Vec::new, |slot| crate::bands::samples_at(indicators, slot));
        let (visible, first, center, band) = facts.window();
        let stride = (visible / DRAWING_TOOLS.len()).max(1);
        let span = (visible / DEMO_SPANS_PER_WINDOW).max(2);
        let mut anchors = Vec::new();
        for (index, tool) in DRAWING_TOOLS.into_iter().enumerate() {
            let count = if tool.freehand() {
                DEMO_FREEHAND_POINTS
            } else {
                tool.required_points()
            };
            for anchor in 0..count {
                let slot =
                    (first + index * stride + anchor * span).min(facts.slots.saturating_sub(1));
                let price = center
                    + (f64::from(anchor as i32) - 1.0) * band * DEMO_ANCHOR_BAND_STEP
                    - (f64::from(index as i32 % 3) - 1.0) * band * DEMO_ROW_BAND_STEP;
                let mut action = Anchor::new(tool, slot, slot as f32 + 0.5, price, Look::Tool);
                action.finish = tool.freehand() && anchor + 1 == count;
                anchors.push(action);
            }
        }
        let mut bands = Vec::new();
        if self.bands {
            let level = (first + visible / 2).min(facts.slots.saturating_sub(1));
            let left = (first + visible / 8).min(facts.slots.saturating_sub(1));
            let right = (first + visible * 3 / 4).min(facts.slots.saturating_sub(1));
            for (band, value) in samples {
                for tool in DRAWING_TOOLS {
                    let points: &[(usize, f64)] = match tool.id() {
                        "horizontal-line" => &[(level, value)],
                        "trend-line" => &[(left, value * 0.5), (right, value * 1.5)],
                        _ => continue,
                    };
                    for &(slot, price) in points {
                        let mut action =
                            Anchor::new(tool, slot, slot as f32 + 0.5, price, Look::Default);
                        action.band = band.clone();
                        bands.push(action);
                    }
                }
            }
        }
        GalleryPlan {
            anchors,
            bands,
            shared: self.shared,
            selected_tool: self.select_tool,
        }
    }
    fn band_sample_slot(&self, slots: usize) -> Option<usize> {
        self.bands.then(|| {
            let visible = 90.min(slots);
            (slots - visible + visible / 2).min(slots.saturating_sub(1))
        })
    }
}
impl GalleryPlan {
    pub fn project_times(&mut self, mut time: impl FnMut(usize) -> Option<i64>) {
        for anchor in self.anchors.iter_mut().chain(&mut self.bands) {
            anchor.project(&mut time);
        }
    }
    pub fn apply_main(&self, target: &mut impl DrawingRecipient) -> Option<f32> {
        let mut requested = None;
        for anchor in &self.anchors {
            let completed = anchor.apply(target);
            if completed && self.shared {
                target.share_selected();
            }
            if completed && self.selected_tool.as_deref() == Some(anchor.tool.id()) {
                requested = target.selected();
            }
        }
        if let Some(index) = requested {
            target.select(index);
            target.selected_center()
        } else {
            None
        }
    }
    pub fn apply_bands(&self, target: &mut impl DrawingRecipient) {
        for anchor in &self.bands {
            anchor.apply(target);
        }
    }
}

pub(crate) struct DraftPlan {
    anchors: Vec<Anchor>,
    pub hand: ParkedHand,
}
impl DrawingDraft {
    pub(super) fn plan(
        self,
        tool: DrawingTool,
        chart: egui::Rect,
        facts: SeriesFacts,
    ) -> DraftPlan {
        let count = self.anchors.min(tool.required_points().saturating_sub(1));
        let (visible, first, center, band) = facts.window();
        let mut anchors = Vec::new();
        for anchor in 0..count {
            let offset = (anchor as f32 + 0.5) / count as f32 - 0.5;
            let slot = ((first as f32 + visible as f32 * (0.5 + offset * DEMO_DRAFT_SPAN))
                as usize)
                .min(facts.slots.saturating_sub(1));
            anchors.push(Anchor::new(
                tool,
                slot,
                slot as f32 + 0.5,
                center + f64::from(offset) * band * DEMO_DRAFT_BAND_SPAN,
                Look::Tool,
            ));
        }
        let position = if count >= 2 {
            chart.center()
        } else {
            chart.center() + DEMO_DRAFT_POINTER_OFFSET * chart.size()
        };
        DraftPlan {
            anchors,
            hand: ParkedHand {
                position,
                constrain: if self.constrain {
                    drawings::Constrain::Level
                } else {
                    drawings::Constrain::Free
                },
            },
        }
    }
}
impl DraftPlan {
    pub fn project_times(&mut self, mut time: impl FnMut(usize) -> Option<i64>) {
        for anchor in &mut self.anchors {
            anchor.project(&mut time);
        }
    }
    pub fn apply(&self, target: &mut impl DrawingRecipient) {
        for anchor in &self.anchors {
            anchor.apply(target);
        }
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
    parameter
        .to_u64()
        .expect("registered count")
        .saturating_mul(2)
        .max(2)
        .into()
}

#[derive(Clone, Copy)]
pub(crate) enum Look {
    Tool,
    Default,
    Profile { outline: bool },
    Avwap,
}

pub(super) struct Anchor {
    pub tool: DrawingTool,
    pub band: DrawingBand,
    pub slot: usize,
    pub point: ChartPoint,
    pub look: Look,
    pub finish: bool,
}

impl Anchor {
    pub fn new(tool: DrawingTool, slot: usize, bar: f32, price: f64, look: Look) -> Self {
        Self {
            tool,
            band: DrawingBand::Price,
            slot,
            point: ChartPoint::at_time(bar, price, None),
            look,
            finish: false,
        }
    }
    pub fn project(&mut self, time: &mut impl FnMut(usize) -> Option<i64>) {
        self.point.time_ms = time(self.slot);
    }
    pub fn apply(&self, target: &mut impl DrawingRecipient) -> bool {
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
    fn share_selected(&mut self);
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
                        .downcast_mut::<drawings::AvwapPayload>()
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
    fn share_selected(&mut self) {
        if let Some(drawing) = self.selected_mut()
            && drawing.shareable()
        {
            drawing.scope = drawings::DrawingScope::AllCharts;
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

#[cfg(test)]
mod tests {
    use super::demo_recut_count;
    #[test]
    fn demo_recut_preserves_the_count_upper_bound() {
        for (count, expected) in [(0, 2), (50, 100), (u64::MAX, u64::MAX)] {
            assert_eq!(demo_recut_count(count.into()), expected.into());
        }
    }
}
