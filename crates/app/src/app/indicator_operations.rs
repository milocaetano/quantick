//! Rare indicator mutations, independent of window focus and layout storage.
//!
//! The adapter supplies a resolved pane host and only the slot bookkeeping.
//! It mirrors a human attachment after registration, mirrors removal before
//! cleanup, and records save intent once after the operation.

use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::indicator_style::StyleOverride;
use crate::indicator_worker::{IndicatorCommand, IndicatorSource, SlotId};
use crate::indicators::state_file::SavedKind;
use crate::pane::{ChartPane, PaneSide};

use super::TabSlot;

/// The two host effects needed by attachment and removal. No focus, layout,
/// library, compilation policy or authority can be changed through this port.
pub(super) trait IndicatorHost {
    fn add(&mut self, source: IndicatorSource) -> SlotId;
    fn remove(&mut self, slot: SlotId);
}

impl IndicatorHost for ChartPane {
    fn add(&mut self, source: IndicatorSource) -> SlotId {
        self.add_indicator(source)
    }

    fn remove(&mut self, slot: SlotId) {
        // Render-side removal precedes the worker command; in-flight events
        // for a removed slot are discarded by the existing views owner.
        self.indicators.remove(slot);
        self.indicator_worker.send(IndicatorCommand::Remove(slot));
    }
}

/// A borrowed operation context, not another state owner. The existing
/// IndicatorState retains these collections and its independent UI state.
pub(super) struct IndicatorSlots<'a> {
    pub slot_kinds: &'a mut Vec<(TabSlot, SavedKind)>,
    pub operator_slots: &'a mut BTreeSet<TabSlot>,
    pub script_files: &'a mut Vec<(TabSlot, usize, SystemTime)>,
    pub pending_hidden: &'a mut Vec<TabSlot>,
    pub pending_styles: &'a mut Vec<(TabSlot, StyleOverride)>,
}

/// The adapter has enough information to mirror a human change without the
/// operation borrowing layout state. Operator overlays have no layout entry.
pub(super) struct ScriptAttachment {
    pub target: TabSlot,
    pub layout_entry: Option<SavedKind>,
}

impl IndicatorSlots<'_> {
    pub fn attach_script(
        &mut self,
        host: &mut impl IndicatorHost,
        (tab, side): (u64, PaneSide),
        name: String,
        text: String,
        by_operator: bool,
    ) -> ScriptAttachment {
        let slot = host.add(IndicatorSource::Script {
            name: name.clone(),
            text,
        });
        let target = TabSlot { tab, side, slot };
        let kind = SavedKind::Script { name };
        self.slot_kinds.push((target, kind.clone()));
        if by_operator {
            self.operator_slots.insert(target);
        }
        ScriptAttachment {
            target,
            layout_entry: (!by_operator).then_some(kind),
        }
    }

    /// Legacy v1 addressing: insertion-order first operator-owned match.
    /// Whole targets remain intact internally even when local numbers collide.
    pub fn operator_target(&self, slot: u64) -> Result<Option<TabSlot>, ()> {
        let mut known = false;
        for (owner, _) in self.slot_kinds.iter() {
            if owner.slot.0 == slot {
                known = true;
                if self.operator_slots.contains(owner) {
                    return Ok(Some(*owner));
                }
            }
        }
        if known { Err(()) } else { Ok(None) }
    }

    /// Also used by silent layout removal. A pane that has already gone has
    /// no host effect, but its bookkeeping must still be retired.
    pub fn remove(&mut self, host: Option<&mut impl IndicatorHost>, target: TabSlot) {
        if let Some(host) = host {
            host.remove(target.slot);
        }
        self.slot_kinds.retain(|(owner, _)| *owner != target);
        self.operator_slots.remove(&target);
        self.script_files.retain(|(owner, ..)| *owner != target);
        self.pending_hidden.retain(|owner| *owner != target);
        self.pending_styles.retain(|(owner, _)| *owner != target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeHost {
        next: u64,
        added: Vec<(SlotId, String)>,
        removed: Vec<SlotId>,
    }

    impl IndicatorHost for FakeHost {
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
        let mut kinds = Vec::new();
        let mut operators = BTreeSet::new();
        let mut files = Vec::new();
        let mut hidden = Vec::new();
        let mut styles = Vec::new();
        let mut slots = IndicatorSlots {
            slot_kinds: &mut kinds,
            operator_slots: &mut operators,
            script_files: &mut files,
            pending_hidden: &mut hidden,
            pending_styles: &mut styles,
        };
        let mut human_host = FakeHost::default();
        let mut first_host = FakeHost::default();
        let mut second_host = FakeHost::default();
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
        assert_eq!(slots.operator_target(0), Err(()));
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
        assert_eq!(*slots.pending_hidden, survivors);
        assert_eq!(
            slots
                .pending_styles
                .iter()
                .map(|(owner, _)| *owner)
                .collect::<Vec<_>>(),
            survivors
        );
        assert_eq!(*slots.operator_slots, BTreeSet::from([second.target]));
        assert_eq!(slots.operator_target(0), Ok(Some(second.target)));
        // Silent layout cleanup also retires metadata when its pane has gone.
        slots.remove(None::<&mut FakeHost>, second.target);
        assert_eq!(slots.operator_target(0), Err(()));
        assert_eq!(human_host.added, [(SlotId(0), "script.human".into())]);
    }
}
