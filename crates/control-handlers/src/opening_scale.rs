//! Opening-burst scaling over the pane owner's narrow port. The rule — the
//! candle target needs active tick FLOW bubbles, the tape target needs only
//! an order-flow view — lives here; the port reads that fact and keeps the
//! preference.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::opening_scale::{OpeningScaleInput, OpeningScaleTarget, descriptor};
use serde_json::Value;

/// The opening-scale capability's narrow port.
pub trait OpeningScalePort {
    /// Whether the addressed pane draws tick FLOW bubbles; refuses a pane
    /// without an order-flow view.
    fn tick_flow_active(&mut self, tab_id: u64, pane_id: u64) -> Result<bool, ControlError>;
    /// Keep `ignore` as `target`'s preference; answers whether it changed.
    fn set_opening_scale(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        target: OpeningScaleTarget,
        ignore: bool,
    ) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: OpeningScalePort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: OpeningScalePort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: OpeningScaleInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let (tab, pane) = (input.tab_id.get(), input.pane_id.get());
    if input.target == OpeningScaleTarget::Candle && !host.tick_flow_active(tab, pane)? {
        return Err(invalid(
            "regional opening scale requires active tick FLOW bubbles",
        ));
    }
    let ignore = input.ignore_opening_burst_in_scale;
    let changed = host.set_opening_scale(tab, pane, input.target, ignore)?;
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
        fn tick_flow_active(&mut self, _tab: u64, _pane: u64) -> Result<bool, ControlError> {
            Ok(self.flow)
        }
        fn set_opening_scale(
            &mut self,
            _tab: u64,
            _pane: u64,
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
        let actor = crate::test_support::FakeWindow::assistant();
        let input = json!({"tab_id": "1", "pane_id": "0", "target": target,
            "ignore_opening_burst_in_scale": ignore});
        set(pane, &mut (), &actor, &input)
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
}
