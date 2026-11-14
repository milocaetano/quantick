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
