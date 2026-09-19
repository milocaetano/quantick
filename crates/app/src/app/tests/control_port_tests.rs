//! Control handlers driven through a fake window: no `QuantickApp`, only the
//! families each handler's signature names (see `app::control_host`).

use crate::app::control_host::tests::fake::FakeWindow;

/// Chrome family: the scene projection reads the tab strip and the chrome
/// the fake drew — nothing, so no feed chip and no quick-range bar is listed.
#[test]
fn the_scene_reads_the_chrome_family_of_a_fake_window() {
    let window = FakeWindow::new();
    let scene = crate::control::scene_snapshot(&window);
    assert_eq!(scene.active_tab_id.get(), 1);
    assert!(
        scene
            .controls
            .iter()
            .all(|control| control.owner.id != "feed_status" && control.owner.id != "quick_range"),
        "a window that drew no chip and no range lists neither"
    );
}

/// Owner hand-outs (scripts): attach and detach go through the scripts family
/// alone, and the operator's authorship is what the handler passed on.
#[test]
fn a_script_is_attached_and_detached_through_the_scripts_family() {
    let mut window = FakeWindow::new();
    let mut access = crate::control::ControlAccess::new();
    let actor = FakeWindow::assistant();
    let attached = crate::app::indicator_control::attach_script(
        &mut window,
        &mut access,
        &actor,
        &serde_json::json!({ "name": "fake", "source": "//@version=5\nindicator(\"fake\")\nplot(close)\n" }),
    )
    .expect("a script that compiles attaches");
    assert_eq!(attached["slot_id"], "0");
    assert_eq!(window.attached.len(), 1);
    assert!(window.attached[0].2, "an agent's script is an operator's");

    let detached = crate::app::indicator_control::detach_script(
        &mut window,
        &mut access,
        &actor,
        &serde_json::json!({ "slot_id": "0" }),
    )
    .expect("the operator's own slot detaches");
    assert_eq!(detached["detached"], true);
}

/// Owner hand-outs, error path: a script that does not compile never reaches
/// the scripts family.
#[test]
fn a_script_that_does_not_compile_never_reaches_the_window() {
    let mut window = FakeWindow::new();
    let error = crate::app::indicator_control::attach_script(
        &mut window,
        &mut crate::control::ControlAccess::new(),
        &FakeWindow::assistant(),
        &serde_json::json!({ "name": "broken", "source": "plot(" }),
    )
    .expect_err("a syntax error is refused");
    assert!(!error.message.is_empty());
    assert!(window.attached.is_empty());
}
