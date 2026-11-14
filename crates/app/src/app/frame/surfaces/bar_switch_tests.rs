use super::*;
use quantick_engine::bar_registry::BUILTIN_BARS;
use quantick_workspace::{arrangement::TabId, lifetime::WorkspaceId};
fn tab(id: u64, dir: &std::path::Path) -> crate::tab::Tab {
    let (_, events) = tokio::sync::mpsc::channel(1);
    let (_, book_events) = tokio::sync::mpsc::channel(1);
    let (commands, _) = tokio::sync::mpsc::channel(1);
    let mut tab = crate::tab::Tab::new(
        id,
        "fixture".into(),
        "TEST".into(),
        BUILTIN_BARS.parse("tick:50").unwrap(),
        crate::feed::FeedHandle {
            events,
            book_events,
            commands,
            notices: crate::feed::silent_notices(),
            capabilities: crate::feed::fixed_capabilities(crate::config::FeedCapabilities::none()),
            latency: crate::feed::unsplit_latency(),
            replay: None,
        },
        dir.to_path_buf(),
    );
    tab.time_panes
        .push(crate::pane::ChartPane::time(id + 1, 60_000));
    tab
}
#[test]
fn bar_switch_host_targets_stable_tab_and_pane_and_preserves_refusal_toast() {
    let dir = crate::scratch::ScratchDir::new("bar-switch-host");
    let mut tabs = ArrangementHost::new(WorkspaceId(1), TabId(10), tab(100, &dir));
    tabs.append(tabs.plan_open(), tab(200, &dir));
    let active = tabs.active_id();
    assert_ne!(active, 10);
    let mut toast = crate::surfaces::toast::new();
    let request = crate::surfaces::BarSwitchRequest {
        tab: 10,
        side: PaneSide::Time(0),
        config: BUILTIN_BARS.parse("time:5m").unwrap(),
    };
    apply_bar_switch(&mut tabs, &mut toast, request);
    assert_eq!(
        tabs.by_id(10).unwrap().time_panes[0].state.spec(),
        &crate::state::BarSpec::Time(300_000)
    );
    assert_eq!(
        tabs.by_id(active).unwrap().time_panes[0].state.spec(),
        &crate::state::BarSpec::Time(60_000)
    );
    assert_eq!(
        tabs.by_id(10).unwrap().flow_pane.state.spec(),
        &crate::state::BarSpec::Tick(50)
    );
    assert_eq!(tabs.active_id(), active);
    assert_eq!(toast.model_state.message(), None);
    apply_bar_switch(
        &mut tabs,
        &mut toast,
        crate::surfaces::BarSwitchRequest {
            config: BUILTIN_BARS.parse("volume:100").unwrap(),
            ..request
        },
    );
    assert_eq!(
        tabs.by_id(10).unwrap().time_panes[0].state.spec(),
        &crate::state::BarSpec::Time(300_000)
    );
    assert_eq!(
        toast.model_state.message(),
        Some("Bars not changed: this source quotes prices but prints no traded volume")
    );
    toast.model_state = Default::default();
    apply_bar_switch(
        &mut tabs,
        &mut toast,
        crate::surfaces::BarSwitchRequest {
            tab: u64::MAX,
            ..request
        },
    );
    assert_eq!(toast.model_state.message(), None);
    assert_eq!(tabs.active_id(), active);
}
