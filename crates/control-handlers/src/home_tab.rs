//! The Home tab through its owner, which the window lends ([`ChromeOwners`]).
use crate::chrome_owners::{ChromeOwners, HomeTab};
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::home_tab::{
    HomeLeaveInput, HomeLeaveResult, HomeShowInput, HomeShowResult, descriptor, leave_descriptor,
};
use quantick_control_schema::scene::ActiveSurfaceDto;
use serde_json::Value;

const NO_HOME: &str = "the Home tab is unavailable";

/// Bring Home over the market on screen.
///
/// # Errors
/// A window that lends no Home.
pub fn show_home<H: ChromeOwners + ?Sized>(app: &mut H) -> Result<(), ControlError> {
    lent(app).map(HomeTab::show)
}

/// Put the market Home hid back on screen.
///
/// # Errors
/// A window that lends no Home.
pub fn leave_home<H: ChromeOwners + ?Sized>(app: &mut H) -> Result<(), ControlError> {
    lent(app).map(HomeTab::leave)
}

fn lent<H: ChromeOwners + ?Sized>(app: &mut H) -> Result<HomeTab<'_>, ControlError> {
    (app.home_tab()).ok_or_else(|| ControlError::invalid_request(NO_HOME))
}

/// Outside the replay trace: Home works during a replay because it changes no
/// market, and a recorded call must not bring Home up when a session re-runs.
pub fn register<H: ChromeOwners + ?Sized, A>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), show)?;
    registry.register_outside_replay(leave_descriptor(), leave)
}

fn show<H: ChromeOwners + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let invalid = |error: serde_json::Error| ControlError::invalid_request(error.to_string());
    let _: HomeShowInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    show_home(app)?;
    serde_json::to_value(HomeShowResult {
        active_surface: ActiveSurfaceDto::Home,
    })
    .map_err(invalid)
}

fn leave<H: ChromeOwners + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let invalid = |error: serde_json::Error| ControlError::invalid_request(error.to_string());
    let _: HomeLeaveInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    leave_home(app)?;
    serde_json::to_value(HomeLeaveResult {
        active_surface: ActiveSurfaceDto::Market,
    })
    .map_err(invalid)
}
