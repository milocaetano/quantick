//! Native chart and stdio MCP both use the window's conversation.
use super::*;
use crate::control::strategy_describe_contract::DescribeHost;
use crate::surfaces::strategy_describe::provider::{Settings, Worker};
use quantick_ui_model::strategy_describe::{Command, Intent, Phase};
use serde_json::{Value, json};
mod http;
mod mcp;

const SENTENCE: &str = "On this Renko chart, alert at brick close when the brick reverses color and its point of control moves in the same direction. For an up brick after a down brick, the POC must be higher. For a down brick after an up brick, the POC must be lower.";
const ORACLE: &str = include_str!("../../../scripts/renko_poc_flip.pine");
fn configured(endpoint: &str, key: &str) -> Worker {
    let dir = crate::scratch::ScratchDir::new("strategy-config");
    let path = dir.join("feeds.toml");
    let mut table: toml::Table = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
    let section = format!(
        "enabled=true\nprovider='openai-compatible'\nmodel='fixture'\nendpoint='{endpoint}'\n{key}"
    );
    table.insert(
        "strategy_describe".into(),
        toml::Value::Table(toml::from_str(&section).unwrap()),
    );
    std::fs::write(&path, toml::to_string(&table).unwrap()).unwrap();
    crate::config::load(Some(&path), |name| {
        (name == "TEST_KEY").then(|| "fixture-credential".into())
    })
    .unwrap()
    .2
}
fn app_with_worker(worker: Worker) -> QuantickApp {
    app_with_history_and_launch(
        0,
        AppLaunch {
            describe: worker,
            ..Default::default()
        },
    )
    .0
}
fn panels(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    run_frame(app, ctx);
    let pos = app
        .chrome
        .title_bar
        .control_rect(quantick_control_schema::window_menu::KEBAB_CONTROL)
        .unwrap()
        .center();
    click_sized(app, ctx, TEST_WINDOW, pos);
    let output = run_frame(app, ctx);
    click_sized(
        app,
        ctx,
        TEST_WINDOW,
        painted_text_center(&output, "Panels").unwrap(),
    );
    run_frame(app, ctx)
}
fn wait_phase(app: &mut QuantickApp, ctx: &egui::Context, phase: Phase) {
    let deadline = std::time::Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        run_frame(app, ctx);
        let status = &app.surfaces.strategy_describe.model_state.status;
        if status.phase == phase {
            break;
        }
        assert!(
            status.phase != Phase::Failed && std::time::Instant::now() < deadline,
            "waiting for {phase:?}: {status:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn grant(app: &mut QuantickApp) {
    grant_annotate_for_test(
        app,
        "all-reads,observe.user_text,annotate,annotate.script,annotate.strategy_describe",
    );
}
#[test]
fn config_gates_entry_and_requests() {
    let fixture = http::provider(vec![]);
    let disabled: Settings = toml::from_str("enabled=false\nkey='fixture-credential'").unwrap();
    for worker in [
        Worker::default(),
        Worker::configured(disabled, |_| panic!("disabled settings resolved env")).unwrap(),
        configured(&fixture.endpoint, "key_env='MISSING'"),
        configured(&fixture.endpoint, "key='   '"),
        configured(&fixture.endpoint, ""),
    ] {
        let mut app = app_with_worker(worker);
        let ctx = egui::Context::default();
        let view = panels(&mut app, &ctx);
        assert!(painted_text_center(&view, "Describe a strategy...").is_none());
        let refused = app
            .dispatch(Intent::Command(Command::Submit {
                revision: 0,
                text: "ring".into(),
            }))
            .unwrap_err();
        assert_eq!(
            refused.context.details.unwrap()["reason_code"],
            "strategy.describe_unavailable"
        );
    }
    let mut app = app_with_worker(Worker::default());
    let ctx = egui::Context::default();
    grant(&mut app);
    let directory = gateway_test_directory("describe-disabled");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut client = mcp::Client::start(&mut app, &ctx, &directory, "annotator");
    client.availability(&mut app, &ctx, false);
    let result = client.call(&mut app, &ctx, "read", json!({}), "");
    assert_eq!(result["result"]["phase"], "unavailable", "{result}");
    let refused = client.call(
        &mut app,
        &ctx,
        "submit",
        json!({"revision":"0","text":"ring"}),
        "disabled",
    );
    assert_eq!(
        refused["error"]["code"], "control.capability_unavailable",
        "{refused}"
    );
    assert!(
        refused
            .to_string()
            .contains("strategy.describe_unavailable")
    );
    let mut enabled = app_with_worker(configured(&fixture.endpoint, "key_env='TEST_KEY'"));
    let view = panels(&mut enabled, &egui::Context::default());
    assert!(painted_text_center(&view, "Describe a strategy...").is_some());
    assert!(fixture.requests.lock().unwrap().is_empty());
}

#[test]
fn menu_open_rejection_is_visible_and_inert() {
    let fixture = http::provider(vec![]);
    let mut app = app_with_worker(configured(&fixture.endpoint, "key='ETHUSDT'"));
    let dir = crate::scratch::ScratchDir::new("describe-rejected-open");
    app.surfaces.strategy_describe.view.library = Some(dir.to_path_buf());
    let ctx = egui::Context::default();
    let click_entry = |app: &mut QuantickApp| {
        let menu = panels(app, &ctx);
        let entry = painted_text_center(&menu, "Describe a strategy...").unwrap();
        click_sized(app, &ctx, TEST_WINDOW, entry);
        // A newly opened egui area takes a layout pass before its text paints.
        run_frame(app, &ctx);
        run_frame(app, &ctx)
    };

    let accepted = click_entry(&mut app);
    assert!(painted_text_center(&accepted, "Send").is_some());
    let opened = app.dispatch(Intent::Command(Command::Read)).unwrap();
    assert_eq!(opened.phase, Phase::Ready);
    let prior = app
        .dispatch(Intent::Command(Command::Cancel {
            revision: opened.revision,
        }))
        .unwrap();
    let _market = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let rejected = click_entry(&mut app);
    let message = "Input contains the provider credential";
    assert_eq!(app.surfaces.toast.model_state.message(), Some(message));
    assert!(painted_text_center(&rejected, message).is_some());
    assert_eq!(app.dispatch(Intent::Command(Command::Read)).unwrap(), prior);

    assert!(fixture.requests.lock().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    assert!(
        app.active_tab()
            .market
            .strategies
            .script_readings()
            .is_empty()
    );
}

#[test]
fn keyboard_shortcuts_remain_available_outside_the_focused_chat_edit() {
    let fixture = http::provider(vec![]);
    let mut app = app_with_worker(configured(&fixture.endpoint, "key='fixture-credential'"));
    let ctx = egui::Context::default();
    app.dispatch(Intent::Command(Command::Open)).unwrap();
    run_frame(&mut app, &ctx);
    let field = egui::Id::new("strategy_describe").with("message");
    ctx.memory_mut(|memory| memory.request_focus(field));
    let output = run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::Text("Ring at close".into()),
            key_press(egui::Key::F10),
        ],
    );
    assert!(painted_text_center(&output, "Ring at close").is_some());
    assert!(
        !app.chrome.window_menu.open,
        "typing must shield global shortcuts"
    );

    ctx.memory_mut(|memory| memory.surrender_focus(field));
    run_frame(&mut app, &ctx);
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::F10)]);
    assert!(
        app.chrome.window_menu.open,
        "unfocused chat must preserve chart shortcuts"
    );
}

#[test]
fn literal_key_is_rejected_at_native_send_and_chart_capture_without_losing_the_chat() {
    let fixture = http::provider(vec![]);
    let mut app = app_with_worker(configured(&fixture.endpoint, "key='fixture-credential'"));
    let ctx = egui::Context::default();
    let output = panels(&mut app, &ctx);
    let entry = painted_text_center(&output, "Describe a strategy...").unwrap();
    click_sized(&mut app, &ctx, TEST_WINDOW, entry);
    let clean = app.dispatch(Intent::Command(Command::Read)).unwrap();
    assert_eq!(clean.phase, Phase::Ready);
    run_frame(&mut app, &ctx);
    ctx.memory_mut(|m| m.request_focus(egui::Id::new("strategy_describe").with("message")));
    let output = run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::Text("pasted fixture-credential".into())],
    );
    let send = painted_text_center(&output, "Send").unwrap();
    click_sized(&mut app, &ctx, TEST_WINDOW, send);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.surfaces.toast.model_state.message(),
        Some("Input contains the provider credential")
    );
    assert_eq!(app.dispatch(Intent::Command(Command::Read)).unwrap(), clean);

    let original = std::mem::replace(
        &mut app.active_tab_mut().active.1,
        "fixture-credential".into(),
    );
    let refused = app.dispatch(Intent::Command(Command::Open)).unwrap_err();
    assert!(!format!("{refused:?}").contains("fixture-credential"));
    assert_eq!(app.surfaces.strategy_describe.model_state.status, clean);
    app.active_tab_mut().active.1 = original;
    assert_eq!(app.dispatch(Intent::Command(Command::Read)).unwrap(), clean);
    assert!(fixture.requests.lock().unwrap().is_empty());
}

#[test]
fn sibling_presentation_keeps_pending_work_and_transferring_the_tab_rebinds_its_dialog() {
    use super::super::native_windows;
    use quantick_ui_model::strategy_describe::{Effects, Reply};
    use quantick_workspace::arrangement::{
        TabId,
        windows::{TransferTarget, WindowId},
    };
    let fixture = http::provider(vec![]);
    let mut app = app_with_worker(configured(&fixture.endpoint, "key='fixture-credential'"));
    let ctx = egui::Context::default();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let pinned = TabId(app.runtime.tabs.active_id());
    let opened = app.dispatch(Intent::Command(Command::Open)).unwrap();
    // Hold the issued effect at the transport boundary until after a focus change.
    let effect = quantick_ui_model::strategy_describe::update(
        &mut app.surfaces.strategy_describe.model_state,
        Intent::Command(Command::Submit {
            revision: opened.revision,
            text: "Alert at close".into(),
        }),
    );
    let Effects::Request(request) = effect else {
        panic!("expected a pending request")
    };
    let child = app
        .runtime
        .tabs
        .transfer_tab(pinned, TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    let paint = |app: &mut QuantickApp, window, events| {
        let id = native_windows::viewport(window);
        let mut input = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
            events,
            ..Default::default()
        };
        input.viewports.entry(id).or_default();
        let now = frame_instant(&ctx);
        ctx.run(input, |ctx| native_windows::paint(app, ctx, window, now))
    };
    paint(&mut app, child, vec![]);
    let dialog = paint(&mut app, child, vec![]);
    assert!(painted_text_center(&dialog, "Waiting for the provider").is_some());
    let sibling = paint(&mut app, WindowId::ROOT, vec![key_press(egui::Key::F10)]);
    assert!(painted_text_center(&sibling, "Waiting for the provider").is_none());
    assert!(
        app.chrome.window_menu.open,
        "sibling chart keeps its shortcut"
    );
    let read = app.dispatch(Intent::Command(Command::Read)).unwrap();
    assert_eq!(
        (read.phase, read.revision),
        (Phase::Waiting, request.revision)
    );
    let root = WindowId::ROOT;
    app.runtime
        .tabs
        .transfer_tab(pinned, TransferTarget::Window(root), 0)
        .unwrap();
    paint(&mut app, root, vec![]);
    assert_eq!(
        app.surfaces.strategy_describe.view.viewport,
        native_windows::viewport(root)
    );
    let child = app
        .runtime
        .tabs
        .transfer_tab(pinned, TransferTarget::NewWindow, 0)
        .unwrap()
        .destination;
    paint(&mut app, child, vec![]);
    app.runtime.tabs.focus_window(root).unwrap();
    let cancelled = app.dispatch(Intent::Command(Command::Read)).unwrap();
    assert_eq!(cancelled.phase, Phase::Cancelled);
    let late = app
        .dispatch(Intent::Replied {
            revision: request.revision,
            reply: Ok(Reply::Question("Late".into())),
        })
        .unwrap();
    assert_eq!(late, cancelled);
    assert!(fixture.requests.lock().unwrap().is_empty());
}

fn seed_renko(app: &mut QuantickApp) {
    let pane = app.active_tab_mut().focused_pane_mut();
    pane.set_spec(
        quantick_engine::bar_registry::BUILTIN_BARS
            .parse("renko:3")
            .unwrap(),
    );
    pane.series.state.set_footprint_group(Decimal::ONE);
    pane.series.state.set_footprint_enabled(true);
    let mut levels: Vec<(i64, i64)> = (0..70).map(|i| (100 + i % 2, 1)).collect();
    levels.extend([
        (99, 1),
        (102, 1000),
        (105, 1),
        (106, 1000),
        (107, 1),
        (103, 1000),
        (101, 1),
    ]);
    for (i, (price, quantity)) in levels.into_iter().enumerate() {
        let mut print = trade(i as u64 + 1);
        print.price = price.into();
        print.quantity = quantity.into();
        pane.ingest_live_trade(&print);
    }
    assert_eq!(pane.closed_slots(), 4);
    assert!(pane.series.state.partial().is_some());
}
#[test]
fn renko_poc_flip_chat_to_alarm() {
    let fixture = http::provider(vec![
        json!({"question":"Both directions?"}),
        json!({"source":"indicator(\"incomplete\")\nx = missing_variable","summary":"Draft"}),
        json!({"source":ORACLE,"summary":"Ring on a POC-confirmed brick reversal."}),
    ]);
    let mut app = app_with_worker(configured(&fixture.endpoint, "key='fixture-credential'"));
    let dir = crate::scratch::ScratchDir::new("describe-renko");
    app.surfaces.strategy_describe.view.library = Some(dir.to_path_buf());
    seed_renko(&mut app);
    let ctx = egui::Context::default();
    let menu = panels(&mut app, &ctx);
    click_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        painted_text_center(&menu, "Describe a strategy...").unwrap(),
    );
    let revision = app.surfaces.strategy_describe.model_state.status.revision;
    app.dispatch(Intent::Command(Command::Submit {
        revision,
        text: SENTENCE.into(),
    }))
    .unwrap();
    wait_phase(&mut app, &ctx, Phase::Question);
    let revision = app.surfaces.strategy_describe.model_state.status.revision;
    app.dispatch(Intent::Command(Command::Submit {
        revision,
        text: "Yes, both.".into(),
    }))
    .unwrap();
    wait_phase(&mut app, &ctx, Phase::Preview);
    let state = &app.surfaces.strategy_describe.model_state;
    assert_eq!(state.context().unwrap().history.len(), 4);
    let status = state.status.clone();
    let markers = &status.draft.as_ref().unwrap().markers;
    assert_eq!(
        markers.iter().map(|m| (m.slot, m.side)).collect::<Vec<_>>(),
        [
            (1, quantick_engine::Side::Buy),
            (3, quantick_engine::Side::Sell)
        ]
    );
    let mut tail = trade(100);
    tail.price = Decimal::from(97);
    app.active_tab_mut()
        .focused_pane_mut()
        .ingest_live_trade(&tail);
    assert!(app.active_tab().focused_pane().closed_slots() > 4);
    assert_eq!(
        app.dispatch(Intent::Command(Command::Read)).unwrap(),
        status
    );
    let cmd = Command::Save {
        revision: status.revision,
        id: "confirmed.pine".into(),
    };
    let saved = app.dispatch(Intent::Command(cmd.clone())).unwrap();
    assert_eq!(saved.phase, Phase::Saved, "{saved:?}");
    assert_eq!(app.dispatch(Intent::Command(cmd)).unwrap(), saved);
    let alarms = app.active_tab().market.strategies.script_readings();
    assert_eq!(alarms.len(), 2);
    assert!(
        alarms
            .iter()
            .all(|a| a.script == "confirmed.pine" && a.disarm_reason.is_none())
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("confirmed.pine")).unwrap(),
        ORACLE
    );
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    for request in requests.iter() {
        assert_eq!(request["model"], "fixture");
        assert!(request.to_string().contains(SENTENCE));
        assert!(!request.to_string().contains("fixture-credential"));
        assert_eq!(request["messages"][1], requests[0]["messages"][1]);
    }
    let correction = requests[2]["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_str()
        .unwrap();
    assert!(
        correction.contains("PINE_")
            && correction.contains("line")
            && correction.contains("missing_variable")
    );
}

#[test]
fn save_failure_does_not_register_alarms_or_retry_a_file_write() {
    for collision in [true, false] {
        let fixture = http::provider(vec![
            json!({"source":ORACLE,"summary":"Confirmed reversal"}),
        ]);
        let mut app = app_with_worker(configured(&fixture.endpoint, "key='fixture-credential'"));
        let dir = crate::scratch::ScratchDir::new("describe-failed-save");
        let file = dir.join("occupied.pine");
        std::fs::write(&file, "original source").unwrap();
        app.surfaces.strategy_describe.view.library = Some(if collision {
            dir.to_path_buf()
        } else {
            file.clone()
        });
        let revision = app
            .dispatch(Intent::Command(Command::Open))
            .unwrap()
            .revision;
        app.dispatch(Intent::Command(Command::Submit {
            revision,
            text: "Ring on reversal".into(),
        }))
        .unwrap();
        wait_phase(&mut app, &egui::Context::default(), Phase::Preview);
        let cmd = Command::Save {
            revision: app.surfaces.strategy_describe.model_state.status.revision,
            id: "occupied.pine".into(),
        };
        let result = app.dispatch(Intent::Command(cmd.clone())).unwrap();
        assert_eq!(result.phase, Phase::Failed);
        assert!(!result.message.is_empty());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original source");
        std::fs::remove_file(&file).unwrap();
        assert_eq!(app.dispatch(Intent::Command(cmd)).unwrap(), result);
        assert!(
            app.active_tab()
                .market
                .strategies
                .script_readings()
                .is_empty()
        );
    }
}

#[test]
fn named_calls_share_ui_state_and_retry_safely() {
    let fixture = http::provider(vec![json!({"source":ORACLE,"summary":"Alarm preview"})]);
    let mut app = app_with_worker(configured(&fixture.endpoint, "key_env='TEST_KEY'"));
    let dir = crate::scratch::ScratchDir::new("describe-mcp");
    app.surfaces.strategy_describe.view.library = Some(dir.to_path_buf());
    let ctx = egui::Context::default();
    let directory = gateway_test_directory("describe-authorized");
    grant(&mut app);
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut denied = mcp::Client::start(&mut app, &ctx, &directory, "observer");
    for (op, payload) in [
        ("open", json!({})),
        ("read", json!({})),
        ("submit", json!({"revision":"0","text":"alarm"})),
        ("cancel", json!({"revision":"0"})),
        ("save", json!({"revision":"0","id":"my.pine"})),
    ] {
        let result = denied.call(&mut app, &ctx, op, payload, op);
        assert_eq!(
            result["error"]["code"], "control.permission_denied",
            "{op}: {result}"
        );
    }
    drop(denied);
    let mut client = mcp::Client::start(&mut app, &ctx, &directory, "annotator");
    client.availability(&mut app, &ctx, true);
    let opened = client.call(&mut app, &ctx, "open", json!({}), "open");
    assert_eq!(opened["result"]["phase"], "ready", "{opened}");
    let revision = opened["result"]["revision"].clone();
    let sensitive = json!({"revision":revision,"text":"pasted fixture-credential"});
    let refused = client.call(
        &mut app,
        &ctx,
        "submit",
        sensitive.clone(),
        "rejected-submit",
    );
    assert_eq!(
        refused["error"]["message"],
        "Input contains the provider credential"
    );
    assert!(!refused.to_string().contains("fixture-credential"));
    assert_eq!(
        client.call(&mut app, &ctx, "submit", sensitive, "rejected-submit")["error"],
        refused["error"]
    );
    assert_eq!(
        client.call(&mut app, &ctx, "read", json!({}), "")["result"],
        opened["result"]
    );
    assert!(fixture.requests.lock().unwrap().is_empty());
    let input = json!({"revision":revision,"text":"Alert on a confirmed reversal"});
    let sent = client.call(&mut app, &ctx, "submit", input.clone(), "send");
    assert_eq!(sent["result"]["phase"], "waiting", "{sent}");
    assert_eq!(
        client.call(&mut app, &ctx, "submit", input, "send")["result"],
        sent["result"]
    );
    wait_phase(&mut app, &ctx, Phase::Preview);
    let read = client.call(&mut app, &ctx, "read", json!({}), "");
    assert_eq!(
        read["result"]["summary"],
        app.surfaces
            .strategy_describe
            .model_state
            .status
            .draft
            .as_ref()
            .unwrap()
            .summary
    );
    let sensitive = json!({"revision":read["result"]["revision"],"id":"fixture-credential.pine"});
    let refused = client.call(&mut app, &ctx, "save", sensitive, "rejected-save");
    assert_eq!(
        refused["error"]["message"],
        "Input contains the provider credential"
    );
    assert!(!refused.to_string().contains("fixture-credential"));
    assert_eq!(
        client.call(&mut app, &ctx, "read", json!({}), "")["result"],
        read["result"]
    );
    assert!(
        app.active_tab()
            .market
            .strategies
            .script_readings()
            .is_empty()
    );
    assert!(!dir.join("fixture-credential.pine").exists());
    let input = json!({"revision":read["result"]["revision"],"id":"mcp.pine"});
    let saved = client.call(&mut app, &ctx, "save", input.clone(), "save");
    assert_eq!(saved["result"]["saved_id"], "mcp.pine", "{saved}");
    assert_eq!(
        client.call(&mut app, &ctx, "save", input, "save")["result"],
        saved["result"]
    );
    let opened = client.call(&mut app, &ctx, "open", json!({}), "another");
    let cancel = json!({"revision":opened["result"]["revision"]});
    let cancelled = client.call(&mut app, &ctx, "cancel", cancel.clone(), "cancel");
    assert_eq!(cancelled["result"]["phase"], "cancelled");
    assert_eq!(
        client.call(&mut app, &ctx, "cancel", cancel, "cancel")["result"],
        cancelled["result"]
    );
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    assert_eq!(
        app.active_tab().market.strategies.script_readings().len(),
        2
    );
}
