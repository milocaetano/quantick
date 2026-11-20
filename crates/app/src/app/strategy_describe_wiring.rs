//! Bind the shared conversation to the presented chart and its provider.
use super::arrangement_host::ArrangementHost;
use crate::control::strategy_describe_contract::DescribeHost;
use crate::surfaces::Surfaces;
use quantick_ui_model::strategy_describe::{Command, Effects, Intent, Status};

impl<T: super::control_host::TabsMutPort + ?Sized> DescribeHost for T {
    fn dispatch(&mut self, intent: Intent) -> Result<Status, String> {
        let (surfaces, tabs) = self.describe_owners();
        dispatch(surfaces, tabs, intent)
    }
}

pub(super) fn dispatch(
    surfaces: &mut Surfaces,
    _tabs: &mut ArrangementHost,
    intent: Intent,
) -> Result<Status, String> {
    if !surfaces.strategy_describe.view.worker.available()
        && !matches!(intent, Intent::Command(Command::Read))
    {
        return Err("Strategy description is unavailable".into());
    }
    let effects = surfaces.update(crate::surfaces::HostIntent::StrategyDescribe(Box::new(
        intent,
    )));
    for effect in effects.model_effects {
        if let quantick_ui_model::HostEffect::StrategyDescribe(Effects::Request(request)) = effect {
            surfaces.strategy_describe.view.worker.submit(request)?;
        }
    }
    Ok(surfaces.strategy_describe.model_state.status.clone())
}

pub(super) fn frame(surfaces: &mut Surfaces, tabs: &mut ArrangementHost, effect: Option<Effects>) {
    if let Some(Effects::Dispatch(command)) = effect {
        let _ = dispatch(surfaces, tabs, Intent::Command(command));
    }
    if let Some((revision, reply)) = surfaces.strategy_describe.view.worker.poll() {
        let _ = dispatch(surfaces, tabs, Intent::Replied { revision, reply });
    }
}
