//! Bind editor requests and its bounded validation worker to existing chart slots.
use super::{arrangement_host::ArrangementHost, indicator_manager::IndicatorState};
use crate::{
    indicator_worker::{IndicatorCommand, IndicatorSource},
    surfaces::Surfaces,
};
use quantick_control_schema::script_editor::{
    NumericValue, capture_numeric_inputs, update_numeric_input,
};
use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction, ScriptLibraryPort};
use quantick_ui_model::script_editor::{Effect, Intent};
use std::time::Instant;

fn attached_target(app: &super::QuantickApp, id: &str) -> Option<super::TabSlot> {
    app.runtime.indicators.registry.slot_kinds().iter().find_map(|(owner, kind)| {
        (owner.workspace == app.runtime.id() && owner.tab == app.runtime.tabs.active_id()
            && owner.side == app.active_tab().focused_side()
            && matches!(kind, quantick_workspace::indicator_document::SavedKind::Script { name } if name == id))
            .then_some(*owner)
    })
}

pub(super) fn set_numeric_input(
    app: &mut super::QuantickApp,
    id: &str,
    name: &str,
    value: NumericValue,
) -> Result<(), String> {
    let editor = app.surfaces.script_editor.model_state.editor();
    if !editor
        .document
        .as_ref()
        .is_some_and(|document| document.id == id)
    {
        return Err("open the attached script before editing its inputs".into());
    }
    if app.runtime.indicators.indicator_settings.is_some() {
        return Err("close indicator settings before editing inputs through control".into());
    }
    let target = attached_target(app, id).ok_or("script is not attached to the focused pane")?;
    let view = app
        .focused_pane()
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == target.slot)
        .ok_or("wait for the attached script to load")?;
    if view.stale.is_some() || editor.diagnostics.is_some() {
        return Err("repair source errors before editing inputs".into());
    }
    if !editor.inputs_ready(&view.descriptor.inputs) {
        return Err(
            "wait for current source validation and chart reload before editing inputs".into(),
        );
    }
    let saved = super::layout_wiring::saved_inputs(
        &app.runtime.tabs,
        &app.runtime.indicators,
        app.runtime.workspace.layouts(),
        target,
    )
    .ok_or("script input persistence is unavailable")?;
    let values = update_numeric_input(&view.descriptor.inputs, &saved, name, &value)?;
    app.indicator_wiring().apply_indicator_settings_change(
        super::indicator_manager::SettingsChange::Inputs {
            target,
            slot: target.slot,
            values,
            commit: true,
        },
    );
    Ok(())
}

pub(super) fn add_to_chart(app: &mut super::QuantickApp, id: &str) -> Result<(), String> {
    ScriptLibraryPort::read(&app.runtime.indicators.script_library, id)?;
    let index = app
        .runtime
        .indicators
        .script_library
        .entries()
        .iter()
        .position(|entry| entry.name == id)
        .ok_or("unknown library script")?;
    let tab = app.runtime.tabs.active_id();
    let side = app.active_tab().focused_side();
    if app.runtime.indicators.registry.slot_kinds().iter().any(|(owner, kind)| {
        owner.workspace == app.runtime.id() && owner.tab == tab && owner.side == side
            && matches!(kind, quantick_workspace::indicator_document::SavedKind::Script { name } if name == id)
    }) {
        return Ok(());
    }
    app.apply_toolbar_action(crate::toolbar::ToolbarAction::AddScriptIndicator(index));
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
        if let Some((owner, _)) = indicators.registry.slot_kinds().iter().find(|(owner, kind)| {
            owner.workspace == tabs.owner() && owner.tab == tabs.active_id() && owner.pane == pane.id
            && matches!(kind, quantick_workspace::indicator_document::SavedKind::Script { name } if name == &document.id)
        }) && let Some(view) = pane.indicators.all().iter().find(|view| view.slot == owner.slot)
          && surfaces.script_editor.model_state.editor().inputs_ready(&view.descriptor.inputs)
          && let Some(saved) = super::layout_wiring::saved_inputs(tabs, indicators, store, *owner) {
            result.numeric_inputs = capture_numeric_inputs(&view.descriptor.inputs, &saved);
        }
    }
    result
}

pub(super) fn apply(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    _tabs: &mut ArrangementHost,
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
        if let Err(error) = apply(surfaces, indicators, tabs, action) {
            surfaces.toast.note(error, now);
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
                            match quantick_pine::compile(&document.source, &document.title) {
                                Ok(compiled) => (compiled.inputs, None),
                                Err(errors) => (
                                    Vec::new(),
                                    Some(
                                        errors
                                            .iter()
                                            .take(8)
                                            .map(|error| {
                                                error.render(&document.title, &document.source)
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n"),
                                    ),
                                ),
                            };
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
    if let Some(index) = indicators
        .script_library
        .entries()
        .iter()
        .position(|e| e.name == document.id)
        && let Some((_, modified)) = indicators.script_library.file_info(index)
    {
        let owners: Vec<_> = indicators
            .registry
            .script_files()
            .iter()
            .filter(|(_, watched, _)| *watched == index)
            .map(|(owner, ..)| *owner)
            .collect();
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
    for (owner, kind) in indicators.registry.slot_kinds() {
        if !matches!(kind, quantick_workspace::indicator_document::SavedKind::Script { name } if name == &document.id)
        {
            continue;
        }
        let saved = super::layout_wiring::saved_inputs(tabs, indicators, store, *owner);
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
