//! The forming bar's progress on the last-price tag, over the market
//! settings owner's narrow port. The setting belongs to the market the
//! addressed pane shows; the port resolves that market and keeps it.
//!
//! A display preference with no market-data side effect, docked outside
//! every replay: a replay's control trace never re-runs it.
//!
//! Rate class: a human or an agent changing a preference. Never per trade.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::price_tag_progress::{PriceTagProgressInput, descriptor};
use serde_json::Value;

/// The price-tag progress capability's narrow port.
pub trait PriceTagProgressPort {
    /// Keep `on` for the market the pane `pane_id` on tab `tab_id` shows;
    /// answers whether it changed, or refuses an address that names nothing.
    fn set_price_tag_progress(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        on: bool,
    ) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: PriceTagProgressPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), set)
}

fn set<H: PriceTagProgressPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: PriceTagProgressInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let changed = host.set_price_tag_progress(input.tab_id.get(), input.pane_id.get(), input.on)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One market, two panes of it, one pane of another.
    #[derive(Default)]
    struct Markets {
        off: Vec<&'static str>,
    }

    impl PriceTagProgressPort for Markets {
        fn set_price_tag_progress(
            &mut self,
            tab: u64,
            pane: u64,
            on: bool,
        ) -> Result<bool, ControlError> {
            let market = match (tab, pane) {
                (1, 0 | 1) => "WIN",
                (2, 0) => "BTC",
                _ => return Err(invalid("the call names an unknown pane")),
            };
            let was_on = !self.off.contains(&market);
            self.off.retain(|kept| *kept != market);
            if !on {
                self.off.push(market);
            }
            Ok(was_on != on)
        }
    }

    fn call(host: &mut Markets, input: &Value) -> Result<Value, ControlError> {
        let actor = crate::test_support::FakeWindow::assistant();
        set(host, &mut (), &actor, input)
    }

    /// Off then off again is one change; the answer names the pane and the
    /// value; the other market is untouched.
    #[test]
    fn the_switch_is_idempotent_and_answers_what_stands() {
        let mut host = Markets::default();
        let off = json!({"tab_id": "1", "pane_id": "1", "on": false});
        let first = call(&mut host, &off).unwrap();
        assert_eq!(
            first,
            json!({"tab_id": "1", "pane_id": "1", "on": false, "changed": true})
        );
        assert_eq!(call(&mut host, &off).unwrap()["changed"], false);
        assert_eq!(host.off, ["WIN"]);
    }

    /// An unknown pane, an unknown field and a missing value are refused
    /// before anything is kept.
    #[test]
    fn a_bad_address_or_input_is_refused_and_keeps_nothing() {
        let mut host = Markets::default();
        for input in [
            json!({"tab_id": "9", "pane_id": "0", "on": false}),
            json!({"tab_id": "1", "pane_id": "0", "on": false, "extra": 1}),
            json!({"tab_id": "1", "pane_id": "0"}),
            json!({"tab_id": "1", "pane_id": "0", "on": "no"}),
        ] {
            assert!(call(&mut host, &input).is_err(), "{input}");
        }
        assert!(host.off.is_empty());
    }
}
