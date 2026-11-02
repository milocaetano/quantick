//! `orderflow.tape.market_fold.set` over the fold's narrow port.
use crate::dock::ActionDock;
use crate::tape_fold::TapeFoldPort;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_market_fold::{TapeMarketFoldInput, descriptor};
use serde_json::Value;

pub fn register<D: ActionDock<H, A>, H: TapeFoldPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: TapeFoldPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeMarketFoldInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let (tab, pane) = (input.tab_id.get(), input.pane_id.get());
    let changed = host.set_tape_market_fold(tab, pane, input.enabled)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tape_fold::TapeFoldPane;
    use serde_json::json;

    struct Pane(bool);

    impl TapeFoldPort for Pane {
        fn tape_fold_pane(&mut self, _: u64, _: u64) -> Result<TapeFoldPane, ControlError> {
            Ok(TapeFoldPane {
                beside_candles: true,
            })
        }
        fn set_tape_fold(
            &mut self,
            _: u64,
            _: u64,
            _: quantick_control_schema::tape_fold::TapeFold,
        ) -> Result<bool, ControlError> {
            Ok(false)
        }
        fn set_tape_market_fold(
            &mut self,
            _tab: u64,
            _pane: u64,
            enabled: bool,
        ) -> Result<bool, ControlError> {
            Ok(std::mem::replace(&mut self.0, enabled) != enabled)
        }
    }

    /// Repeating a value changes nothing, and a bad input keeps nothing.
    #[test]
    fn the_setting_is_kept_and_a_repeat_says_so() {
        let actor = crate::test_support::FakeWindow::assistant();
        let input = json!({"tab_id": "1", "pane_id": "0", "enabled": false});
        let mut pane = Pane(true);
        let first = set(&mut pane, &mut (), &actor, &input).expect("kept");
        assert_eq!(first["changed"], true);
        assert!(!pane.0);
        let again = set(&mut pane, &mut (), &actor, &input).expect("kept");
        assert_eq!(again["changed"], false);
        let bad = json!({"tab_id": "1", "pane_id": "0"});
        assert!(set(&mut pane, &mut (), &actor, &bad).is_err());
    }
}
