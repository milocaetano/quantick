//! Drawing-chrome quick-range state. The domain owner decides gestures and conversions.
pub use core::Action;
use core::conversion_plan::{
    ConversionInput, ConversionReadback, PendingConversion, PlacementOutcome,
    QuickRangeConversionPlan, StartedConversion,
};
use quantick_chart_interaction::pane::PaneSide;
use quantick_chart_interaction::quick_range::{self as core, Command, Event, Phase};
use quantick_drawings::{self as drawings, ChartPoint, DrawingBand, DrawingPayload, NewDrawing};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub tab: u64,
    pub side: PaneSide,
    pub pane: u64,
    pub revision: u64,
    pub layout: Option<u64>,
}

impl Owner {
    pub fn context(self) -> core::RangeContext {
        core::RangeContext {
            owner: core::Owner {
                tab: self.tab,
                pane: self.pane,
                layout: self.layout,
            },
            revision: self.revision,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct PendingRangePlacement {
    pub conversion: PendingConversion,
    pub side: PaneSide,
}

pub struct Paint<'a> {
    pub anchors: [ChartPoint; 2],
    pub style: drawings::DrawingStyle,
    pub payload: &'a dyn DrawingPayload,
}

#[derive(Default)]
pub struct State {
    pending_launch: Option<core::DemoRequest>,
    demo_requested: Option<core::DemoRequest>,
    model: core::QuickRangeModel,
    side: Option<PaneSide>,
    look: Option<NewDrawing>,
}

fn anchor(point: ChartPoint) -> core::Anchor {
    core::Anchor {
        bar: point.bar,
        price: point.price,
        time_ms: point.time_ms,
    }
}

fn point(anchor: core::Anchor) -> ChartPoint {
    ChartPoint::at_time(anchor.bar, anchor.price, anchor.time_ms)
}

impl State {
    pub fn demo_requested(&self) -> Option<&core::DemoRequest> {
        self.demo_requested.as_ref()
    }

    fn reconcile(&mut self, owner: Option<Owner>) {
        match owner {
            Some(owner) => self.reconcile_owner(owner),
            None => self.remove_owner(),
        }
    }

    pub fn owner(&self) -> Option<Owner> {
        let context = self.model.context()?;
        Some(Owner {
            tab: context.owner.tab,
            side: self.side?,
            pane: context.owner.pane,
            revision: context.revision,
            layout: context.owner.layout,
        })
    }

    fn reconcile_owner(&mut self, owner: Owner) {
        self.model.observe(Event::Reconcile, owner.context());
        if self
            .model
            .context()
            .is_some_and(|c| c.owner.pane == owner.pane)
        {
            self.side = Some(owner.side);
        }
    }

    fn remove_owner(&mut self) {
        if let Some(context) = self.model.context() {
            self.model
                .observe(Event::PaneRemoved(context.owner.pane), context);
        }
    }

    fn note_selection(&mut self, pane: u64, selected: Option<u64>) {
        self.model.observe(
            Event::Selection(selected.map(|drawing| core::Selection { pane, drawing })),
            self.model.context().unwrap_or_default(),
        );
    }

    pub fn held(&self) -> bool {
        self.model.held()
    }

    pub fn band(&self) -> Option<&DrawingBand> {
        self.model.band()
    }

    fn release(&mut self, owner: Owner) {
        self.model.update(Command::Release, owner.context());
    }

    fn reconcile_tab(&mut self, tab: u64) -> Option<bool> {
        self.model.observe(
            Event::ActiveTab(tab),
            self.model.context().unwrap_or_default(),
        );
        self.model.view().map(|view| view.phase != Phase::Dragging)
    }

    pub fn paint(&self, owner: Owner) -> Option<Paint<'_>> {
        let view = self.model.view()?;
        let look = self.look.as_ref()?;
        (view.paintable() && view.context.owner == owner.context().owner).then(|| Paint {
            anchors: view.anchors.map(point),
            style: look.style,
            payload: look.payload.as_ref(),
        })
    }

    pub fn stale(&self, owner: Owner) -> bool {
        self.model.view().is_some_and(|view| {
            view.context.owner == owner.context().owner && view.phase == Phase::Stale
        })
    }

    fn convert(&mut self, action: Action) -> Option<PendingRangePlacement> {
        let context = self.model.context()?;
        let side = self.side?;
        let StartedConversion::Awaiting(conversion) =
            QuickRangeConversionPlan::start(&mut self.model, ConversionInput { action, context })
        else {
            return None;
        };
        Some(PendingRangePlacement { conversion, side })
    }

    fn finish_conversion(
        &mut self,
        conversion: PendingConversion,
        outcome: PlacementOutcome,
    ) -> ConversionReadback {
        let context = self.model.context().unwrap_or_default();
        conversion.finish(&mut self.model, context, outcome)
    }

    pub fn paint_anchors(&self) -> Option<[ChartPoint; 2]> {
        self.model.view().map(|view| view.anchors.map(point))
    }
    pub fn view(&self) -> Option<core::RangeView> {
        self.model.view()
    }
    pub fn present(&self) -> bool {
        self.model.present()
    }
    pub fn unavailable_reason(&self, tool_id: &str) -> Option<&'static str> {
        self.model.action_unavailable(|band| {
            drawings::DrawingTool::by_id(tool_id).is_none_or(|tool| tool.holds_band(band))
        })
    }
    fn press(
        &mut self,
        owner: Owner,
        position: [f32; 2],
        value: ChartPoint,
        eligibility: core::GestureEligibility,
        band: DrawingBand,
    ) -> bool {
        self.model.update(
            Command::Press {
                position,
                anchor: anchor(value),
                eligibility,
                band,
            },
            owner.context(),
        );
        let reset = self.model.context() == Some(owner.context()) && self.model.view().is_none();
        if reset {
            self.side = Some(owner.side);
        }
        reset
    }
    fn drag(
        &mut self,
        owner: Owner,
        position: [f32; 2],
        value: ChartPoint,
        threshold_px: f32,
        level: bool,
    ) -> bool {
        self.model
            .update(
                Command::Drag {
                    position,
                    anchor: anchor(value),
                    threshold_px,
                    level,
                },
                owner.context(),
            )
            .opened
    }
    fn opened(&mut self, look: NewDrawing) {
        self.look = Some(look);
    }
    fn dismiss(&mut self) -> bool {
        self.model
            .update(Command::Dismiss, self.model.context().unwrap_or_default())
            .consumed
    }
}

/// Input crossing the drawing-chrome model boundary. Domain transitions stay in their owner.
pub enum Intent {
    QueueLaunch(Option<core::DemoRequest>),
    ApplyLaunch,
    StageDemo(Staging),
    Reconcile(Option<Owner>),
    Selection {
        pane: u64,
        selected: Option<u64>,
    },
    Press {
        owner: Owner,
        position: [f32; 2],
        value: ChartPoint,
        eligibility: core::GestureEligibility,
        band: DrawingBand,
    },
    Drag {
        owner: Owner,
        position: [f32; 2],
        value: ChartPoint,
        threshold_px: f32,
        level: bool,
    },
    Opened(NewDrawing),
    Release(Owner),
    Dismiss,
    ActiveTab(u64),
    Convert(Action),
    FinishConversion {
        conversion: PendingConversion,
        outcome: PlacementOutcome,
    },
}

pub fn update(state: &mut State, intent: Intent) -> super::DrawingChromeAsk {
    let mut effects = super::DrawingChromeAsk::default();
    match intent {
        Intent::QueueLaunch(request) => state.pending_launch = request,
        Intent::ApplyLaunch => {
            if let Some(request) = state.pending_launch.take() {
                state.demo_requested = Some(request);
            }
        }
        Intent::StageDemo(staging) => state.stage_demo(staging),
        Intent::Reconcile(owner) => state.reconcile(owner),
        Intent::Selection { pane, selected } => state.note_selection(pane, selected),
        Intent::Press {
            owner,
            position,
            value,
            eligibility,
            band,
        } => {
            state.press(owner, position, value, eligibility, band);
        }
        Intent::Drag {
            owner,
            position,
            value,
            threshold_px,
            level,
        } => {
            state.drag(owner, position, value, threshold_px, level);
        }
        Intent::Opened(look) => state.opened(look),
        Intent::Release(owner) => state.release(owner),
        Intent::Dismiss => {
            state.dismiss();
        }
        Intent::ActiveTab(tab) => {
            state.reconcile_tab(tab);
        }
        Intent::Convert(action) => effects.place_quick_range = state.convert(action).map(Box::new),
        Intent::FinishConversion {
            conversion,
            outcome,
        } => effects.set_quick_range_readback(state.finish_conversion(conversion, outcome)),
    }
    effects
}

#[cfg(test)]
#[path = "quick_range/tests/conversion.rs"]
mod conversion_tests;

#[path = "quick_range/scenario.rs"]
mod scenario;
pub use scenario::Staging;
#[cfg(test)]
#[path = "quick_range/tests/scenario.rs"]
mod tests;
