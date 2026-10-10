use super::*;
use crate::pane::{ContextPress, Effect, Intent, update};
fn facts() -> Facts {
    Facts {
        layers: vec![],
        flow_opening: None,
        tape: None,
        drawing: None,
        places: vec![],
        indicators: vec![],
        objects: vec![],
    }
}
fn object(id: u64, index: usize) -> ObjectFact {
    ObjectFact {
        id: DrawingId(id),
        index,
        name: format!("object {id}"),
        selected: false,
        hidden: false,
        locked: true,
        author: Some("agent".into()),
        band: None,
        foreign_market: true,
        off_series: true,
        shared: true,
    }
}
#[test]
fn descriptors_pin_empty_sections_utf8_and_captured_tape_order() {
    let mut model = Model::default();
    let _ = update(
        &mut model,
        Intent::OpenMenu(ContextPress {
            price: 123.0,
            on_tape: true,
            drawing: None,
            places: vec![],
        }),
    );
    let mut facts = facts();
    facts.tape = Some(TapeFact {
        window: LaneWindow::Fixed { ms: 60_000 },
        reference_ms: Some(20_000),
    });
    let entries = describe(&model, &facts);
    assert_eq!(
        entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        [
            "chart layers",
            "objects (0)",
            "clear objects\u{2026}",
            "",
            "",
            "",
            "tape",
            "tape window: 1 min",
            ""
        ]
    );
    assert_eq!(
        entries[0].hint.as_deref(),
        Some("what the candles beside the tape draw")
    );
    assert_eq!(
        entries[1].disabled.as_deref(),
        Some("nothing is drawn on this chart")
    );
    assert!(matches!(entries[4].kind, Kind::Trade(123.0)));
    let Kind::Submenu(windows) = &entries[7].kind else {
        panic!("window submenu")
    };
    let choices: Vec<_> = windows
        .iter()
        .filter_map(|e| match &e.kind {
            Kind::Select {
                selected, choice, ..
            } => Some((*selected, choice.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(choices.len(), 6);
    for (i, ms) in LANE_WINDOW_PRESETS_MS.into_iter().enumerate() {
        assert_eq!(
            choices[i + 1],
            (
                ms == 60_000,
                MenuIntent::SetLaneWindow(LaneWindow::Fixed { ms })
            )
        );
    }
    assert!(matches!(
        windows.last().unwrap().kind,
        Kind::Seconds {
            value: 60.0,
            minimum: 0.2,
            ..
        }
    ));
    let inverted = inverted_entry(false);
    assert_eq!(inverted.label, "Inverted chart");
    assert_eq!(
        inverted.hint.as_deref(),
        Some(
            "flip the chart upside down \u{2014} low prices at the top. Also reached by dragging the axis down until the bars flatten and turn over"
        )
    );
}
#[test]
fn locked_drawing_and_objects_keep_verbs_stable_ids_and_chip_order() {
    let mut facts = facts();
    facts.drawing = Some(DrawingMenuFact {
        drawing: DrawingFact {
            id: DrawingId(7),
            name: "region".into(),
            locked: true,
        },
        label: "rectangle".into(),
        hidden: true,
        strategy_region: true,
        strategy: None,
    });
    let entries = describe(&Model::default(), &facts);
    assert_eq!(
        entries[..6]
            .iter()
            .map(|e| e.label.as_str())
            .collect::<Vec<_>>(),
        [
            "rectangle",
            "name this object",
            "Add strategy\u{2026}",
            "Unlock",
            "Show",
            "Delete"
        ]
    );
    assert_eq!(
        entries[5].disabled.as_deref(),
        Some("unlock first \u{2014} a locked object never deletes by accident")
    );
    let rows = object_entries(&[object(7, 0), object(99, 1)]);
    let Kind::Row { children, .. } = &rows[0].kind else {
        panic!("row")
    };
    assert_eq!(
        children[..6]
            .iter()
            .map(|e| e.label.as_str())
            .collect::<Vec<_>>(),
        [
            "object 99",
            "assistant",
            "locked",
            "other market",
            "off series",
            "all charts"
        ]
    );
    assert!(matches!(
        children[0].kind,
        Kind::Select {
            choice: MenuIntent::ObjectsAsk(ObjectAction::Select(DrawingId(99))),
            ..
        }
    ));
    let Kind::Row {
        children: actions,
        reverse,
    } = &children[6].kind
    else {
        panic!("actions")
    };
    assert!(*reverse);
    assert_eq!(
        actions.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
        ["Delete", "Front", "Unlock", "Hide"]
    );
    assert!(actions.iter().all(|e| e.disabled.is_none()));
}
#[test]
fn stale_capture_discards_draft_and_clear_requires_fresh_confirmation() {
    let mut model = Model::default();
    let _ = update(
        &mut model,
        Intent::OpenMenu(ContextPress {
            price: 100.0,
            on_tape: false,
            drawing: Some(DrawingFact {
                id: DrawingId(7),
                name: "old".into(),
                locked: false,
            }),
            places: vec![],
        }),
    );
    let _ = update(
        &mut model,
        Intent::RenameDraft {
            text: "half typed".into(),
            blur: false,
            drawing: None,
        },
    );
    assert!(update(&mut model, Intent::RefreshMenu { drawing: None }).is_empty());
    assert_eq!(model.menu.drawing, None);
    assert!(model.menu.rename.is_empty());
    let _ = update(&mut model, Intent::AskClear { count: 2 });
    assert!(
        update(
            &mut model,
            Intent::AnswerClear {
                count: 0,
                answer: true
            }
        )
        .is_empty()
    );
    let _ = update(&mut model, Intent::AskClear { count: 2 });
    assert!(
        update(
            &mut model,
            Intent::AnswerClear {
                count: 2,
                answer: false
            }
        )
        .is_empty()
    );
    let _ = update(&mut model, Intent::AskClear { count: 2 });
    assert_eq!(
        update(
            &mut model,
            Intent::AnswerClear {
                count: 2,
                answer: true
            }
        ),
        [Effect::Menu(MenuIntent::ObjectsAsk(
            ObjectAction::DeleteAll
        ))]
    );
    assert!(
        update(
            &mut model,
            Intent::AnswerClear {
                count: 2,
                answer: true
            }
        )
        .is_empty()
    );
}
#[test]
fn axis_mode_resets_once_per_change() {
    use crate::pane_axis::{ScaleAction, ScaleTarget};
    let mut model = Model::default();
    assert!(
        update(
            &mut model,
            Intent::PriceAxisMode {
                tape_only: false,
                native_tape: false
            }
        )
        .is_empty()
    );
    assert_eq!(
        update(
            &mut model,
            Intent::PriceAxisMode {
                tape_only: false,
                native_tape: true
            }
        ),
        [Effect::Scale {
            target: ScaleTarget::Price,
            action: ScaleAction::Reset
        }]
    );
    assert!(
        update(
            &mut model,
            Intent::PriceAxisMode {
                tape_only: false,
                native_tape: true
            }
        )
        .is_empty()
    );
    assert_eq!(
        update(
            &mut model,
            Intent::Menu {
                choice: MenuIntent::SetPriceInverted(true),
                drawing: None
            }
        ),
        [Effect::Menu(MenuIntent::SetPriceInverted(true))]
    );
}

#[test]
fn blocked_layer_retains_requested_check_and_explanation() {
    let entry = layer_entry(LayerFact {
        layer: ChartLayer::TapeHeatmap,
        visible: true,
        blocked: Some(LayerBlock::new("book", "this feed has no book")),
    });
    assert_eq!(entry.label, "L2 heatmap");
    assert_eq!(entry.disabled.as_deref(), Some("this feed has no book"));
    assert!(matches!(
        entry.kind,
        Kind::Check {
            checked: true,
            choice: MenuIntent::SetLayerVisible {
                layer: ChartLayer::TapeHeatmap,
                visible: false
            },
            ..
        }
    ));
}
