//! One temporary range and its feature-scoped commands.
//!
//! All coordinates, owner facts and revisions are supplied by the caller.
//! Updates allocate nothing; drawing construction happens only through effects.

pub mod conversion_plan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Profile,
    Retracement,
    Projection,
}

impl Action {
    pub const ALL: [Self; 3] = [Self::Profile, Self::Retracement, Self::Projection];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Owner {
    pub tab: u64,
    pub pane: u64,
    pub layout: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RangeContext {
    pub owner: Owner,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub bar: f32,
    pub price: f64,
    pub time_ms: Option<i64>,
}

impl Anchor {
    pub fn valid(self) -> bool {
        self.bar.is_finite() && self.bar >= 0.0 && self.price.is_finite()
    }
}

/// Physical input facts, independent of any drawing toolkit. This is the
/// history canvas, including empty projected space, excluding the live lane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureArea {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GestureEligibility {
    pub pointer_tool: bool,
    pub unoccluded: bool,
    pub area: GestureArea,
}

impl GestureEligibility {
    pub fn permits(self, position: [f32; 2]) -> bool {
        self.pointer_tool && self.unoccluded && self.area.accepts(position)
    }
}

impl GestureArea {
    pub fn accepts(self, at: [f32; 2]) -> bool {
        (0..2).all(|i| at[i].is_finite() && at[i] >= self.min[i] && at[i] <= self.max[i])
    }

    pub fn clamp(self, at: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|i| at[i].clamp(self.min[i], self.max[i].max(self.min[i])))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub pane: u64,
    pub drawing: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Dragging,
    Ready,
    Pending(u64),
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RangeView {
    pub context: RangeContext,
    pub anchors: [Anchor; 2],
    pub phase: Phase,
}

impl RangeView {
    pub fn paintable(self) -> bool {
        self.phase != Phase::Stale
    }

    pub fn actionable(self) -> bool {
        self.phase == Phase::Ready
    }

    pub fn unavailable_reason(self) -> Option<&'static str> {
        match self.phase {
            Phase::Stale => Some("range_series_changed"),
            Phase::Pending(_) => Some("range_conversion_pending"),
            Phase::Dragging => Some("range_gesture_in_progress"),
            Phase::Ready => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaceRequest {
    pub id: u64,
    pub action: Action,
    pub context: RangeContext,
    pub anchors: [Anchor; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Press {
        position: [f32; 2],
        anchor: Anchor,
        eligibility: GestureEligibility,
    },
    Drag {
        position: [f32; 2],
        anchor: Anchor,
        threshold_px: f32,
    },
    Release,
    Dismiss,
    Convert(Action),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Reconcile,
    ActiveTab(u64),
    PaneRemoved(u64),
    Selection(Option<Selection>),
    Completed(u64),
    Refused(u64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Place(PlaceRequest),
    ExplainRefusal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transition {
    /// The renderer may now construct the ruler's opening style/payload once.
    pub opened: bool,
    /// Whether a dismiss command consumed an interaction layer.
    pub consumed: bool,
    pub effect: Option<Effect>,
}

#[derive(Clone, Copy, Debug, Default)]
enum State {
    #[default]
    Idle,
    Pressed {
        context: RangeContext,
        position: [f32; 2],
        anchor: Anchor,
    },
    Selected(RangeView),
}

#[derive(Default)]
pub struct QuickRangeModel {
    state: State,
    next_request: u64,
    selection: Option<Selection>,
}

impl QuickRangeModel {
    pub fn present(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    pub fn context(&self) -> Option<RangeContext> {
        match self.state {
            State::Idle => None,
            State::Pressed { context, .. } => Some(context),
            State::Selected(view) => Some(view.context),
        }
    }

    pub fn view(&self) -> Option<RangeView> {
        match self.state {
            State::Selected(view) => Some(view),
            _ => None,
        }
    }

    /// Reconciliation runs before commands can expose a view or emit work.
    /// Input from a different pane cannot rewrite the owning pane's state.
    fn reconcile(&mut self, context: RangeContext) {
        let Some(current) = self.context() else {
            return;
        };
        if current.owner.tab != context.owner.tab || current.owner.pane != context.owner.pane {
            return;
        }
        if current.owner.layout != context.owner.layout {
            self.state = State::Idle;
        } else if current.revision != context.revision {
            match &mut self.state {
                State::Selected(view) => view.phase = Phase::Stale,
                _ => self.state = State::Idle,
            }
        }
    }

    pub fn update(&mut self, command: Command, context: RangeContext) -> Transition {
        self.reconcile(context);
        let mut result = Transition::default();
        match command {
            Command::Press {
                position,
                anchor,
                eligibility,
            } if eligibility.permits(position) && anchor.valid() => {
                self.state = State::Pressed {
                    context,
                    position,
                    anchor,
                };
            }
            Command::Drag {
                position,
                anchor,
                threshold_px,
            } if anchor.valid() && threshold_px.is_finite() && threshold_px >= 0.0 => {
                match &mut self.state {
                    State::Pressed {
                        context: start_context,
                        position: start,
                        anchor: first,
                    } if *start_context == context => {
                        let distance = (position[0] - start[0]).hypot(position[1] - start[1]);
                        if distance.is_finite() && distance > threshold_px {
                            self.state = State::Selected(RangeView {
                                context,
                                anchors: [*first, anchor],
                                phase: Phase::Dragging,
                            });
                            result.opened = true;
                        }
                    }
                    State::Selected(view)
                        if view.context == context && view.phase == Phase::Dragging =>
                    {
                        view.anchors[1] = anchor;
                    }
                    _ => {}
                }
            }
            Command::Release => match &mut self.state {
                State::Pressed {
                    context: start_context,
                    ..
                } if *start_context == context => {
                    self.state = State::Idle;
                }
                State::Selected(view)
                    if view.context == context && view.phase == Phase::Dragging =>
                {
                    view.phase = Phase::Ready;
                }
                _ => {}
            },
            Command::Dismiss => {
                result.consumed = self.present();
                self.state = State::Idle;
            }
            Command::Convert(action) => {
                if let State::Selected(view) = &mut self.state
                    && view.context == context
                    && view.actionable()
                    && let Some(id) = self.next_request.checked_add(1)
                {
                    self.next_request = id;
                    result.effect = Some(Effect::Place(PlaceRequest {
                        id,
                        action,
                        context,
                        anchors: view.anchors,
                    }));
                    view.phase = Phase::Pending(id);
                }
            }
            _ => {}
        }
        result
    }

    pub fn observe(&mut self, event: Event, context: RangeContext) -> Transition {
        self.reconcile(context);
        let mut result = Transition::default();
        match event {
            Event::ActiveTab(tab) if self.context().is_some_and(|c| c.owner.tab != tab) => {
                self.state = State::Idle;
            }
            Event::PaneRemoved(pane) if self.context().is_some_and(|c| c.owner.pane == pane) => {
                self.state = State::Idle;
            }
            Event::Selection(selection) => {
                if selection != self.selection && selection.is_some() {
                    self.state = State::Idle;
                }
                self.selection = selection;
            }
            Event::Completed(id) if self.view().is_some_and(|v| v.phase == Phase::Pending(id)) => {
                self.state = State::Idle;
            }
            Event::Refused(id) if self.view().is_some_and(|v| v.phase == Phase::Pending(id)) => {
                if let State::Selected(view) = &mut self.state {
                    view.phase = Phase::Ready;
                }
                result.effect = Some(Effect::ExplainRefusal);
            }
            _ => {}
        }
        result
    }
}
