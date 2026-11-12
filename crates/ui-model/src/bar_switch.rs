//! Quick bar-switch presentation; bar parsing and candidate policy belong to engine.
use quantick_chart_interaction::pane::PaneSide;
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration, quick_query_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarSwitchRequest {
    pub tab: u64,
    pub side: PaneSide,
    pub config: BarConfiguration,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Open {
    pub query: String,
    pub candidates: Vec<BarConfiguration>,
    pub selected: usize,
    pub tab: u64,
    pub side: PaneSide,
    pub follows_focus: bool,
    pub focus_pending: bool,
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct State {
    open: Option<Open>,
}
impl State {
    pub fn open(&self) -> Option<&Open> {
        self.open.as_ref()
    }
}
#[derive(Debug)]
pub enum Intent {
    Open {
        tab: u64,
        side: PaneSide,
        query: String,
        follows_focus: bool,
    },
    Observe {
        tab: u64,
        side: PaneSide,
        text: String,
        widget_focused: bool,
        modified: bool,
    },
    Edit {
        query: String,
        down: bool,
        up: bool,
    },
    FocusRequested,
    Finish {
        clicked: Option<usize>,
        enter: bool,
        escape: bool,
        abandoned: bool,
    },
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Option<BarSwitchRequest>;
    fn update(state: &mut State, intent: Intent) -> Self::Effects {
        match intent {
            Intent::Open {
                tab,
                side,
                query,
                follows_focus,
            } => {
                let query = quick_query_text(&query);
                state.open = Some(Open {
                    candidates: BUILTIN_BARS.quick_matches(&query),
                    query,
                    selected: 0,
                    tab,
                    side,
                    follows_focus,
                    focus_pending: true,
                });
            }
            Intent::Observe {
                tab,
                side,
                text,
                widget_focused,
                modified,
            } => {
                if let Some(open) = state.open.as_mut() {
                    if open.follows_focus {
                        open.tab = tab;
                        open.side = side;
                    } else if open.tab != tab || open.side != side {
                        state.open = None;
                    }
                } else if !widget_focused
                    && !modified
                    && !text.is_empty()
                    && text.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Self::update(
                        state,
                        Intent::Open {
                            tab,
                            side,
                            query: text,
                            follows_focus: false,
                        },
                    );
                }
            }
            Intent::Edit { query, down, up } => {
                if let Some(open) = state.open.as_mut() {
                    let query = quick_query_text(&query);
                    if query != open.query {
                        open.candidates = BUILTIN_BARS.quick_matches(&query);
                        open.query = query;
                        open.selected = 0;
                    }
                    let last = open.candidates.len().saturating_sub(1);
                    if down {
                        open.selected = open.selected.saturating_add(1).min(last);
                    }
                    if up {
                        open.selected = open.selected.saturating_sub(1);
                    }
                    open.selected = open.selected.min(last);
                }
            }
            Intent::FocusRequested => {
                if let Some(open) = state.open.as_mut() {
                    open.focus_pending = false;
                }
            }
            Intent::Finish {
                clicked,
                enter,
                escape,
                abandoned,
            } => {
                if let Some(open) = state.open.as_ref() {
                    let chosen = clicked.or(enter.then_some(open.selected));
                    let request =
                        chosen
                            .and_then(|index| open.candidates.get(index))
                            .map(|config| BarSwitchRequest {
                                tab: open.tab,
                                side: open.side,
                                config: *config,
                            });
                    if escape || request.is_some() || open.query.is_empty() || abandoned {
                        state.open = None;
                    }
                    return request;
                }
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "bar_switch/tests.rs"]
mod tests;
