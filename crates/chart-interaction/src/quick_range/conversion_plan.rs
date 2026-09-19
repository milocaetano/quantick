//! Fixed conversion stages around the caller's registered drawing effect.

use super::{
    Action, Command, Effect, Event, PlaceRequest, QuickRangeModel, RangeContext, RangeView,
};

#[cfg(test)]
use crate::stage_registry::nodes_in_valid_order;
use crate::stage_registry::{StageNode, declare_stages};

#[cfg(test)]
mod tests;

declare_stages! {
    enum Stage {
        Update after [],
        YieldEffect after [Update],
        DeliverResult after [YieldEffect],
        Readback after [DeliverResult],
    }
}

type Step = StageNode<Stage>;

const STEPS: [Step; Stage::COUNT] = Stage::NODES;

// Validation also rejects cycles: no member of a cycle can have all its
// prerequisites among the stages already visited.
#[cfg(test)]
const fn valid(steps: &[Step]) -> bool {
    nodes_in_valid_order(steps, Stage::COUNT)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConversionInput {
    pub action: Action,
    pub context: RangeContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementOutcome {
    Placed,
    InputInvalid,
    ActionRefused,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConversionReadback {
    pub view: Option<RangeView>,
    pub explain_refusal: bool,
}

#[must_use = "complete the yielded drawing effect or inspect the settled result"]
#[derive(Debug, PartialEq)]
pub enum StartedConversion {
    Settled(ConversionReadback),
    Awaiting(PendingConversion),
}

/// The only production plan. Validation is constant; an action performs four
/// fixed dispatches, with no allocation, sorting or chart/history access.
pub struct QuickRangeConversionPlan;

impl QuickRangeConversionPlan {
    pub fn start(model: &mut QuickRangeModel, input: ConversionInput) -> StartedConversion {
        Execution::new(input).start(model, &STEPS)
    }
}

/// A single continuation across the caller's drawing action. It cannot be
/// constructed, copied or completed twice by a consumer.
///
/// ```compile_fail
/// use quantick_chart_interaction::quick_range::{QuickRangeModel, RangeContext};
/// use quantick_chart_interaction::quick_range::conversion_plan::{PendingConversion, PlacementOutcome};
/// fn twice(pending: PendingConversion, model: &mut QuickRangeModel) {
///     pending.finish(model, RangeContext::default(), PlacementOutcome::Placed);
///     pending.finish(model, RangeContext::default(), PlacementOutcome::Placed);
/// }
/// ```
#[must_use = "finish the drawing effect to reconcile the pending range"]
#[derive(Debug, PartialEq)]
pub struct PendingConversion {
    request: PlaceRequest,
    cursor: usize,
}

impl PendingConversion {
    pub fn request(&self) -> &PlaceRequest {
        &self.request
    }

    pub fn finish(
        self,
        model: &mut QuickRangeModel,
        current_context: RangeContext,
        outcome: PlacementOutcome,
    ) -> ConversionReadback {
        self.finish_with(model, current_context, outcome, &STEPS)
    }

    fn finish_with(
        self,
        model: &mut QuickRangeModel,
        current_context: RangeContext,
        outcome: PlacementOutcome,
        steps: &[Step],
    ) -> ConversionReadback {
        let mut execution = Execution {
            input: ConversionInput {
                action: self.request.action,
                context: current_context,
            },
            cursor: self.cursor,
            request: Some(self.request),
            outcome: Some(outcome),
            readback: None,
            explain_refusal: false,
        };
        assert!(
            !execution.run(model, current_context, steps),
            "one drawing effect per plan"
        );
        execution.readback.expect("plan reads its result")
    }
}

#[derive(Debug, PartialEq)]
struct Execution {
    input: ConversionInput,
    cursor: usize,
    request: Option<PlaceRequest>,
    outcome: Option<PlacementOutcome>,
    readback: Option<ConversionReadback>,
    explain_refusal: bool,
}

impl Execution {
    fn new(input: ConversionInput) -> Self {
        Self {
            input,
            cursor: 0,
            request: None,
            outcome: None,
            readback: None,
            explain_refusal: false,
        }
    }

    fn start(mut self, model: &mut QuickRangeModel, steps: &[Step]) -> StartedConversion {
        if self.run(model, self.input.context, steps) {
            // Only the request and resume position cross the effect boundary.
            // A supplied finish outcome replaces any private seeded outcome.
            assert!(
                self.readback.is_none(),
                "readback cannot precede suspension"
            );
            assert!(!self.explain_refusal, "refusal cannot precede suspension");
            StartedConversion::Awaiting(PendingConversion {
                request: self.request.expect("yielded placement"),
                cursor: self.cursor,
            })
        } else {
            StartedConversion::Settled(self.readback.expect("plan reads its result"))
        }
    }

    /// Both production halves and private order-mutation tests use these
    /// bodies. The cursor advances before handing the effect to the caller.
    fn run(&mut self, model: &mut QuickRangeModel, context: RangeContext, steps: &[Step]) -> bool {
        while let Some(step) = steps.get(self.cursor) {
            self.cursor += 1;
            match step.stage {
                Stage::Update => {
                    if let Some(Effect::Place(request)) = model
                        .update(Command::Convert(self.input.action), context)
                        .effect
                    {
                        self.request = Some(request);
                    }
                }
                Stage::YieldEffect if self.request.is_some() => return true,
                Stage::YieldEffect => {}
                Stage::DeliverResult => {
                    if let (Some(request), Some(outcome)) = (self.request, self.outcome) {
                        let event = if outcome == PlacementOutcome::Placed {
                            Event::Completed(request.id)
                        } else {
                            Event::Refused(request.id)
                        };
                        let transition = model.observe(event, context);
                        self.explain_refusal = outcome == PlacementOutcome::ActionRefused
                            && transition.effect == Some(Effect::ExplainRefusal);
                    }
                }
                Stage::Readback => {
                    self.readback = Some(ConversionReadback {
                        view: model.view(),
                        explain_refusal: self.explain_refusal,
                    });
                }
            }
        }
        false
    }
}
