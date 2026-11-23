//! Admission, revision identity and bounded progress for one chart conversation.
pub use crate::strategy_describe_contract::*;
use std::sync::Arc;

#[derive(Default)]
pub struct State {
    pub status: Status,
    context: Option<Arc<Context>>,
    validating: Option<(String, String)>,
    saving: Option<String>,
    saved_attempt: Option<String>,
    repairs: u8,
}

impl Context {
    /// Live closes and changed observations on this chart do not replace a conversation.
    pub fn matches_chart(
        &self,
        workspace: quantick_workspace::lifetime::WorkspaceId,
        tab: u64,
        pane: u64,
        market: (&str, &str),
        bars: &str,
    ) -> bool {
        (
            self.workspace,
            self.tab,
            self.pane,
            self.feed.as_str(),
            self.symbol.as_str(),
            self.bars.as_str(),
        ) == (workspace, tab, pane, market.0, market.1, bars)
    }
}
impl State {
    pub fn context(&self) -> Option<&Context> {
        self.context.as_deref()
    }
    pub fn admit(&self, command: &Command) -> Result<(), &'static str> {
        let (revision, allowed) = match command {
            Command::Open | Command::Read => return Ok(()),
            Command::Submit { revision, .. } => (
                revision,
                matches!(self.status.phase, Phase::Ready | Phase::Question),
            ),
            Command::Cancel { revision } => (revision, self.context.is_some()),
            Command::Save { revision, id } => (
                revision,
                self.status.phase == Phase::Preview
                    || self.status.saved_id.as_ref() == Some(id)
                    || self.saved_attempt.as_ref() == Some(id),
            ),
        };
        if *revision != self.status.revision {
            return Err("This conversation revision has expired");
        }
        if !allowed {
            return Err("This action is unavailable at the current conversation stage");
        }
        Ok(())
    }

    fn stop(&mut self, phase: Phase, message: String) -> Effects {
        self.status.phase = phase;
        self.status.message = message;
        self.status.draft = None;
        self.validating = None;
        self.saving = None;
        self.context = None;
        Effects::None
    }
    fn append(&mut self, turn: Turn) -> bool {
        fn size(turn: &Turn) -> usize {
            match turn {
                Turn::Assistant(s) | Turn::Trader(s) => s.len(),
            }
        }
        let size_new = size(&turn);
        if size_new == 0
            || size_new > MAX_TEXT_BYTES
            || self.status.transcript.len() >= MAX_TURNS
            || self.status.transcript.iter().map(size).sum::<usize>() + size_new
                > MAX_TRANSCRIPT_BYTES
        {
            return false;
        }
        self.status.transcript.push(turn);
        true
    }
    fn request(&mut self, repair: Option<(String, Vec<Diagnostic>)>) -> Effects {
        let Some(context) = self.context.clone() else {
            return Effects::None;
        };
        self.status.revision += 1;
        self.status.phase = Phase::Waiting;
        self.status.message = if repair.is_some() {
            format!("Correcting the script ({} of {MAX_REPAIRS})", self.repairs)
        } else {
            "Waiting for the provider".into()
        };
        Effects::Request(Request {
            revision: self.status.revision,
            context,
            transcript: self.status.transcript.clone(),
            repair,
        })
    }
    fn command(&mut self, command: Command) -> Effects {
        let phase = self.status.phase;
        let current = self.status.revision;
        match command {
            Command::Open if phase != Phase::Unavailable => Effects::Dispatch(Command::Open),
            Command::Submit { revision, text }
                if current == revision && matches!(phase, Phase::Question | Phase::Ready) =>
            {
                if text.trim().is_empty() || !self.append(Turn::Trader(text)) {
                    self.status.message =
                        "Message is blank or exceeds the conversation limit".into();
                    return Effects::None;
                }
                self.request(None)
            }
            Command::Cancel { revision } if current == revision && self.context.is_some() => {
                self.status.revision += 1;
                self.stop(Phase::Cancelled, "Conversation cancelled".into())
            }
            Command::Save { revision, id }
                if current == revision && phase == Phase::Preview && self.saving.is_none() =>
            {
                self.saving = Some(id.clone());
                Effects::Save { revision, id }
            }
            _ => Effects::None,
        }
    }
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        update(state, intent)
    }
}

pub fn update(state: &mut State, intent: Intent) -> Effects {
    let current = state.status.revision;
    match intent {
        Intent::Command(command) => state.command(command),
        Intent::Opened(context) => {
            *state = State {
                status: Status {
                    revision: current + 1,
                    phase: Phase::Ready,
                    message: "Send the description and this chart's captured context to your configured provider.".into(),
                    ..Status::default()
                },
                context: Some(context),
                ..State::default()
            };
            Effects::None
        }
        Intent::TargetChanged if state.context.is_some() => {
            state.status.revision += 1;
            state.stop(
                Phase::Cancelled,
                "Chart changed. Open a new conversation on this chart.".into(),
            )
        }
        Intent::Replied { revision, reply }
            if revision == current
                && state.status.phase == Phase::Waiting
                && state.validating.is_none() =>
        {
            match reply {
                Ok(Reply::Question(question)) if !question.trim().is_empty() => {
                    if !state.append(Turn::Assistant(question)) {
                        return state.stop(Phase::Failed, "Conversation limit reached".into());
                    }
                    state.status.phase = Phase::Question;
                    state.status.message = "Answer the clarification to continue".into();
                    Effects::None
                }
                Ok(Reply::Candidate { source, summary })
                    if !source.trim().is_empty()
                        && source.len() <= MAX_SOURCE_BYTES
                        && !summary.trim().is_empty()
                        && summary.len() <= MAX_TEXT_BYTES =>
                {
                    let Some(context) = state.context.clone() else {
                        return Effects::None;
                    };
                    state.validating = Some((source.clone(), summary.clone()));
                    Effects::Preview {
                        revision,
                        context,
                        source,
                        summary,
                    }
                }
                Err(message) => state.stop(Phase::Failed, message.chars().take(1024).collect()),
                _ => state.stop(
                    Phase::Failed,
                    "Provider returned an empty or oversized reply".into(),
                ),
            }
        }
        Intent::Previewed { revision, result }
            if revision == current && state.status.phase == Phase::Waiting =>
        {
            let Some((source, summary)) = state.validating.take() else {
                return Effects::None;
            };
            match result {
                Ok(mut draft) if draft.source == source && draft.summary == summary => {
                    let history = &state
                        .context
                        .as_ref()
                        .expect("validation has a pinned chart")
                        .history;
                    let range = history.first().zip(history.last()).map_or_else(
                        || "No loaded closed bars".into(),
                        |(first, last)| {
                            format!(
                                "Captured slots {}–{} ({} closed bars)",
                                first.close.slot,
                                last.close.slot,
                                history.len()
                            )
                        },
                    );
                    let total = draft.markers.len();
                    draft.markers.truncate(MAX_MARKERS);
                    state.status.message = format!(
                        "{range} · showing {} of {total} signals",
                        draft.markers.len()
                    );
                    state.status.draft = Some(draft);
                    state.status.phase = Phase::Preview;
                    Effects::None
                }
                Err(errors) if !errors.is_empty() && state.repairs < MAX_REPAIRS => {
                    state.repairs += 1;
                    state.request(Some((source, errors)))
                }
                Err(errors) => {
                    let message = errors.first().map_or_else(
                        || "Validation failed".into(),
                        |d| {
                            format!(
                                "{} {} {}",
                                d.line.map_or_else(String::new, |n| format!("Line {n}")),
                                d.code,
                                d.message
                            )
                        },
                    );
                    state.stop(Phase::Failed, format!("Repair limit reached. {message}"))
                }
                _ => state.stop(Phase::Failed, "Preview does not match the candidate".into()),
            }
        }
        Intent::Saved { revision, result } if revision == current && state.saving.is_some() => {
            let expected = state.saving.take();
            state.saved_attempt.clone_from(&expected);
            match result {
                Ok(id) if expected.as_ref() == Some(&id) => {
                    state.status.saved_id = Some(id.clone());
                    state.stop(Phase::Saved, format!("Saved {id} and armed its alarms"))
                }
                Err(message) => state.stop(Phase::Failed, message),
                _ => state.stop(Phase::Failed, "Save returned an unexpected identity".into()),
            }
        }
        _ => Effects::None,
    }
}
#[cfg(test)]
#[path = "strategy_describe/tests.rs"]
mod tests;
