//! Deterministic acknowledgement arbitration; time is supplied in nanoseconds.
use std::borrow::Cow;
const WINDOW: i128 = 8_000_000_000;
#[derive(Debug, PartialEq)]
struct Pending {
    message: Cow<'static, str>,
    shown_at: i128,
    offers_undo: bool,
}
#[derive(Default, Debug, PartialEq)]
pub struct State {
    pending: Option<Pending>,
    deferred: Option<Cow<'static, str>>,
}
impl State {
    pub fn message(&self) -> Option<&str> {
        self.pending.as_ref().map(|p| p.message.as_ref())
    }
    pub fn offers_undo(&self) -> bool {
        self.pending.as_ref().is_some_and(|p| p.offers_undo)
    }
}
#[derive(Debug, PartialEq)]
pub enum Intent {
    Note {
        message: Cow<'static, str>,
        now: i128,
    },
    NoteWithUndo {
        message: Cow<'static, str>,
        now: i128,
    },
    Tick(i128),
    Undo,
}
#[derive(Default, Debug, PartialEq)]
pub struct Effects {
    pub undo_drawing: bool,
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Note { message, now } => {
                if state.pending.as_ref().is_some_and(|p| {
                    p.offers_undo && now.saturating_sub(p.shown_at).max(0) < WINDOW
                }) {
                    state.deferred = Some(message);
                } else {
                    state.pending = Some(Pending {
                        message,
                        shown_at: now,
                        offers_undo: false,
                    });
                }
            }
            Intent::NoteWithUndo { message, now } => {
                state.deferred = None;
                state.pending = Some(Pending {
                    message,
                    shown_at: now,
                    offers_undo: true,
                });
            }
            Intent::Tick(now) => {
                if state
                    .pending
                    .as_ref()
                    .is_some_and(|p| now.saturating_sub(p.shown_at).max(0) >= WINDOW)
                {
                    state.pending = None;
                }
                if state.pending.is_none() {
                    if let Some(message) = state.deferred.take() {
                        state.pending = Some(Pending {
                            message,
                            shown_at: now,
                            offers_undo: false,
                        });
                    }
                }
            }
            Intent::Undo => {
                state.pending = None;
                return Effects { undo_drawing: true };
            }
        }
        Effects::default()
    }
}
#[cfg(test)]
#[path = "toast/tests.rs"]
mod tests;
