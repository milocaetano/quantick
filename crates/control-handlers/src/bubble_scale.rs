//! Bubble magnification over the pane owner's narrow port. One rule says
//! which bubbles a pane may size; the pane's size sliders and
//! `orderflow.bubbles.size.set` both ask it, so they cannot disagree.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::bubble_scale::{BubbleScaleInput, BubbleScaleTarget, descriptor};
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
    /// Says which part of the rule `target` fails.
    pub fn admit(self, target: BubbleScaleTarget) -> Result<(), &'static str> {
        if target == BubbleScaleTarget::Candle && !self.tick_chart {
            Err("candle bubble size requires a tick chart")
        } else if self.may_scale(target) {
            Ok(())
        } else {
            Err("the requested bubble display is not active")
        }
    }
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
    host.bubble_scale_pane(tab, pane)?
        .admit(input.target)
        .map_err(invalid)?;
    let (changed, kept) = host.set_bubble_scale(tab, pane, input.target, scale)?;
    serde_json::to_value(input.result(kept, changed)?).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "bubble_scale_tests.rs"]
mod bubble_scale_tests;
