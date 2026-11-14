//! Global layout editors; the host validates targets and applies domain edits.
use quantick_chart_interaction::pane::PaneSide;
use quantick_workspace::layout_document::LayoutId;

/// What the strip asked for this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripAction {
    Switch(LayoutId),
    Create,
    BeginRename(LayoutId),
    CommitRename(LayoutId, String),
    CancelRename,
    Delete(LayoutId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub tab: u64,
    pub pane: PaneSide,
    pub layout: LayoutId,
    pub draft: String,
}

#[derive(Debug, Default)]
pub struct State {
    rename: Option<Rename>,
    pending_delete: Option<LayoutId>,
}

impl State {
    pub fn rename(&self) -> Option<&Rename> {
        self.rename.as_ref()
    }

    pub fn pending_delete(&self) -> Option<LayoutId> {
        self.pending_delete
    }
}

#[derive(Debug)]
pub enum Intent {
    /// The host has verified both the layout and its addressed pane exist.
    BeginRename {
        tab: u64,
        pane: PaneSide,
        layout: LayoutId,
        name: String,
    },
    Edit(String),
    Finish {
        escaped: bool,
        enter: bool,
        lost_focus: bool,
    },
    /// An explicit external commit does not require an active inline editor.
    Commit {
        layout: LayoutId,
        name: String,
    },
    CancelRename,
    RequestDelete {
        layout: LayoutId,
        exists: bool,
    },
    AnswerDelete(bool),
    Reload,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum Effects {
    #[default]
    None,
    Rename {
        layout: LayoutId,
        name: String,
        target: Option<(u64, PaneSide)>,
    },
    Focus {
        tab: u64,
        pane: PaneSide,
    },
    Delete(LayoutId),
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;

    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::BeginRename {
                tab,
                pane,
                layout,
                name,
            } => {
                state.rename = Some(Rename {
                    tab,
                    pane,
                    layout,
                    draft: name,
                });
            }
            Intent::Edit(text) => {
                if let Some(rename) = &mut state.rename {
                    rename.draft = text;
                }
            }
            Intent::Finish {
                escaped,
                enter,
                lost_focus,
            } => {
                if escaped {
                    if let Some(rename) = state.rename.take() {
                        return Effects::Focus {
                            tab: rename.tab,
                            pane: rename.pane,
                        };
                    }
                } else if (enter || lost_focus)
                    && let Some(rename) = state.rename.take()
                {
                    return Effects::Rename {
                        layout: rename.layout,
                        name: rename.draft,
                        target: Some((rename.tab, rename.pane)),
                    };
                }
            }
            Intent::Commit { layout, name } => {
                state.rename = None;
                return Effects::Rename {
                    layout,
                    name,
                    target: None,
                };
            }
            Intent::CancelRename => state.rename = None,
            Intent::RequestDelete { layout, exists } => {
                state.pending_delete = exists.then_some(layout);
            }
            Intent::AnswerDelete(confirm) => {
                if let Some(layout) = state.pending_delete.take()
                    && confirm
                {
                    return Effects::Delete(layout);
                }
            }
            Intent::Reload => {
                state.rename = None;
                state.pending_delete = None;
            }
        }
        Effects::None
    }
}

#[cfg(test)]
#[path = "layout_dialogs/tests.rs"]
mod tests;
