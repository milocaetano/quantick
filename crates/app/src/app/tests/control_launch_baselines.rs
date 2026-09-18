//! Literal control launch inputs: each case is a hook table fed through
//! `fixed_env`, so every case runs in `cargo test` and none reads the
//! process environment.
use super::*;
use quantick_feed::replay::test_support as replay_test_support;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tracing_subscriber::prelude::*;

#[derive(Clone, Default)]
pub(super) struct RecordedEvents(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for RecordedEvents {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
}

impl RecordedEvents {
    fn named(&self, code: &str) -> Vec<BTreeMap<String, String>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|fields| fields.get("event_code").is_some_and(|value| value == code))
            .cloned()
            .collect()
    }
}

fn local_read(app: &mut QuantickApp, name: &str, input: Value) -> Value {
    let mut access = app.control.control_access.take().unwrap();
    let result = access.invoke_local_read(app, name, input);
    app.control.control_access = Some(access);
    result.unwrap()
}

fn description(app: &mut QuantickApp) -> Value {
    local_read(app, crate::control::DESCRIBE_CAPABILITY_ID, json!({}))
}

type Env = &'static [(&'static str, &'static str)];

fn captured_control_launch(env: Env) -> super::super::AppLaunch {
    super::super::AppLaunch {
        control: super::super::control_host::ControlLaunch::capture(fixed_env(env)),
        ..Default::default()
    }
}

const CONSTRUCTOR_CASES: &[(&str, Env)] = &[
    ("C01", &[]),
    ("C02", &[("QUANTICK_CONTROL_PANEL", "1")]),
    ("panel-padded", &[("QUANTICK_CONTROL_PANEL", " 1 ")]),
    ("panel-zero", &[("QUANTICK_CONTROL_PANEL", "0")]),
    ("C05", &[("QUANTICK_CONTROL_ACCESS", "1")]),
    ("access-padded", &[("QUANTICK_CONTROL_ACCESS", " 1")]),
    ("access-word", &[("QUANTICK_CONTROL_ACCESS", "true")]),
    ("C08", &[("QUANTICK_CONTROL_MARK", "1")]),
    ("C09", &[("QUANTICK_CONTROL_MARK", " 1 ")]),
    ("C10", &[("QUANTICK_CONTROL_MARK", " keep this ")]),
    ("mark-blank", &[("QUANTICK_CONTROL_MARK", "   ")]),
    ("C12", &[("QUANTICK_CONTROL_ANNOTATE", " keep this ")]),
    ("C13", &[("QUANTICK_CONTROL_NOTIFY", " popup :a:b ")]),
    (
        "C14",
        &[("QUANTICK_CONTROL_EVIDENCE", " all, scene.controls ,all ")],
    ),
    (
        "blank-actions",
        &[
            ("QUANTICK_CONTROL_ANNOTATE", " "),
            ("QUANTICK_CONTROL_NOTIFY", ""),
            ("QUANTICK_CONTROL_EVIDENCE", "  "),
        ],
    ),
    ("C18", &[("QUANTICK_CONTROL_SCOPES", "annotate-tier")]),
    (
        "C19",
        &[("QUANTICK_CONTROL_SCOPES", "observe.chart,not.a.scope")],
    ),
    ("C20", &[("QUANTICK_CONTROL_SCOPES", " , ")]),
    (
        "C22",
        &[
            ("QUANTICK_CONTROL_PANEL", "1"),
            ("QUANTICK_CONTROL_ACCESS", "1"),
            ("QUANTICK_CONTROL_MARK", "phase"),
            ("QUANTICK_CONTROL_ANNOTATE", "alpha"),
            ("QUANTICK_CONTROL_NOTIFY", "toast:beta"),
            ("QUANTICK_CONTROL_EVIDENCE", "all"),
        ],
    ),
];

const DEFAULT_GRANT: &[&str] = &[
    "observe",
    "observe.attention",
    "observe.chart",
    "observe.drawings",
    "observe.events",
    "observe.health",
    "observe.indicators",
    "observe.market",
    "observe.orderflow",
    "observe.replay",
    "observe.system",
    "observe.workspace",
];
const ANNOTATE_GRANT: &[&str] = &[
    "annotate",
    "annotate.attention",
    "annotate.chart",
    "annotate.notification",
    "annotate.script",
    "annotate.sound",
    "observe",
];

#[test]
fn literal_constructor() {
    for &(case, env) in CONSTRUCTOR_CASES {
        constructor_case(case, env);
    }
}

fn constructor_case(case: &str, env: Env) {
    eprintln!("control constructor case {case}");
    let recorded = RecordedEvents::default();
    let subscriber = tracing_subscriber::registry().with(recorded.clone());
    tracing::subscriber::with_default(subscriber, || {
        let (mut app, _, _, _) = test_app_with_launch(captured_control_launch(env));
        let panel = matches!(case, "C02" | "C22");
        let enable = matches!(case, "C05" | "C22");
        let mark = match case {
            "C08" => Some(""),
            "C09" => Some(" 1 "),
            "C10" => Some(" keep this "),
            "C22" => Some("phase"),
            _ => None,
        };
        let annotation = match case {
            "C12" => Some(" keep this "),
            "C22" => Some("alpha"),
            _ => None,
        };
        let notification = match case {
            "C13" => Some(" popup :a:b "),
            "C22" => Some("toast:beta"),
            _ => None,
        };
        let evidence = match case {
            "C14" => Some(" all, scene.controls ,all "),
            "C22" => Some("all"),
            _ => None,
        };
        assert_eq!(app.control.scenarios.pending().enable, enable);
        assert_eq!(app.control.scenarios.pending().mark, mark);
        assert_eq!(app.control.scenarios.pending().annotation, annotation);
        assert_eq!(app.control.scenarios.pending().notification, notification);
        assert_eq!(app.control.scenarios.pending().evidence, evidence);
        let described = description(&mut app);
        let grant = match case {
            "C18" => ANNOTATE_GRANT,
            "C20" => &["observe"],
            _ => DEFAULT_GRANT,
        };
        assert_eq!(described["effective_scopes"], json!(grant));
        assert_eq!(
            described["effective_profile"],
            if case == "C18" {
                "annotator"
            } else {
                "observer"
            }
        );
        assert_eq!(
            recorded.named("CONTROL_SCOPE_HOOK_REFUSED").len(),
            usize::from(case == "C19")
        );
        assert!(app.active_tab().drawing_pane().drawings.items().is_empty());
        let access = app.control.control_access.as_mut().unwrap();
        assert!(!access.is_enabled());
        assert!(!access.needs_frame_service());
        assert_eq!(access.retained_evidence_for_test(), 0);
        assert!(access.journal().read(1, 64, 1 << 20).events.is_empty());
        // Paint only the access panel: a full app frame would consume ACCESS.
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| access.draw_panel(ctx));
        assert_eq!(
            ctx.memory(|memory| memory
                .area_rect(egui::Id::new("control_access_panel"))
                .is_some()),
            panel
        );
        assert!(!access.needs_frame_service());
    });
}

fn events(app: &QuantickApp) -> Vec<Value> {
    app.control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 20)
        .events
        .into_iter()
        .map(|event| serde_json::to_value(event).unwrap())
        .collect()
}

pub(super) fn hook_bundle(app: &mut QuantickApp, recorded: &RecordedEvents) -> Value {
    let captures = recorded.named("CONTROL_EVIDENCE_CAPTURED");
    let id = &captures.last().expect("the actual hook logged a capture")["evidence_id"];
    let mut bytes = Vec::new();
    let mut cursor = Value::Null;
    loop {
        let mut input = json!({"evidence_id": id.trim_matches('"')});
        if !cursor.is_null() {
            input["cursor"] = cursor;
        }
        let page = local_read(app, "evidence.read", input);
        for chunk in page["page"]["items"].as_array().unwrap() {
            bytes.extend(decode_wire_base64(chunk["data"].as_str().unwrap()));
        }
        cursor = page["page"]["next_cursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    serde_json::from_slice(&bytes).unwrap()
}

const ALL_READS: (&str, &str) = ("QUANTICK_CONTROL_SCOPES", "all-reads");
const EVIDENCE_READS: (&str, &str) = ("QUANTICK_CONTROL_SCOPES", "all-reads,observe.evidence");
const SCREENSHOT_READS: (&str, &str) = (
    "QUANTICK_CONTROL_SCOPES",
    "all-reads,observe.evidence,observe.screenshot",
);
const EVIDENCE_WITH_IMAGE: (&str, &str) = ("QUANTICK_CONTROL_EVIDENCE", "all,screenshot");

const RECIPIENT_CASES: &[(&str, Env)] = &[
    ("F01", &[("QUANTICK_CONTROL_ACCESS", "1")]),
    ("F03-empty", &[("QUANTICK_CONTROL_MARK", "1")]),
    ("F03-padded", &[("QUANTICK_CONTROL_MARK", " 1 ")]),
    ("F04", &[("QUANTICK_CONTROL_MARK", "after-sidecar")]),
    ("F05", &[("QUANTICK_CONTROL_ANNOTATE", " alpha ")]),
    ("F06", &[("QUANTICK_CONTROL_ANNOTATE", "   ")]),
    ("F07-toast", &[("QUANTICK_CONTROL_NOTIFY", "toast:message")]),
    (
        "F07-popup",
        &[("QUANTICK_CONTROL_NOTIFY", "popup:hello:again")],
    ),
    ("F07-sound", &[("QUANTICK_CONTROL_NOTIFY", "sound:beep")]),
    ("F07-invalid", &[("QUANTICK_CONTROL_NOTIFY", "shout:loud")]),
    (
        "F08",
        &[EVIDENCE_READS, ("QUANTICK_CONTROL_EVIDENCE", "all")],
    ),
    (
        "F09-all",
        &[EVIDENCE_READS, ("QUANTICK_CONTROL_EVIDENCE", "all")],
    ),
    (
        "F09-empty",
        &[EVIDENCE_READS, ("QUANTICK_CONTROL_EVIDENCE", " all , ")],
    ),
    ("F10", &[EVIDENCE_READS, EVIDENCE_WITH_IMAGE]),
    ("F11", &[SCREENSHOT_READS, EVIDENCE_WITH_IMAGE]),
    ("F12", &[SCREENSHOT_READS, EVIDENCE_WITH_IMAGE]),
    ("F13", &[SCREENSHOT_READS, EVIDENCE_WITH_IMAGE]),
    ("F14", &[SCREENSHOT_READS, EVIDENCE_WITH_IMAGE]),
    (
        "F15-refusal",
        &[
            ALL_READS,
            ("QUANTICK_CONTROL_MARK", "phase"),
            ("QUANTICK_CONTROL_ANNOTATE", "alpha"),
            ("QUANTICK_CONTROL_NOTIFY", "toast:beta"),
            ("QUANTICK_CONTROL_EVIDENCE", "all"),
        ],
    ),
    (
        "F15-bundle",
        &[
            EVIDENCE_READS,
            ("QUANTICK_CONTROL_MARK", "phase"),
            ("QUANTICK_CONTROL_ANNOTATE", "alpha"),
            ("QUANTICK_CONTROL_NOTIFY", "toast:beta"),
            ("QUANTICK_CONTROL_EVIDENCE", "all"),
        ],
    ),
];

#[test]
fn literal_recipient() {
    for &(case, env) in RECIPIENT_CASES {
        recipient_case(case, env);
    }
}

/// The combined case enables real local access: it publishes a descriptor in
/// the host's own runtime discovery directory and asserts that directory's
/// inventory is unchanged afterwards, which a live Quantick instance or a
/// parallel test publishing beside it would break. Run it alone, on a host
/// with no instance open:
/// `cargo test -p quantick-app literal_recipient_owned_instance -- --ignored`.
#[test]
#[ignore = "publishes into the host's real runtime discovery directory"]
fn literal_recipient_owned_instance() {
    const OWNED: Env = &[
        ("QUANTICK_CONTROL_ACCESS", "1"),
        ALL_READS,
        ("QUANTICK_CONTROL_MARK", "phase"),
        ("QUANTICK_CONTROL_ANNOTATE", "alpha"),
        ("QUANTICK_CONTROL_NOTIFY", "toast:beta"),
        ("QUANTICK_CONTROL_EVIDENCE", "all"),
    ];
    recipient_case("F02-F15", OWNED);
}

fn recipient_case(case: &str, env: Env) {
    eprintln!("control recipient case {case}");
    let recorded = RecordedEvents::default();
    let subscriber = tracing_subscriber::registry().with(recorded.clone());
    tracing::subscriber::with_default(subscriber, || recipient(case, env, &recorded));
}

fn recipient(case: &str, env: Env, recorded: &RecordedEvents) {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history_and_launch(8, captured_control_launch(env));
    match case {
        "F02-F15" => combined_owned_instance(&mut app, &ctx, recorded),
        "F15-refusal" => {
            assert!(!app.control.scenarios.pending().enable);
            run_frame(&mut app, &ctx);
            assert_combined_actions(&app);
            assert_eq!(recorded.named("CONTROL_EVIDENCE_HOOK_REFUSED").len(), 1);
            assert!(recorded.named("CONTROL_EVIDENCE_CAPTURED").is_empty());
            assert!(
                app.control
                    .control_access
                    .as_ref()
                    .unwrap()
                    .is_disabled_for_test()
            );
        }
        "F15-bundle" => {
            assert!(!app.control.scenarios.pending().enable);
            run_frame(&mut app, &ctx);
            let bundle = hook_bundle(&mut app, recorded);
            let expected_scopes: Vec<_> = app
                .control
                .control_access
                .as_ref()
                .unwrap()
                .readable_scopes()
                .into_iter()
                .map(|scope| scope.to_string())
                .collect();
            let actual_scopes: Vec<_> = bundle["snapshot"]["scopes"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            assert_eq!(actual_scopes, expected_scopes);
            let journal = bundle["events"]["events"].as_array().unwrap();
            assert_eq!(
                journal
                    .iter()
                    .filter(|event| event["kind"] == "attention.mark.created")
                    .count(),
                1
            );
            assert_eq!(
                journal
                    .iter()
                    .filter(|event| event["module_id"] == "notify")
                    .count(),
                1
            );
            assert_eq!(app.active_tab().drawing_pane().drawings.items().len(), 1);
            let mark = journal
                .iter()
                .position(|event| event["kind"] == "attention.mark.created")
                .unwrap();
            let annotation = journal
                .iter()
                .position(|event| event["kind"] == "annotate.object.created")
                .unwrap();
            let notification = journal
                .iter()
                .position(|event| event["module_id"] == "notify")
                .unwrap();
            assert!(mark < annotation && annotation < notification);
            assert_eq!(journal[mark]["payload"]["note"]["redacted"], true);
            assert_eq!(
                journal[notification]["payload"]["message"]["redacted"],
                true
            );
            let projected = &bundle["snapshot"]["scopes"]["analysis.drawings"]["value"]["tabs"][0]
                ["panes"][0]["drawings"];
            assert_eq!(projected.as_array().unwrap().len(), 1);
            assert_eq!(
                projected[0]["drawing_id"],
                app.active_tab().drawing_pane().drawings.items()[0]
                    .id
                    .0
                    .to_string()
            );
            assert!(!bundle.to_string().contains("alpha"));
            assert!(!bundle.to_string().contains("beta"));
            assert!(
                app.control
                    .control_access
                    .as_ref()
                    .unwrap()
                    .is_disabled_for_test()
            );
        }
        "F04" => {
            let directory = crate::scratch::ScratchDir::new("control7-trace-order");
            let session = recording_at(&directory);
            let (mut seed, _) = app_with_history_and_launch(8, captured_control_launch(env));
            seed.active_tab_mut().replay =
                Some(replay_test_support::detached_link(session.clone()));
            seed.take_mark(Some("sidecar-first".to_owned()));
            drop(seed);
            app.active_tab_mut().replay = Some(replay_test_support::detached_link(session));
            run_frame(&mut app, &ctx);
            let marks: Vec<_> = events(&app)
                .into_iter()
                .filter(|event| event["kind"] == "attention.mark.created")
                .collect();
            assert_eq!(marks.len(), 2);
            assert_eq!(marks[0]["payload"]["note"], "sidecar-first");
            assert_eq!(marks[1]["payload"]["note"], "after-sidecar");
            assert_eq!(marks[1]["actor"]["kind"], "human_ui");
            run_frame(&mut app, &ctx);
            assert_eq!(
                events(&app)
                    .iter()
                    .filter(|event| event["kind"] == "attention.mark.created")
                    .count(),
                2
            );
        }
        "F05" => {
            let (mut empty, sender, _commands, _book) =
                test_app_with_launch(captured_control_launch(env));
            assert_eq!(empty.active_tab().drawing_pane().slots(), 0);
            run_frame(&mut empty, &ctx);
            assert_eq!(
                empty.control.scenarios.pending().annotation,
                Some(" alpha ")
            );
            assert!(
                empty
                    .active_tab()
                    .drawing_pane()
                    .drawings
                    .items()
                    .is_empty()
            );
            empty
                .active_tab_mut()
                .flow_pane
                .spec
                .retain(crate::state::BarSpec::Tick(1));
            empty.active_tab_mut().apply_spec_changes();
            empty.active_tab_mut().apply_spec_changes();
            sender
                .try_send(FeedEvent::Backfilled((1..=8).map(trade).collect()))
                .unwrap();
            run_frame(&mut empty, &ctx);
            assert!(empty.control.scenarios.pending().annotation.is_none());
            let drawings = empty.active_tab().drawing_pane().drawings.items();
            assert_eq!(drawings.len(), 1);
            assert_eq!(drawings[0].author.as_ref().unwrap().actor_kind, "agent");
            assert_eq!(
                drawings[0].tool.inline_text(drawings[0].payload.as_ref()),
                Some(" alpha ")
            );
            assert_eq!(drawings[0].points.len(), 1);
            assert_eq!(drawings[0].points[0].price, 100.8);
            assert_eq!(drawings[0].points[0].time_ms, Some(1800));
            assert_eq!(drawings[0].points[0].bar, 7.5);
            run_frame(&mut empty, &ctx);
            assert_eq!(empty.active_tab().drawing_pane().drawings.items().len(), 1);
        }
        "F06" => {
            assert!(app.control.scenarios.pending().annotation.is_none());
            run_frame(&mut app, &ctx);
            assert!(app.active_tab().drawing_pane().drawings.items().is_empty());
            assert!(app.control.scenarios.pending().annotation.is_none());
        }
        "F01" => {
            assert!(app.control.scenarios.pending().enable);
            let access = app.control.control_access.take().unwrap();
            run_frame(&mut app, &ctx);
            assert!(!app.control.scenarios.pending().enable);
            app.control.control_access = Some(access);
            run_frame(&mut app, &ctx);
            assert!(
                !app.control
                    .control_access
                    .as_ref()
                    .unwrap()
                    .needs_frame_service()
            );
        }
        "F03-empty" | "F03-padded" => {
            run_frame(&mut app, &ctx);
            let marks: Vec<_> = events(&app)
                .into_iter()
                .filter(|event| event["kind"] == "attention.mark.created")
                .collect();
            assert_eq!(marks.len(), 1);
            assert_eq!(marks[0]["actor"]["kind"], "human_ui");
            assert!(marks[0]["payload"]["target"]["pointer"].is_null());
            assert_eq!(
                marks[0]["payload"]["target"]["pointer_availability"],
                json!({"available": false, "reason": "pointer_is_not_over_a_painted_chart"})
            );
            assert_eq!(
                marks[0]["payload"]["note"],
                if case == "F03-empty" {
                    Value::Null
                } else {
                    json!(" 1 ")
                }
            );
            assert!(app.control.scenarios.pending().mark.is_none());
            run_frame(&mut app, &ctx);
            assert_eq!(
                events(&app)
                    .iter()
                    .filter(|event| event["kind"] == "attention.mark.created")
                    .count(),
                1
            );
        }
        "F07-toast" | "F07-popup" | "F07-sound" | "F07-invalid" => {
            run_frame(&mut app, &ctx);
            assert!(app.control.scenarios.pending().notification.is_none());
            let notifications: Vec<_> = events(&app)
                .into_iter()
                .filter(|event| event["module_id"] == "notify")
                .collect();
            if case == "F07-invalid" {
                assert!(notifications.is_empty());
                assert_eq!(recorded.named("CONTROL_NOTIFY_HOOK_REFUSED").len(), 1);
            } else {
                assert_eq!(notifications.len(), 1);
                let (channel, message) = match case {
                    "F07-toast" => ("toast", "message"),
                    "F07-popup" => ("popup", "hello:again"),
                    _ => ("sound", "beep"),
                };
                assert_eq!(notifications[0]["payload"]["channel"], channel);
                assert_eq!(notifications[0]["payload"]["message"], message);
                assert_eq!(notifications[0]["actor"]["kind"], "agent");
                if case == "F07-popup" {
                    let popup = app.surfaces.agent_popup.pending().unwrap();
                    assert_eq!(popup.title, "From your assistant");
                    assert_eq!(popup.message, "hello:again");
                    assert!(popup.author.contains("agent"));
                }
            }
            run_frame(&mut app, &ctx);
            assert_eq!(
                events(&app)
                    .iter()
                    .filter(|event| event["module_id"] == "notify")
                    .count(),
                notifications.len()
            );
        }
        "F08" | "F09-all" | "F09-empty" | "F10" | "F11" | "F12" | "F13" | "F14" => {
            if case == "F08" {
                let access = app.control.control_access.take().unwrap();
                run_frame(&mut app, &ctx);
                assert_eq!(app.control.scenarios.pending().evidence, Some("all"));
                app.control.control_access = Some(access);
            }
            if matches!(case, "F11" | "F14") {
                for frame in 1..=120 {
                    let output = run_frame(&mut app, &ctx);
                    let viewport = output.viewport_output.get(&egui::ViewportId::ROOT).unwrap();
                    assert!(
                        viewport
                            .commands
                            .iter()
                            .any(|command| matches!(command, egui::ViewportCommand::Screenshot))
                    );
                    assert_eq!(viewport.repaint_delay, std::time::Duration::ZERO);
                    assert_eq!(
                        app.control.scenarios.pending().evidence,
                        Some("all,screenshot"),
                        "wait frame {frame}"
                    );
                    assert_eq!(
                        app.control
                            .control_access
                            .as_ref()
                            .unwrap()
                            .retained_evidence_for_test(),
                        0
                    );
                }
            } else if matches!(case, "F12" | "F13") {
                run_frame(&mut app, &ctx);
                assert!(app.control.scenarios.pending().evidence.is_some());
                if case == "F12" {
                    let mut access = app.control.control_access.take().unwrap();
                    access.publish_screenshot_for_test(&mut app, test_screenshot(2, 2));
                    app.control.control_access = Some(access);
                } else {
                    let access = app.control.control_access.as_mut().unwrap();
                    assert!(access.is_disabled_for_test());
                    access
                        .configure_scopes(
                            "observe.system,observe.market,observe.events,observe.evidence",
                        )
                        .unwrap();
                }
            }
            run_frame(&mut app, &ctx);
            assert!(app.control.scenarios.pending().evidence.is_none());
            assert_eq!(
                app.control
                    .control_access
                    .as_ref()
                    .unwrap()
                    .retained_evidence_for_test(),
                1
            );
            let bundle = hook_bundle(&mut app, recorded);
            let expected_scopes: Vec<_> = app
                .control
                .control_access
                .as_ref()
                .unwrap()
                .readable_scopes()
                .into_iter()
                .map(|scope| scope.to_string())
                .collect();
            let actual_scopes: Vec<_> = bundle["snapshot"]["scopes"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            assert_eq!(actual_scopes, expected_scopes);
            if matches!(case, "F10" | "F13") {
                assert_eq!(
                    recorded
                        .named("CONTROL_EVIDENCE_HOOK_SCREENSHOT_NOT_GRANTED")
                        .len(),
                    1
                );
                assert!(bundle["screenshot"].is_null());
                assert!(
                    bundle["coverage"]["not_captured"]
                        .to_string()
                        .contains("not_requested")
                );
                if case == "F10" {
                    assert_ne!(
                        app.surfaces.toast.message(),
                        Some("Your assistant captured a picture of this window.")
                    );
                    assert!(
                        !app.control
                            .control_access
                            .as_ref()
                            .unwrap()
                            .screenshot_armed_for_test()
                    );
                }
            }
            if matches!(case, "F11" | "F14") {
                assert_eq!(
                    recorded
                        .named("CONTROL_EVIDENCE_HOOK_GAVE_UP_ON_IMAGE")
                        .len(),
                    1
                );
                assert!(
                    bundle["coverage"]["not_captured"]
                        .to_string()
                        .contains("frame_not_delivered")
                );
                assert!(
                    !bundle["coverage"]["not_captured"]
                        .to_string()
                        .contains("not_requested")
                );
            }
            if case == "F12" {
                assert!(!bundle["screenshot"].is_null());
                assert_eq!(
                    app.surfaces.toast.message(),
                    Some("Your assistant captured a picture of this window.")
                );
            }
            if case == "F14" {
                app.control
                    .scenarios
                    .queue_evidence("all,screenshot".to_owned());
                run_frame(&mut app, &ctx);
                assert!(app.control.scenarios.pending().evidence.is_none());
                assert_eq!(
                    recorded
                        .named("CONTROL_EVIDENCE_HOOK_GAVE_UP_ON_IMAGE")
                        .len(),
                    2
                );
                assert_eq!(
                    app.control
                        .control_access
                        .as_ref()
                        .unwrap()
                        .retained_evidence_for_test(),
                    2
                );
            } else {
                run_frame(&mut app, &ctx);
                assert_eq!(
                    app.control
                        .control_access
                        .as_ref()
                        .unwrap()
                        .retained_evidence_for_test(),
                    1
                );
            }
        }
        other => panic!("unregistered recipient fixture {other}"),
    }
}

fn combined_owned_instance(app: &mut QuantickApp, ctx: &egui::Context, recorded: &RecordedEvents) {
    // A fresh local description exposes identity without enabling or reading a token.
    let described = description(app);
    assert_eq!(described["effective_scopes"], json!(DEFAULT_GRANT));
    assert_eq!(described["effective_profile"], "observer");
    let instance = described["instance_id"].as_str().unwrap().to_owned();
    let directory = quantick_control_local::discovery::runtime_instances_dir().unwrap();
    let path = directory.join(format!("{instance}.json"));
    assert!(
        !path.exists(),
        "the exact owned path must be absent before enable"
    );
    let inventory = || {
        let mut names = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before = inventory();
    println!(
        "CONTROL7_OWNERSHIP_BEFORE pid={} instance={} path={} inventory={before:?}",
        std::process::id(),
        instance,
        path.display()
    );
    assert!(app.control.scenarios.pending().enable);
    run_frame(app, ctx);
    assert!(!app.control.scenarios.pending().enable);
    assert_combined_actions(app);
    // all-reads deliberately excludes the prompt-only evidence permission.
    assert_eq!(recorded.named("CONTROL_EVIDENCE_HOOK_REFUSED").len(), 1);
    assert!(recorded.named("CONTROL_EVIDENCE_CAPTURED").is_empty());
    for _ in 0..REPLY_WAIT_FRAMES {
        if app.control.control_access.as_ref().unwrap().is_enabled() {
            break;
        }
        run_frame(app, ctx);
        std::thread::yield_now();
    }
    let access = app.control.control_access.as_ref().unwrap();
    assert!(access.is_enabled());
    assert_eq!(access.descriptor_path_for_test().as_ref(), Some(&path));
    // Only the just-created exact descriptor is decoded. No token is printed.
    let descriptor: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(descriptor["instance_id"], instance);
    assert_eq!(
        descriptor["process_id"].as_u64(),
        Some(u64::from(std::process::id()))
    );
    let options = quantick_control_local::client::ConnectOptions::observer(
        "control7 owned baseline",
        env!("CARGO_PKG_VERSION"),
        DEFAULT_GRANT
            .iter()
            .map(|id| quantick_control::id::PermissionId::new(*id).unwrap())
            .collect(),
    );
    let mut client = quantick_control_local::client::LocalClient::connect(
        serde_json::from_value(descriptor).unwrap(),
        &options,
    )
    .unwrap();
    let reply = client
        .invoke(crate::control::DESCRIBE_CAPABILITY_ID, json!({}))
        .unwrap();
    let remote = success_result(&reply);
    assert_eq!(remote["instance_id"], instance);
    assert_eq!(remote["effective_profile"], "observer");
    assert_eq!(remote["effective_scopes"], json!(DEFAULT_GRANT));
    drop(client);
    app.control
        .control_access
        .as_mut()
        .unwrap()
        .shutdown_for_exit();
    assert!(
        !path.exists(),
        "ordinary shutdown removes exactly its own descriptor"
    );
    assert_eq!(inventory(), before);
    println!(
        "CONTROL7_OWNERSHIP_AFTER pid={} instance={} path={} removed=true",
        std::process::id(),
        instance,
        path.display()
    );
}

fn assert_combined_actions(app: &QuantickApp) {
    let items = app.active_tab().drawing_pane().drawings.items();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].tool.inline_text(items[0].payload.as_ref()),
        Some("alpha")
    );
    assert_eq!(items[0].author.as_ref().unwrap().actor_kind, "agent");
    let journal = events(app);
    let mark = journal
        .iter()
        .position(|event| event["kind"] == "attention.mark.created")
        .unwrap();
    let annotation = journal
        .iter()
        .position(|event| event["kind"] == "annotate.object.created")
        .unwrap();
    let notify = journal
        .iter()
        .position(|event| event["module_id"] == "notify")
        .unwrap();
    assert!(mark < annotation && annotation < notify);
    assert_eq!(journal[mark]["payload"]["note"], "phase");
    assert_eq!(journal[notify]["payload"]["message"], "beta");
}
