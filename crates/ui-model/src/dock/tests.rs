use super::*;
use crate::Model as _;

#[test]
fn dock_intents_delegate_to_the_workspace_owner() {
    let mut state = State::default();
    let mut owner = Dock::new();
    let steps = [
        Intent::OpenTab(DockTab::Trading),
        Intent::ToggleVisible,
        Intent::Restore {
            visible: false,
            tab: Some(DockTab::Trades),
        },
        Intent::OpenTab(DockTab::Session),
        Intent::Select(DockTab::Session),
        Intent::Select(DockTab::Trading),
        Intent::Select(DockTab::Trades),
        Intent::RememberWidth {
            tab: DockTab::Trading,
            width: 500.0,
        },
        Intent::RememberWidth {
            tab: DockTab::Trades,
            width: 10.0,
        },
        Intent::RememberWidth {
            tab: DockTab::Session,
            width: 300.0,
        },
        Intent::Collapse,
    ];
    for intent in steps {
        match intent {
            Intent::ToggleVisible => owner.toggle_visible(),
            Intent::Restore { visible, tab } => owner.restore(visible, tab),
            Intent::OpenTab(tab) => owner.open_tab(tab),
            Intent::Collapse => owner.collapse(),
            Intent::Select(tab) => owner.select(tab),
            Intent::RememberWidth { tab, width } => owner.remember_width(tab, width),
        }
        Model::update(&mut state, intent);
        assert_eq!(state.visible(), owner.visible());
        assert_eq!(state.tab(), owner.tab());
        for tab in DockTab::ALL {
            assert_eq!(state.width(tab), owner.width(tab));
        }
    }
}

#[test]
fn paper_passes_merge_flags_and_keep_the_first_value() {
    let mut first = PaperPanelEffects {
        open_alerts: true,
        navigate_to_trade: Some((1, 2)),
        ..PaperPanelEffects::default()
    };
    first.merge(PaperPanelEffects {
        close_replay: true,
        navigate_to_trade: Some((3, 4)),
        risk_settings_changed: true,
        ..PaperPanelEffects::default()
    });
    assert_eq!(
        first,
        PaperPanelEffects {
            open_alerts: true,
            close_replay: true,
            navigate_to_trade: Some((1, 2)),
            risk_settings_changed: true,
            ..PaperPanelEffects::default()
        }
    );
}

#[test]
fn reveal_collapsed_shows_the_dock_without_a_tab_either_way() {
    for (visible, tab) in [(false, None), (true, Some(DockTab::Trading))] {
        let mut dock = crate::Adapter::<Model, (), ()>::new((), |_| ());
        dock.restore(visible, tab);
        dock.reveal_collapsed();
        assert!(dock.visible());
        assert_eq!(dock.tab(), None);
    }
}
