//! Opening-burst scaling over the pane owner's narrow port. The rule — the
//! candle target needs active tick FLOW bubbles, the tape target needs only
//! an order-flow view — lives here; the port reads that fact and keeps the
//! preference.
use crate::addressed_pane::AddressedPanePort;
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::opening_scale::{OpeningScaleInput, OpeningScaleTarget, descriptor};
use serde_json::Value;

/// The opening-scale capability's narrow port on the pane.
pub trait OpeningScalePort {
    /// Whether the pane draws tick FLOW bubbles; refuses a pane without an
    /// order-flow view.
    fn tick_flow_active(&self) -> Result<bool, ControlError>;
    /// Keep `ignore` as `target`'s preference; answers whether it changed.
    fn set_opening_scale(
        &mut self,
        target: OpeningScaleTarget,
        ignore: bool,
    ) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: AddressedPanePort<Pane: OpeningScalePort> + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: AddressedPanePort<Pane: OpeningScalePort> + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: OpeningScaleInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    answer(host.addressed_pane(input.tab_id, input.pane_id)?, input)
}

fn answer<P: OpeningScalePort + ?Sized>(
    pane: &mut P,
    input: OpeningScaleInput,
) -> Result<Value, ControlError> {
    if input.target == OpeningScaleTarget::Candle && !pane.tick_flow_active()? {
        return Err(invalid(
            "regional opening scale requires active tick FLOW bubbles",
        ));
    }
    let ignore = input.ignore_opening_burst_in_scale;
    let changed = pane.set_opening_scale(input.target, ignore)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Default)]
    struct Pane {
        flow: bool,
        kept: [bool; 2],
        sets: usize,
    }

    impl OpeningScalePort for Pane {
        fn tick_flow_active(&self) -> Result<bool, ControlError> {
            Ok(self.flow)
        }
        fn set_opening_scale(
            &mut self,
            target: OpeningScaleTarget,
            ignore: bool,
        ) -> Result<bool, ControlError> {
            self.sets += 1;
            let slot = &mut self.kept[usize::from(target == OpeningScaleTarget::Candle)];
            let changed = *slot != ignore;
            *slot = ignore;
            Ok(changed)
        }
    }

    fn call(pane: &mut Pane, target: &str, ignore: bool) -> Result<Value, ControlError> {
        let input = json!({"tab_id": "1", "pane_id": "0", "target": target,
            "ignore_opening_burst_in_scale": ignore});
        answer(pane, serde_json::from_value(input).map_err(invalid)?)
    }

    /// The candle target is refused before anything is kept unless tick FLOW
    /// bubbles draw; the tape target never asks.
    #[test]
    fn the_candle_target_needs_tick_flow_and_the_tape_target_does_not() {
        let mut pane = Pane::default();
        assert!(call(&mut pane, "candle", true).is_err());
        assert_eq!(pane.sets, 0);
        let tape = call(&mut pane, "tape", true).unwrap();
        assert_eq!(
            (tape["target"].clone(), tape["changed"].clone()),
            (json!("tape"), json!(true))
        );
        assert_eq!(call(&mut pane, "tape", true).unwrap()["changed"], false);
        pane.flow = true;
        assert_eq!(call(&mut pane, "candle", true).unwrap()["changed"], true);
        assert_eq!(pane.kept, [true, true]);
    }

    /// A call naming no target sets the tape's preference, answers the value
    /// it kept, and switching it back is a change; the preference is saved.
    #[test]
    fn no_target_means_the_tape_and_the_answer_echoes_the_preference() {
        assert_eq!(
            descriptor().persistence,
            quantick_control::registry::EffectPersistence::Durable
        );
        let mut pane = Pane::default();
        let set = |pane: &mut Pane, ignore: bool| {
            let input = json!({"tab_id": "1", "pane_id": "0",
                "ignore_opening_burst_in_scale": ignore});
            answer(pane, serde_json::from_value(input).unwrap()).unwrap()
        };
        let on = set(&mut pane, true);
        assert_eq!(
            (
                &on["target"],
                &on["ignore_opening_burst_in_scale"],
                &on["changed"]
            ),
            (&json!("tape"), &json!(true), &json!(true))
        );
        assert_eq!(pane.kept, [true, false]);
        let off = set(&mut pane, false);
        assert_eq!(
            (&off["ignore_opening_burst_in_scale"], &off["changed"]),
            (&json!(false), &json!(true))
        );
    }
}
