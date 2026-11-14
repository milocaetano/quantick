//! Repeated adapter mechanics; each invocation defines its own local View.
macro_rules! chrome_probe_view {
    ($model:ident, $tag:ident) => {
        use quantick_ui_model::$model::{Effects, Intent, Model, State};
        #[derive(Default)]
        pub(crate) struct View;
        impl crate::surfaces::View<Model> for View {
            fn id(&self) -> &'static str {
                stringify!($model)
            }
            fn paint(
                &mut self,
                state: &State,
                _: &mut crate::surfaces::ViewContext<'_>,
                _: Option<&crate::surfaces::SurfaceEnv<'_>>,
            ) -> Option<Intent> {
                state.visible.then_some(Intent::Add(1))
            }
        }
        pub(crate) fn effects(effects: Effects) -> crate::surfaces::SurfaceResponse {
            crate::surfaces::SurfaceResponse {
                model_effects: vec![quantick_ui_model::HostEffect::$tag(effects)],
                ..Default::default()
            }
        }
    };
}
