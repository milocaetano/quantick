//! The Home tab through its owner, which the window lends ([`HomeTabHost`]).
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::home_tab::{
    HomeLeaveInput, HomeLeaveResult, HomeShowInput, HomeShowResult, descriptor, leave_descriptor,
};
use quantick_control_schema::scene::ActiveSurfaceDto;
use quantick_workspace::arrangement::strip::{MarketShown, StripSurface};
use serde_json::Value;

const NO_HOME: &str = "the Home tab is unavailable";

/// What the window lends the Home calls.
pub trait HomeTabHost {
    /// The owner, borrowed for one call; `None` for a host without a strip.
    fn home_tab(&mut self) -> Option<HomeTab<'_>> {
        None
    }
}

/// Home's owner for one call: the strip's surface and the market on screen,
/// which Home holds while it hides it.
pub struct HomeTab<'a> {
    surface: &'a mut StripSurface,
    shown: MarketShown<'a>,
}

impl<'a> HomeTab<'a> {
    #[must_use]
    pub fn new(surface: &'a mut StripSurface, shown: MarketShown<'a>) -> Self {
        Self { surface, shown }
    }
    /// Bring Home over the market on screen.
    pub fn show(self) {
        self.surface.show_home(self.shown);
    }
    /// Put the market Home hid back on screen.
    pub fn leave(self) {
        self.surface.show_market();
    }
}

/// Bring Home over the market on screen.
///
/// # Errors
/// A window that lends no Home.
pub fn show_home<H: HomeTabHost + ?Sized>(app: &mut H) -> Result<(), ControlError> {
    lent(app).map(HomeTab::show)
}

/// Put the market Home hid back on screen.
///
/// # Errors
/// A window that lends no Home.
pub fn leave_home<H: HomeTabHost + ?Sized>(app: &mut H) -> Result<(), ControlError> {
    lent(app).map(HomeTab::leave)
}

fn lent<H: HomeTabHost + ?Sized>(app: &mut H) -> Result<HomeTab<'_>, ControlError> {
    (app.home_tab()).ok_or_else(|| ControlError::invalid_request(NO_HOME))
}

/// Outside the replay trace: Home works during a replay because it changes no
/// market, and a recorded call must not bring Home up when a session re-runs.
pub fn register<H: HomeTabHost + ?Sized, A>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), show)?;
    registry.register_outside_replay(leave_descriptor(), leave)
}

fn show<H: HomeTabHost + ?Sized, A>(
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

fn leave<H: HomeTabHost + ?Sized, A>(
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

#[cfg(test)]
#[path = "home_tab_tests.rs"]
mod tests;
