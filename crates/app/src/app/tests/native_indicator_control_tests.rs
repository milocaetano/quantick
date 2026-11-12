//! Gateway authority, retry and shared persistence for catalog-native edits.
use super::*;
const EDIT: &str = "indicator.native.edit";
const LIST: &str = "indicator.native.list";
#[test]
fn native_control_edits_share_layout_persistence() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("native-indicator-control");
    grant_annotate_for_test(
        &mut app,
        "all-reads,cockpit,cockpit.layout,cockpit.indicators",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let target = json!({"tab_id":app.runtime.tabs.active_id().to_string(),"pane_id":app.focused_pane().id.to_string()});
    let mut observer = connect(
        &directory,
        &options(
            "observer",
            &[
                "observe",
                "observe.indicators",
                "cockpit",
                "cockpit.indicators",
            ],
        ),
    );
    let add = json!({"target":target,"operation":{"action":"add","native_id":"native.volume"}});
    let denied = keyed_call(
        &mut app,
        &mut observer,
        "denied",
        EDIT,
        add.clone(),
        "denied",
    )
    .0;
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let mut layout_only = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let denied = keyed_call(
        &mut app,
        &mut layout_only,
        "layout-denied",
        EDIT,
        add.clone(),
        "layout-denied",
    )
    .0;
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let mut client = connect(
        &directory,
        &options(
            "cockpit",
            &[
                "cockpit",
                "cockpit.indicators",
                "observe",
                "observe.indicators",
            ],
        ),
    );
    let no_key = unkeyed_call(&mut app, &mut client, EDIT, add.clone()).0;
    assert_eq!(error_code(&no_key), Some(codes::INVALID_REQUEST));
    let first =
        success_result(&keyed_call(&mut app, &mut client, "add", EDIT, add.clone(), "add").0);
    let retry =
        success_result(&keyed_call(&mut app, &mut client, "retry", EDIT, add.clone(), "add").0);
    assert_eq!(first, retry);
    let second =
        success_result(&keyed_call(&mut app, &mut client, "second", EDIT, add, "second").0);
    assert_ne!(first["slot_id"], second["slot_id"]);
    let slot = first["slot_id"].clone();
    let read = json!({"target":target});
    let state = success_result(&unkeyed_call(&mut app, &mut observer, LIST, read.clone()).0);
    assert_eq!(state["total"], 2);
    let edit = |app: &mut QuantickApp, client: &mut LocalClient, key: &str, operation: Value| {
        keyed_call(
            app,
            client,
            key,
            EDIT,
            json!({"target":target,"operation":operation}),
            key,
        )
        .0
    };
    let inputs = edit(
        &mut app,
        &mut client,
        "inputs",
        json!({"action":"set_inputs","slot_id":slot,"values":{"length":{"type":"int","value":17},"show_average":{"type":"bool","value":true}}}),
    );
    success_result(&inputs);
    let invalid = edit(
        &mut app,
        &mut client,
        "invalid",
        json!({"action":"set_inputs","slot_id":slot,"values":{"length":{"type":"int","value":0},"show_average":{"type":"bool","value":false}}}),
    );
    assert_eq!(error_code(&invalid), Some(codes::INVALID_REQUEST));
    let deadline = Instant::now() + GATEWAY_TEST_WAIT;
    loop {
        run_frame(&mut app, &ctx);
        if app
            .focused_pane()
            .indicators
            .all()
            .iter()
            .any(|v| v.slot.0.to_string() == slot.as_str().unwrap())
        {
            break;
        }
        assert!(Instant::now() < deadline, "native worker did not load");
        std::thread::yield_now();
    }
    success_result(&edit(
        &mut app,
        &mut client,
        "style",
        json!({"action":"set_plot_style","slot_id":slot,"plot":0,"style":{"visible":true,"color":[10,20,30,125],"width":"2.5"}}),
    ));
    success_result(&edit(
        &mut app,
        &mut client,
        "hide",
        json!({"action":"set_hidden","slot_id":slot,"hidden":true}),
    ));
    let state = success_result(&unkeyed_call(&mut app, &mut observer, LIST, read.clone()).0);
    let row = state["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["slot_id"] == slot)
        .unwrap();
    assert_eq!(row["inputs"]["length"]["value"], 17);
    assert_eq!(row["inputs"]["show_average"]["value"], true);
    assert_eq!(row["hidden"], true);
    assert_eq!(row["plot_styles"][0]["color"], json!([10, 20, 30, 125]));
    assert_eq!(row["plot_styles"][0]["width"], "2.5");
    let before = state.clone();
    let invalid = edit(
        &mut app,
        &mut client,
        "bad-style",
        json!({"action":"set_plot_style","slot_id":slot,"plot":99,"style":{"color":[1,2,3,4]}}),
    );
    assert_eq!(error_code(&invalid), Some(codes::INVALID_REQUEST));
    assert_eq!(
        before,
        success_result(&unkeyed_call(&mut app, &mut observer, LIST, read.clone()).0)
    );
    let book = app.runtime.workspace.layouts().book();
    let encoded = toml::to_string_pretty(book).unwrap();
    let restored = quantick_workspace::layout_document::parse(&encoded).unwrap();
    assert_eq!(&restored, book);
    let denied = edit(
        &mut app,
        &mut layout_only,
        "remove-denied",
        json!({"action":"remove","slot_id":slot}),
    );
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    success_result(&edit(
        &mut app,
        &mut client,
        "remove",
        json!({"action":"remove","slot_id":slot}),
    ));
    let state = success_result(&unkeyed_call(&mut app, &mut observer, LIST, read).0);
    assert_eq!(state["total"], 1);
    disable_test_gateway(&mut app, &ctx);
}
