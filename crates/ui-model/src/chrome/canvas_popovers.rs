//! One application-wide owner for the toolbar popovers.
use quantick_control_handlers::canvas::{CanvasAct, add_act};
use quantick_workspace::canvas::{LayoutPreset, WidgetSpec, preset};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Projection {
    pub add_open: bool,
    pub layout_open: bool,
}
#[derive(Debug, Default)]
pub struct State(Projection, crate::layout_picker::State);
impl State {
    pub fn projection(&self) -> Projection {
        self.0
    }
    /// The layout picker's hover, press, focus and confirmation.
    pub fn picker(&self) -> &crate::layout_picker::State {
        &self.1
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
    /// What the window saw over the open layout picker this frame.
    Layout(crate::layout_picker::Seen),
    /// A capture's staging, with the current layout and the clock: open the
    /// picker held past its confirmation, then `pick=<id>` picks a layout,
    /// `focus=<id>` rings a cell for the keyboard.
    StageLayout(String, Option<&'static LayoutPreset>, u64),
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
        let effects = match intent {
            Intent::Layout(seen) if state.0.layout_open => {
                let step = state.1.step(seen);
                state.0.layout_open &= !step.close;
                return pick_act(step.pick);
            }
            Intent::StageLayout(hook, current, now_ms) => {
                state.0.layout_open = true;
                state.1.held = true;
                let (word, id) = hook.split_once('=').unwrap_or((hook.as_str(), ""));
                let named = preset(id);
                state.1.focus = named.filter(|_| word == "focus");
                let pick = named.filter(|named| word == "pick" && Some(*named) != current);
                if let Some(to) = pick {
                    let was = current;
                    state.1.changed = Some(crate::layout_picker::Changed {
                        to,
                        was,
                        at_ms: now_ms,
                    });
                }
                return pick_act(pick);
            }
            _ => layout_closed(state, intent),
        };
        if !state.0.layout_open {
            state.1 = crate::layout_picker::State::default();
        }
        effects
    }
}

/// The layouts a pass's `effects` picked, as the toolbar's one layout action.
pub fn picked(effects: Vec<crate::HostEffect>) -> Vec<crate::chrome_toolbar::Action> {
    let layout = |effect| match effect {
        crate::HostEffect::CanvasPopovers(Effects(Some(CanvasAct::PickLayout { preset_id }))) => {
            preset(&preset_id)
        }
        _ => None,
    };
    let picks = effects.into_iter().filter_map(layout);
    picks
        .map(crate::chrome_toolbar::Action::SetLayout)
        .collect()
}

/// A picked layout, as the act `canvas.layout.pick` takes.
fn pick_act(pick: Option<&'static LayoutPreset>) -> Effects {
    let preset_id = |preset: &LayoutPreset| preset.id.to_owned();
    Effects(pick.map(|pick| CanvasAct::PickLayout {
        preset_id: preset_id(pick),
    }))
}

/// Every intent but the picker's own frame and staging.
fn layout_closed(state: &mut State, intent: Intent) -> Effects {
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
        Intent::ToolbarHidden => state.0.layout_open = false,
        Intent::Layout(_) | Intent::StageLayout(..) => {}
    }
    Effects::default()
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
                colour: None,
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
        use crate::layout_picker::{Seen, Target};
        let flow = quantick_workspace::canvas::preset("flow");
        let charts = quantick_workspace::canvas::preset("charts-2");
        let mut state = State::default();
        Model::update(&mut state, Intent::ToggleLayout);
        assert!(state.projection().layout_open);
        Model::update(&mut state, Intent::Layout(Seen::default()));
        assert!(state.projection().layout_open);
        let outside = Seen {
            outside: true,
            ..Seen::default()
        };
        Model::update(&mut state, Intent::Layout(outside));
        assert!(!state.projection().layout_open);
        Model::update(&mut state, Intent::ToggleLayout);
        let over = charts.map(Target::Cell);
        let press = Seen {
            current: flow,
            over,
            press: true,
            ..Seen::default()
        };
        Model::update(&mut state, Intent::Layout(press));
        let release = Seen {
            release: true,
            ..press
        };
        assert_eq!(
            Model::update(&mut state, Intent::Layout(release)),
            Effects(Some(CanvasAct::PickLayout {
                preset_id: "charts-2".to_owned()
            }))
        );
        assert!(state.projection().layout_open, "the pick confirms first");
        assert!(state.picker().changed.is_some());
        Model::update(&mut state, Intent::SetLayoutOpen(false));
        assert_eq!(state.picker(), &crate::layout_picker::State::default());
        let staged = Intent::StageLayout("pick=charts-2".to_owned(), flow, 5);
        assert!(Model::update(&mut state, staged).0.is_some());
        assert!(state.picker().held && state.projection().layout_open);
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
