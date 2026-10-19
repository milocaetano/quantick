//! Bubble magnification over the pane owner's narrow port. One rule says
//! which bubbles a pane may size; the pane's size sliders and
//! `orderflow.bubbles.size.set` both ask it, so they cannot disagree.
use crate::dock::ActionDock;
use quantick_control::{
    error::ControlError, id::PreconditionId, registry::RegistryError, wire::ActorContext,
};
use quantick_control_schema::bubble_scale::{
    BubbleScaleInput, BubbleScaleTarget, DISPLAY_ACTIVE_PRECONDITION, TICK_CHART_PRECONDITION,
    descriptor,
};
use serde_json::Value;

/// What the bubble size rule reads about a pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BubbleScalePane {
    /// The pane draws tick bars.
    pub tick_chart: bool,
    /// The aggression bubbles over the candles are switched on.
    pub candle_bubbles: bool,
    /// The pane shows the tape alone, so no candle is drawn.
    pub tape_only: bool,
    /// The tape is on the canvas and draws its bubbles.
    pub tape_bubbles: bool,
}

impl BubbleScalePane {
    /// The one rule for sizing bubbles: candle bubbles on a tick chart that
    /// draws them beside its candles, Tape bubbles wherever the tape draws
    /// them.
    #[must_use]
    pub fn may_scale(self, target: BubbleScaleTarget) -> bool {
        match target {
            BubbleScaleTarget::Candle => self.tick_chart && self.candle_bubbles && !self.tape_only,
            BubbleScaleTarget::Tape => self.tape_bubbles,
        }
    }

    /// Whether `target` may be sized, by [`Self::may_scale`].
    ///
    /// # Errors
    ///
    /// Names which part of the rule `target` fails.
    pub fn admit(self, target: BubbleScaleTarget) -> Result<(), ScaleRefusal> {
        if target == BubbleScaleTarget::Candle && !self.tick_chart {
            Err(ScaleRefusal::NotTickChart)
        } else if self.may_scale(target) {
            Ok(())
        } else {
            Err(ScaleRefusal::DisplayInactive)
        }
    }
}

/// Which part of the size rule a target fails, so a caller branches on the
/// reason instead of parsing a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleRefusal {
    /// Candle bubble size on a pane that does not draw tick bars.
    NotTickChart,
    /// The target's bubbles are not drawing on the pane.
    DisplayInactive,
}

impl From<ScaleRefusal> for ControlError {
    fn from(refusal: ScaleRefusal) -> Self {
        match refusal {
            ScaleRefusal::NotTickChart => refused_precondition(
                TICK_CHART_PRECONDITION,
                "candle bubble size requires a tick chart",
                "switch the pane to a tick chart, or size the tape target",
            ),
            ScaleRefusal::DisplayInactive => refused_precondition(
                DISPLAY_ACTIVE_PRECONDITION,
                "the requested bubble display is not active",
                "switch the target's bubbles on (orderflow.bubbles reports them), then call again",
            ),
        }
    }
}

/// A refusal of a declared precondition: not retryable as sent, it names the
/// precondition and the step that meets it.
pub(crate) fn refused_precondition(
    precondition: &'static str,
    message: &str,
    next_step: &str,
) -> ControlError {
    let mut error = ControlError::invalid_request(message);
    error.context.violated_precondition = Some(
        PreconditionId::new(precondition).expect("bubble precondition IDs are valid registry IDs"),
    );
    error.context.next_steps = vec![next_step.to_owned()];
    error
}

/// The size capability's narrow port: the facts the rule reads, and setting
/// a gain the rule admitted. The policy stays here.
pub trait BubbleScalePort {
    /// The addressed pane's facts for [`BubbleScalePane::admit`]; refuses a
    /// pane without an order-flow view.
    fn bubble_scale_pane(
        &mut self,
        tab_id: u64,
        pane_id: u64,
    ) -> Result<BubbleScalePane, ControlError>;
    /// Set `target`'s gain to `scale`; answers whether the look changed and
    /// the gain the owner keeps.
    fn set_bubble_scale(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        target: BubbleScaleTarget,
        scale: f32,
    ) -> Result<(bool, f32), ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: BubbleScalePort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: BubbleScalePort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: BubbleScaleInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let scale = input.validated_scale()?;
    let (tab, pane) = (input.tab_id.get(), input.pane_id.get());
    host.bubble_scale_pane(tab, pane)?.admit(input.target)?;
    let (changed, kept) = host.set_bubble_scale(tab, pane, input.target, scale)?;
    serde_json::to_value(input.result(kept, changed)?).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "bubble_scale_tests.rs"]
mod bubble_scale_tests;
