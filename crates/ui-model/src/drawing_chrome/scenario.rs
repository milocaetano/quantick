//! Drawing launch state and synchronous phase plans. Hosts supply all facts.
#[path = "scenario/plan.rs"]
mod plan;
#[path = "scenario/profile.rs"]
mod profile;
use super::InspectorTab;
pub use plan::*;
pub use profile::*;
use quantick_drawings::{DrawingBand, DrawingTool};
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DrawingsDemo {
    pub bands: bool,
    /// `QUANTICK_DRAWINGS_DEMO_SHARED`: `1` splits the canvas, `0` keeps them to their chart.
    pub shared: Option<bool>,
    pub select_tool: Option<String>,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrvpDemo {
    pub compare: bool,
    pub stress: bool,
    pub select: bool,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrawingDraft {
    pub anchors: usize,
    pub constrain: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct DrawingDemoLaunch {
    pub gallery: Option<DrawingsDemo>,
    pub recut: bool,
    pub profile: Option<FrvpDemo>,
    pub avwap: bool,
    pub draft: Option<DrawingDraft>,
}
/// How much of the newest chart the `QUANTICK_DRAWINGS_DEMO` hook spreads its
/// objects across. Close to what a default viewport shows, so every object
/// lands on screen — a demo the camera cannot see proves nothing.
const DEMO_VISIBLE_SLOTS: usize = 90;
/// The band to spread across before the pane has an auto-range to read (no
/// bars yet): a fraction of price, since there is nothing better to ask.
const DEMO_FALLBACK_BAND_FRACTION: f64 = 0.004;
/// One-minute venue candles the `=stress` scene installs behind the tape — a
/// time chart's worth of history, and far more than one fold pass spends, so
/// the range folds across frames rather than in the one that placed it. The
/// time pane folds them to whatever interval it is showing, so the slot count
/// is this number only at 1m — the scene is about the fold surviving a long
/// history, not about an exact count.
const FRVP_STRESS_CANDLES: i64 = 25_000;

/// Fresh geometry facts; timestamp projection happens only for chosen slots.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct SeriesFacts {
    pub slots: usize,
    pub last_close: Option<f64>,
    pub auto_range: Option<(f64, f64)>,
}
impl SeriesFacts {
    pub fn window(self) -> (usize, usize, f64, f64) {
        let visible = DEMO_VISIBLE_SLOTS.min(self.slots);
        let close = self.last_close.unwrap_or(1.0);
        let (center, band) = self
            .auto_range
            .filter(|(lo, hi)| hi > lo)
            .map_or((close, close * DEMO_FALLBACK_BAND_FRACTION), |(lo, hi)| {
                ((lo + hi) / 2.0, hi - lo)
            });
        (visible, self.slots - visible, center, band)
    }
}

#[derive(Debug, PartialEq)]
pub enum ProfilePreparation {
    Wait,
    Ready,
    Prefix { candles: i64 },
}
#[derive(Debug, PartialEq)]
pub struct ProfileFacts {
    pub slots: usize,
    pub prefix: usize,
}

#[derive(Debug, Default, PartialEq)]
pub struct Launch {
    pub demos: DrawingDemoLaunch,
    pub manager: bool,
    pub inspector: bool,
    pub note: bool,
    pub tab: Option<InspectorTab>,
    pub inspector_position: Option<[f32; 2]>,
    pub bar_position: Option<[f32; 2]>,
}
#[derive(Default)]
pub struct State {
    pending_launch: Option<Launch>,
    input: DrawingDemoLaunch,
    pending_note: bool,
    gallery_receipt: Option<GalleryReceipt>,
}
impl State {
    pub fn gallery_requested(&self) -> bool {
        self.input.gallery.is_some()
    }
    pub fn gallery_ready(&self, slots: usize, tool_count: usize) -> bool {
        self.gallery_requested() && tool_count > 0 && slots >= 8 * tool_count
    }
    pub fn gallery_sample_slot(&self, slots: usize) -> Option<usize> {
        self.input.gallery.as_ref()?.band_sample_slot(slots)
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
    pub fn note_requested(&self) -> bool {
        self.pending_note
    }
}
struct GalleryReceipt {
    local: bool,
    tool: Option<String>,
    selected: Option<usize>,
}
pub enum Intent {
    GalleryPlaced {
        tool: DrawingTool,
        completed: bool,
        selected: Option<usize>,
    },
    GalleryFinished,
    QueueLaunch(Launch),
    ApplyLaunch,
    RequestGallery(DrawingsDemo),
    GalleryReady {
        series: SeriesFacts,
        tools: Vec<DrawingTool>,
        samples: Vec<(DrawingBand, f64)>,
    },
    DraftReady {
        tool: Option<DrawingTool>,
        chart_available: bool,
        series: SeriesFacts,
    },
    PrepareProfile {
        slots: Option<usize>,
        tool_available: bool,
    },
    ProfileReady(ProfileFacts),
    AvwapReady {
        slots: usize,
    },
    RequestNote(bool),
    PlanNote {
        chart_available: bool,
        series: SeriesFacts,
    },
    NotePlaced(bool),
}
#[derive(Debug, PartialEq)]
pub enum Effect {
    KeepSelectedLocal,
    GallerySelection(Option<usize>),
    Launch {
        inspector_position: Option<[f32; 2]>,
        bar_position: Option<[f32; 2]>,
    },
    Gallery(GalleryPlan),
    Draft(DraftPlan),
    ProfilePreparation(ProfilePreparation),
    Profile(ProfileGeometry),
    Avwap(AvwapGeometry),
    Note(NotePlacement),
}
/// One optional boxed response means idle frames allocate nothing.
pub(super) fn update(state: &mut super::State, intent: Intent) -> super::DrawingChromeAsk {
    let scenario = &mut state.scenario;
    let effect = match intent {
        Intent::GalleryPlaced {
            tool,
            completed,
            selected,
        } => scenario.gallery_receipt.as_mut().and_then(|receipt| {
            if completed && receipt.tool.as_deref() == Some(tool.id()) {
                receipt.selected = selected;
            }
            (completed && receipt.local).then_some(Effect::KeepSelectedLocal)
        }),
        Intent::GalleryFinished => scenario
            .gallery_receipt
            .take()
            .map(|receipt| Effect::GallerySelection(receipt.selected)),
        Intent::QueueLaunch(launch) => {
            scenario.pending_launch = Some(launch);
            None
        }
        Intent::ApplyLaunch => scenario.pending_launch.take().map(|launch| {
            scenario.input = launch.demos;
            state.core.manager.open |= launch.manager;
            state.core.shared.open |= launch.inspector;
            scenario.pending_note |= launch.note;
            if let Some(tab) = launch.tab {
                state.inspector.tab = tab;
            }
            if let Some(position) = launch.inspector_position {
                state.inspector.place_by_hand(position);
            }
            Effect::Launch {
                inspector_position: launch.inspector_position,
                bar_position: launch.bar_position,
            }
        }),
        Intent::RequestGallery(request) => {
            scenario.input.gallery = Some(request);
            None
        }
        Intent::GalleryReady {
            series,
            tools,
            samples,
        } => {
            if !scenario.gallery_ready(series.slots, tools.len()) {
                None
            } else {
                scenario.input.gallery.take().map(|request| {
                    scenario.gallery_receipt = Some(GalleryReceipt {
                        local: request.shared == Some(false),
                        tool: request.select_tool.clone(),
                        selected: None,
                    });
                    Effect::Gallery(request.plan(series, &tools, samples))
                })
            }
        }
        Intent::DraftReady {
            tool,
            chart_available,
            series,
        } => match (scenario.input.draft, tool) {
            (Some(request), Some(tool)) if chart_available && series.slots > 0 => {
                scenario.input.draft = None;
                Some(Effect::Draft(request.plan(tool, series)))
            }
            _ => None,
        },
        Intent::PrepareProfile {
            slots,
            tool_available,
        } => {
            let preparation = match scenario.input.profile {
                Some(_) if slots.is_none_or(|slots| slots < 12) => ProfilePreparation::Wait,
                Some(_) if !tool_available => {
                    scenario.input.profile = None;
                    ProfilePreparation::Wait
                }
                Some(request) if request.stress => ProfilePreparation::Prefix {
                    candles: FRVP_STRESS_CANDLES,
                },
                Some(_) => ProfilePreparation::Ready,
                None => ProfilePreparation::Wait,
            };
            Some(Effect::ProfilePreparation(preparation))
        }
        Intent::ProfileReady(facts) => scenario.input.profile.take().and_then(|request| {
            if request.compare && facts.slots.saturating_sub(facts.prefix) < 60 {
                scenario.input.profile = Some(request);
                None
            } else {
                Some(Effect::Profile(ProfileGeometry::new(request, facts)))
            }
        }),
        Intent::AvwapReady { slots } => {
            if !scenario.input.avwap || slots < 12 {
                None
            } else {
                scenario.input.avwap = false;
                Some(Effect::Avwap(AvwapGeometry {
                    slot: slots.saturating_sub(40).min(slots - 1),
                }))
            }
        }
        Intent::RequestNote(pending) => {
            scenario.pending_note = pending;
            None
        }
        Intent::NotePlaced(placed) => {
            if placed {
                scenario.pending_note = false;
            }
            None
        }
        Intent::PlanNote {
            chart_available,
            series,
        } => NotePlacement::plan(chart_available, series).map(Effect::Note),
    };
    super::DrawingChromeAsk::scenario(effect)
}

#[derive(Debug, PartialEq)]
pub struct NotePlacement {
    pub slot: usize,
    pub price: f64,
}
impl NotePlacement {
    pub fn plan(chart_available: bool, series: SeriesFacts) -> Option<Self> {
        if !chart_available || series.slots == 0 {
            return None;
        }
        let price = series
            .auto_range
            .filter(|(lo, hi)| hi > lo)
            .map_or(series.last_close.unwrap_or(1.0), |(lo, hi)| (lo + hi) / 2.0);
        let visible = 90.min(series.slots);
        let slot = (series.slots - visible / 2).min(series.slots.saturating_sub(1));
        Some(Self { slot, price })
    }
}
#[cfg(test)]
#[path = "scenario/tests.rs"]
mod tests;
