//! The real gateway exercises authority, strict targets and keyed window operations.
use super::*;
use quantick_control_handlers::native_window::NativeWindowHost;

#[test]
fn market_window_operations_validate_authority_preserve_identity_and_replay_safely() {
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    let _third = open_second_tab(&mut app, &ctx, "BTCUSDT");
    let ids: Vec<_> = app.runtime.tabs.iter_with_ids().map(|(id, _)| id).collect();
    let identities = |app: &QuantickApp| {
        app.runtime
            .tabs
            .iter_with_ids()
            .map(|(id, tab)| {
                (
                    id,
                    std::ptr::from_ref(tab),
                    tab.flow_pane.indicator_worker.progress().instance,
                )
            })
            .collect::<Vec<_>>()
    };
    let before = identities(&app);
    let directory = gateway_test_directory("market-window-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let calls = [
        (
            "layout.market_tab.reorder",
            json!({"tab_id":ids[0].to_string(),"position":"1"}),
        ),
        (
            "layout.market_tab.detach",
            json!({"tab_id":ids[0].to_string()}),
        ),
        (
            "layout.market_tab.attach",
            json!({"tab_id":ids[0].to_string(),"window_id":"0","position":"1"}),
        ),
        (
            "layout.market_tab.select",
            json!({"tab_id":ids[0].to_string(),"window_id":"0"}),
        ),
        (
            "layout.market_tab.drag.begin",
            json!({"tab_id":ids[0].to_string()}),
        ),
        ("layout.market_tab.drag.cancel", json!({})),
        ("layout.window.close", json!({"window_id":"0"})),
        (
            "layout.window.placement.set",
            json!({"placement":{"x_px":-1500,"y_px":200,"width_px":900,"height_px":600}}),
        ),
    ];
    let baseline = app.market_windows();
    for (capability, input) in &calls {
        let (denied, _) = unkeyed_call(&mut app, &mut observer, capability, input.clone());
        assert!(matches!(denied.outcome, ResponseOutcome::Failure { .. }));
        let (missing_key, _) = unkeyed_call(&mut app, &mut client, capability, input.clone());
        assert!(matches!(
            missing_key.outcome,
            ResponseOutcome::Failure { .. }
        ));
        assert_eq!(
            app.market_windows(),
            baseline,
            "refused calls change nothing"
        );
    }
    let begin = json!({"tab_id":ids[0].to_string()});
    let (begun, _) = keyed_call(
        &mut app,
        &mut client,
        "begin",
        "layout.market_tab.drag.begin",
        begin,
        "begin-key",
    );
    assert_eq!(success_result(&begun)["drag"]["pending"], true);
    assert_eq!(
        app.market_windows().windows,
        baseline.windows,
        "staging cannot transfer ownership"
    );
    let (cancelled, _) = keyed_call(
        &mut app,
        &mut client,
        "cancel",
        "layout.market_tab.drag.cancel",
        json!({}),
        "cancel-key",
    );
    assert_eq!(success_result(&cancelled)["drag"]["cancelled"], true);
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "cancel-again",
        "layout.market_tab.drag.cancel",
        json!({}),
        "cancel-key",
    );
    assert_eq!(retry.outcome, cancelled.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    assert_eq!(app.market_windows().windows, baseline.windows);

    let reorder = json!({"tab_id":ids[2].to_string(),"position":"0"});
    let (first, _) = keyed_call(
        &mut app,
        &mut client,
        "reorder",
        "layout.market_tab.reorder",
        reorder.clone(),
        "reorder-key",
    );
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "reorder-again",
        "layout.market_tab.reorder",
        reorder,
        "reorder-key",
    );
    assert_eq!(retry.outcome, first.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    assert_eq!(
        app.market_windows().windows[0].ordered_tab_ids[0].get(),
        ids[2]
    );
    let detach = json!({"tab_id":ids[1].to_string()});
    let (detached, _) = keyed_call(
        &mut app,
        &mut client,
        "detach",
        "layout.market_tab.detach",
        detach.clone(),
        "detach-key",
    );
    let window = success_result(&detached)["window_id"].clone();
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "detach-again",
        "layout.market_tab.detach",
        detach,
        "detach-key",
    );
    assert_eq!(retry.outcome, detached.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    assert_eq!(app.market_windows().windows.len(), 2);
    let placement = json!({"window_id":window,"placement":{"x_px":-1700,"y_px":180,"width_px":900,"height_px":600}});
    let (placed, _) = keyed_call(
        &mut app,
        &mut client,
        "place",
        "layout.window.placement.set",
        placement.clone(),
        "place-key",
    );
    assert_eq!(success_result(&placed)["requested"]["x_px"], -1700);
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "place-again",
        "layout.window.placement.set",
        placement,
        "place-key",
    );
    assert_eq!(retry.outcome, placed.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    let selection = app.market_windows().windows[1].selected_tab_id;
    let (selected, _) = keyed_call(
        &mut app,
        &mut client,
        "select",
        "layout.market_tab.select",
        json!({"tab_id":ids[0].to_string(),"window_id":"0"}),
        "select-key",
    );
    success_result(&selected);
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "select-again",
        "layout.market_tab.select",
        json!({"tab_id":ids[0].to_string(),"window_id":"0"}),
        "select-key",
    );
    assert_eq!(retry.outcome, selected.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    assert_eq!(
        app.market_windows().windows[1].selected_tab_id,
        selection,
        "another window's selection is independent"
    );
    let state = app.market_windows();
    for (number, input) in [
        json!({"tab_id":ids[1].to_string(),"window_id":"0","position":"999999"}),
        json!({"tab_id":ids[1].to_string(),"window_id":"99999","position":"0"}),
        json!({"tab_id":ids[1].to_string(),"window_id":"0","position":"0","extra":true}),
    ]
    .into_iter()
    .enumerate()
    {
        let (refused, _) = keyed_call(
            &mut app,
            &mut client,
            &format!("invalid-{number}"),
            "layout.market_tab.attach",
            input,
            &format!("invalid-key-{number}"),
        );
        assert!(matches!(refused.outcome, ResponseOutcome::Failure { .. }));
        assert_eq!(app.market_windows(), state);
    }
    let (attached, _) = keyed_call(
        &mut app,
        &mut client,
        "attach",
        "layout.market_tab.attach",
        json!({"tab_id":ids[1].to_string(),"window_id":"0","position":"1"}),
        "attach-key",
    );
    success_result(&attached);
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "attach-again",
        "layout.market_tab.attach",
        json!({"tab_id":ids[1].to_string(),"window_id":"0","position":"1"}),
        "attach-key",
    );
    assert_eq!(retry.outcome, attached.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    assert_eq!(app.market_windows().windows.len(), 1);
    let (detached, _) = keyed_call(
        &mut app,
        &mut client,
        "detach-root",
        "layout.market_tab.detach",
        json!({"tab_id":ids[0].to_string()}),
        "detach-root-key",
    );
    let survivor = success_result(&detached)["window_id"].clone();
    assert_ne!(window, survivor, "retired native IDs are not reused");
    let (closed, _) = keyed_call(
        &mut app,
        &mut client,
        "close-root",
        "layout.window.close",
        json!({"window_id":"0"}),
        "close-root-key",
    );
    assert_eq!(success_result(&closed)["window_id"], survivor);
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "close-root-again",
        "layout.window.close",
        json!({"window_id":"0"}),
        "close-root-key",
    );
    assert_eq!(retry.outcome, closed.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    let snapshot = app.market_windows();
    assert_eq!(snapshot.windows.len(), 1);
    assert_eq!(snapshot.windows[0].ordered_tab_ids.len(), 3);
    let (exit, _) = keyed_call(
        &mut app,
        &mut client,
        "no-exit",
        "layout.window.close",
        json!({"window_id":survivor}),
        "no-exit-key",
    );
    assert!(matches!(exit.outcome, ResponseOutcome::Failure { .. }));
    assert_eq!(app.market_windows(), snapshot);
    assert_eq!(
        identities(&app),
        before,
        "no runtime or indicator worker is recreated"
    );
    disable_test_gateway(&mut app, &ctx);
}
