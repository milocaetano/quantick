use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_chart::state::{ChartState, SpecSelector};
use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingStyle, NewDrawing};
use quantick_engine::BarSpec;
use serde_json::json;

/// The bundled scripts, `renko_poc_flip.pine` among them.
const BUNDLED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../app/scripts");

const TAB: u64 = 31;
const FLOW: u64 = 5;
const CONTEXT: u64 = 6;

struct Pane {
    id: u64,
    state: ChartState,
    spec: SpecSelector,
    drawings: Drawings<DrawingTool>,
}

impl Pane {
    fn new(id: u64) -> Self {
        Self {
            id,
            state: ChartState::new(BarSpec::Tick(1)),
            spec: SpecSelector::new(BarSpec::Tick(1)),
            drawings: Drawings::default(),
        }
    }
}

struct Window {
    dir: Option<String>,
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
            library: self.dir.as_deref(),
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

fn window() -> Window {
    Window {
        dir: Some(BUNDLED.to_owned()),
        strategies: TabStrategies::default(),
        panes: vec![Pane::new(FLOW), Pane::new(CONTEXT)],
    }
}

type Handler =
    fn(&mut Window, &mut FakeAccess, &ActorContext, &Value) -> Result<Value, ControlError>;

fn call(
    handler: Handler,
    window: &mut Window,
    access: &mut FakeAccess,
    input: Value,
) -> Result<Value, ControlError> {
    handler(window, access, &FakeWindow::assistant(), &input)
}

fn arm_renko(
    window: &mut Window,
    access: &mut FakeAccess,
    extra: Value,
) -> Result<Value, ControlError> {
    let mut input = json!({
        "script": "renko_poc_flip.pine",
        "side": "buy",
        "placement": {"kind": "anywhere"},
    });
    for (key, value) in extra.as_object().expect("an object") {
        input[key] = value.clone();
    }
    call(arm::<Window, FakeAccess>, window, access, input)
}

#[test]
fn arm_list_and_disarm_a_script_strategy() {
    let mut window = window();
    let mut access = FakeAccess::default();
    let armed = arm_renko(&mut window, &mut access, json!({})).expect("arms");
    assert_eq!(armed["tab_id"], "31");
    assert_eq!(armed["strategy"]["id"], "1");
    assert_eq!(
        armed["strategy"]["pane_id"], "5",
        "the flow chart by default"
    );
    assert_eq!(armed["strategy"]["script"], "renko_poc_flip.pine");
    assert_eq!(armed["strategy"]["side"], "buy");
    assert_eq!(armed["strategy"]["placement"], json!({"kind": "anywhere"}));
    assert_eq!(armed["strategy"]["state"], "watching");
    assert_eq!(armed["strategy"]["disarm_reason"], Value::Null);
    assert_eq!(armed["strategy"]["alarm_only"], true);

    let line = json!({
        "pane_id": "6",
        "side": "sell",
        "placement": {"kind": "line", "level": "100.50", "direction": "down"},
    });
    let second = arm_renko(&mut window, &mut access, line).expect("arms");
    assert_eq!(second["strategy"]["id"], "2");
    assert_eq!(second["strategy"]["pane_id"], "6");

    let listed = call(
        list::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({}),
    )
    .expect("lists");
    assert_eq!(listed["library"]["folder"], BUNDLED);
    let scripts = listed["library"]["scripts"].as_array().expect("names");
    assert!(
        scripts.contains(&json!("renko_poc_flip.pine")),
        "{scripts:?}"
    );
    let ids: Vec<_> = (listed["armed"].as_array().expect("armed").iter())
        .map(|strategy| strategy["id"].clone())
        .collect();
    assert_eq!(ids, [json!("1"), json!("2")]);
    assert_eq!(
        listed["armed"][1]["placement"],
        json!({"kind": "line", "level": "100.50", "direction": "down"})
    );

    let gone = call(
        disarm::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({"id": "1"}),
    )
    .expect("disarms");
    assert_eq!(gone["disarmed"], true);
    assert_eq!(gone["strategy"]["id"], "1");
    assert_eq!(gone["strategy"]["state"], "disarmed");
    assert_eq!(gone["strategy"]["disarm_reason"], "disarmed");
    let again = call(
        disarm::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({"id": "1"}),
    )
    .expect("answers");
    assert_eq!(again["disarmed"], false);
    assert_eq!(again["strategy"], Value::Null);
    let left = call(
        list::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({}),
    )
    .expect("lists");
    assert_eq!(left["armed"].as_array().map(Vec::len), Some(1));

    let kinds: Vec<_> = access
        .events
        .iter()
        .map(|event| event.kind.to_string())
        .collect();
    assert_eq!(
        kinds,
        [ARMED_EVENT_KIND, ARMED_EVENT_KIND, DISARMED_EVENT_KIND],
        "each arm and the one real disarm are journaled"
    );
}

#[test]
fn a_refused_arm_names_why_and_arms_nothing() {
    let mut window = window();
    let mut access = FakeAccess::default();
    let missing = arm_renko(&mut window, &mut access, json!({"script": "absent.pine"}))
        .expect_err("no such file");
    assert!(
        missing.message.contains("absent.pine"),
        "{}",
        missing.message
    );
    let silent = arm_renko(&mut window, &mut access, json!({"script": "rsi.pine"}))
        .expect_err("no alertcondition");
    assert!(
        silent.message.contains("no alertcondition"),
        "{}",
        silent.message
    );
    let price = json!({"placement": {"kind": "range", "low": "cheap", "high": "101"}});
    let bad = arm_renko(&mut window, &mut access, price).expect_err("not a price");
    assert!(bad.message.contains("`low`"), "{}", bad.message);
    let pane = arm_renko(&mut window, &mut access, json!({"pane_id": "99"})).expect_err("no chart");
    assert!(pane.message.contains("no such chart"), "{}", pane.message);
    let tab = arm_renko(&mut window, &mut access, json!({"tab_id": "7"})).expect_err("no tab");
    assert!(tab.message.contains("no open tab"), "{}", tab.message);
    let unknown =
        arm_renko(&mut window, &mut access, json!({"stake": 1})).expect_err("unknown field");
    assert!(unknown.message.contains("stake"), "{}", unknown.message);
    assert!(window.strategies.script_readings().is_empty());
    assert!(access.events.is_empty(), "a refusal journals nothing");
}

#[test]
fn a_region_placement_must_name_a_rectangle_on_that_chart() {
    let mut window = window();
    let mut access = FakeAccess::default();
    let gone = json!({"placement": {"kind": "region", "drawing_id": "40"}});
    let error = arm_renko(&mut window, &mut access, gone).expect_err("no such drawing");
    assert!(error.message.contains("drawing 40"), "{}", error.message);

    let rectangle = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle tool");
    for (bar, price) in [(0_u8, 100.0), (3, 102.0)] {
        let point = ChartPoint::at_time(f32::from(bar), price, Some(1_000 + i64::from(bar)));
        window.panes[0]
            .drawings
            .place_with(rectangle, &DrawingBand::Price, point, |tool| NewDrawing {
                style: DrawingStyle::default(),
                payload: tool.default_payload(),
            });
    }
    let drawn = window.panes[0].drawings.items()[0].id;
    let region = json!({"placement": {"kind": "region", "drawing_id": drawn.0.to_string()}});
    let armed = arm_renko(&mut window, &mut access, region).expect("a rectangle on the flow chart");
    assert_eq!(
        armed["strategy"]["placement"],
        json!({"kind": "region", "drawing_id": drawn.0.to_string()})
    );
    let elsewhere = json!({
        "pane_id": "6",
        "placement": {"kind": "region", "drawing_id": drawn.0.to_string()},
    });
    arm_renko(&mut window, &mut access, elsewhere).expect_err("the rectangle is not on that chart");
}

#[test]
fn an_unreadable_folder_lists_its_reason_and_a_missing_script_names_it() {
    let mut window = window();
    window.dir = Some(format!("{BUNDLED}/no-such-folder"));
    let mut access = FakeAccess::default();
    let listed = call(
        list::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({}),
    )
    .expect("the list still answers");
    assert_eq!(listed["library"]["found"], false);
    let reason = listed["library"]["error"].as_str().expect("labelled");
    assert!(reason.contains("strategy library folder"), "{reason}");
    assert!(reason.contains("no-such-folder"), "{reason}");
    let missing = arm_renko(&mut window, &mut access, json!({"script": "mine.pine"}))
        .expect_err("not bundled, and the folder is unreadable");
    assert!(missing.message.contains("mine.pine"), "{}", missing.message);
    assert!(
        missing.message.contains("no-such-folder"),
        "{}",
        missing.message
    );
}

#[test]
fn the_default_folder_absent_reads_as_no_folder() {
    let mut window = window();
    window.dir = None;
    let listed = call(
        list::<Window, FakeAccess>,
        &mut window,
        &mut FakeAccess::default(),
        json!({}),
    )
    .expect("answers");
    assert_eq!(listed["library"]["folder"], "strategies");
    assert_eq!(listed["library"]["found"], false);
    assert_eq!(listed["library"]["error"], Value::Null);
}

#[test]
fn an_unreadable_folder_still_lists_and_arms_the_bundled_scripts() {
    let mut window = window();
    window.dir = Some(format!("{BUNDLED}/no-such-folder"));
    let mut access = FakeAccess::default();
    let listed = call(
        list::<Window, FakeAccess>,
        &mut window,
        &mut access,
        json!({}),
    )
    .expect("the list still answers");
    let scripts = listed["library"]["scripts"].as_array().expect("names");
    assert!(
        scripts.contains(&json!("renko_poc_flip.pine")),
        "{scripts:?}"
    );
    arm_renko(&mut window, &mut access, json!({})).expect("the bundled flip arms");
}

#[test]
fn a_side_no_title_names_is_refused_with_the_titles() {
    let mut window = window();
    let mut access = FakeAccess::default();
    let source = "//@version=5\nindicator(\"t\")\nalertcondition(close > open, title=\"Long up\")\nplot(close)\n";
    let folder = crate::scratch::thread_dir("script-strategy-side");
    std::fs::write(folder.join("long_only.pine"), source).expect("writable");
    window.dir = Some(folder.to_str().expect("unicode").to_owned());
    let error = arm_renko(
        &mut window,
        &mut access,
        json!({"script": "long_only.pine", "side": "sell"}),
    )
    .expect_err("no title names sell");
    assert!(error.message.contains("`Long up`"), "{}", error.message);
}

#[test]
fn the_warmup_carries_each_closed_bar_s_footprint_poc() {
    use quantick_engine::{Side as TradeSide, Trade};
    let mut state = ChartState::new(BarSpec::Tick(2));
    state.set_footprint_enabled(true);
    let print = |id: u64, price: u32, quantity: u32| Trade {
        agg_id: id,
        timestamp_ms: 1_000 + i64::try_from(id).expect("small"),
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side: TradeSide::Buy,
    };
    // A down brick traded mostly at 100, then the first print of the next.
    for trade in [print(1, 101, 1), print(2, 100, 3), print(3, 100, 1)] {
        state.ingest_live(&trade);
    }
    let spec = SpecSelector::new(BarSpec::Tick(2));
    let series = PaneSeriesRead {
        history_prefix: &[],
        state: &state,
        spec: &spec,
    };
    let warm = footprint_warmup(&series, 5);
    assert_eq!(warm.len(), 1, "one closed brick");
    assert_eq!(warm[0].0.close, Decimal::from(100));
    let poc = warm[0].1.expect("the brick kept its footprint");
    assert!(
        poc >= Decimal::from(100) && poc < Decimal::from(101),
        "{poc}"
    );
}
