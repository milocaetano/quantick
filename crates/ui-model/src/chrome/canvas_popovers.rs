//! One application-wide owner for the toolbar popovers.
use quantick_control_handlers::canvas::{CanvasAct, add_act};
use quantick_workspace::canvas::WidgetSpec;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Projection {
    pub add_open: bool,
    pub layout_open: bool,
}
#[derive(Debug, Default)]
pub struct State(Projection);
impl State {
    pub fn projection(&self) -> Projection {
        self.0
    }
}
#[derive(Debug)]
pub enum Intent {
    ToggleAdd,
    ToggleLayout,
    SetAddOpen(bool),
    SetLayoutOpen(bool),
    DismissAdd {
        outside: bool,
        escaped: bool,
    },
    Pick {
        widget: &'static WidgetSpec,
        available: bool,
    },
    SettleLayout {
        outside: bool,
        selected: bool,
    },
    ToolbarHidden,
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects(pub Option<CanvasAct>);
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::ToggleAdd => state.0.add_open ^= true,
            Intent::ToggleLayout => state.0.layout_open ^= true,
            Intent::SetAddOpen(open) => state.0.add_open = open,
            Intent::SetLayoutOpen(open) => state.0.layout_open = open,
            Intent::DismissAdd { outside, escaped } => state.0.add_open &= !outside && !escaped,
            Intent::Pick { widget, available } => {
                if state.0.add_open && available {
                    state.0.add_open = false;
                    return Effects(Some(add_act(widget).1));
                }
            }
            Intent::SettleLayout { outside, selected } => {
                state.0.layout_open &= !outside && !selected
            }
            Intent::ToolbarHidden => state.0.layout_open = false,
        }
        Effects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;
    #[test]
    fn canvas_popovers_have_one_owner_across_toolbar_refresh() {
        use crate::chrome_toolbar as toolbar;
        let words = quantick_paper::desk::EntryTexts {
            face: String::new(),
            label: String::new(),
            hover: String::new(),
        };
        let snapshot = toolbar::Snapshot {
            capsule: toolbar::Capsule {
                tree: quantick_workspace::canvas::LAYOUT_PRESETS[0].tree.clone(),
                workspace: "Default".into(),
                unsaved: false,
                marks: Default::default(),
                menu_open: false,
                door_open: false,
                door_hover: String::new(),
            },
            market: ('B', "Fixture".into()),
            replay: None,
            deal_recording: None,
            focused_bars: <&quantick_engine::bar_registry::BarDefinition>::from(
                quantick_engine::BarKind::Tick,
            )
            .default_config(),
            history: toolbar::HistoryButton {
                main: Default::default(),
                status: quantick_sources::history_run::RunStatus::Idle,
                tz: Default::default(),
            },
            history_candles: 0,
            older_candles: quantick_sources::candles::OlderCandles::NotArrivedYet,
            capabilities: quantick_sources::config::ProviderKind::Binance.capabilities(),
            dock_visible: false,
            paper: quantick_paper::desk::TradeBar {
                blocked: None,
                buy: words.clone(),
                sell: words,
                close: None,
                reverse_hover: String::new(),
            },
        };
        let mut toolbar = toolbar::State::default();
        let mut popovers = State::default();
        for open in [true, false, true] {
            Model::update(&mut popovers, Intent::SetAddOpen(open));
            Model::update(&mut popovers, Intent::SetLayoutOpen(open));
            toolbar::Model::update(
                &mut toolbar,
                toolbar::Intent::Refresh(Box::new(snapshot.clone())),
            );
            assert_eq!(
                popovers.projection(),
                Projection {
                    add_open: open,
                    layout_open: open
                }
            );
        }
        assert_eq!(
            toolbar::Model::update(
                &mut toolbar,
                toolbar::Intent::Actions(vec![toolbar::Action::SaveWorkspace])
            ),
            toolbar::Effects(vec![toolbar::Effect::Action(
                toolbar::Action::SaveWorkspace
            )])
        );
    }
    #[test]
    fn capsule_toggles_and_dismissal_use_cached_state() {
        let mut state = State::default();
        Model::update(&mut state, Intent::ToggleLayout);
        assert!(state.projection().layout_open);
        Model::update(
            &mut state,
            Intent::SettleLayout {
                outside: false,
                selected: false,
            },
        );
        assert!(state.projection().layout_open);
        Model::update(
            &mut state,
            Intent::SettleLayout {
                outside: true,
                selected: false,
            },
        );
        assert!(!state.projection().layout_open);
        Model::update(&mut state, Intent::ToggleLayout);
        Model::update(
            &mut state,
            Intent::SettleLayout {
                outside: false,
                selected: true,
            },
        );
        assert!(!state.projection().layout_open);
        Model::update(&mut state, Intent::ToggleAdd);
        assert!(state.projection().add_open);
        Model::update(&mut state, Intent::ToggleAdd);
        assert!(!state.projection().add_open);
    }
    #[test]
    fn home_closes_layout_without_closing_add() {
        let mut state = State::default();
        Model::update(&mut state, Intent::SetAddOpen(true));
        Model::update(&mut state, Intent::SetLayoutOpen(true));
        Model::update(&mut state, Intent::ToolbarHidden);
        assert_eq!(
            state.projection(),
            Projection {
                add_open: true,
                layout_open: false
            }
        );
    }
    #[test]
    fn add_dismissals_and_unavailable_rows_never_emit_an_action() {
        let mut state = State::default();
        let widget = &quantick_workspace::canvas::WIDGETS[0];
        for (outside, escaped) in [(true, false), (false, true)] {
            Model::update(&mut state, Intent::SetAddOpen(true));
            assert_eq!(
                Model::update(
                    &mut state,
                    Intent::Pick {
                        widget,
                        available: false
                    }
                ),
                Effects::default()
            );
            assert!(state.projection().add_open);
            Model::update(&mut state, Intent::DismissAdd { outside, escaped });
            assert!(!state.projection().add_open);
        }
        Model::update(&mut state, Intent::ToggleAdd);
        assert_eq!(
            Model::update(
                &mut state,
                Intent::Pick {
                    widget,
                    available: true
                }
            ),
            Effects(Some(add_act(widget).1))
        );
        assert!(!state.projection().add_open);
        assert_eq!(
            Model::update(
                &mut state,
                Intent::Pick {
                    widget,
                    available: true
                }
            ),
            Effects::default()
        );
    }
}
