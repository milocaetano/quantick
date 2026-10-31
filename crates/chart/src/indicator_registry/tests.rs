use super::*;

#[derive(Default)]
struct FakeHost {
    pane: u64,
    next: u64,
    added: Vec<(SlotId, String)>,
    removed: Vec<SlotId>,
}

impl IndicatorHost for FakeHost {
    fn pane_id(&self) -> u64 {
        self.pane
    }
    fn add(&mut self, source: IndicatorSource) -> SlotId {
        let slot = SlotId(self.next);
        self.next += 1;
        self.added.push((slot, source.kind_id()));
        slot
    }

    fn remove(&mut self, slot: SlotId) {
        self.removed.push(slot);
    }
}

#[test]
fn fake_hosts_exercise_production_ownership_and_complete_target_cleanup() {
    let mut slots = IndicatorRegistry::new(WorkspaceId(0));
    let mut human_host = FakeHost {
        pane: 100,
        ..Default::default()
    };
    let mut first_host = FakeHost {
        pane: 101,
        ..Default::default()
    };
    let mut second_host = FakeHost {
        pane: 200,
        ..Default::default()
    };
    let source = "indicator(\"probe\")\nplot(close)";
    let human = slots.attach_script(
        &mut human_host,
        (7, PaneSide::Flow),
        "human".into(),
        source.into(),
        false,
    );
    assert_eq!(
        human.layout_entry,
        Some(SavedKind::Script {
            name: "human".into()
        })
    );
    assert_eq!(slots.operator_target(0), Err(NotOperatorOwned));
    assert_eq!(slots.operator_target(99), Ok(None));
    let first = slots.attach_script(
        &mut first_host,
        (7, PaneSide::Time(0)),
        "first".into(),
        source.into(),
        true,
    );
    let second = slots.attach_script(
        &mut second_host,
        (8, PaneSide::Time(0)),
        "second".into(),
        source.into(),
        true,
    );
    assert!(first.layout_entry.is_none());
    assert!(second.layout_entry.is_none());
    assert_eq!(human.target.slot, first.target.slot);
    assert_eq!(first.target.slot, second.target.slot);
    assert_eq!(slots.operator_target(0), Ok(Some(first.target)));
    for target in [human.target, first.target, second.target] {
        slots.script_files.push((target, 3, SystemTime::UNIX_EPOCH));
        slots.pending_hidden.push(target);
        slots.pending_mouse_vertical_lines.push(target);
        slots
            .pending_styles
            .push((target, StyleOverride::default()));
    }
    slots.remove(Some(&mut first_host), first.target);
    assert_eq!(first_host.removed, [SlotId(0)]);
    assert!(human_host.removed.is_empty());
    assert!(second_host.removed.is_empty());
    let survivors = [human.target, second.target];
    assert_eq!(
        slots
            .slot_kinds
            .iter()
            .map(|(owner, _)| *owner)
            .collect::<Vec<_>>(),
        survivors
    );
    assert_eq!(
        slots
            .script_files
            .iter()
            .map(|(owner, ..)| *owner)
            .collect::<Vec<_>>(),
        survivors
    );
    assert_eq!(slots.pending_hidden, survivors);
    assert_eq!(slots.pending_mouse_vertical_lines, survivors);
    assert_eq!(
        slots
            .pending_styles
            .iter()
            .map(|(owner, _)| *owner)
            .collect::<Vec<_>>(),
        survivors
    );
    assert_eq!(slots.operator_slots, BTreeSet::from([second.target]));
    assert_eq!(slots.operator_target(0), Ok(Some(second.target)));
    // Silent layout cleanup also retires metadata when its pane has gone.
    slots.remove(None::<&mut FakeHost>, second.target);
    assert_eq!(slots.operator_target(0), Err(NotOperatorOwned));
    assert_eq!(human_host.added, [(SlotId(0), "script.human".into())]);
}
#[test]
fn independent_native_host_observes_exact_source_and_full_cleanup() {
    #[derive(Default)]
    struct NativeHost {
        sources: Vec<(String, usize)>,
        removed: Vec<SlotId>,
    }
    impl IndicatorHost for NativeHost {
        fn pane_id(&self) -> u64 {
            0
        }
        fn add(&mut self, source: IndicatorSource) -> SlotId {
            let IndicatorSource::Native { id, values } = source else {
                panic!("native attachment must send a native source");
            };
            self.sources.push((id, values.len()));
            SlotId(41)
        }
        fn remove(&mut self, slot: SlotId) {
            self.removed.push(slot);
        }
    }
    for id in ["native.ema", "native.nonesuch"] {
        let mut slots = IndicatorRegistry::new(WorkspaceId(0));
        let mut host = NativeHost::default();
        let attached = slots.attach_native(&mut host, (73, PaneSide::Time(1)), id);
        let expected = TabSlot {
            workspace: WorkspaceId(0),
            pane: 0,
            tab: 73,
            side: PaneSide::Time(1),
            slot: SlotId(41),
        };
        let expected_kind = SavedKind::Native { id: id.to_owned() };
        assert_eq!(host.sources, [(id.to_owned(), 0)]);
        assert_eq!(attached.target, expected);
        assert_eq!(attached.layout_entry, Some(expected_kind.clone()));
        assert_eq!(slots.slot_kinds, [(expected, expected_kind)]);
        assert!(slots.operator_slots.is_empty());
        assert_eq!(slots.operator_target(41), Err(NotOperatorOwned));
        assert!(slots.script_files.is_empty());
        assert!(slots.pending_hidden.is_empty());
        assert!(slots.pending_styles.is_empty());
        let survivor = TabSlot {
            tab: 74,
            ..expected
        };
        for target in [expected, survivor] {
            slots.script_files.push((target, 3, SystemTime::UNIX_EPOCH));
            slots.pending_hidden.push(target);
            slots.pending_mouse_vertical_lines.push(target);
            slots
                .pending_styles
                .push((target, StyleOverride::default()));
        }
        slots.remove(Some(&mut host), expected);
        assert_eq!(host.removed, [SlotId(41)]);
        assert!(slots.slot_kinds.is_empty());
        assert!(slots.operator_slots.is_empty());
        assert_eq!(slots.script_files, [(survivor, 3, SystemTime::UNIX_EPOCH)]);
        assert_eq!(slots.pending_hidden, [survivor]);
        assert_eq!(slots.pending_styles.len(), 1);
        assert_eq!(slots.pending_styles[0].0, survivor);
    }
}

#[test]
fn forgetting_a_tab_clears_all_slot_collections_without_touching_other_tabs() {
    let owners = [7, 9].map(|tab| TabSlot {
        workspace: WorkspaceId(0),
        pane: 0,
        tab,
        side: PaneSide::Flow,
        slot: SlotId(0),
    });
    let mut state = IndicatorRegistry::new(WorkspaceId(0));
    for owner in owners {
        state.slot_kinds.push((owner, SavedKind::native("cvd")));
        state.operator_slots.insert(owner);
        state
            .script_files
            .push((owner, 0, std::time::SystemTime::UNIX_EPOCH));
        state.pending_hidden.push(owner);
        state.pending_mouse_vertical_lines.push(owner);
        state.pending_styles.push((owner, StyleOverride::default()));
    }
    state.forget_tab(7);
    assert_eq!(
        state
            .slot_kinds
            .iter()
            .map(|(owner, _)| *owner)
            .collect::<Vec<_>>(),
        [owners[1]]
    );
    assert_eq!(
        state.operator_slots.iter().copied().collect::<Vec<_>>(),
        [owners[1]]
    );
    assert_eq!(
        state
            .script_files
            .iter()
            .map(|(owner, ..)| *owner)
            .collect::<Vec<_>>(),
        [owners[1]]
    );
    assert_eq!(state.pending_hidden, [owners[1]]);
    assert_eq!(state.pending_mouse_vertical_lines, [owners[1]]);
    assert_eq!(
        state
            .pending_styles
            .iter()
            .map(|(owner, _)| *owner)
            .collect::<Vec<_>>(),
        [owners[1]]
    );
    state.forget_tab(7);
    assert_eq!(
        state.pending_hidden,
        [owners[1]],
        "repeated close notification is harmless"
    );
}

#[derive(Default)]
struct ViewHost(std::collections::BTreeMap<TabSlot, crate::indicator_views::IndicatorViews>);
impl IndicatorViewHost for ViewHost {
    fn view_mut(&mut self, target: TabSlot) -> Option<&mut crate::indicator_views::IndicatorView> {
        self.0.get_mut(&target)?.view_mut(target.slot)
    }
}
impl ViewHost {
    fn publish(&mut self, target: TabSlot, overlay: bool) {
        let mut views = crate::indicator_views::IndicatorViews::new();
        assert_eq!(views.allocate_slot("test"), target.slot);
        views.apply(quantick_indicator_session::IndicatorEvent::Rebuilt {
            slot: target.slot,
            descriptor: quantick_indicators::IndicatorDescriptor {
                title: "test".into(),
                short_title: None,
                overlay,
                plots: Vec::new(),
                fills: Vec::new(),
                inputs: Vec::new(),
            },
            columns: Vec::new(),
            bar_paint: Vec::new(),
            rows: 0,
            inputs: Vec::new(),
            stale: None,
        });
        self.0.insert(target, views);
    }
}

#[test]
fn deferred_state_retries_known_targets_applies_exact_style_once_and_discards_stale_work() {
    use crate::indicator_style::PlotOverride;
    use quantick_indicators::Rgba8;
    let mut registry = IndicatorRegistry::new(WorkspaceId(12));
    let mut views = ViewHost::default();
    let targets = [7, 9, 11].map(|tab| {
        registry
            .attach_native(
                &mut FakeHost::default(),
                (tab, PaneSide::Flow),
                "native.ema",
            )
            .target
    });
    let style = StyleOverride::from_plots(vec![
        PlotOverride {
            visible: Some(false),
            color: Some(Rgba8 {
                r: 9,
                g: 17,
                b: 33,
                a: 71,
            }),
            width: Some(0.75),
        },
        PlotOverride::default(),
        PlotOverride {
            width: Some(6.125),
            ..Default::default()
        },
    ]);
    for target in targets {
        registry.defer_hidden(target);
        registry.defer_style(target, style.clone());
        registry.defer_mouse_vertical_line(target);
    }
    // Unknown work can survive an adapter handoff, but must never retry forever.
    let stale = TabSlot {
        tab: 99,
        ..targets[0]
    };
    registry.defer_hidden(stale);
    registry.defer_style(stale, style.clone());
    registry.defer_mouse_vertical_line(stale);
    registry.apply_pending(&mut views);
    assert_eq!(registry.pending_hidden(), targets);
    assert_eq!(registry.pending_mouse_vertical_lines(), targets);
    assert_eq!(registry.pending_styles().len(), 3);
    assert!(
        registry
            .pending_styles()
            .iter()
            .all(|(_, queued)| queued == &style)
    );
    registry.remove(None::<&mut FakeHost>, targets[2]);
    views.publish(targets[0], false);
    views.publish(targets[1], true);
    registry.apply_pending(&mut views);
    assert!(registry.pending_hidden().is_empty());
    assert!(registry.pending_styles().is_empty());
    assert!(registry.pending_mouse_vertical_lines().is_empty());
    for target in &targets[..2] {
        let view = views.view_mut(*target).unwrap();
        assert!(view.hidden);
        assert_eq!(view.style, style);
        assert_eq!(view.mouse_vertical_line, *target == targets[0]);
    }
    registry.apply_pending(&mut views);
    assert!(
        views.view_mut(targets[0]).unwrap().hidden,
        "a second drain cannot toggle twice"
    );
}

#[test]
fn pane_moves_keep_stable_identity_and_remap_every_owned_queue_in_both_directions() {
    let mut registry = IndicatorRegistry::new(WorkspaceId(12));
    let mut targets = Vec::new();
    for (tab, side) in [
        (7, PaneSide::Flow),
        (7, PaneSide::Time(0)),
        (7, PaneSide::Time(1)),
        (7, PaneSide::Time(2)),
        (9, PaneSide::Time(0)),
    ] {
        let target = registry
            .attach_script(
                &mut FakeHost::default(),
                (tab, side),
                "test".into(),
                "source".into(),
                true,
            )
            .target;
        registry.watch_script(target, targets.len(), SystemTime::UNIX_EPOCH);
        registry.defer_hidden(target);
        registry.defer_style(target, StyleOverride::default());
        registry.defer_mouse_vertical_line(target);
        targets.push(target);
    }
    for (from, to, sides) in [
        (
            1,
            3,
            [
                PaneSide::Flow,
                PaneSide::Time(2),
                PaneSide::Time(0),
                PaneSide::Time(1),
                PaneSide::Time(0),
            ],
        ),
        (
            3,
            1,
            [
                PaneSide::Flow,
                PaneSide::Time(0),
                PaneSide::Time(1),
                PaneSide::Time(2),
                PaneSide::Time(0),
            ],
        ),
    ] {
        registry.move_context_pane(7, from, to);
        let expected: Vec<_> = targets
            .iter()
            .zip(sides)
            .map(|(target, side)| TabSlot { side, ..*target })
            .collect();
        assert_eq!(
            registry
                .slot_kinds()
                .iter()
                .map(|(target, _)| *target)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            registry
                .script_files()
                .iter()
                .map(|(target, ..)| *target)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            registry
                .pending_styles()
                .iter()
                .map(|(target, _)| *target)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(registry.pending_hidden(), expected);
        assert_eq!(registry.pending_mouse_vertical_lines(), expected);
        assert_eq!(
            registry.operator_slots(),
            &expected.iter().copied().collect()
        );
        assert_eq!(
            expected
                .iter()
                .map(|target| target.address())
                .collect::<Vec<_>>(),
            targets
                .iter()
                .map(|target| target.address())
                .collect::<Vec<_>>()
        );
    }
    registry.forget_tab(7);
    assert_eq!(registry.slot_kinds().len(), 1);
    assert_eq!(registry.operator_target(0), Ok(Some(targets[4])));
    registry.clear();
    assert_eq!(registry.operator_target(0), Ok(None));
    assert!(registry.script_files().is_empty());
    assert!(registry.pending_hidden().is_empty());
    assert!(registry.pending_styles().is_empty());
    assert!(registry.pending_mouse_vertical_lines().is_empty());
}

#[test]
fn restored_registration_rejects_foreign_and_duplicate_targets_and_keeps_layout_order() {
    let mut registry = IndicatorRegistry::new(WorkspaceId(12));
    let first = registry
        .attach_native(&mut FakeHost::default(), (7, PaneSide::Flow), "native.ema")
        .target;
    let operator = registry
        .attach_script(
            &mut FakeHost {
                next: 1,
                ..Default::default()
            },
            (7, PaneSide::Flow),
            "operator".into(),
            "source".into(),
            true,
        )
        .target;
    let last = TabSlot {
        slot: SlotId(2),
        ..first
    };
    assert!(!registry.register_restored(first, SavedKind::native("duplicate")));
    assert!(!registry.register_restored(
        TabSlot {
            workspace: WorkspaceId(13),
            ..last
        },
        SavedKind::native("foreign")
    ));
    assert!(registry.register_restored(last, SavedKind::native("last")));
    assert_eq!(
        registry.layout_slots_at(7, PaneSide::Flow),
        [first.slot, last.slot]
    );
    registry.exclude_from_layout(first);
    assert_eq!(registry.layout_slots_at(7, PaneSide::Flow), [last.slot]);
    assert_eq!(
        registry.operator_target(operator.slot.0),
        Ok(Some(operator))
    );
}
