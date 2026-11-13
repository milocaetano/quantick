//! Confirmation lifecycle and cached prompts; workspace owns removal policy.
use quantick_control_handlers::canvas::{self, CanvasAct};
use quantick_workspace::canvas::{
    CanvasState, PaneMenuItem, RemoveAnswer, RemovePrompt, remove_prompt,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub live: CanvasState,
    pub saved: Option<CanvasState>,
    pub name: String,
    pub autosave: bool,
}

#[derive(Default, Debug)]
pub struct State {
    pending: Option<u32>,
    snapshot: Option<Snapshot>,
    prompt: Option<RemovePrompt>,
}
impl State {
    pub fn pending(&self) -> Option<u32> {
        self.pending
    }
    pub fn prompt(&self) -> Option<&RemovePrompt> {
        self.prompt.as_ref()
    }
    pub fn open(&self) -> Option<(u32, &RemovePrompt)> {
        self.pending.zip(self.prompt.as_ref())
    }
    /// Capture changed domain facts only when an input or projection needs updating.
    pub fn observe(
        &self,
        picked: Option<(u32, PaneMenuItem)>,
        escape: bool,
        live: &CanvasState,
        saved: Option<&CanvasState>,
        workspace: (&str, bool),
    ) -> Option<Intent> {
        let (name, autosave) = workspace;
        let unchanged = self.snapshot.as_ref().is_some_and(|snapshot| {
            &snapshot.live == live
                && snapshot.saved.as_ref() == saved
                && snapshot.name == name
                && snapshot.autosave == autosave
        });
        (picked.is_some() || escape || (self.pending.is_some() && !unchanged)).then(|| {
            Intent::MenuStep {
                picked,
                escape,
                snapshot: Box::new(Snapshot {
                    live: live.clone(),
                    saved: saved.cloned(),
                    name: name.to_owned(),
                    autosave,
                }),
            }
        })
    }
}

#[derive(Debug)]
pub enum Intent {
    MenuStep {
        picked: Option<(u32, PaneMenuItem)>,
        escape: bool,
        snapshot: Box<Snapshot>,
    },
    Answer {
        clicked: Option<RemoveAnswer>,
        escaped: bool,
    },
}

#[derive(Default, Debug, PartialEq)]
pub enum Effects {
    #[default]
    None,
    /// An answered Cancel carries None and overrides any earlier menu action.
    Act(Option<CanvasAct>),
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::MenuStep {
                picked,
                escape,
                snapshot,
            } => {
                let before = state.pending;
                let act = canvas::menu_step(
                    picked,
                    escape && before.is_none(),
                    &mut state.pending,
                    &snapshot.live,
                    snapshot.saved.as_ref(),
                );
                if before != state.pending || state.snapshot.as_ref() != Some(snapshot.as_ref()) {
                    state.prompt =
                        state
                            .pending
                            .zip(snapshot.saved.as_ref())
                            .map(|(slot, saved)| {
                                remove_prompt(
                                    &snapshot.name,
                                    saved,
                                    &snapshot.live,
                                    slot,
                                    snapshot.autosave,
                                )
                            });
                    state.snapshot = Some(*snapshot);
                }
                Effects::Act(act)
            }
            Intent::Answer { clicked, escaped } => {
                let Some(answer) = clicked.or(escaped.then_some(RemoveAnswer::Cancel)) else {
                    return Effects::None;
                };
                let Some(slot) = state.pending.take() else {
                    return Effects::None;
                };
                state.prompt = None;
                Effects::Act(canvas::confirmed(answer, slot))
            }
        }
    }
}

#[cfg(test)]
#[path = "remove_chart/tests.rs"]
mod tests;
