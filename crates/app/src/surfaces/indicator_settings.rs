//! Registered addressed indicator editor; the borrowed host retains IO and style authority.
use super::{Adapter, BorrowedSurfaceEnv, HostEffect, SurfaceResponse, ViewContext};
use quantick_ui_model::indicator_settings::{Effects, Intent, Model, PresetEditor};
use quantick_ui_model::{Slot, UpdateSession};

pub(crate) type IndicatorSettingsSurface = Adapter<Model, View>;
#[derive(Default)]
pub(crate) struct View;

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "indicator_settings"
    }
    fn paint_borrowed(
        &mut self,
        session: &mut UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        slot: Slot,
        env: Option<&mut BorrowedSurfaceEnv<'_>>,
    ) -> SurfaceResponse {
        if slot != Slot::EditorDialogs {
            return SurfaceResponse::default();
        }
        let Some(BorrowedSurfaceEnv::Indicators(env)) = env else {
            return SurfaceResponse::default();
        };
        let valid = env.target == session.state().target() && env.view.is_some();
        let label = env
            .view
            .as_deref()
            .map(|view| view.label().to_owned())
            .unwrap_or_default();
        let has_style = env
            .view
            .as_deref()
            .is_some_and(|view| !view.descriptor.plots.is_empty());
        session.update(Intent::ObserveTarget {
            valid,
            label,
            has_style,
        });
        if session.state().dialog().is_none() {
            return SurfaceResponse::default();
        }
        let view = env.view.as_deref_mut().expect("validated view");
        let plots = view.themed_plots();
        let names: Option<Vec<String>> = env
            .kind
            .map(|kind| env.presets.names_for(kind).map(str::to_owned).collect());
        let intent = crate::indicator_panel::draw(
            target.context(),
            session,
            &view.descriptor.inputs,
            &plots,
            &mut view.style,
            names.as_deref(),
        );
        PresetEditor {
            target: env.target,
            kind: env.kind,
            specs: &view.descriptor.inputs,
            presets: env.presets,
            path: env.presets_path,
        }
        .dispatch(session, intent, |name, saved, declared, kept| {
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "INDICATOR_PRESET_REBOUND", preset = %name, saved, declared, kept,
                action = "bound_by_position",
                "some of this preset's values no longer bind; those inputs took their defaults");
        });
        SurfaceResponse::default()
    }
}

pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    SurfaceResponse {
        model_effects: (effects != Effects::default())
            .then_some(HostEffect::IndicatorSettings(effects))
            .into_iter()
            .collect(),
    }
}
