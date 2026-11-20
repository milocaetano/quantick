use crate::app::control_host::tests::fake::FakeWindow;
use crate::control::{ControlAccess, strategy_describe};
use quantick_control_host::actions::ActionRegistry;
use quantick_control_schema::strategy_describe as wire;
use quantick_ui_model::Slot;
use serde_json::json;

#[test]
fn registered_contracts_are_inert() {
    let mut host = FakeWindow::new();
    let effect = host
        .surfaces
        .update(crate::surfaces::HostIntent::StrategyDescribe(Box::new(
            quantick_ui_model::strategy_describe::Intent::Command(
                quantick_ui_model::strategy_describe::Command::Open,
            ),
        )));
    assert!(effect.model_effects.is_empty());
    let surface = &host.surfaces.strategy_describe;
    assert!(surface.supports_pass(Slot::Overlay));
    assert!(!surface.view.worker.available());
    let ids = crate::control::registered_action_versions();
    for id in [
        wire::OPEN_CAPABILITY_ID,
        wire::READ_CAPABILITY_ID,
        wire::SUBMIT_CAPABILITY_ID,
        wire::CANCEL_CAPABILITY_ID,
        wire::SAVE_CAPABILITY_ID,
    ] {
        assert!(
            ids.iter()
                .any(|(name, version)| name == id && *version == 1)
        );
    }
    use quantick_ui_model::{Model as _, chrome_menu_bar as menu};
    let mut menu_state = menu::State::default();
    menu::Model::update(
        &mut menu_state,
        menu::Intent::Refresh(Box::new(menu::Snapshot {
            open: true,
            chart_shown: true,
            describe_available: surface.view.worker.available(),
            ..Default::default()
        })),
    );
    let panels = menu_state
        .nodes
        .iter()
        .find_map(|node| match node {
            menu::Node::Submenu {
                id: quantick_control_schema::window_menu::SubmenuId::Panels,
                nodes,
                ..
            } => Some(nodes),
            _ => None,
        })
        .expect("Panels menu");
    assert!(!panels.iter().any(
        |node| matches!(node, menu::Node::Row(row) if row.action == menu::Action::DescribeStrategy)
    ));
    let mut registry = ActionRegistry::<FakeWindow, ControlAccess>::new();
    strategy_describe::register(&mut registry).unwrap();
    let mut access = ControlAccess::new();
    let actor = FakeWindow::assistant();
    for (id, input) in [
        (wire::OPEN_CAPABILITY_ID, json!({})),
        (wire::READ_CAPABILITY_ID, json!({})),
        (
            wire::SUBMIT_CAPABILITY_ID,
            json!({"revision":"0","text":"Alert on a reversal"}),
        ),
        (wire::CANCEL_CAPABILITY_ID, json!({"revision":"0"})),
        (
            wire::SAVE_CAPABILITY_ID,
            json!({"revision":"0","id":"reversal.pine"}),
        ),
    ] {
        let action = registry.lookup(id, 1).unwrap();
        let answer = (action.handler)(&mut host, &mut access, &actor, &input);
        if id == wire::READ_CAPABILITY_ID {
            assert_eq!(answer.unwrap()["phase"], "unavailable");
        } else {
            assert_eq!(
                answer.unwrap_err().code.as_str(),
                quantick_control::error::codes::CAPABILITY_UNAVAILABLE
            );
        }
    }
    assert!(host.surfaces.strategy_describe.view.worker.poll().is_none());
}
