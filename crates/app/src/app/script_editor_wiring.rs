//! Bind editor requests and its bounded validation worker to existing chart slots.
use super::{arrangement_host::ArrangementHost, indicator_manager::IndicatorState};
use crate::{
    indicator_worker::{IndicatorCommand, IndicatorSource},
    surfaces::Surfaces,
};
use quantick_stores::script_library::{ScriptDocument, ScriptEditorAction, ScriptLibraryPort};
use std::time::Instant;

pub(super) fn inspect(
    surfaces: &Surfaces,
    indicators: &IndicatorState,
    offset: usize,
    limit: usize,
) -> quantick_control_schema::script_editor::EditorResult {
    quantick_control_schema::script_editor::capture(
        &surfaces.script_editor.state,
        indicators.script_library.entries(),
        offset,
        limit,
    )
}

pub(super) fn apply(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    _tabs: &mut ArrangementHost,
    action: ScriptEditorAction,
) -> Result<Option<ScriptDocument>, String> {
    let saved = matches!(action, ScriptEditorAction::Save);
    let document = surfaces
        .script_editor
        .state
        .dispatch(&mut indicators.script_library, action)?;
    if saved && let Some(document) = &document {
        observe_saved(indicators, document);
    }
    Ok(document)
}

pub(super) fn frame(
    surfaces: &mut Surfaces,
    indicators: &mut IndicatorState,
    tabs: &mut ArrangementHost,
    actions: Vec<ScriptEditorAction>,
    now: Instant,
) {
    for action in actions {
        if let Err(error) = apply(surfaces, indicators, tabs, action) {
            surfaces.toast.note(error, now);
        }
    }
    let editor = &mut surfaces.script_editor;
    if let Some(receiver) = &editor.compilation {
        match receiver.try_recv() {
            Ok((revision, document, error)) => {
                editor.compilation = None;
                let valid = error.is_none();
                if editor.state.complete(revision, error) && valid {
                    reload(indicators, tabs, &document);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                editor.compilation = None;
                editor.state.complete(
                    editor.state.revision,
                    Some("source validation worker stopped".into()),
                );
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => (),
        }
    }
    if editor.compilation.is_some() {
        return;
    }
    let elapsed =
        u64::try_from(now.saturating_duration_since(editor.epoch).as_millis()).unwrap_or(u64::MAX);
    let Some((revision, document)) = editor.state.due(elapsed) else {
        return;
    };
    if let Err(error) = indicators.script_library.save(&document) {
        editor.state.complete(revision, Some(error));
        editor.state.dirty = true;
        return;
    }
    observe_saved(indicators, &document);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    match std::thread::Builder::new()
        .name("quantick-script-validator".into())
        .spawn(move || {
            let error = quantick_pine::compile(&document.source, &document.title)
                .err()
                .map(|errors| {
                    errors
                        .iter()
                        .take(8)
                        .map(|error| error.render(&document.title, &document.source))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
            let _ = sender.send((revision, document, error));
        }) {
        Ok(_) => editor.compilation = Some(receiver),
        Err(error) => {
            editor
                .state
                .complete(revision, Some(format!("cannot validate source: {error}")));
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

fn reload(indicators: &IndicatorState, tabs: &mut ArrangementHost, document: &ScriptDocument) {
    for (owner, kind) in indicators.registry.slot_kinds() {
        if !matches!(kind, quantick_workspace::indicator_document::SavedKind::Script { name } if name == &document.id)
        {
            continue;
        }
        if owner.workspace == tabs.owner()
            && let Some(tab) = tabs.by_id_mut(owner.tab)
            && let Some(pane) = tab.pane_at_mut(owner.side.index())
            && pane.id == owner.pane
        {
            pane.indicator_worker.send(IndicatorCommand::Reload {
                slot: owner.slot,
                source: IndicatorSource::Script {
                    name: document.id.clone(),
                    text: document.source.clone(),
                },
            });
        }
    }
}
