//! Exact range operations remain behind the existing admitted control contract.

use super::*;
use quantick_control::error::codes;
use serde_json::{Value, json};

const ACTIONS: [(&str, u32, usize); 3] = [
    (crate::control::PROFILE_CAPABILITY_ID, 2, 2),
    (crate::control::FIB_RETRACEMENT_CAPABILITY_ID, 1, 2),
    (crate::control::FIB_PROJECTION_CAPABILITY_ID, 1, 3),
];

fn exact_input(app: &QuantickApp, count: usize) -> Value {
    let pane = app.active_tab().drawing_pane();
    let mut first = anchor_at_slot(app, 0);
    let mut last = anchor_at_slot(app, 7);
    first["bar_position"] = json!("0.5");
    last["bar_position"] = json!("7.5");
    let mut anchors = vec![first, last.clone()];
    if count == 3 {
        anchors.push(last);
    }
    json!({
        "anchors": anchors,
        "chart_reference": {
            "pane_id": pane.id.to_string(),
            "series_revision": pane.pagination_revision().to_string(),
            "layout_id": pane.layout_id().map(|id| id.0.to_string()),
        }
    })
}

#[test]
fn exact_range_validation_is_atomic_for_every_action() {
    for (capability, version, count) in ACTIONS {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(8);
        run_frame(&mut app, &ctx);
        let valid = exact_input(&app, count);
        for field in [
            "pane_id",
            "series_revision",
            "layout_id",
            "time_unix_ms",
            "price",
        ] {
            let mut invalid = valid.clone();
            match field {
                "time_unix_ms" => invalid["anchors"][count - 1][field] = json!(-1),
                "price" => {
                    invalid["anchors"][count - 1][field] = json!("79228162514264337593543950335")
                }
                _ => invalid["chart_reference"][field] = json!("18446744073709551615"),
            }
            assert!(
                app.control_action(
                    capability,
                    version,
                    crate::control::ActionOrigin::Human,
                    invalid
                )
                .is_err(),
                "{capability}: {field}"
            );
            let pane = app.active_tab().drawing_pane();
            assert!(
                pane.drawings.items().is_empty(),
                "no partial object after {field}"
            );
            assert!(
                pane.drawings.draft().is_none(),
                "no partial draft after {field}"
            );
        }
        let result = app
            .control_action(
                capability,
                version,
                crate::control::ActionOrigin::Human,
                valid,
            )
            .unwrap();
        assert_eq!(result["anchors"][0]["slot"], "0");
        assert_eq!(result["anchors"][1]["slot"], "7");
        assert_eq!(app.active_tab().drawing_pane().drawings.items().len(), 1);
    }
}

#[test]
fn legacy_timestamp_mode_does_not_start_consuming_fallback_positions() {
    for (capability, version, count) in ACTIONS {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(8);
        run_frame(&mut app, &ctx);
        let mut input = exact_input(&app, count);
        input.as_object_mut().unwrap().remove("chart_reference");
        for anchor in input["anchors"].as_array_mut().unwrap() {
            anchor["bar_position"] = json!("-1");
        }
        let result = app
            .control_action(
                capability,
                version,
                crate::control::ActionOrigin::Human,
                input,
            )
            .unwrap();
        assert_eq!(result["anchors"][0]["slot"], "0");
        assert_eq!(result["anchors"][1]["slot"], "7");
    }
}

#[test]
fn all_actions_resolve_fractional_market_and_first_future_slots_exactly() {
    for (capability, version, count) in ACTIONS {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(8);
        run_frame(&mut app, &ctx);
        let mut input = exact_input(&app, count);
        let pane = app.active_tab().drawing_pane();
        let future = pane.slots() as f32 - 0.25;
        let market_time = pane.anchor_time(0.75).unwrap();
        assert_eq!(pane.anchor_time(future), None);
        input["anchors"][0]["bar_position"] = json!("0.75");
        input["anchors"][0]["time_unix_ms"] = json!(market_time);
        input["anchors"][count - 1]["bar_position"] = json!(future.to_string());
        input["anchors"][count - 1]["time_unix_ms"] = Value::Null;
        let result = app
            .control_action(
                capability,
                version,
                crate::control::ActionOrigin::Human,
                input,
            )
            .unwrap();
        assert_eq!(result["anchors"][0]["slot"], "1", "{capability}");
        assert_eq!(result["anchors"][0]["time_unix_ms"], market_time);
        assert_eq!(
            result["anchors"][count - 1]["bar_position"],
            future.to_string()
        );
        assert!(result["anchors"][count - 1].get("slot").is_none());
        let drawing = app
            .active_tab()
            .drawing_pane()
            .drawings
            .items()
            .last()
            .unwrap();
        assert_eq!(drawing.points[0].bar, 0.75);
        assert_eq!(drawing.points[count - 1].bar, future);
    }
}

#[test]
fn quick_range_wire_round_trip_preserves_near_boundary_coordinates_for_all_actions() {
    use quantick_chart_interaction::quick_range::{
        Action, Anchor, Owner, PlaceRequest, RangeContext,
    };
    for duplicate_times in [false, true] {
        for (action, (capability, version, count)) in Action::ALL.into_iter().zip(ACTIONS) {
            let ctx = egui::Context::default();
            let (mut app, _commands) = app_with_history(8);
            if duplicate_times {
                let pane = &mut app.active_tab_mut().flow_pane;
                pane.reset_series();
                let prints: Vec<_> = (1..=8)
                    .map(|id| {
                        let mut print = trade(id);
                        print.timestamp_ms = 10_000;
                        print
                    })
                    .collect();
                pane.ingest_backfill(&prints);
            }
            run_frame(&mut app, &ctx);
            let tab_id = app.tabs.active_id();
            let tab = app.active_tab();
            let pane = &tab.flow_pane;
            let future = pane.slots() as f32 - 0.4996;
            let request = PlaceRequest {
                id: 1,
                action,
                context: RangeContext {
                    owner: Owner {
                        tab: tab_id,
                        pane: pane.id,
                        layout: pane.layout_id().map(|id| id.0),
                    },
                    revision: pane.pagination_revision(),
                },
                anchors: [
                    Anchor {
                        bar: 0.5004,
                        price: 100.0,
                        time_ms: pane.anchor_time(0.5004),
                    },
                    Anchor {
                        bar: future,
                        price: 101.0,
                        time_ms: pane.anchor_time(future),
                    },
                ],
            };
            assert!(request.anchors[0].time_ms.is_some());
            assert!(request.anchors[1].time_ms.is_none());
            let input =
                crate::control::quick_range_input(&request, crate::pane::PaneSide::Flow).unwrap();
            let result = app
                .control_action(
                    capability,
                    version,
                    crate::control::ActionOrigin::Human,
                    input,
                )
                .unwrap();
            assert_eq!(result["anchors"][0]["slot"], "1");
            let future_readback = result["anchors"][count - 1]["bar_position"]
                .as_str()
                .unwrap()
                .parse::<f32>()
                .unwrap();
            assert_eq!(future_readback.to_bits(), future.to_bits());
            let drawing = app.active_tab().flow_pane.drawings.items().last().unwrap();
            assert_eq!(drawing.points[0].bar.to_bits(), 0.5004_f32.to_bits());
            assert_eq!(drawing.points[count - 1].bar.to_bits(), future.to_bits());
        }
    }
}

fn receive(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
) -> quantick_control::wire::ResponseEnvelope {
    for _ in 0..400 {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    client.read().expect("the admitted control path answered")
}

#[test]
fn exact_range_remote_calls_preserve_grants_authorship_and_idempotency_policy() {
    for granted in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(8);
        run_frame(&mut app, &ctx);
        let directory = gateway_test_directory("quick-range-contract");
        if granted {
            grant_annotate_for_test(&mut app, "all-reads,annotate-tier");
        }
        enable_test_gateway(&mut app, &ctx, &directory, 8);
        let mut client =
            quantick_control_local::client::discover_in(&directory, &annotator_test_options())
                .unwrap()
                .select(None)
                .unwrap();
        for (index, (capability, version, count)) in ACTIONS.into_iter().enumerate() {
            let input = exact_input(&app, count);
            client
                .send_with_request_id(
                    quantick_control::id::RequestId::new(format!("range-{index}")).unwrap(),
                    capability,
                    version,
                    input.clone(),
                )
                .unwrap();
            let response = receive(&mut app, &ctx, &mut client);
            if granted {
                assert_eq!(success_result(&response)["anchors"][0]["slot"], "0");
                assert_eq!(
                    app.active_tab().drawing_pane().drawings.items().len(),
                    index + 1
                );
                assert!(
                    app.active_tab().drawing_pane().drawings.items()[index]
                        .author
                        .is_some()
                );
                client
                    .send_with_idempotency_key(
                        quantick_control::id::RequestId::new(format!("key-{index}")).unwrap(),
                        capability,
                        version,
                        input,
                        quantick_control::id::IdempotencyKey::new(format!("range-key-{index}"))
                            .unwrap(),
                    )
                    .unwrap();
                let refused = receive(&mut app, &ctx, &mut client);
                assert_eq!(
                    response_error(&refused).code.as_str(),
                    codes::INVALID_REQUEST
                );
                assert!(
                    response_error(&refused)
                        .message
                        .contains("forbids idempotency keys")
                );
                assert_eq!(
                    app.active_tab().drawing_pane().drawings.items().len(),
                    index + 1
                );
            } else {
                assert_eq!(
                    response_error(&response).code.as_str(),
                    codes::PERMISSION_DENIED
                );
                assert!(app.active_tab().drawing_pane().drawings.items().is_empty());
            }
        }
        disable_test_gateway(&mut app, &ctx);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
