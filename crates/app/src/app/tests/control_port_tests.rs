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
