//! Bind editor requests and its bounded validation worker to existing chart slots.
use super::indicator_wiring::IndicatorWiring;
use super::{arrangement_host::ArrangementHost, indicator_manager::IndicatorState};
use crate::{
    indicator_worker::{IndicatorCommand, IndicatorSource},
    surfaces::Surfaces,
};
use quantick_control_handlers::indicator_host::{IndicatorRefusal, NumericInputGate};
use quantick_control_schema::script_editor::{
    NumericValue, capture_numeric_inputs, update_numeric_input,
};
use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction, ScriptLibraryPort};
use quantick_ui_model::script_editor::{Effect, Intent};
use std::time::Instant;

fn attached_target(wiring: &IndicatorWiring<'_>, id: &str) -> Option<super::TabSlot> {
    let runtime = &*wiring.runtime;
    let (tab, side) = (
        runtime.tabs.active_id(),
        runtime.active_tab().focused_side(),
    );
    let mut owners = runtime.indicators.registry.script_slots(id);
    owners.find(|owner| owner.workspace == runtime.id() && owner.tab == tab && owner.side == side)
}

pub(super) fn set_numeric_input(
    wiring: &mut IndicatorWiring<'_>,
    id: &str,
    name: &str,
    value: NumericValue,
) -> Result<(), IndicatorRefusal> {
    let editor = wiring.surfaces.script_editor.model_state.editor();
    let target = attached_target(wiring, id);
    let pane = wiring.runtime.active_tab().focused_pane();
    let view = target.map(|target| {
        let mut views = pane.indicators.all().iter();
        (views.find(|view| view.slot == target.slot)).map(|view| (target, view))
    });
    let loaded = view.flatten().map(|(_, view)| view);
    let (target, view) = NumericInputGate {
        script_open: (editor.document.as_ref()).is_some_and(|document| document.id == id),
        settings_open: (wiring.surfaces.indicator_settings.model_state.dialog()).is_some(),
        view,
        source_errors: loaded.is_some_and(|view| view.stale.is_some())
            || editor.diagnostics.is_some(),
        inputs_ready: loaded.is_some_and(|view| editor.inputs_ready(&view.descriptor.inputs)),
    }
    .admit()?;
    let saved = super::layout_wiring::saved_inputs(
        &wiring.runtime.tabs,
        &wiring.runtime.indicators,
        wiring.runtime.workspace.layouts(),
        target,
    )
    .ok_or(IndicatorRefusal::InputsUnavailable)?;
    let values = update_numeric_input(&view.descriptor.inputs, &saved, name, &value)
        .map_err(IndicatorRefusal::Rejected)?;
    wiring.apply_indicator_settings_change(super::indicator_manager::SettingsChange::Inputs {
        target,
        slot: target.slot,
        values,
        commit: true,
    });
    Ok(())
}

pub(super) fn add_to_chart(
    wiring: &mut IndicatorWiring<'_>,
    id: &str,
) -> Result<(), IndicatorRefusal> {
    let runtime = &*wiring.runtime;
    ScriptLibraryPort::read(&runtime.indicators.script_library, id)
        .map_err(IndicatorRefusal::Rejected)?;
    let index = (runtime.indicators.script_library)
        .index_of(id)
        .ok_or(IndicatorRefusal::UnknownLibraryScript)?;
    if attached_target(wiring, id).is_none() {
        wiring.add_library_script_at(index);
    }
    Ok(())
}

pub(super) fn inspect(
    surfaces: &Surfaces,
    indicators: &IndicatorState,
    tabs: &ArrangementHost,
    store: &crate::workspace_store::LayoutStore,
    offset: usize,
    limit: usize,
) -> quantick_control_schema::script_editor::EditorResult {
    let mut result = quantick_control_schema::script_editor::capture(
        surfaces.script_editor.model_state.editor(),
        indicators.script_library.entries(),
        offset,
        limit,
    );
    if let Some(document) = &result.document {
        let pane = tabs[tabs.active_index()].focused_pane();
        let mut owners = indicators.registry.script_slots(&document.id);
        if let Some(owner) = owners.find(|owner| {
            owner.workspace == tabs.owner()
                && owner.tab == tabs.active_id()
                && owner.pane == pane.id
        }) && let Some(view) = pane
            .indicators
            .all()
            .iter()
            .find(|view| view.slot == owner.slot)
            && surfaces
                .script_editor
                .model_state
                .editor()
                .inputs_ready(&view.descriptor.inputs)
            && let Some(saved) = super::layout_wiring::saved_inputs(tabs, indicators, store, owner)
        {
            result.numeric_inputs = capture_numeric_inputs(&view.descriptor.inputs, &saved);
        }
    }
    result
}

pub(super) fn apply(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    action: ScriptEditorAction,
) -> Result<Option<ScriptDocument>, String> {
    let effects = update(surfaces, Intent::Action(action));
    for effect in effects {
        if let Effect::Dispatch { mut editor, action } = effect {
            let result = editor.dispatch(&mut indicators.script_library, action);
            for effect in update(surfaces, Intent::ActionSettled { editor, result }) {
                match effect {
                    Effect::ObserveSaved(document) => observe_saved(indicators, &document),
                    Effect::ActionResult(result) => return result,
                    _ => unreachable!("action settlement effect"),
                }
            }
        }
    }
    unreachable!("synchronous editor action result")
}

fn update(surfaces: &mut Surfaces, intent: Intent) -> Vec<Effect> {
    surfaces
        .script_editor
        .update(intent)
        .model_effects
        .into_iter()
        .flat_map(|effect| {
            let quantick_ui_model::HostEffect::ScriptEditor(effects) = effect else {
                unreachable!("editor effect route")
            };
            effects
        })
        .collect()
}

pub(super) fn frame(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    tabs: &mut ArrangementHost,
    store: &crate::workspace_store::LayoutStore,
    actions: Vec<ScriptEditorAction>,
    now: Instant,
) {
    for action in actions {
        if let Err(error) = apply(surfaces, indicators, action) {
            crate::surfaces::toast::note(&mut surfaces.toast, error, now);
        }
    }
    let view = &mut surfaces.script_editor.view;
    let completion =
        view.compilation
            .as_ref()
            .and_then(|(revision, receiver)| match receiver.try_recv() {
                Ok((revision, document, error, inputs)) => Some(Intent::Completed {
                    revision,
                    document,
                    error,
                    inputs,
                }),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Intent::Failed {
                    revision: *revision,
                    error: "source validation worker stopped".into(),
                }),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            });
    if let Some(intent) = completion {
        view.compilation = None;
        let effects = update(surfaces, intent);
        execute(surfaces, indicators, tabs, store, effects);
    }
    let elapsed = u64::try_from(
        now.saturating_duration_since(surfaces.script_editor.view.epoch)
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let effects = update(surfaces, Intent::Tick(elapsed));
    execute(surfaces, indicators, tabs, store, effects);
}

fn execute(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    tabs: &mut ArrangementHost,
    store: &crate::workspace_store::LayoutStore,
    effects: Vec<Effect>,
) {
    for effect in effects {
        let next = match effect {
            Effect::Save { revision, document } => {
                let result = indicators.script_library.save(&document);
                Some(Intent::Saved {
                    revision,
                    document,
                    result,
                })
            }
            Effect::ObserveSaved(document) => {
                observe_saved(indicators, &document);
                None
            }
            Effect::Publish(document) => {
                reload(indicators, tabs, store, &document);
                None
            }
            Effect::Validate { revision, document } => {
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                match std::thread::Builder::new()
                    .name("quantick-script-validator".into())
                    .spawn(move || {
                        let (inputs, error) =
                            quantick_pine::validate(&document.source, &document.title);
                        let _ = sender.send((revision, document, error, inputs));
                    }) {
                    Ok(_) => {
                        surfaces.script_editor.view.compilation = Some((revision, receiver));
                        None
                    }
                    Err(error) => Some(Intent::Failed {
                        revision,
                        error: format!("cannot validate source: {error}"),
                    }),
                }
            }
            _ => unreachable!("validation effect route"),
        };
        if let Some(intent) = next {
            let effects = update(surfaces, intent);
            execute(surfaces, indicators, tabs, store, effects);
        }
    }
}

fn observe_saved(indicators: &mut IndicatorState, document: &ScriptDocument) {
    // Manual and idle saves share the watcher suppression: only a current
    // validation completion may publish an editor edit.
    if let Some(index) = indicators.script_library.index_of(&document.id)
        && let Some((_, modified)) = indicators.script_library.file_info(index)
    {
        let owners: Vec<_> = indicators.registry.watching(index).collect();
        for owner in owners {
            indicators.registry.observe_script(owner, modified);
        }
    }
}

fn reload(
    indicators: &IndicatorState,
    tabs: &mut ArrangementHost,
    store: &crate::workspace_store::LayoutStore,
    document: &ScriptDocument,
) {
    for owner in indicators.registry.script_slots(&document.id) {
        let saved = super::layout_wiring::saved_inputs(tabs, indicators, store, owner);
        if owner.workspace == tabs.owner()
            && let Some(tab) = tabs.by_id_mut(owner.tab)
            && let Some(pane) = tab.pane_at_mut(owner.side.index())
            && pane.id == owner.pane
        {
            let source = IndicatorSource::Script {
                name: document.id.clone(),
                text: document.source.clone(),
            };
            pane.indicator_worker.send(match saved {
                Some(values) => IndicatorCommand::ReloadWithInputs {
                    slot: owner.slot,
                    source,
                    values,
                },
                None => IndicatorCommand::Reload {
                    slot: owner.slot,
                    source,
                },
            });
        }
    }
}
