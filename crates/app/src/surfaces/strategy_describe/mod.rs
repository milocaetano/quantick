use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use quantick_ui_model::strategy_describe::{Effects, Intent, Model, State};

pub(crate) mod provider;

#[derive(Default)]
pub(crate) struct View {
    pub(crate) worker: provider::Worker,
}

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "strategy_describe"
    }
    fn paint(
        &mut self,
        _state: &State,
        _target: &mut ViewContext<'_>,
        _env: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }

    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) -> Option<Intent> {
        (self.worker.available()
            && crate::hooks::captured::var("QUANTICK_STRATEGY_DESCRIBE").as_deref() == Some("1"))
        .then_some(Intent::Command(
            quantick_ui_model::strategy_describe::Command::Open,
        ))
    }
}

pub(crate) fn effects(effect: Effects) -> SurfaceResponse {
    SurfaceResponse::effect_if(
        effect != Effects::None,
        super::HostEffect::StrategyDescribe(effect),
    )
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_STRATEGY_DESCRIBE"];
