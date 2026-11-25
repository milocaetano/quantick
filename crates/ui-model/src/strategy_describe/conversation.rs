//! One chart conversation driven against its provider and its chart: which
//! intents are admitted, when the pinned chart has gone stale, and the order
//! a preview and a save take. The host supplies the two ports and paints.
use super::{
    Command, Context, Draft, Effects, HistoricalClose, Intent, Phase, Reply, Request, State,
    Status, update,
};
use quantick_chart_interaction::drawing_model::DrawingId;
use quantick_control::error::ControlError;
use quantick_session::{
    script_draft::{self, DraftInput},
    strategies::TabStrategies,
};

/// A provider reply for one request revision.
pub type Completion = (u64, Result<Reply, String>);

/// The network worker a conversation sends through. Credentials and transport
/// stay with the host; every text leaving the chart is checked here first.
pub trait Provider {
    fn available(&self) -> bool;
    fn validate_input(&self, text: &str) -> Result<(), &'static str>;
    fn validate_chart(&self, context: &Context) -> Result<(), &'static str>;
    /// Accept one request; a refusal comes back as that revision's failed reply.
    fn submit(&mut self, request: Request) -> Result<(), String>;
    /// Drop any reply still in flight.
    fn invalidate(&self);
    fn poll(&mut self) -> Option<Completion>;
}

/// The focused chart a conversation pins, and the tab it saves onto.
pub trait Chart {
    /// Capture the focused chart's pane, placement and closed history.
    fn capture(&mut self) -> Context;
    /// Whether the focused chart is still the one `context` pinned.
    fn shows(&self, context: &Context) -> bool;
    /// The conversation window opened on the captured chart.
    fn opened(&mut self);
    /// Whether the selected alarm region on the pinned tab can still arm.
    fn check_region(&self, context: &Context, drawing: DrawingId) -> Result<(), String>;
    /// The pinned tab's closes since its live seam, which warm a saved alarm.
    fn warm_history(&self, context: &Context) -> Vec<HistoricalClose>;
    /// Publish the source to the script library under `id`.
    fn store(&mut self, id: &str, source: &str) -> Result<(), String>;
    fn strategies(&mut self, context: &Context) -> &mut TabStrategies<DrawingId>;
}

fn retarget(state: &mut State, provider: &impl Provider, chart: &impl Chart) {
    if state.context().is_some_and(|context| !chart.shows(context)) {
        provider.invalidate();
        update(state, Intent::TargetChanged);
    }
}

/// Apply one intent and run its effects until the conversation waits.
pub fn dispatch(
    state: &mut State,
    provider: &mut impl Provider,
    chart: &mut impl Chart,
    intent: Intent,
) -> Result<Status, ControlError> {
    if !provider.available() {
        if matches!(intent, Intent::Command(Command::Read)) {
            return Ok(state.status.clone());
        }
        return Err(quantick_control_schema::strategy_describe::unavailable_error());
    }
    let opened = match &intent {
        Intent::Command(Command::Open) => {
            let context = chart.capture();
            provider
                .validate_chart(&context)
                .map_err(ControlError::invalid_request)?;
            Some(context)
        }
        Intent::Command(Command::Submit { text, .. } | Command::Save { id: text, .. }) => {
            provider
                .validate_input(text)
                .map_err(ControlError::invalid_request)?;
            None
        }
        _ => None,
    };
    if state.status.phase == Phase::Unavailable {
        state.status.phase = Phase::Closed;
    }
    retarget(state, provider, chart);
    if let Intent::Command(command) = &intent {
        state
            .admit(command)
            .map_err(ControlError::invalid_request)?;
        if matches!(command, Command::Open | Command::Cancel { .. }) {
            provider.invalidate();
        }
    }
    let intent = if let Some(context) = opened {
        chart.opened();
        Intent::Opened(context.into())
    } else {
        intent
    };
    let mut effect = update(state, intent);
    loop {
        let intent = match effect {
            Effects::Request(request) => {
                let revision = request.revision;
                match provider.submit(request) {
                    Ok(()) => break,
                    Err(message) => Intent::Replied {
                        revision,
                        reply: Err(message),
                    },
                }
            }
            Effects::Preview {
                revision,
                context,
                source,
                summary,
            } => {
                let result = script_draft::preview(DraftInput {
                    id: "preview.pine",
                    source: &source,
                    placement: context.placement,
                    history: &context.history,
                })
                .map(|markers| Draft {
                    source,
                    summary,
                    markers,
                });
                Intent::Previewed { revision, result }
            }
            Effects::Save { revision, id } => {
                let result = save(state, chart, id);
                Intent::Saved { revision, result }
            }
            Effects::None | Effects::Dispatch(_) => break,
        };
        effect = update(state, intent);
    }
    Ok(state.status.clone())
}

/// Warm and admit the previewed source before the library is touched; arm it
/// only once its file exists.
fn save(state: &State, chart: &mut impl Chart, id: String) -> Result<String, String> {
    let context = state.context().expect("save requires pinned context");
    let source = &state
        .status
        .draft
        .as_ref()
        .expect("save requires preview")
        .source;
    if let Some(drawing) = context.placement.drawing() {
        chart.check_region(context, drawing)?;
    }
    let warm = chart.warm_history(context);
    let prepared = script_draft::prepare(DraftInput {
        id: &id,
        source,
        placement: context.placement,
        history: &warm,
    })
    .map_err(|errors| {
        errors.first().map_or_else(
            || "Warmup failed".into(),
            |d| format!("Warmup failed: {} {}", d.code, d.message),
        )
    })?;
    chart.store(&id, source)?;
    script_draft::commit(chart.strategies(context), context.pane, prepared);
    Ok(id)
}

/// One presented frame: drop a stale chart, run the view's or the model's
/// queued command, then deliver a finished reply. Returns the command's refusal.
pub fn frame(
    state: &mut State,
    provider: &mut impl Provider,
    chart: &mut impl Chart,
    command: Option<Command>,
    effect: Option<Effects>,
) -> Result<(), ControlError> {
    retarget(state, provider, chart);
    let command = command.or(match effect {
        Some(Effects::Dispatch(c)) => Some(c),
        _ => None,
    });
    let refused = command
        .map(|command| dispatch(state, provider, chart, Intent::Command(command)))
        .and_then(Result::err);
    if let Some((revision, reply)) = provider.poll() {
        let _ = dispatch(state, provider, chart, Intent::Replied { revision, reply });
    }
    refused.map_or(Ok(()), Err)
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
