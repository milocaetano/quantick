//! `orderflow.tape.market_fold.set` over the fold's narrow port.
use crate::dock::ActionDock;
use crate::tape_fold::TapeFoldPort;
use crate::tape_pane::PaneTapePort;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_market_fold::{TapeMarketFoldInput, descriptor};
use serde_json::Value;

pub fn register<D: ActionDock<H, A>, H: PaneTapePort<Pane: TapeFoldPort> + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: PaneTapePort<Pane: TapeFoldPort> + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeMarketFoldInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    answer(host.pane_tape(input.tab_id, input.pane_id)?, input)
}

fn answer<P: TapeFoldPort + ?Sized>(
    pane: &mut P,
    input: TapeMarketFoldInput,
) -> Result<Value, ControlError> {
    let changed = pane.set_tape_market_fold(input.enabled)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Pane(bool);

    impl TapeFoldPort for Pane {
        fn set_tape_fold(
            &mut self,
            _: quantick_control_schema::tape_fold::TapeFold,
        ) -> Result<bool, ControlError> {
            Ok(false)
        }
        fn set_tape_market_fold(&mut self, enabled: bool) -> Result<bool, ControlError> {
            Ok(std::mem::replace(&mut self.0, enabled) != enabled)
        }
    }

    /// Repeating a value changes nothing, and a bad input keeps nothing.
    #[test]
    fn the_setting_is_kept_and_a_repeat_says_so() {
        let call = |pane: &mut Pane, input: Value| {
            answer(pane, serde_json::from_value(input).map_err(invalid)?)
        };
        let input = json!({"tab_id": "1", "pane_id": "0", "enabled": false});
        let mut pane = Pane(true);
        let first = call(&mut pane, input.clone()).expect("kept");
        assert_eq!(first["changed"], true);
        assert!(!pane.0);
        let again = call(&mut pane, input).expect("kept");
        assert_eq!(again["changed"], false);
        let bad = json!({"tab_id": "1", "pane_id": "0"});
        assert!(call(&mut pane, bad).is_err());
    }
}
