use super::strategy_describe_contract::DescribeHost;
use quantick_control::{error::ControlError, registry::RegistryError};
use quantick_control_handlers::{JournalAccess, dock::ActionDock};
use quantick_control_host::{authority::STRATEGY_MODULE_ID, journal::NewEvent};
use quantick_control_schema::strategy_describe as wire;
use quantick_ui_model::strategy_describe::Intent;
use serde_json::json;

pub(crate) fn register<H: DescribeHost + ?Sized, A: JournalAccess>(
    registry: &mut impl ActionDock<H, A>,
    available: bool,
) -> Result<(), RegistryError> {
    macro_rules! dock {
        ($id:path) => {
            registry.register_outside_replay(wire::descriptor($id, available), |host, access, actor, input| {
                let intent = Intent::Command(wire::command($id, input)?);
                let status = host.dispatch(intent)?;
                let result = wire::Status::from(status);
                if $id != wire::READ_CAPABILITY_ID {
                    access.record_event(NewEvent::by(STRATEGY_MODULE_ID, wire::EVENT, actor, json!({"operation": $id, "revision": result.revision})));
                }
                serde_json::to_value(result).map_err(|_| ControlError::invalid_request("Invalid strategy description result"))
            })?;
        };
    }
    dock!(wire::OPEN_CAPABILITY_ID);
    dock!(wire::READ_CAPABILITY_ID);
    dock!(wire::SUBMIT_CAPABILITY_ID);
    dock!(wire::CANCEL_CAPABILITY_ID);
    dock!(wire::SAVE_CAPABILITY_ID);
    Ok(())
}
