//! What the strategy desk keeps beside a tab's instances, below both the
//! tab's strategies and the desk (`crate::strategy_desk`): the name an
//! instance answers to, each script's compiled source, the paused scripts and
//! today's firings.

use std::collections::{BTreeMap, BTreeSet};

use quantick_script_trigger::Script;
use quantick_strategy::ArmedState;
use quantick_strategy::anchors::AnchoredInstance;

/// The span of one "today", in milliseconds: a UTC day.
pub const DAY_MS: u64 = 86_400_000;

/// Which instance of a tab a desk command or read names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceRef<D> {
    /// A script, by the id `arm_script` answered.
    Script(u64),
    /// Instance `id` riding `drawing` on chart `pane_id`.
    Drawing { pane_id: u64, drawing: D, id: u64 },
}

/// What the desk keeps beside a tab's instances.
#[derive(Debug)]
pub(crate) struct DeskBook<D> {
    /// Each armed script's compiled source, by instance id.
    pub(crate) scripts: BTreeMap<u64, Script>,
    pub(crate) paused_scripts: BTreeSet<u64>,
    /// Per instance, the UTC day of its last firing and its firings that day.
    fires: Vec<(InstanceRef<D>, u64, u32)>,
    last_print_ms: Option<u64>,
}

impl<D> Default for DeskBook<D> {
    fn default() -> Self {
        Self {
            scripts: BTreeMap::new(),
            paused_scripts: BTreeSet::new(),
            fires: Vec::new(),
            last_print_ms: None,
        }
    }
}

impl<D: Copy + PartialEq> DeskBook<D> {
    pub(crate) fn record_script(&mut self, id: u64, script: Script) {
        self.scripts.insert(id, script);
    }

    pub(crate) fn forget_script(&mut self, id: u64) {
        self.scripts.remove(&id);
        self.paused_scripts.remove(&id);
        self.fires
            .retain(|(key, _, _)| *key != InstanceRef::Script(id));
    }

    pub(crate) fn script_paused(&self, id: u64) -> bool {
        self.paused_scripts.contains(&id)
    }

    pub(crate) fn heard(&mut self, now_ms: u64) {
        self.last_print_ms = Some(now_ms);
    }

    pub(crate) fn fired(&mut self, key: InstanceRef<D>, now_ms: u64) {
        let day = now_ms / DAY_MS;
        match self.fires.iter_mut().find(|(held, _, _)| *held == key) {
            Some((_, held_day, count)) if *held_day == day => *count += 1,
            Some((_, held_day, count)) => (*held_day, *count) = (day, 1),
            None => self.fires.push((key, day, 1)),
        }
    }

    /// Count each drawn instance of chart `pane_id` whose setup the close
    /// just judged qualified, while it still listens.
    pub(crate) fn count_drawn(
        &mut self,
        pane_id: u64,
        instances: &[AnchoredInstance<D>],
        now_ms: u64,
    ) {
        for instance in instances {
            let listening = !matches!(instance.armed.state(), ArmedState::Disarmed { .. });
            let key = drawn_ref(pane_id, instance);
            if let Some(key) = key.filter(|_| listening)
                && instance.armed.last_close_opportunity().is_some()
            {
                self.fired(key, now_ms);
            }
        }
    }

    pub(crate) fn fires_today(&self, key: InstanceRef<D>) -> u32 {
        let Some(today) = self.last_print_ms.map(|ms| ms / DAY_MS) else {
            return 0;
        };
        let held = self.fires.iter().find(|(held, _, _)| *held == key);
        held.filter(|(_, day, _)| *day == today)
            .map_or(0, |(_, _, count)| *count)
    }

    pub(crate) fn forget(&mut self, key: InstanceRef<D>) {
        self.fires.retain(|(held, _, _)| *held != key);
    }
}

/// The name a numbered drawn instance answers to.
pub(crate) fn drawn_ref<D: Copy>(
    pane_id: u64,
    instance: &AnchoredInstance<D>,
) -> Option<InstanceRef<D>> {
    let id = instance.armed.desk_id()?;
    Some(InstanceRef::Drawing {
        pane_id,
        drawing: instance.drawing,
        id,
    })
}
