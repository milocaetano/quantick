//! One addressed indicator editor. The host retains descriptor, preset and worker authority.
use quantick_chart::indicator_registry::{TabSlot, remap_side};
use quantick_indicators::{InputSpec, InputValue};

#[path = "indicator_settings/preset.rs"]
mod preset;
pub use preset::PresetEditor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Inputs,
    Style,
}

impl SettingsTab {
    /// `QUANTICK_INDICATOR_SETTINGS`: `<index>` or `<index>:<tab>`, the tab
    /// `inputs`/`0` or `style`/`1`. Nonsense is no hook rather than a guessed
    /// one: a capture of the wrong tab would claim to show what it does not.
    #[must_use]
    pub fn hook(value: &str) -> Option<(usize, Self)> {
        let (index, tab) = value.split_once(':').unwrap_or((value, "inputs"));
        let index = index.trim().parse::<usize>().ok()?;
        let tab = match tab.trim().to_ascii_lowercase().as_str() {
            "inputs" | "0" => Self::Inputs,
            "style" | "1" => Self::Style,
            _ => return None,
        };
        Some((index, tab))
    }
}

#[derive(Debug)]
pub struct SettingsDialog {
    pub target: TabSlot,
    pub title: String,
    pub tab: SettingsTab,
    pub draft: Vec<InputValue>,
    pub committed: Vec<InputValue>,
    pub previewed: bool,
    pub settled: bool,
    pub preset_label: Option<String>,
    pub preset_name_draft: String,
}

#[derive(Debug, Default)]
pub struct State {
    dialog: Option<SettingsDialog>,
}

impl State {
    pub fn dialog(&self) -> Option<&SettingsDialog> {
        self.dialog.as_ref()
    }

    pub fn target(&self) -> Option<TabSlot> {
        self.dialog().map(|dialog| dialog.target)
    }

    pub fn previewed(&self) -> bool {
        self.dialog().is_some_and(|dialog| dialog.previewed)
    }
}

pub const DEFAULT_PRESET: &str = "Default";
pub const DISPLAY_SECTION: &str = "Display";

#[derive(Debug, Default, PartialEq, Eq)]
pub enum SettingsOutcome {
    #[default]
    Open,
    Close,
    Apply,
    Preview,
    LoadPreset(Option<String>),
    SavePreset(String),
    DeletePreset(String),
    StyleChanged,
}

#[derive(Debug)]
pub enum Intent {
    /// The host has resolved this address to a real view and its values.
    Open {
        target: TabSlot,
        title: String,
        values: Vec<InputValue>,
    },
    ObserveTarget {
        valid: bool,
        label: String,
        has_style: bool,
    },
    SelectTab(SettingsTab),
    EditInput {
        index: usize,
        value: InputValue,
    },
    EditPresetName(String),
    /// Native facts, after preset-row and button precedence has been observed.
    FinishPaint {
        changed: bool,
        style_changed: bool,
        outcome: SettingsOutcome,
        window_closed: bool,
        escape: bool,
    },
    Apply,
    Preview,
    Close,
    PresetLoaded {
        target: TabSlot,
        values: Vec<InputValue>,
        label: String,
    },
    PresetSaved {
        target: TabSlot,
        name: String,
    },
    PresetDeleted {
        target: TabSlot,
        name: String,
    },
    ContextPaneMoved {
        tab: u64,
        from: usize,
        to: usize,
    },
}

#[derive(Debug, PartialEq)]
pub enum Change {
    Inputs {
        target: TabSlot,
        values: Vec<InputValue>,
        commit: bool,
    },
    Style(TabSlot),
}

#[derive(Debug, PartialEq)]
pub enum LocalOperation {
    Load {
        target: TabSlot,
        name: Option<String>,
    },
    Save {
        target: TabSlot,
        name: String,
        values: Vec<InputValue>,
    },
    Delete {
        target: TabSlot,
        name: String,
    },
}

#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub local_operation: Option<LocalOperation>,
    pub changes: Vec<Change>,
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;

    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Open {
                target,
                title,
                values,
            } => {
                state.dialog = Some(SettingsDialog {
                    target,
                    title,
                    tab: SettingsTab::Inputs,
                    committed: values.clone(),
                    draft: values,
                    previewed: false,
                    settled: false,
                    preset_label: None,
                    preset_name_draft: String::new(),
                });
            }
            Intent::ObserveTarget {
                valid,
                label,
                has_style,
            } => {
                if !valid {
                    state.dialog = None;
                } else if let Some(dialog) = &mut state.dialog {
                    dialog.title = label;
                    if !has_style {
                        dialog.tab = SettingsTab::Inputs;
                    }
                }
            }
            Intent::SelectTab(tab) => {
                if let Some(dialog) = &mut state.dialog {
                    dialog.tab = tab;
                }
            }
            Intent::EditInput { index, value } => {
                if let Some(dialog) = &mut state.dialog
                    && let Some(cell) = dialog.draft.get_mut(index)
                {
                    *cell = value;
                }
            }
            Intent::EditPresetName(name) => {
                if let Some(dialog) = &mut state.dialog {
                    dialog.preset_name_draft = name;
                }
            }
            Intent::FinishPaint {
                mut changed,
                style_changed,
                mut outcome,
                window_closed,
                escape,
            } => {
                let Some(dialog) = &mut state.dialog else {
                    return Effects::default();
                };
                if !dialog.settled {
                    dialog.settled = true;
                    if changed {
                        dialog.committed = dialog.draft.clone();
                        changed = false;
                    }
                }
                if changed && outcome == SettingsOutcome::Open {
                    dialog.preset_label = None;
                    outcome = SettingsOutcome::Preview;
                }
                if style_changed && outcome == SettingsOutcome::Open {
                    outcome = SettingsOutcome::StyleChanged;
                }
                if window_closed || escape {
                    outcome = SettingsOutcome::Close;
                }
                return finish(state, outcome);
            }
            Intent::Apply => return finish(state, SettingsOutcome::Apply),
            Intent::Preview => return finish(state, SettingsOutcome::Preview),
            Intent::Close => return finish(state, SettingsOutcome::Close),
            Intent::PresetLoaded {
                target,
                values,
                label,
            } => {
                if let Some(dialog) = &mut state.dialog
                    && dialog.target == target
                {
                    dialog.draft = values;
                    dialog.preset_label = Some(label);
                    return finish(state, SettingsOutcome::Preview);
                }
            }
            Intent::PresetSaved { target, name } => {
                if let Some(dialog) = &mut state.dialog
                    && dialog.target == target
                {
                    dialog.preset_label = Some(name.trim().to_owned());
                    dialog.preset_name_draft.clear();
                }
            }
            Intent::PresetDeleted { target, name } => {
                if let Some(dialog) = &mut state.dialog
                    && dialog.target == target
                    && dialog.preset_label.as_deref() == Some(&name)
                {
                    dialog.preset_label = None;
                }
            }
            Intent::ContextPaneMoved { tab, from, to } => {
                if let Some(dialog) = &mut state.dialog
                    && dialog.target.tab == tab
                {
                    dialog.target.side = remap_side(dialog.target.side, from, to);
                }
            }
        }
        Effects::default()
    }
}

fn finish(state: &mut State, outcome: SettingsOutcome) -> Effects {
    let Some(dialog) = &mut state.dialog else {
        return Effects::default();
    };
    let target = dialog.target;
    let mut effects = Effects::default();
    match outcome {
        SettingsOutcome::Open => {}
        SettingsOutcome::Close => {
            if dialog.previewed {
                effects.changes.push(Change::Inputs {
                    target,
                    values: dialog.committed.clone(),
                    commit: false,
                });
            }
            state.dialog = None;
        }
        SettingsOutcome::Apply | SettingsOutcome::Preview => {
            let commit = outcome == SettingsOutcome::Apply;
            if commit {
                dialog.committed = dialog.draft.clone();
            }
            dialog.previewed = !commit;
            effects.changes.push(Change::Inputs {
                target,
                values: dialog.draft.clone(),
                commit,
            });
        }
        SettingsOutcome::StyleChanged => effects.changes.push(Change::Style(target)),
        SettingsOutcome::LoadPreset(name) => {
            effects.local_operation = Some(LocalOperation::Load { target, name });
        }
        SettingsOutcome::SavePreset(name) => {
            effects.local_operation = Some(LocalOperation::Save {
                target,
                name,
                values: dialog.draft.clone(),
            });
        }
        SettingsOutcome::DeletePreset(name) => {
            effects.local_operation = Some(LocalOperation::Delete { target, name });
        }
    }
    effects
}

pub fn split_section(title: &str) -> (&str, &str) {
    title
        .split_once(": ")
        .map_or(("", title), |(section, label)| (section, label))
}

/// Presentation ordering never changes positional binding of values.
pub fn row_order(specs: &[InputSpec]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..specs.len()).collect();
    order.sort_by_key(|&index| split_section(specs[index].title()).0 != DISPLAY_SECTION);
    order
}

#[cfg(test)]
#[path = "indicator_settings/tests.rs"]
mod tests;

#[cfg(test)]
mod hook_tests {
    use super::*;

    /// The harness hook is the only way a scripted capture can see the
    /// settings dialog, so it has to name both halves — and refuse nonsense
    /// rather than guess, which would produce a screenshot of the wrong tab.
    #[test]
    fn the_settings_hook_names_an_indicator_and_a_tab() {
        assert_eq!(
            SettingsTab::hook("0"),
            Some((0, SettingsTab::Inputs)),
            "a bare index opens on Inputs"
        );
        assert_eq!(SettingsTab::hook("2:style"), Some((2, SettingsTab::Style)));
        assert_eq!(
            SettingsTab::hook("1:1"),
            Some((1, SettingsTab::Style)),
            "the numeric spelling works too"
        );
        assert_eq!(SettingsTab::hook("0:colours"), None);
        assert_eq!(SettingsTab::hook("first"), None);
        assert_eq!(SettingsTab::hook(""), None);
    }
}
