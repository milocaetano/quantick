#[cfg(test)]
mod cases {
    use super::super::*;
    use quantick_ui_model::indicator_settings::{Change, SettingsOutcome};

    #[test]
    fn preset_winner_executes_once_and_close_prevents_store_io() {
        let directory = crate::scratch::ScratchDir::new("indicator-settings-preset");
        let path = directory.path().join("presets.toml");
        let mut presets = crate::indicators::preset_file::PresetStore::load(&path);
        let kind = quantick_workspace::indicator_document::SavedKind::native("native.ema");
        let target = quantick_chart::indicator_registry::TabSlot {
            workspace: quantick_workspace::lifetime::WorkspaceId(1),
            tab: 1,
            pane: 1,
            side: crate::pane::PaneSide::Flow,
            slot: crate::indicator_worker::SlotId(2),
        };
        let mut editor = IndicatorSettingsSurface::new(View, effects);
        let open = || Intent::Open {
            target,
            title: "EMA".to_owned(),
            values: vec![quantick_indicators::InputValue::Int(9)],
        };
        editor.update(open());
        let finish = |escape| Intent::FinishPaint {
            changed: false,
            style_changed: false,
            outcome: SettingsOutcome::SavePreset(" mine ".to_owned()),
            window_closed: false,
            escape,
        };
        editor.with_updates(
            |response| assert!(response.model_effects.is_empty()),
            |_, session| {
                dispatch(
                    session,
                    Some(target),
                    Some(&kind),
                    &[],
                    &mut presets,
                    &path,
                    finish(true),
                );
            },
        );
        assert!(
            !path.exists(),
            "Escape overrides the pending save before IO"
        );
        assert!(presets.get(&kind, "mine").is_none());
        assert!(editor.model_state.target().is_none());
        editor.update(open());
        editor.update(Intent::EditPresetName(" mine ".to_owned()));
        editor.with_updates(
            |response| assert!(response.model_effects.is_empty()),
            |_, session| {
                dispatch(
                    session,
                    Some(target),
                    Some(&kind),
                    &[],
                    &mut presets,
                    &path,
                    finish(false),
                );
            },
        );
        assert!(path.exists());
        assert_eq!(presets.names_for(&kind).collect::<Vec<_>>(), vec!["mine"]);
        let dialog = editor.model_state.dialog().unwrap();
        assert_eq!(dialog.preset_label.as_deref(), Some("mine"));
        assert!(dialog.preset_name_draft.is_empty());
        let mut requests = 0;
        let specs = [InputSpec::Int {
            name: "length".to_owned(),
            title: "Length".to_owned(),
            default: 3,
            min: None,
            max: None,
            step: None,
            options: vec![],
        }];
        editor.with_updates(
            |response| {
                for effect in response.model_effects {
                    if let HostEffect::IndicatorSettings(effects) = effect {
                        assert!(effects.local_operation.is_none());
                        for change in effects.changes {
                            assert_eq!(
                                change,
                                Change::Inputs {
                                    target,
                                    values: vec![quantick_indicators::InputValue::Int(9)],
                                    commit: false
                                }
                            );
                            requests += 1;
                        }
                    }
                }
            },
            |_, session| {
                dispatch(
                    session,
                    Some(target),
                    Some(&kind),
                    &specs,
                    &mut presets,
                    &path,
                    Intent::FinishPaint {
                        changed: false,
                        style_changed: false,
                        outcome: SettingsOutcome::LoadPreset(Some("mine".to_owned())),
                        window_closed: false,
                        escape: false,
                    },
                );
            },
        );
        assert_eq!(
            requests, 1,
            "one terminal receipt emits one addressed preview"
        );
        assert!(editor.model_state.previewed());
    }
}
