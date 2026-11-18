//! Apply the selected editor operation to its addressed preset store.
use super::{DEFAULT_PRESET, InputSpec, Intent, LocalOperation};
use quantick_stores::indicator_state::{SavedInput, SavedInputExt};

use super::{Model, TabSlot};
use crate::UpdateSession;
use quantick_stores::indicator_presets::PresetStore;
use quantick_workspace::indicator_document::SavedKind;
use std::path::Path;

/// The current editor address and the preset document it can change.
pub struct PresetEditor<'a> {
    pub target: Option<TabSlot>,
    pub kind: Option<&'a SavedKind>,
    pub specs: &'a [InputSpec],
    pub presets: &'a mut PresetStore,
    pub path: &'a Path,
}
impl PresetEditor<'_> {
    pub fn dispatch(
        &mut self,
        session: &mut UpdateSession<'_, Model>,
        intent: Intent,
        mut rebound: impl FnMut(&str, usize, usize, usize),
    ) {
        session.with_local_effects(
            &mut |mut effects| {
                let receipt = effects
                    .local_operation
                    .take()
                    .and_then(|operation| self.execute(operation, &mut rebound));
                (effects, receipt)
            },
            |session| session.update(intent),
        );
    }

    fn execute(
        &mut self,
        operation: LocalOperation,
        rebound: impl FnOnce(&str, usize, usize, usize),
    ) -> Option<Intent> {
        let target = match &operation {
            LocalOperation::Load { target, .. }
            | LocalOperation::Save { target, .. }
            | LocalOperation::Delete { target, .. } => *target,
        };
        if self.target != Some(target) {
            return None;
        }
        match operation {
            LocalOperation::Load { name, .. } => {
                // Keep unreadable cells in place so later values cannot drift left.
                let saved: Vec<_> = match &name {
                    None => Vec::new(),
                    Some(name) => self
                        .presets
                        .get(self.kind?, name)?
                        .iter()
                        .map(SavedInput::to_value)
                        .collect(),
                };
                let bound = quantick_indicators::bind_by_position(self.specs, &saved);
                if bound.kept < saved.len() {
                    rebound(
                        name.as_deref().unwrap_or(DEFAULT_PRESET),
                        saved.len(),
                        self.specs.len(),
                        bound.kept,
                    );
                }
                Some(Intent::PresetLoaded {
                    target,
                    values: bound.values,
                    label: name.unwrap_or_else(|| DEFAULT_PRESET.to_owned()),
                })
            }
            LocalOperation::Save { name, values, .. } => {
                let kind = self.kind?;
                let inputs = values.iter().map(SavedInput::from_value).collect();
                if !self.presets.insert(kind, &name, inputs) {
                    return None;
                }
                self.presets.save(self.path);
                Some(Intent::PresetSaved { target, name })
            }
            LocalOperation::Delete { name, .. } => {
                if !self.presets.remove(self.kind?, &name) {
                    return None;
                }
                self.presets.save(self.path);
                Some(Intent::PresetDeleted { target, name })
            }
        }
    }
}

#[cfg(test)]
#[path = "preset/tests.rs"]
mod tests;
