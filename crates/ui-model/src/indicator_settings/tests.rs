#[cfg(test)]
mod cases {
    use super::super::*;
    use crate::Model as _;
    use quantick_chart::indicator_views::IndicatorViews;
    use quantick_chart_interaction::pane::PaneSide;
    use quantick_workspace::lifetime::WorkspaceId;

    fn target() -> TabSlot {
        TabSlot {
            workspace: WorkspaceId(42),
            tab: 9,
            pane: 12,
            side: PaneSide::from_index(1),
            slot: IndicatorViews::default().allocate_slot("test"),
        }
    }

    fn opened() -> State {
        let mut state = State::default();
        assert_eq!(
            Model::update(
                &mut state,
                Intent::Open {
                    target: target(),
                    title: "Length".to_owned(),
                    values: vec![InputValue::Int(10)],
                }
            ),
            Effects::default()
        );
        state
    }

    fn paint(state: &mut State, changed: bool, outcome: SettingsOutcome) -> Effects {
        Model::update(
            state,
            Intent::FinishPaint {
                changed,
                style_changed: false,
                outcome,
                window_closed: false,
                escape: false,
            },
        )
    }

    fn edit(state: &mut State, value: i64) {
        assert_eq!(
            Model::update(
                state,
                Intent::EditInput {
                    index: 0,
                    value: InputValue::Int(value),
                }
            ),
            Effects::default()
        );
    }

    fn inputs(value: i64, commit: bool) -> Effects {
        Effects {
            local_operation: None,
            changes: vec![Change::Inputs {
                target: target(),
                values: vec![InputValue::Int(value)],
                commit,
            }],
        }
    }

    #[test]
    fn indicator_settings_keeps_one_address_and_preview_lifecycle() {
        let mut state = opened();
        assert_eq!(
            paint(&mut state, false, SettingsOutcome::Open),
            Effects::default()
        );
        edit(&mut state, 20);
        assert_eq!(
            paint(&mut state, true, SettingsOutcome::Open),
            inputs(20, false)
        );
        assert!(state.previewed());
        assert_eq!(state.target(), Some(target()));
        assert_eq!(state.dialog().unwrap().committed, vec![InputValue::Int(10)]);
        assert_eq!(Model::update(&mut state, Intent::Apply), inputs(20, true));
        assert!(!state.previewed());
        assert_eq!(state.dialog().unwrap().committed, vec![InputValue::Int(20)]);
        edit(&mut state, 30);
        assert_eq!(
            Model::update(&mut state, Intent::Preview),
            inputs(30, false)
        );
        assert_eq!(Model::update(&mut state, Intent::Close), inputs(20, false));
        assert!(state.dialog().is_none());
        assert!(state.target().is_none());
        assert!(!state.previewed());
        for intent in [Intent::Apply, Intent::Preview, Intent::Close] {
            assert_eq!(Model::update(&mut state, intent), Effects::default());
        }
    }

    #[test]
    fn indicator_settings_normalization_and_outcome_precedence() {
        let mut state = opened();
        edit(&mut state, 11);
        assert_eq!(
            paint(&mut state, true, SettingsOutcome::Open),
            Effects::default()
        );
        assert!(state.dialog().unwrap().settled);
        assert_eq!(state.dialog().unwrap().committed, vec![InputValue::Int(11)]);
        assert!(!state.previewed());
        Model::update(
            &mut state,
            Intent::PresetSaved {
                target: target(),
                name: "named".into(),
            },
        );
        edit(&mut state, 12);
        assert_eq!(
            paint(&mut state, true, SettingsOutcome::Apply),
            inputs(12, true)
        );
        assert_eq!(
            state.dialog().unwrap().preset_label.as_deref(),
            Some("named")
        );
        edit(&mut state, 13);
        let effects = Model::update(
            &mut state,
            Intent::FinishPaint {
                changed: true,
                style_changed: true,
                outcome: SettingsOutcome::Open,
                window_closed: false,
                escape: false,
            },
        );
        assert_eq!(effects, inputs(13, false));
        assert!(state.dialog().unwrap().preset_label.is_none());
        assert_eq!(
            Model::update(
                &mut state,
                Intent::FinishPaint {
                    changed: false,
                    style_changed: true,
                    outcome: SettingsOutcome::Open,
                    window_closed: false,
                    escape: false,
                }
            ),
            Effects {
                local_operation: None,
                changes: vec![Change::Style(target())]
            }
        );
        for close_by_escape in [false, true] {
            let mut state = opened();
            for candidate in [
                SettingsOutcome::LoadPreset(None),
                SettingsOutcome::SavePreset("saved".into()),
                SettingsOutcome::DeletePreset("saved".into()),
                SettingsOutcome::Apply,
            ] {
                let effects = Model::update(
                    &mut state,
                    Intent::FinishPaint {
                        changed: true,
                        style_changed: true,
                        outcome: candidate,
                        window_closed: !close_by_escape,
                        escape: close_by_escape,
                    },
                );
                assert_eq!(
                    effects,
                    Effects::default(),
                    "close overrides every candidate before IO"
                );
                assert!(state.target().is_none());
                state = opened();
            }
        }
        let mut state = opened();
        edit(&mut state, 15);
        assert_eq!(
            paint(&mut state, true, SettingsOutcome::Apply),
            inputs(15, true)
        );
        assert_eq!(Model::update(&mut state, Intent::Close), Effects::default());
    }

    #[test]
    fn indicator_settings_preset_receipts_preserve_positional_values() {
        let mut state = opened();
        paint(&mut state, false, SettingsOutcome::Open);
        assert_eq!(
            paint(
                &mut state,
                false,
                SettingsOutcome::LoadPreset(Some("pair".into()))
            ),
            Effects {
                local_operation: Some(LocalOperation::Load {
                    target: target(),
                    name: Some("pair".into())
                }),
                changes: vec![],
            }
        );
        // The authoritative binder supplies both positions, including the default
        // substituted at the unreadable cell. This model must not rebind or filter.
        let values = vec![InputValue::Int(10), InputValue::Int(77)];
        assert_eq!(
            Model::update(
                &mut state,
                Intent::PresetLoaded {
                    target: target(),
                    values: values.clone(),
                    label: "pair".into(),
                }
            ),
            Effects {
                local_operation: None,
                changes: vec![Change::Inputs {
                    target: target(),
                    values: values.clone(),
                    commit: false,
                }]
            }
        );
        assert_eq!(state.dialog().unwrap().draft, values);
        assert_eq!(state.dialog().unwrap().committed, vec![InputValue::Int(10)]);
        assert_eq!(
            state.dialog().unwrap().preset_label.as_deref(),
            Some("pair")
        );
        Model::update(&mut state, Intent::EditPresetName(" saved ".into()));
        assert_eq!(
            paint(
                &mut state,
                false,
                SettingsOutcome::SavePreset("saved".into())
            ),
            Effects {
                local_operation: Some(LocalOperation::Save {
                    target: target(),
                    name: "saved".into(),
                    values
                }),
                changes: vec![],
            }
        );
        assert_eq!(state.dialog().unwrap().preset_name_draft, " saved ");
        Model::update(
            &mut state,
            Intent::PresetSaved {
                target: target(),
                name: " saved ".into(),
            },
        );
        assert_eq!(
            state.dialog().unwrap().preset_label.as_deref(),
            Some("saved")
        );
        assert!(state.dialog().unwrap().preset_name_draft.is_empty());
        Model::update(
            &mut state,
            Intent::PresetDeleted {
                target: target(),
                name: "other".into(),
            },
        );
        assert_eq!(
            state.dialog().unwrap().preset_label.as_deref(),
            Some("saved")
        );
        assert_eq!(
            paint(
                &mut state,
                false,
                SettingsOutcome::DeletePreset("saved".into())
            ),
            Effects {
                local_operation: Some(LocalOperation::Delete {
                    target: target(),
                    name: "saved".into()
                }),
                changes: vec![],
            }
        );
        Model::update(
            &mut state,
            Intent::PresetDeleted {
                target: target(),
                name: "saved".into(),
            },
        );
        assert!(state.dialog().unwrap().preset_label.is_none());
    }

    #[test]
    fn indicator_settings_invalid_target_closes_without_retargeting() {
        let mut state = opened();
        Model::update(&mut state, Intent::SelectTab(SettingsTab::Style));
        Model::update(
            &mut state,
            Intent::ObserveTarget {
                valid: true,
                label: "updated".into(),
                has_style: false,
            },
        );
        assert_eq!(state.dialog().unwrap().title, "updated");
        assert_eq!(state.dialog().unwrap().tab, SettingsTab::Inputs);
        assert_eq!(state.target(), Some(target()));
        edit(&mut state, 99);
        Model::update(&mut state, Intent::Preview);
        assert_eq!(
            Model::update(
                &mut state,
                Intent::ObserveTarget {
                    valid: false,
                    label: "focused other pane".into(),
                    has_style: true
                }
            ),
            Effects::default()
        );
        assert!(state.target().is_none());
        assert_eq!(
            Model::update(
                &mut state,
                Intent::PresetLoaded {
                    target: target(),
                    values: vec![InputValue::Int(1)],
                    label: "late".into()
                }
            ),
            Effects::default()
        );
        assert!(state.dialog().is_none());
        let mut state = opened();
        let mut other = target();
        other.pane += 1;
        assert_eq!(
            Model::update(
                &mut state,
                Intent::PresetLoaded {
                    target: other,
                    values: vec![InputValue::Int(99)],
                    label: "foreign".into()
                }
            ),
            Effects::default()
        );
        assert_eq!(state.dialog().unwrap().draft, vec![InputValue::Int(10)]);
    }

    #[test]
    fn indicator_settings_move_context_remaps_only_its_side() {
        let mut state = opened();
        Model::update(
            &mut state,
            Intent::ContextPaneMoved {
                tab: 10,
                from: 1,
                to: 2,
            },
        );
        assert_eq!(state.target(), Some(target()));
        Model::update(
            &mut state,
            Intent::ContextPaneMoved {
                tab: 9,
                from: 1,
                to: 2,
            },
        );
        let mut moved = target();
        moved.side = PaneSide::from_index(2);
        assert_eq!(state.target(), Some(moved));
        assert_eq!(state.target().unwrap().pane, target().pane);
        Model::update(
            &mut state,
            Intent::ContextPaneMoved {
                tab: 9,
                from: 3,
                to: 1,
            },
        );
        moved.side = remap_side(moved.side, 3, 1);
        assert_eq!(state.target(), Some(moved));
        Model::update(&mut state, Intent::Close);
        Model::update(
            &mut state,
            Intent::ContextPaneMoved {
                tab: 9,
                from: 1,
                to: 2,
            },
        );
        assert!(state.target().is_none());
    }

    fn stepped_float(title: &str) -> InputSpec {
        InputSpec::Float {
            name: "x".to_owned(),
            title: title.to_owned(),
            default: 0.1,
            min: Some(0.0),
            max: Some(0.5),
            step: Some(0.05),
            options: Vec::new(),
        }
    }

    #[test]
    fn display_rows_hoist_to_the_top_in_render_order_only() {
        let specs = vec![
            stepped_float("1 Context: max height (×ATR)"),
            InputSpec::Bool {
                name: "show".to_owned(),
                title: "Display: context semaphore".to_owned(),
                default: true,
            },
            stepped_float("Advanced: min bar body (×ATR)"),
        ];
        assert_eq!(row_order(&specs), vec![1, 0, 2]);
    }

    #[test]
    fn titles_split_into_section_and_label() {
        assert_eq!(
            split_section("1 Context: window (bars)"),
            ("1 Context", "window (bars)")
        );
        assert_eq!(split_section("Length"), ("", "Length"));
    }
}
