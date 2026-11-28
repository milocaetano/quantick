use super::*;
use crate::script_strategy::ScriptPane;
use crate::tabs::TabDirectory;
use crate::test_support::{FakeAccess, FakeTab, FakeWindow, Handler, docked};
use quantick_chart::state::{ChartState, SpecSelector};
use quantick_control_host::actions::ActionRegistry;
use quantick_control_host::authority::CAPABILITY_VERSION;
use quantick_engine::BarSpec;
use quantick_session::script_strategies::{ScriptArm, ScriptPlacement, SignalAction};
use quantick_session::strategies::TabStrategies;
use serde_json::{Value, json};

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
        action: SignalAction::Alarm,
    };
    let armed = window.strategies.arm_script(request, |_| Vec::new());
    armed.expect("the fixture arms");
}

fn call(
    handler: Handler<Window>,
    window: &mut Window,
    access: &mut FakeAccess,
    input: Value,
) -> Value {
    handler(window, access, &FakeWindow::assistant(), &input)
        .unwrap_or_else(|error| panic!("{input}: {error:?}"))
}

/// Run capability `id` as both families dock it; a command must answer that
/// `id` is the call that ran.
fn invoke(
    registry: &ActionRegistry<Window, FakeAccess>,
    id: &str,
    window: &mut Window,
    access: &mut FakeAccess,
    input: &Value,
) -> Result<Value, ControlError> {
    let action = (registry.lookup(id, CAPABILITY_VERSION)).unwrap_or_else(|| panic!("{id} docks"));
    let answer = (action.handler)(window, access, &FakeWindow::assistant(), input)?;
    if let Some(ran) = answer.get("capability") {
        assert_eq!(ran, id, "{id} ran another call's handler");
    }
    Ok(answer)
}

fn script(id: &str) -> Value {
    json!({"kind": "script", "id": id})
}

#[test]
fn every_desk_action_has_a_call() {
    use crate::script_strategy as scripts;

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
    let mut registry = ActionRegistry::new();
    register(&mut registry).expect("the desk docks");
    crate::script_strategy::register(&mut registry).expect("the scripts dock");
    let window = &mut window;
    let access = &mut access;
    let mut run = |id: &str, input: Value| invoke(&registry, id, window, access, &input);

    let listed = run(LIST_CAPABILITY_ID, json!({})).expect(LIST_CAPABILITY_ID);
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
    let read = run(DETAIL_CAPABILITY_ID, asked).expect(DETAIL_CAPABILITY_ID);
    assert_eq!(read["description"], "Steady rise");
    assert_eq!(read["inputs"], json!([{"name": "Length", "value": "14"}]));
    assert_eq!(read["code"], STEADY);
    assert_eq!(read["placement"], json!({"kind": "anywhere"}));
    assert_eq!(read["fires_today"], 0);
    assert_eq!(read["history_slots"], json!([]));

    let one = json!({"tab_id": "31", "instance": script("1")});
    let paused = run(PAUSE_CAPABILITY_ID, one.clone()).expect(PAUSE_CAPABILITY_ID);
    assert_eq!(paused["changed"], 1);
    assert_eq!(paused["tab_id"], "31");
    let resumed = run(RESUME_CAPABILITY_ID, one.clone()).expect(RESUME_CAPABILITY_ID);
    assert_eq!(resumed["changed"], 1);
    let asset = json!({"symbol": "WINZ26", "bar_type": flow_bars});
    let asset_paused =
        run(ASSET_PAUSE_CAPABILITY_ID, asset.clone()).expect(ASSET_PAUSE_CAPABILITY_ID);
    assert_eq!(asset_paused["changed"], 1, "only the flow chart's asset");
    assert_eq!(asset_paused["tab_id"], Value::Null);
    let listed = run(LIST_CAPABILITY_ID, json!({})).expect(LIST_CAPABILITY_ID);
    assert_eq!(listed["groups"][0]["paused"], true);
    assert_eq!(listed["groups"][1]["paused"], false);
    let asset_resumed = run(ASSET_RESUME_CAPABILITY_ID, asset).expect(ASSET_RESUME_CAPABILITY_ID);
    assert_eq!(asset_resumed["changed"], 1);

    let copy = run(DUPLICATE_CAPABILITY_ID, one.clone()).expect(DUPLICATE_CAPABILITY_ID);
    assert_eq!(copy["created"], script("3"));
    let save = json!({"tab_id": "31", "instance": script("1"), "name": "steady_copy.pine"});
    let saved = run(SAVE_AS_CAPABILITY_ID, save.clone()).expect(SAVE_AS_CAPABILITY_ID);
    assert_eq!(saved["saved_as"], "steady_copy.pine");
    let written = std::fs::read_to_string(scratch.path().join("steady_copy.pine"));
    assert_eq!(written.ok().as_deref(), Some(STEADY));
    let refused = run(SAVE_AS_CAPABILITY_ID, save);
    assert!(refused.is_err(), "the name is taken");

    let watching = run(ARM_CAPABILITY_ID, one.clone());
    assert!(watching.is_err(), "a watching strategy is not armed again");
    let removed = run(
        REMOVE_CAPABILITY_ID,
        json!({"tab_id": "31", "instance": script("3")}),
    )
    .expect(REMOVE_CAPABILITY_ID);
    assert_eq!(removed["changed"], 1);
    let listed = run(LIST_CAPABILITY_ID, json!({})).expect(LIST_CAPABILITY_ID);
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
            PAUSE_CAPABILITY_ID,
            RESUME_CAPABILITY_ID,
            ASSET_PAUSE_CAPABILITY_ID,
            ASSET_RESUME_CAPABILITY_ID,
            DUPLICATE_CAPABILITY_ID,
            SAVE_AS_CAPABILITY_ID,
            REMOVE_CAPABILITY_ID,
        ],
        "each command that went through is journaled"
    );

    // The calls left, each with a field only its own handler answers.
    let _ = window.strategies.pane_mut(CONTEXT).scripts[0]
        .halt(quantick_session::strategies::DRAWING_REMOVED);
    let arm =
        json!({"script": "steady_copy.pine", "side": "buy", "placement": {"kind": "anywhere"}});
    for (id, input, answers) in [
        (
            ARM_CAPABILITY_ID,
            json!({"tab_id": "31", "instance": script("2")}),
            "/changed",
        ),
        (
            ACTION_SET_CAPABILITY_ID,
            json!({"id": "1", "action": "alarm"}),
            "/changed",
        ),
        (scripts::LIST_CAPABILITY_ID, json!({}), "/library"),
        (scripts::ARM_CAPABILITY_ID, arm, "/strategy/script"),
        (
            scripts::DISARM_CAPABILITY_ID,
            json!({"id": "1"}),
            "/disarmed",
        ),
    ] {
        let answer = invoke(&registry, id, window, access, &input).expect(id);
        assert!(answer.pointer(answers).is_some(), "{id}: {answer}");
    }
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
    let _ = window.strategies.pane_mut(FLOW).scripts[0]
        .halt(quantick_session::strategies::DRAWING_REMOVED);

    let armed = call(
        docked::<Desk<Arm>, _>(),
        &mut window,
        &mut access,
        json!({"tab_id": "31", "instance": script("1")}),
    );

    assert_eq!(armed["changed"], 1);
    let listed = call(
        docked::<ListInput, _>(),
        &mut window,
        &mut access,
        json!({}),
    );
    assert_eq!(listed["groups"][0]["rows"][0]["state"], "watching");
}

#[test]
fn a_replaying_tab_groups_under_the_recordings_symbol() {
    let mut replaying = FakeTab::new(TAB, "BTCUSDT");
    replaying.replaying = true;
    replaying.replay_symbol = Some("WINZ26".to_owned());
    let window = FakeWindow::new(vec![FakeTab::new(7, "BTCUSDT"), replaying]);

    assert_eq!(window.desk_symbol(0).as_deref(), Some("BTCUSDT"));
    assert_eq!(
        window.desk_symbol(1).as_deref(),
        Some("WINZ26"),
        "a replay's desk names the recording, not the live feed beneath it"
    );
    assert_eq!(window.desk_symbol(2), None, "a closed tab has no symbol");
}

#[test]
fn script_action_set_reads_back_through_list() {
    let scratch = crate::scratch::ScratchDir::new("script-action");
    let mut window = Window {
        library: scratch.path().display().to_string(),
        strategies: TabStrategies::default(),
        panes: vec![Pane::new(FLOW, BarSpec::Tick(4))],
    };
    let mut access = FakeAccess::default();
    arm_steady(&mut window, FLOW);
    let (window, access) = (&mut window, &mut access);
    let set = docked::<ActionSetInput, _>();
    let list = docked::<crate::script_strategy::ListInput, _>();

    let order =
        json!({"id": "1", "action": "order", "quantity": "2", "stop": "100", "gain": "200"});
    let answer = call(set, window, access, order);
    assert_eq!(answer["capability"], ACTION_SET_CAPABILITY_ID);
    assert_eq!(answer["changed"], 1);
    let journaled = access.events.iter().map(|event| event.kind.to_string());
    assert_eq!(journaled.collect::<Vec<_>>(), [CHANGED_EVENT_KIND]);

    let armed = &call(list, window, access, json!({}))["armed"][0];
    assert_eq!(armed["action"], "order");
    assert_eq!(armed["alarm_only"], false);
    assert_eq!(
        (&armed["quantity"], &armed["stop"], &armed["gain"]),
        (&json!("2"), &json!("100"), &json!("200"))
    );
    let input = json!({"tab_id": "31", "instance": script("1")});
    let detail = call(docked::<Detail, _>(), window, access, input);
    assert_eq!(detail["strategy"]["action"], "order");
    assert_eq!(
        (&detail["quantity"], &detail["gain"]),
        (&json!("2"), &json!("200"))
    );

    let again = json!({"id": "1", "action": "order", "quantity": "Ticket"});
    assert_eq!(call(set, window, access, again)["changed"], 1);
    let armed = &call(list, window, access, json!({}))["armed"][0];
    assert_eq!(
        (&armed["quantity"], &armed["stop"]),
        (&json!("ticket"), &json!("0"))
    );

    let alarm = json!({"id": "1", "action": "alarm"});
    assert_eq!(call(set, window, access, alarm.clone())["changed"], 1);
    assert_eq!(
        call(set, window, access, alarm)["changed"],
        0,
        "already ringing"
    );
    let armed = &call(list, window, access, json!({}))["armed"][0];
    assert_eq!(
        (&armed["action"], &armed["quantity"]),
        (&json!("alarm"), &Value::Null)
    );

    let actor = FakeWindow::assistant();
    for (input, says) in [
        (
            json!({"id": "1", "action": "alarm", "stop": "5"}),
            "an alarm takes none",
        ),
        (json!({"id": "1", "action": "order"}), "needs a quantity"),
        (
            json!({"id": "1", "action": "order", "quantity": "two"}),
            "`two`",
        ),
        (
            json!({"id": "1", "action": "order", "quantity": "0"}),
            "above zero",
        ),
        (
            json!({"id": "1", "action": "order", "quantity": "1", "stop": "-1"}),
            "never below",
        ),
        (json!({"id": "9", "action": "alarm"}), "no armed strategy"),
    ] {
        let refused = set(window, access, &actor, &input).expect_err("refused");
        assert!(
            refused.message.contains(says),
            "{input}: {}",
            refused.message
        );
    }
}
