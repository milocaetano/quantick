#[cfg(test)]
mod cases {
    use super::super::super::{Change, Effects, SettingsOutcome};
    use super::super::*;
    use crate::Adapter;

    fn target_slot() -> TabSlot {
        quantick_chart::indicator_registry::TabSlot {
            workspace: quantick_workspace::lifetime::WorkspaceId(1),
            tab: 1,
            pane: 1,
            side: quantick_chart_interaction::pane::PaneSide::Flow,
            slot: quantick_chart::indicator_views::IndicatorViews::default().allocate_slot("test"),
        }
    }

    #[test]
    fn malformed_preset_cell_preserves_its_position_and_previews_its_valid_neighbor() {
        use quantick_indicators::{Indicator, InputValue, SourceId, native::Ema};
        let directory = crate::scratch::ScratchDir::new("indicator-preset-rebind");
        let path = directory.path().join("presets.toml");
        let kind = quantick_workspace::indicator_document::SavedKind::native("native.ema");
        let mut presets = PresetStore::load(&path);
        assert!(presets.insert(
            &kind,
            "positional",
            vec![
                SavedInput::Source("not-a-source".into()),
                SavedInput::Source("open".into()),
            ]
        ));
        presets.save(&path);
        let mut presets = PresetStore::load(&path);
        let target = target_slot();
        let ema = Ema::new(9, SourceId::Close);
        let specs = &ema.descriptor().inputs;
        let original: Vec<_> = specs.iter().map(InputSpec::default_value).collect();
        let preview = vec![original[0].clone(), InputValue::Source(SourceId::Open)];
        let mut editor = Adapter::<Model, (), Effects>::new((), std::convert::identity);
        editor.update(Intent::Open {
            target,
            title: "EMA".into(),
            values: original.clone(),
        });
        let mut rebinds = Vec::new();
        let mut changes = Vec::new();
        editor.with_updates(
            |effects| changes.extend(effects.changes),
            |_, session| {
                PresetEditor {
                    target: Some(target),
                    kind: Some(&kind),
                    specs,
                    presets: &mut presets,
                    path: &path,
                }
                .dispatch(
                    session,
                    Intent::FinishPaint {
                        changed: false,
                        style_changed: false,
                        outcome: SettingsOutcome::LoadPreset(Some("positional".into())),
                        window_closed: false,
                        escape: false,
                    },
                    |name, saved, expected, kept| {
                        rebinds.push((name.to_owned(), saved, expected, kept))
                    },
                );
            },
        );
        assert_eq!(rebinds, [("positional".to_owned(), 2, 2, 1)]);
        assert_eq!(
            changes,
            [Change::Inputs {
                target,
                values: preview.clone(),
                commit: false
            }]
        );
        let dialog = editor.model_state.dialog().unwrap();
        assert_eq!(dialog.draft, preview);
        assert_eq!(dialog.committed, original);
        assert!(dialog.previewed);
        assert_eq!(dialog.preset_label.as_deref(), Some("positional"));
    }

    #[test]
    fn preset_winner_executes_once_and_close_prevents_store_io() {
        let directory = crate::scratch::ScratchDir::new("indicator-settings-preset");
        let path = directory.path().join("presets.toml");
        let mut presets = PresetStore::load(&path);
        let kind = quantick_workspace::indicator_document::SavedKind::native("native.ema");
        let target = target_slot();
        let mut editor = Adapter::<Model, (), Effects>::new((), std::convert::identity);
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
            |response| assert!(response == Effects::default()),
            |_, session| {
                PresetEditor {
                    target: Some(target),
                    kind: Some(&kind),
                    specs: &[],
                    presets: &mut presets,
                    path: &path,
                }
                .dispatch(session, finish(true), |_, _, _, _| {
                    panic!("unexpected rebound")
                });
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
            |response| assert!(response == Effects::default()),
            |_, session| {
                PresetEditor {
                    target: Some(target),
                    kind: Some(&kind),
                    specs: &[],
                    presets: &mut presets,
                    path: &path,
                }
                .dispatch(session, finish(false), |_, _, _, _| {
                    panic!("unexpected rebound")
                });
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
            |effects| {
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
            },
            |_, session| {
                PresetEditor {
                    target: Some(target),
                    kind: Some(&kind),
                    specs: &specs,
                    presets: &mut presets,
                    path: &path,
                }
                .dispatch(
                    session,
                    Intent::FinishPaint {
                        changed: false,
                        style_changed: false,
                        outcome: SettingsOutcome::LoadPreset(Some("mine".to_owned())),
                        window_closed: false,
                        escape: false,
                    },
                    |_, _, _, _| panic!("unexpected rebound"),
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
