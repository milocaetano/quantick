//! Registered addressed indicator editor; the borrowed host retains IO and style authority.
use super::{Adapter, BorrowedSurfaceEnv, HostEffect, SurfaceEnv, SurfaceResponse, ViewContext};
use crate::indicators::state_file::{SavedInput, SavedInputExt};
use quantick_indicators::InputSpec;
use quantick_ui_model::indicator_settings::{
    DEFAULT_PRESET, Effects, Intent, LocalOperation, Model, State,
};
use quantick_ui_model::{Slot, UpdateSession};

pub(crate) type IndicatorSettingsSurface = Adapter<Model, View>;
#[derive(Default)]
pub(crate) struct View;

impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "indicator_settings"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
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
        dispatch(
            session,
            env.target,
            env.kind,
            &view.descriptor.inputs,
            env.presets,
            env.presets_path,
            intent,
        );
        SurfaceResponse::default()
    }
}

/// Install IO only after the native frame has selected its winning outcome.
pub(crate) fn dispatch(
    session: &mut UpdateSession<'_, Model>,
    target: Option<quantick_chart::indicator_registry::TabSlot>,
    kind: Option<&quantick_workspace::indicator_document::SavedKind>,
    specs: &[InputSpec],
    presets: &mut crate::indicators::preset_file::PresetStore,
    path: &std::path::Path,
    intent: Intent,
) {
    session.with_local_effects(
        &mut |mut effects| {
            let receipt = effects
                .local_operation
                .take()
                .and_then(|operation| execute(target, kind, specs, presets, path, operation));
            (effects, receipt)
        },
        |session| session.update(intent),
    );
}

fn execute(
    current: Option<quantick_chart::indicator_registry::TabSlot>,
    kind: Option<&quantick_workspace::indicator_document::SavedKind>,
    specs: &[InputSpec],
    presets: &mut crate::indicators::preset_file::PresetStore,
    path: &std::path::Path,
    operation: LocalOperation,
) -> Option<Intent> {
    let target = match &operation {
        LocalOperation::Load { target, .. }
        | LocalOperation::Save { target, .. }
        | LocalOperation::Delete { target, .. } => *target,
    };
    if current != Some(target) {
        return None;
    }
    match operation {
        LocalOperation::Load { name, .. } => {
            // Keep unreadable cells in place so later values cannot drift left.
            let saved: Vec<_> = match &name {
                None => Vec::new(),
                Some(name) => presets
                    .get(kind?, name)?
                    .iter()
                    .map(SavedInput::to_value)
                    .collect(),
            };
            let bound = quantick_indicators::bind_by_position(specs, &saved);
            if bound.kept < saved.len() {
                tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                    event_code = "INDICATOR_PRESET_REBOUND",
                    preset = %name.as_deref().unwrap_or(DEFAULT_PRESET), saved = saved.len(),
                    declared = specs.len(), kept = bound.kept, action = "bound_by_position",
                    "some of this preset's values no longer bind; those inputs took their defaults");
            }
            Some(Intent::PresetLoaded {
                target,
                values: bound.values,
                label: name.unwrap_or_else(|| DEFAULT_PRESET.to_owned()),
            })
        }
        LocalOperation::Save { name, values, .. } => {
            let kind = kind?;
            let inputs = values.iter().map(SavedInput::from_value).collect();
            if !presets.insert(kind, &name, inputs) {
                return None;
            }
            presets.save(path);
            Some(Intent::PresetSaved { target, name })
        }
        LocalOperation::Delete { name, .. } => {
            if !presets.remove(kind?, &name) {
                return None;
            }
            presets.save(path);
            Some(Intent::PresetDeleted { target, name })
        }
    }
}

pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    SurfaceResponse {
        model_effects: (effects != Effects::default())
            .then_some(HostEffect::IndicatorSettings(effects))
            .into_iter()
            .collect(),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "indicator_settings/tests.rs"]
mod tests;
