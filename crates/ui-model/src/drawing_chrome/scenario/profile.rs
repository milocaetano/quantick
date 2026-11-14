use super::plan::{Anchor, Look};
use super::{FrvpDemo, ProfileFacts};
use quantick_drawings::DrawingTool;
#[derive(Debug, PartialEq)]
pub struct ProfilePlan {
    pub anchors: Vec<Anchor>,
    pub select: bool,
}
#[derive(Debug, PartialEq)]
pub struct ProfileGeometry {
    request: FrvpDemo,
    ranges: Vec<(usize, usize, bool)>,
    pub close_slot: usize,
}
impl ProfileGeometry {
    pub(super) fn new(request: FrvpDemo, facts: ProfileFacts) -> Self {
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
    pub fn plan(self, tool: DrawingTool, close: Option<f64>) -> ProfilePlan {
        let anchors = self
            .ranges
            .into_iter()
            .flat_map(|(from, to, outline)| {
                [from, to]
                    .into_iter()
                    .enumerate()
                    .map(move |(index, slot)| {
                        let mut anchor = Anchor::new(
                            tool,
                            slot,
                            slot as f32,
                            close.unwrap_or(1.0),
                            Look::Profile { outline },
                        );
                        anchor.select_after = self.request.select && index == 1;
                        anchor
                    })
            })
            .collect();
        ProfilePlan {
            anchors,
            select: self.request.select,
        }
    }
}
#[derive(Debug, PartialEq)]
pub struct AvwapGeometry {
    pub slot: usize,
}
impl AvwapGeometry {
    pub fn plan(self, tool: DrawingTool, close: Option<f64>) -> ProfilePlan {
        ProfilePlan {
            anchors: vec![Anchor::new(
                tool,
                self.slot,
                self.slot as f32,
                close.unwrap_or(1.0),
                Look::Avwap,
            )],
            select: false,
        }
    }
}
