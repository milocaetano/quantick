//! Cached pane-header presentation and input transitions. Chart types remain
//! authoritative for identity, available controls and domain requests.
use quantick_chart::connection_state::Dot;
use quantick_chart::flow_band::{WindowOption, window_label, window_options};
use quantick_chart::pane_header::{
    BubbleSample, FlowHeaderState, HeaderIntent, LayerGlyph, PaneHeader, PaneIdentity,
    SAMPLE_RESET_SCALE, SideSegment, dragged_scale,
};
use quantick_engine::bar_registry::BarConfiguration;
use quantick_orderflow::LaneSide;

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub identity: PaneIdentity,
    pub bars: BarConfiguration,
    pub flow: Option<FlowHeaderState>,
    pub chip: Option<[Dot; 3]>,
    pub slot: Option<(String, bool)>,
    pub settings_open: bool,
    pub lights_on: bool,
}

impl Snapshot {
    pub fn time(bars: BarConfiguration) -> Self {
        Self {
            identity: PaneIdentity::Time,
            bars,
            flow: None,
            chip: None,
            slot: None,
            settings_open: false,
            lights_on: false,
        }
    }
}

#[derive(Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
    pub header: Option<PaneHeader>,
    pub segments: Option<[SideSegment; 2]>,
    pub window_label: Option<String>,
    pub window_options: Vec<WindowOption>,
    drag: Option<(BubbleSample, f32)>,
}

pub enum Intent {
    Refresh(Snapshot),
    RefreshFlow(Option<FlowHeaderState>),
    Side(LaneSide),
    Layer {
        glyph: LayerGlyph,
        secondary: bool,
    },
    SampleGesture {
        sample: BubbleSample,
        scale: f32,
        started: bool,
        dragged: bool,
        up_px: f32,
        reset: bool,
        ended: bool,
    },
    WindowPick(usize),
    Request(HeaderIntent),
}

#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub request: Option<HeaderIntent>,
    pub focus: bool,
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;

    fn update(state: &mut State, intent: Intent) -> Effects {
        let request = match intent {
            Intent::Refresh(snapshot) => {
                if state.snapshot.as_ref() == Some(&snapshot) {
                    return Effects::default();
                }
                let previous = state.snapshot.as_ref();
                if previous.is_none_or(|old| {
                    old.identity != snapshot.identity || old.bars != snapshot.bars
                }) {
                    state.header = Some(PaneHeader::of(snapshot.identity, snapshot.bars));
                }
                state.project_flow(snapshot.flow);
                state.snapshot = Some(snapshot);
                None
            }
            Intent::RefreshFlow(flow) => {
                state.project_flow(flow);
                if let Some(snapshot) = state.snapshot.as_mut() {
                    snapshot.flow = flow;
                }
                None
            }
            Intent::Side(side) => state.side(side).and_then(SideSegment::click),
            Intent::Layer { glyph, secondary } => {
                if secondary {
                    Some(HeaderIntent::OpenLayerSettings(glyph.toggle))
                } else {
                    glyph.click()
                }
            }
            Intent::SampleGesture {
                sample,
                scale,
                started,
                dragged,
                up_px,
                reset,
                ended,
            } => {
                if started {
                    state.drag = Some((sample, scale));
                }
                let shown = if reset {
                    SAMPLE_RESET_SCALE
                } else if dragged {
                    state.sample_scale(sample, scale, up_px, started)
                } else {
                    scale
                };
                if ended {
                    state.drag = None;
                }
                (shown != scale).then_some(HeaderIntent::SetSample(sample, shown))
            }
            Intent::WindowPick(index) => state
                .window_options
                .get(index)
                .and_then(|option| option.pick)
                .map(HeaderIntent::SetLaneWindow),
            Intent::Request(request) => Some(request),
        };
        Effects {
            focus: matches!(request, Some(HeaderIntent::OpenLayerSettings(_))),
            request,
        }
    }
}

impl State {
    fn project_flow(&mut self, flow: Option<FlowHeaderState>) {
        let previous = self.snapshot.as_ref().and_then(|snapshot| snapshot.flow);
        let sides = |flow: FlowHeaderState| (flow.view, flow.shown_view, flow.market_held);
        if previous.map(sides) != flow.map(sides) {
            self.segments = flow.as_ref().map(SideSegment::pair);
        }
        let window =
            |flow: FlowHeaderState| (flow.window, flow.window_setting, flow.window_reference_ms);
        if previous.map(window) != flow.map(window) {
            self.window_label = flow.as_ref().map(window_label);
            self.window_options = flow.as_ref().map_or_else(Vec::new, window_options);
        }
    }

    pub fn sample_scale(
        &self,
        sample: BubbleSample,
        fallback: f32,
        up_px: f32,
        started: bool,
    ) -> f32 {
        let start = self
            .drag
            .filter(|(which, _)| !started && *which == sample)
            .map_or(fallback, |(_, start)| start);
        dragged_scale(start, up_px)
    }

    pub fn side(&self, side: LaneSide) -> Option<SideSegment> {
        self.segments
            .and_then(|segments| segments.into_iter().find(|segment| segment.side == side))
    }
}

#[cfg(test)]
#[path = "pane_header/tests.rs"]
mod tests;
