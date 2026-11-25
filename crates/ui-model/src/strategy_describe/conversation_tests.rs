use super::*;
use crate::strategy_describe::{Phase, Turn};
use quantick_engine::Bar;
use quantick_session::{script_strategies::ScriptPlacement, strategies::LiveClose};
use rust_decimal::Decimal;

const SOURCE: &str =
    "//@version=5\nindicator(\"Up close\")\nalertcondition(close > open, title=\"Buy up close\")";

#[derive(Default)]
struct FakeProvider {
    off: bool,
    refuse_chart: bool,
    refuse_submit: bool,
    sent: Vec<Request>,
    invalidated: std::cell::Cell<usize>,
    replies: Vec<Completion>,
}
impl Provider for FakeProvider {
    fn available(&self) -> bool {
        !self.off
    }
    fn validate_input(&self, text: &str) -> Result<(), &'static str> {
        if text.contains("secret") {
            return Err("Input contains the provider credential");
        }
        Ok(())
    }
    fn validate_chart(&self, _: &Context) -> Result<(), &'static str> {
        if self.refuse_chart {
            return Err("Input contains the provider credential");
        }
        Ok(())
    }
    fn submit(&mut self, request: Request) -> Result<(), String> {
        if self.refuse_submit {
            return Err("Strategy provider is busy or disconnected".into());
        }
        self.sent.push(request);
        Ok(())
    }
    fn invalidate(&self) {
        self.invalidated.set(self.invalidated.get() + 1);
    }
    fn poll(&mut self) -> Option<Completion> {
        self.replies.pop()
    }
}

struct FakeChart {
    tab: u64,
    opened: usize,
    region_fault: Option<String>,
    store_fault: Option<String>,
    stored: Vec<(String, String)>,
    strategies: TabStrategies<DrawingId>,
}
impl Default for FakeChart {
    fn default() -> Self {
        Self {
            tab: 11,
            opened: 0,
            region_fault: None,
            store_fault: None,
            stored: vec![],
            strategies: TabStrategies::default(),
        }
    }
}
fn close(slot: usize, open: i64, close: i64) -> HistoricalClose {
    let (open, close) = (Decimal::from(open), Decimal::from(close));
    HistoricalClose {
        close: LiveClose {
            bar: Bar {
                open,
                high: open.max(close),
                low: open.min(close),
                close,
                open_time: 1,
                close_time: 2,
                sell_volume: Decimal::ONE,
                buy_volume: Decimal::ONE,
                trade_count: 2,
            },
            poc: None,
            slot,
        },
        region: None,
    }
}
impl Chart for FakeChart {
    fn capture(&mut self) -> Context {
        Context {
            workspace: quantick_workspace::lifetime::WorkspaceId(7),
            tab: self.tab,
            pane: 24,
            feed: "recorded".into(),
            symbol: "TEST".into(),
            bars: "renko:1".into(),
            placement: ScriptPlacement::Anywhere,
            indicators: vec![],
            drawings: vec![],
            availability: vec![],
            history: vec![close(4, 10, 11), close(5, 11, 10), close(6, 10, 12)],
        }
    }
    fn shows(&self, context: &Context) -> bool {
        context.tab == self.tab
    }
    fn opened(&mut self) {
        self.opened += 1;
    }
    fn check_region(&self, _: &Context, _: DrawingId) -> Result<(), String> {
        self.region_fault.clone().map_or(Ok(()), Err)
    }
    fn warm_history(&self, _: &Context) -> Vec<HistoricalClose> {
        vec![close(6, 10, 12)]
    }
    fn store(&mut self, id: &str, source: &str) -> Result<(), String> {
        if let Some(fault) = &self.store_fault {
            return Err(fault.clone());
        }
        self.stored.push((id.into(), source.into()));
        Ok(())
    }
    fn strategies(&mut self, _: &Context) -> &mut TabStrategies<DrawingId> {
        &mut self.strategies
    }
}

fn command(command: Command) -> Intent {
    Intent::Command(command)
}
fn open(state: &mut State, provider: &mut FakeProvider, chart: &mut FakeChart) -> Status {
    dispatch(state, provider, chart, command(Command::Open)).unwrap()
}
fn submit(state: &mut State, provider: &mut FakeProvider, chart: &mut FakeChart) -> Status {
    let revision = state.status.revision;
    let text = "Alert when a bar closes up".into();
    dispatch(state, provider, chart, command(Command::Submit { revision, text })).unwrap()
}
fn reply(state: &mut State, provider: &mut FakeProvider, chart: &mut FakeChart) -> Status {
    let revision = state.status.revision;
    let reply = Ok(Reply::Candidate {
        source: SOURCE.into(),
        summary: "Buys on an up close".into(),
    });
    dispatch(state, provider, chart, Intent::Replied { revision, reply }).unwrap()
}
fn save(
    state: &mut State,
    provider: &mut FakeProvider,
    chart: &mut FakeChart,
) -> Result<Status, ControlError> {
    let revision = state.status.revision;
    let id = "up_close.pine".to_string();
    dispatch(state, provider, chart, command(Command::Save { revision, id }))
}

#[test]
fn an_unavailable_provider_answers_only_read() {
    let (mut state, mut chart) = (State::default(), FakeChart::default());
    let mut provider = FakeProvider {
        off: true,
        ..FakeProvider::default()
    };
    let read = dispatch(&mut state, &mut provider, &mut chart, command(Command::Read));
    assert_eq!(read.unwrap().phase, Phase::Unavailable);
    let refused = dispatch(&mut state, &mut provider, &mut chart, command(Command::Open));
    assert_eq!(refused.unwrap_err().message, "Strategy description is unavailable");
    assert_eq!(chart.opened, 0);
}

#[test]
fn open_pins_the_captured_chart_only_after_the_provider_accepts_it() {
    let (mut state, mut chart) = (State::default(), FakeChart::default());
    let mut provider = FakeProvider {
        refuse_chart: true,
        ..FakeProvider::default()
    };
    let refused = dispatch(&mut state, &mut provider, &mut chart, command(Command::Open));
    assert_eq!(refused.unwrap_err().message, "Input contains the provider credential");
    assert!(state.context().is_none());
    assert_eq!(chart.opened, 0);

    provider.refuse_chart = false;
    let status = open(&mut state, &mut provider, &mut chart);
    assert_eq!(status.phase, Phase::Ready);
    assert_eq!(chart.opened, 1);
    assert_eq!(state.context().map(|c| c.tab), Some(11));
}

#[test]
fn input_carrying_the_credential_is_refused_before_admission() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    open(&mut state, &mut provider, &mut chart);
    let revision = state.status.revision;
    let text = "use secret".into();
    let refused =
        dispatch(&mut state, &mut provider, &mut chart, command(Command::Submit { revision, text }));
    assert_eq!(refused.unwrap_err().message, "Input contains the provider credential");
    assert!(state.status.transcript.is_empty());
    assert!(provider.sent.is_empty());
}

#[test]
fn a_changed_focused_chart_cancels_the_pinned_conversation() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    open(&mut state, &mut provider, &mut chart);
    let before = provider.invalidated.get();
    chart.tab = 12;
    let status = dispatch(&mut state, &mut provider, &mut chart, command(Command::Read)).unwrap();
    assert_eq!(status.phase, Phase::Cancelled);
    assert_eq!(status.message, "Chart changed. Open a new conversation on this chart.");
    assert!(state.context().is_none());
    assert!(provider.invalidated.get() > before);
}

#[test]
fn a_refused_submission_fails_its_revision() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    open(&mut state, &mut provider, &mut chart);
    provider.refuse_submit = true;
    let status = submit(&mut state, &mut provider, &mut chart);
    assert_eq!(status.phase, Phase::Failed);
    assert_eq!(status.message, "Strategy provider is busy or disconnected");
}

#[test]
fn a_candidate_is_previewed_on_the_pinned_history() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    open(&mut state, &mut provider, &mut chart);
    let waiting = submit(&mut state, &mut provider, &mut chart);
    assert_eq!(waiting.phase, Phase::Waiting);
    assert_eq!(provider.sent.len(), 1);
    assert!(matches!(&provider.sent[0].transcript[..], [Turn::Trader(_)]));
    let status = reply(&mut state, &mut provider, &mut chart);
    assert_eq!(status.phase, Phase::Preview);
    let draft = status.draft.expect("previewed draft");
    assert_eq!(draft.source, SOURCE);
    assert_eq!(draft.markers.iter().map(|m| m.slot).collect::<Vec<_>>(), [4, 6]);
}

#[test]
fn an_invalid_candidate_is_sent_back_for_repair() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    open(&mut state, &mut provider, &mut chart);
    submit(&mut state, &mut provider, &mut chart);
    let revision = state.status.revision;
    let reply = Ok(Reply::Candidate {
        source: "indicator(\"bad\")\nx = missing_symbol".into(),
        summary: "Broken".into(),
    });
    let status =
        dispatch(&mut state, &mut provider, &mut chart, Intent::Replied { revision, reply })
            .unwrap();
    assert_eq!(status.phase, Phase::Waiting);
    let repair = provider.sent.last().and_then(|r| r.repair.as_ref());
    assert!(repair.is_some_and(|(_, diagnostics)| !diagnostics.is_empty()));
}

#[test]
fn save_stores_the_source_before_arming_it() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    open(&mut state, &mut provider, &mut chart);
    submit(&mut state, &mut provider, &mut chart);
    reply(&mut state, &mut provider, &mut chart);
    let status = save(&mut state, &mut provider, &mut chart).unwrap();
    assert_eq!(status.phase, Phase::Saved);
    assert_eq!(status.message, "Saved up_close.pine and armed its alarms");
    assert_eq!(chart.stored, [("up_close.pine".to_string(), SOURCE.to_string())]);
    assert!(chart.strategies.pane(24).is_some_and(|pane| !pane.is_empty()));
}

#[test]
fn a_failed_store_arms_nothing() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    chart.store_fault = Some("A script with this name exists".into());
    open(&mut state, &mut provider, &mut chart);
    submit(&mut state, &mut provider, &mut chart);
    reply(&mut state, &mut provider, &mut chart);
    let status = save(&mut state, &mut provider, &mut chart).unwrap();
    assert_eq!(status.phase, Phase::Failed);
    assert_eq!(status.message, "A script with this name exists");
    assert!(chart.strategies.pane(24).is_none_or(|pane| pane.is_empty()));
}

#[test]
fn a_region_that_can_no_longer_arm_stops_the_save_before_the_store() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    chart.region_fault = Some("Region was deleted".into());
    let mut context = chart.capture();
    context.placement = ScriptPlacement::Region(DrawingId(3));
    update(&mut state, Intent::Opened(context.into()));
    submit(&mut state, &mut provider, &mut chart);
    assert_eq!(reply(&mut state, &mut provider, &mut chart).phase, Phase::Preview);
    let status = save(&mut state, &mut provider, &mut chart).unwrap();
    assert_eq!(status.phase, Phase::Failed);
    assert_eq!(status.message, "Region was deleted");
    assert!(chart.stored.is_empty());
}

#[test]
fn frame_runs_the_queued_command_then_delivers_a_finished_reply() {
    let (mut state, mut provider, mut chart) =
        (State::default(), FakeProvider::default(), FakeChart::default());
    let opened = frame(&mut state, &mut provider, &mut chart, Some(Command::Open), None);
    assert!(opened.is_ok());
    assert_eq!(chart.opened, 1);
    submit(&mut state, &mut provider, &mut chart);
    let revision = state.status.revision;
    provider.replies.push((revision, Ok(Reply::Question("Which side?".into()))));
    frame(&mut state, &mut provider, &mut chart, None, None).unwrap();
    assert_eq!(state.status.phase, Phase::Question);

    let stale = Command::Cancel { revision: 0 };
    let refused = frame(&mut state, &mut provider, &mut chart, None, Some(Effects::Dispatch(stale)));
    assert_eq!(refused.unwrap_err().message, "This conversation revision has expired");
}
