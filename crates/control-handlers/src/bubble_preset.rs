//! Named whole looks over the pane owner's narrow port.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::bubble_preset::{ApplyInput, ApplyResult, ListInput, ListResult};
use serde_json::Value;

pub trait BubblePresetPort {
    fn apply_bubble_preset(&mut self, input: &ApplyInput) -> Result<ApplyResult, ControlError>;
    fn list_bubble_presets(&mut self, input: &ListInput) -> Result<ListResult, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: BubblePresetPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(
        quantick_control_schema::bubble_preset::apply_descriptor(),
        apply,
    )?;
    registry.register(
        quantick_control_schema::bubble_preset::list_descriptor(),
        list,
    )
}

fn apply<H: BubblePresetPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.apply_bubble_preset(&input)?).map_err(invalid)
}

fn list<H: BubblePresetPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.list_bubble_presets(&input)?).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

/// The existing chart-only selector retains its independent contract.
pub trait TickBubblePort {
    fn set_tick_bubbles(
        &mut self,
        input: &quantick_control_schema::tick_bubbles::TickBubblesInput,
    ) -> Result<quantick_control_schema::tick_bubbles::TickBubblesResult, ControlError>;
}
pub fn register_tick<D: ActionDock<H, A>, H: TickBubblePort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(quantick_control_schema::tick_bubbles::descriptor(), tick)
}
fn tick<H: TickBubblePort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = serde_json::from_value(input.clone()).map_err(invalid)?;
    serde_json::to_value(host.set_tick_bubbles(&input)?).map_err(invalid)
}

#[cfg(test)]
mod tick_rule_tests {
    use super::*;
    use quantick_control_schema::tick_bubbles::TickBubblesSnapshot;
    use serde_json::json;

    /// Review #19/#29: one rule decides tick spheres for the picker and for
    /// tick.set. Choosing needs a tick chart and the asset's config mode;
    /// dropping them is always allowed, whatever put them on.
    #[test]
    fn tick_spheres_need_a_tick_chart_and_the_asset_mode_but_anyone_may_drop_them() {
        for tick_chart in [false, true] {
            for asset_mode in [false, true] {
                let pane = TickSpherePane {
                    tick_chart,
                    asset_mode,
                };
                assert_eq!(pane.may_choose(), tick_chart && asset_mode);
                assert_eq!(
                    pane.admit(Some("Binance BTC Spot spheres")).is_ok(),
                    tick_chart && asset_mode,
                    "{pane:?}"
                );
                assert!(pane.admit(None).is_ok(), "{pane:?}");
            }
        }
    }

    struct Pane {
        facts: TickSpherePane,
        worn: Option<Option<String>>,
    }
    impl TickBubblePort for Pane {
        fn tick_sphere_pane(
            &mut self,
            _tab_id: u64,
            _pane_id: u64,
        ) -> Result<TickSpherePane, ControlError> {
            Ok(self.facts)
        }
        fn wear_tick_spheres(
            &mut self,
            _tab_id: u64,
            _pane_id: u64,
            preset: Option<&str>,
        ) -> Result<(bool, Option<TickBubblesSnapshot>), ControlError> {
            self.worn = Some(preset.map(str::to_owned));
            Ok((true, None))
        }
    }

    #[test]
    fn tick_set_asks_the_rule_before_the_pane_wears_anything() {
        let input = |preset: Value| json!({"tab_id": "1", "pane_id": "2", "preset": preset});
        let actor = crate::test_support::FakeWindow::assistant();
        let mut volume_chart = Pane {
            facts: TickSpherePane {
                tick_chart: false,
                asset_mode: true,
            },
            worn: None,
        };
        let refused = tick(
            &mut volume_chart,
            &mut (),
            &actor,
            &input(json!("Binance BTC Spot spheres")),
        );
        assert!(refused.is_err());
        assert_eq!(volume_chart.worn, None, "refused before any edit");
        let mut plain_asset = Pane {
            facts: TickSpherePane {
                tick_chart: true,
                asset_mode: false,
            },
            worn: None,
        };
        let dropped = tick(&mut plain_asset, &mut (), &actor, &input(Value::Null)).unwrap();
        assert_eq!(
            plain_asset.worn,
            Some(None),
            "spheres can always be dropped"
        );
        assert_eq!(dropped["changed"], true);
        let mut btc = Pane {
            facts: TickSpherePane {
                tick_chart: true,
                asset_mode: true,
            },
            worn: None,
        };
        tick(
            &mut btc,
            &mut (),
            &actor,
            &input(json!("Binance BTC Spot spheres")),
        )
        .unwrap();
        assert_eq!(btc.worn, Some(Some("Binance BTC Spot spheres".to_owned())));
    }
}
