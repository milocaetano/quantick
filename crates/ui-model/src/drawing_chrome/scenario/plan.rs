use super::{DrawingDraft, DrawingsDemo, SeriesFacts};
use quantick_drawings::{ChartPoint, DrawingBand, DrawingTool};
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
const DEMO_DRAFT_POINTER_OFFSET: [f32; 2] = [0.2, -0.15];
#[derive(Debug, PartialEq)]
pub struct GalleryPlan {
    pub anchors: Vec<Anchor>,
    pub bands: Vec<Anchor>,
    pub shared: Option<bool>,
}
impl DrawingsDemo {
    pub fn plan(
        self,
        facts: SeriesFacts,
        tools: &[DrawingTool],
        samples: Vec<(DrawingBand, f64)>,
    ) -> GalleryPlan {
        let (visible, first, center, band) = facts.window();
        let stride = (visible / tools.len()).max(1);
        let span = (visible / DEMO_SPANS_PER_WINDOW).max(2);
        let mut anchors = Vec::new();
        for (index, tool) in tools.iter().copied().enumerate() {
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
                for &tool in tools {
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
        }
    }
    pub fn band_sample_slot(&self, slots: usize) -> Option<usize> {
        self.bands.then(|| {
            let visible = 90.min(slots);
            (slots - visible + visible / 2).min(slots.saturating_sub(1))
        })
    }
}
#[derive(Debug, PartialEq)]
pub struct DraftPlan {
    pub anchors: Vec<Anchor>,
    pub hand: [f32; 2],
    pub constrain: bool,
}
impl DrawingDraft {
    pub fn plan(self, tool: DrawingTool, facts: SeriesFacts) -> DraftPlan {
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
        let hand = if count >= 2 {
            [0.0, 0.0]
        } else {
            DEMO_DRAFT_POINTER_OFFSET
        };
        DraftPlan {
            anchors,
            hand,
            constrain: self.constrain,
        }
    }
}
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Look {
    Tool,
    Default,
    Profile { outline: bool },
    Avwap,
}

#[derive(Debug, PartialEq)]
pub struct Anchor {
    pub tool: DrawingTool,
    pub band: DrawingBand,
    pub slot: usize,
    pub point: ChartPoint,
    pub look: Look,
    pub finish: bool,
    pub select_after: bool,
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
            select_after: false,
        }
    }
}

pub fn recut_count(count: u64) -> u64 {
    count.saturating_mul(2).max(2)
}
