use super::*;
use crate::script_strategy::ScriptPane;
use crate::tabs::TabDirectory;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_chart::state::{ChartState, SpecSelector};
use quantick_engine::BarSpec;
use quantick_session::script_strategies::{ScriptArm, ScriptPlacement};
use quantick_session::strategies::TabStrategies;
use serde_json::json;

const TAB: u64 = 31;
const FLOW: u64 = 5;
const CONTEXT: u64 = 6;

const STEADY: &str = "//@version=5\n\
    indicator(\"Steady rise\")\n\
    len = input.int(14, title=\"Length\")\n\
    alertcondition(close > open, title=\"Long up\")\n\
    plot(ta.sma(close, len))\n";

struct Pane {
    id: u64,
    state: ChartState,
    spec: SpecSelector,
    drawings: Drawings<DrawingTool>,
}

impl Pane {
    fn new(id: u64, spec: BarSpec) -> Self {
        Self {
            id,
            state: ChartState::new(spec),
            spec: SpecSelector::new(spec),
            drawings: Drawings::default(),
        }
    }
}

struct Window {
    library: String,
    strategies: TabStrategies<DrawingId>,
    panes: Vec<Pane>,
}

impl TabDirectory for Window {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (tab_id == TAB).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        TAB
    }
}

impl ScriptStrategyHost for Window {
    fn script_tab(&mut self, index: usize) -> Option<ScriptTab<'_>> {
        (index == 0).then(|| ScriptTab {
            library: Some(self.library.as_str()),
            strategies: &mut self.strategies,
            panes: (self.panes.iter())
                .map(|pane| ScriptPane {
                    id: pane.id,
                    series: PaneSeriesRead {
                        history_prefix: &[],
                        state: &pane.state,
                        spec: &pane.spec,
                    },
                    drawings: &pane.drawings,
                })
                .collect(),
        })
    }
}

impl StrategyDeskHost for Window {
    fn desk_tab_count(&self) -> usize {
        1
    }
    fn desk_symbol(&self, index: usize) -> Option<String> {
        (index == 0).then(|| "WINZ26".to_owned())
    }
}

fn arm_steady(window: &mut Window, pane_id: u64) {
    let request = ScriptArm {
        pane_id,
        script: "steady.pine",
        source: STEADY,
        side: Side::Buy,
        place: ScriptPlacement::Anywhere,
    };
    let armed = window.strategies.arm_script(request, |_| Vec::new());
    armed.expect("the fixture arms");
}

type Handler =
    fn(&mut Window, &mut FakeAccess, &ActorContext, &Value) -> Result<Value, ControlError>;

fn call(handler: Handler, window: &mut Window, access: &mut FakeAccess, input: Value) -> Value {
    handler(window, access, &FakeWindow::assistant(), &input)
        .unwrap_or_else(|error| panic!("{input}: {error:?}"))
}

fn script(id: &str) -> Value {
    json!({"kind": "script", "id": id})
}

#[test]
fn every_desk_action_has_a_call() {
    let scratch = crate::scratch::ScratchDir::new("strategy-desk");
    let mut window = Window {
        library: scratch.path().display().to_string(),
        strategies: TabStrategies::default(),
        panes: vec![
            Pane::new(FLOW, BarSpec::Tick(4)),
            Pane::new(CONTEXT, BarSpec::Time(60_000)),
        ],
    };
    let mut access = FakeAccess::default();
    arm_steady(&mut window, FLOW);
    arm_steady(&mut window, CONTEXT);
    let window = &mut window;
    let access = &mut access;

    let listed = call(list::<Window, FakeAccess>, window, access, json!({}));
    let groups = listed["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 2, "one group per bar type: {listed}");
    assert_eq!(groups[0]["symbol"], "WINZ26");
    assert_eq!(groups[0]["count"], 1);
    let first = &groups[0]["rows"][0];
    assert_eq!(first["instance"], script("1"));
    assert_eq!(first["link"], json!({"kind": "chart"}));
    assert_eq!(first["action"], "alarm");
    assert_eq!(first["state"], "watching");
    let flow_bars = groups[0]["bar_type"].clone();

    let asked = json!({"tab_id": "31", "instance": script("1")});
    let read = call(detail::<Window, FakeAccess>, window, access, asked);
    assert_eq!(read["description"], "Steady rise");
    assert_eq!(read["inputs"], json!([{"name": "Length", "value": "14"}]));
    assert_eq!(read["code"], STEADY);
    assert_eq!(read["placement"], json!({"kind": "anywhere"}));
    assert_eq!(read["fires_today"], 0);
    assert_eq!(read["history_slots"], json!([]));

    let one = json!({"tab_id": "31", "instance": script("1")});
    let paused = call(pause::<Window, FakeAccess>, window, access, one.clone());
    assert_eq!(paused["changed"], 1);
    assert_eq!(paused["tab_id"], "31");
    let resumed = call(resume::<Window, FakeAccess>, window, access, one.clone());
    assert_eq!(resumed["changed"], 1);
    let asset = json!({"symbol": "WINZ26", "bar_type": flow_bars});
    let asset_paused = call(
        asset_pause::<Window, FakeAccess>,
        window,
        access,
        asset.clone(),
    );
    assert_eq!(asset_paused["changed"], 1, "only the flow chart's asset");
    assert_eq!(asset_paused["tab_id"], Value::Null);
    let listed = call(list::<Window, FakeAccess>, window, access, json!({}));
    assert_eq!(listed["groups"][0]["paused"], true);
    assert_eq!(listed["groups"][1]["paused"], false);
    let asset_resumed = call(asset_resume::<Window, FakeAccess>, window, access, asset);
    assert_eq!(asset_resumed["changed"], 1);

    let copy = call(duplicate::<Window, FakeAccess>, window, access, one.clone());
    assert_eq!(copy["created"], script("3"));
    let save = json!({"tab_id": "31", "instance": script("1"), "name": "steady_copy.pine"});
    let saved = call(save_as::<Window, FakeAccess>, window, access, save.clone());
    assert_eq!(saved["saved_as"], "steady_copy.pine");
    let written = std::fs::read_to_string(scratch.path().join("steady_copy.pine"));
    assert_eq!(written.ok().as_deref(), Some(STEADY));
    let refused = save_as::<Window, FakeAccess>(window, access, &FakeWindow::assistant(), &save);
    assert!(refused.is_err(), "the name is taken");

    let watching = arm::<Window, FakeAccess>(window, access, &FakeWindow::assistant(), &one);
    assert!(watching.is_err(), "a watching strategy is not armed again");
    let removed = call(
        remove::<Window, FakeAccess>,
        window,
        access,
        json!({"tab_id": "31", "instance": script("3")}),
    );
    assert_eq!(removed["changed"], 1);
    let listed = call(list::<Window, FakeAccess>, window, access, json!({}));
    assert_eq!(listed["groups"][0]["count"], 1, "the copy is gone");

    let kinds = access.events.iter().map(|event| event.kind.to_string());
    let capabilities = access
        .events
        .iter()
        .map(|event| event.payload["capability"].clone());
    assert!(kinds.into_iter().all(|kind| kind == CHANGED_EVENT_KIND));
    let capabilities: Vec<_> = capabilities.collect();
    assert_eq!(
        capabilities,
        [
            wire::PAUSE_CAPABILITY_ID,
            wire::RESUME_CAPABILITY_ID,
            wire::ASSET_PAUSE_CAPABILITY_ID,
            wire::ASSET_RESUME_CAPABILITY_ID,
            wire::DUPLICATE_CAPABILITY_ID,
            wire::SAVE_AS_CAPABILITY_ID,
            wire::REMOVE_CAPABILITY_ID,
        ],
        "each command that went through is journaled"
    );
}

#[test]
fn an_ended_script_is_armed_again_through_its_call() {
    let mut window = Window {
        library: String::new(),
        strategies: TabStrategies::default(),
        panes: vec![Pane::new(FLOW, BarSpec::Tick(4))],
    };
    let mut access = FakeAccess::default();
    arm_steady(&mut window, FLOW);
    window.strategies.pane_mut(FLOW).scripts[0].halt(quantick_session::strategies::DRAWING_REMOVED);

    let armed = call(
        arm::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({"tab_id": "31", "instance": script("1")}),
    );

    assert_eq!(armed["changed"], 1);
    let listed = call(
        list::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({}),
    );
    assert_eq!(listed["groups"][0]["rows"][0]["state"], "watching");
}
