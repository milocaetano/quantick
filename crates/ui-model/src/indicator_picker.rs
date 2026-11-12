//! Indicator library interaction policy over the authoritative stores picker.
use quantick_control_schema::indicator_picker::{PickerInput, PickerResult};
use quantick_stores::{
    indicator_picker::{Category, Choice, Entry, Picker},
    script_library::ScriptEntry,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveEntry {
    pub slot: u64,
    pub label: String,
    pub hidden: bool,
    pub errored: bool,
    pub stale: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Errored,
    Stale,
    Hidden,
    Running,
}
impl ActiveEntry {
    pub fn status(&self) -> Status {
        if self.errored {
            Status::Errored
        } else if self.stale {
            Status::Stale
        } else if self.hidden {
            Status::Hidden
        } else {
            Status::Running
        }
    }
}
#[derive(Default)]
pub struct State {
    pub picker: Picker,
    pub entries: Vec<Entry>,
    pub active: Vec<ActiveEntry>,
    pub focus_requested: bool,
    pub control_result: Option<Result<PickerResult, String>>,
    scripts: Vec<ScriptEntry>,
    indicators: Vec<ActiveEntry>,
    projected: Option<Picker>,
}
impl State {
    pub fn empty_message(&self) -> Option<&'static str> {
        if self.picker.category == Category::Active {
            self.active
                .is_empty()
                .then_some("No indicators on this chart match your search.")
        } else {
            self.entries
                .is_empty()
                .then_some("No indicators match your search.")
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveAction {
    ToggleHidden,
    Settings,
    Remove,
}
impl ActiveAction {
    pub fn hint(self) -> &'static str {
        match self {
            Self::ToggleHidden => "hide/show without removing (no recompute)",
            Self::Settings => "settings (applying recomputes from scratch)",
            Self::Remove => "remove this indicator",
        }
    }
}
pub enum Intent {
    Refresh {
        target: (u64, u64),
        scripts: Vec<ScriptEntry>,
        active: Vec<ActiveEntry>,
    },
    Open((u64, u64)),
    Close,
    Query(String),
    Category(Category),
    FocusHandled,
    SelectEntry(Choice),
    ViewSource(usize),
    ActiveAction(u64, ActiveAction),
    Control {
        target: (u64, u64),
        scripts: Vec<ScriptEntry>,
        input: PickerInput,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    Add(Choice),
    ViewSource(String),
    Active(u64, ActiveAction),
}
pub struct Model;
impl Model {
    pub fn control(
        state: &mut State,
        target: (u64, u64),
        scripts: &[ScriptEntry],
        input: PickerInput,
    ) -> Result<PickerResult, String> {
        <Self as crate::Model>::update(
            state,
            Intent::Control {
                target,
                scripts: scripts.to_vec(),
                input,
            },
        );
        state
            .control_result
            .take()
            .expect("control intent returns its result")
    }
}
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Vec<Effect>;
    fn update(state: &mut State, intent: Intent) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut changed = false;
        match intent {
            Intent::Refresh {
                target,
                scripts,
                active,
            } => {
                state.picker.retain_target(target);
                if state.scripts != scripts {
                    state.scripts = scripts;
                    changed = true;
                }
                if state.indicators != active {
                    state.indicators = active;
                    changed = true;
                }
            }
            Intent::Open(target) => state.picker.open(target),
            Intent::Close => state.picker.close(),
            Intent::Query(query) => state.picker.query = query,
            Intent::Category(category) => state.picker.category = category,
            Intent::FocusHandled => state.focus_requested = false,
            Intent::Control {
                target,
                scripts,
                input,
            } => {
                if state.scripts != scripts {
                    state.scripts = scripts;
                    changed = true;
                }
                state.control_result = Some(quantick_control_schema::indicator_picker::apply(
                    &mut state.picker,
                    &state.scripts,
                    target,
                    input,
                ));
            }
            Intent::SelectEntry(choice) => {
                if state.picker.target.is_some()
                    && state.entries.iter().any(|entry| entry.choice == choice)
                {
                    effects.push(Effect::Add(choice));
                    state.picker.close();
                }
            }
            Intent::ViewSource(index) => {
                if state.picker.target.is_some()
                    && state
                        .entries
                        .iter()
                        .any(|entry| entry.choice == Choice::Script(index))
                {
                    effects.push(Effect::ViewSource(state.scripts[index].name.clone()));
                    state.picker.close();
                }
            }
            Intent::ActiveAction(slot, action) => {
                if state.picker.target.is_some()
                    && state.active.iter().any(|entry| entry.slot == slot)
                {
                    effects.push(Effect::Active(slot, action));
                    if action != ActiveAction::ToggleHidden {
                        state.picker.close();
                    }
                }
            }
        }
        state.focus_requested |= state.picker.take_focus();
        if state.picker.target.is_none() {
            state.focus_requested = false;
        }
        if changed || state.projected.as_ref() != Some(&state.picker) {
            state.entries = state.picker.entries(&state.scripts);
            state.active = state
                .indicators
                .iter()
                .filter(|entry| state.picker.matches(&entry.label))
                .cloned()
                .collect();
            state.projected = Some(state.picker.clone());
        }
        effects
    }
}

#[cfg(test)]
#[path = "indicator_picker/tests.rs"]
mod tests;
