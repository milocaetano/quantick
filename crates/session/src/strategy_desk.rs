//! The strategy desk: every instance a tab has armed — on a drawing or as a
//! script — in one list grouped by asset, and the commands the desk sends
//! them.
//!
//! An asset is a symbol and a bar type: the tab answers the symbol, each
//! chart its bar type ([`DeskTab`]). An instance is named by
//! [`InstanceRef`]: a script by the id it was armed with, a drawn instance by
//! its chart, its drawing and the number the desk gave it. Both numbers come
//! from one tab-owned sequence, in arming order, and are never reused, so a
//! sibling's removal leaves every other name pointing where it did.
//!
//! A paused instance rings and places nothing, yet keeps its history: a
//! paused script still warms on every close and a paused drawn instance's
//! ruler still reads them ([`ArmedStrategy::pause`]), so a resumed one judges
//! the next close as a never-paused one would. An asset reads paused when
//! none of its instances watches and at least one is paused.
//!
//! History markers replay the chart's loaded bars through a fresh copy of the
//! instance — the kernel the live run uses, never a second bar path — so a
//! marker sits where a live run armed on the first loaded bar fires.
//!
//! "Fires today" counts the closes an instance fired on during the UTC day of
//! the latest print the tab heard. Told the time, never reading it.
//!
//! [`ArmedStrategy::pause`]: quantick_strategy::ArmedStrategy::pause

use std::collections::{BTreeMap, BTreeSet};

use quantick_engine::{Bar, Side};
use quantick_script_trigger::{Script, ScriptInstance, ScriptState, WARMUP_BARS};
use quantick_strategy::anchors::AnchoredInstance;
use quantick_strategy::presets::CompiledPreset;
use quantick_strategy::runner::placement_or_hold;
use quantick_strategy::{ArmedState, ArmedStrategy, ForceTrigger, Region};
use rust_decimal::Decimal;

use crate::script_strategies::ScriptPlacement;
use crate::strategies::{PaneStrategies, TabStrategies, rearm_instance};
use crate::strategy_reading::ArmedReading;

/// The span of one "today", in milliseconds: a UTC day.
pub const DAY_MS: u64 = 86_400_000;

/// What the desk groups by.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Asset {
    pub symbol: String,
    /// The chart's bar type as its toolbar summarises it (`tick(4)`).
    pub bar_type: String,
}

/// One tab as the desk reads it: its id, the symbol it shows and each
/// chart's bar type. An instance on a chart missing here is not listed.
#[derive(Debug, Clone, Copy)]
pub struct DeskTab<'a> {
    pub tab_id: u64,
    pub symbol: &'a str,
    pub panes: &'a [(u64, String)],
}

impl DeskTab<'_> {
    fn asset(&self, pane_id: u64) -> Option<Asset> {
        let (_, bar_type) = self.panes.iter().find(|(id, _)| *id == pane_id)?;
        Some(Asset {
            symbol: self.symbol.to_owned(),
            bar_type: bar_type.clone(),
        })
    }
}

/// Which instance of a tab a desk command or read names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceRef<D> {
    /// A script, by the id `arm_script` answered.
    Script(u64),
    /// Instance `id` riding `drawing` on chart `pane_id`.
    Drawing { pane_id: u64, drawing: D, id: u64 },
}

/// What the desk asks of a tab's instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyDeskCommand<D> {
    Pause(InstanceRef<D>),
    Resume(InstanceRef<D>),
    /// Drop the instance; a drawn one's resting entry is swept, its
    /// drawing's other instances stay.
    Remove(InstanceRef<D>),
    PauseAsset(Asset),
    ResumeAsset(Asset),
    /// Arm an ended instance again, rewarmed, unpaused.
    Arm(InstanceRef<D>),
    /// Hand back a script instance's source to keep in the library as `name`.
    SaveAs {
        instance: InstanceRef<D>,
        name: String,
    },
    /// Arm a copy beside the instance, paused.
    Duplicate(InstanceRef<D>),
}

/// What a desk command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskOutcome<D> {
    /// How many instances changed: zero when each already stood as asked.
    Changed(usize),
    /// The copy a duplicate armed, paused.
    Duplicated(InstanceRef<D>),
    /// The source to write under `name`: the library's files are the
    /// caller's, and the instance itself is unchanged.
    Save { name: String, source: String },
}

/// What an instance rides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskLink<D> {
    Drawing {
        drawing: D,
        name: Option<String>,
    },
    /// A script on a fixed range, a line or anywhere on the chart.
    Chart,
}

/// What a firing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskAction {
    Alarm,
    Order,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskState {
    Watching,
    Paused,
    /// Stopped until armed again, with why in the badge's words.
    Ended {
        reason: String,
    },
}

/// One instance as the desk lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskRow<D> {
    pub tab_id: u64,
    pub pane_id: u64,
    pub id: InstanceRef<D>,
    /// The script's file name, or the drawn instance's preset.
    pub name: String,
    pub state: DeskState,
    pub link: DeskLink<D>,
    pub action: DeskAction,
    /// The instance's badge line.
    pub status: String,
}

/// One asset's instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskGroup<D> {
    pub asset: Asset,
    pub paused: bool,
    pub rows: Vec<DeskRow<D>>,
}

/// Every tab's instances, grouped by asset; read tab by tab.
#[derive(Debug, Clone)]
pub struct StrategyDeskView<D> {
    groups: BTreeMap<Asset, Vec<DeskRow<D>>>,
}

impl<D> Default for StrategyDeskView<D> {
    fn default() -> Self {
        Self {
            groups: BTreeMap::new(),
        }
    }
}

impl<D: Copy + PartialEq> StrategyDeskView<D> {
    /// Add `tab`'s instances, naming drawings through `name_of(pane, drawing)`;
    /// a drawn instance not yet numbered gets its desk number here.
    pub fn read(
        &mut self,
        tab: &DeskTab<'_>,
        strategies: &mut TabStrategies<D>,
        name_of: impl Fn(u64, D) -> Option<String>,
    ) {
        strategies.number_instances();
        for (asset, row) in strategies.desk_rows(tab, &name_of) {
            self.groups.entry(asset).or_default().push(row);
        }
    }

    /// The groups in asset order, each tab's rows in chart then arming order.
    #[must_use]
    pub fn groups(self) -> Vec<DeskGroup<D>> {
        let groups = self.groups.into_iter();
        groups
            .map(|(asset, rows)| {
                let paused = !rows.iter().any(|row| row.state == DeskState::Watching)
                    && rows.iter().any(|row| row.state == DeskState::Paused);
                DeskGroup {
                    asset,
                    paused,
                    rows,
                }
            })
            .collect()
    }
}

/// One instance in full, for the desk's detail pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyDetail<D> {
    pub row: DeskRow<D>,
    /// The script's declared title, or the drawn preset's setup in words.
    pub description: String,
    pub side: Side,
    /// Where it may fire: a drawn instance always rides its region.
    pub placement: ScriptPlacement<D>,
    /// Each declared input with the value it runs on, read-only.
    pub inputs: Vec<(String, String)>,
    pub fires_today: u32,
    /// The script's source; a drawn instance has none.
    pub code: Option<String>,
    /// The loaded slots a fresh copy fires on, oldest first.
    pub history: Vec<usize>,
}

/// What the desk keeps beside a tab's instances.
#[derive(Debug)]
pub(crate) struct DeskBook<D> {
    /// Each armed script's compiled source, by instance id.
    scripts: BTreeMap<u64, Script>,
    paused_scripts: BTreeSet<u64>,
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

    fn fires_today(&self, key: InstanceRef<D>) -> u32 {
        let Some(today) = self.last_print_ms.map(|ms| ms / DAY_MS) else {
            return 0;
        };
        let held = self.fires.iter().find(|(held, _, _)| *held == key);
        held.filter(|(_, day, _)| *day == today)
            .map_or(0, |(_, _, count)| *count)
    }

    fn forget(&mut self, key: InstanceRef<D>) {
        self.fires.retain(|(held, _, _)| *held != key);
    }
}

/// The name a numbered drawn instance answers to.
fn drawn_ref<D: Copy>(pane_id: u64, instance: &AnchoredInstance<D>) -> Option<InstanceRef<D>> {
    let id = instance.armed.desk_id()?;
    Some(InstanceRef::Drawing {
        pane_id,
        drawing: instance.drawing,
        id,
    })
}

const NO_SUCH: &str = "no armed strategy on this tab has that id; read strategy.desk.list again";
const STILL_WATCHING: &str = "the strategy is still watching; arm reaches only an ended one";
const ENDED: &str = "the strategy has ended; arm it again before pausing or resuming it";
const NO_SCRIPT: &str = "a strategy on a drawing has no script to save";
const STARTS_ORDERS: &str =
    "this strategy places paper orders; only the trader resumes, arms or duplicates it";

/// Where an instance sits on its tab.
enum Located {
    Script { pane_id: u64, index: usize },
    Drawn { pane_id: u64, index: usize },
}

impl<D: Copy + PartialEq> TabStrategies<D> {
    /// Give every drawn instance armed since the last call its desk number,
    /// from the sequence script ids come from: panes in id order, each in
    /// arming order.
    pub(crate) fn number_instances(&mut self) {
        let mut next = self.next_script_id;
        for (_, pane) in self.panes_mut() {
            for instance in &mut pane.anchors.instances {
                if instance.armed.desk_id().is_none() {
                    instance.armed.set_desk_id(next);
                    next += 1;
                }
            }
        }
        self.next_script_id = next;
    }

    fn locate(&self, id: InstanceRef<D>) -> Option<Located> {
        match id {
            InstanceRef::Script(script) => self.panes().find_map(|(pane_id, pane)| {
                let index = pane.scripts.iter().position(|held| held.id == script)?;
                Some(Located::Script { pane_id, index })
            }),
            InstanceRef::Drawing {
                pane_id,
                drawing,
                id,
            } => {
                let instances = &self.pane(pane_id)?.anchors.instances;
                let index = instances
                    .iter()
                    .position(|held| held.drawing == drawing && held.armed.desk_id() == Some(id))?;
                Some(Located::Drawn { pane_id, index })
            }
        }
    }

    fn pane_at(&mut self, pane_id: u64) -> &mut PaneStrategies<D> {
        self.existing_mut(pane_id)
            .expect("a located instance's pane exists")
    }

    /// Every instance of `tab` with the asset it groups under.
    fn desk_rows(
        &self,
        tab: &DeskTab<'_>,
        name_of: &impl Fn(u64, D) -> Option<String>,
    ) -> Vec<(Asset, DeskRow<D>)> {
        let mut rows = Vec::new();
        for (pane_id, pane) in self.panes() {
            let Some(asset) = tab.asset(pane_id) else {
                continue;
            };
            for index in 0..pane.anchors.instances.len() {
                let located = Located::Drawn { pane_id, index };
                rows.push((asset.clone(), self.row(tab.tab_id, &located, name_of)));
            }
            for index in 0..pane.scripts.len() {
                let located = Located::Script { pane_id, index };
                rows.push((asset.clone(), self.row(tab.tab_id, &located, name_of)));
            }
        }
        rows
    }

    fn row(
        &self,
        tab_id: u64,
        located: &Located,
        name_of: &impl Fn(u64, D) -> Option<String>,
    ) -> DeskRow<D> {
        match *located {
            Located::Drawn { pane_id, index } => {
                let instances = &self.pane(pane_id).expect("located").anchors.instances;
                let instance = &instances[index];
                let state = if !instance.watching() {
                    DeskState::Ended {
                        reason: instance.armed.status_line(),
                    }
                } else if instance.armed.paused() {
                    DeskState::Paused
                } else {
                    DeskState::Watching
                };
                DeskRow {
                    tab_id,
                    pane_id,
                    id: drawn_ref(pane_id, instance).expect("numbered before it is read"),
                    name: instance.preset.clone(),
                    state,
                    link: DeskLink::Drawing {
                        drawing: instance.drawing,
                        name: name_of(pane_id, instance.drawing),
                    },
                    action: if instance.alarm_only() {
                        DeskAction::Alarm
                    } else {
                        DeskAction::Order
                    },
                    status: instance.armed.status_line(),
                }
            }
            Located::Script { pane_id, index } => {
                let script = &self.pane(pane_id).expect("located").scripts[index];
                let paused = self.desk.script_paused(script.id);
                let state = match script.state() {
                    ScriptState::Disarmed { reason } => DeskState::Ended { reason },
                    ScriptState::Watching if paused => DeskState::Paused,
                    ScriptState::Watching => DeskState::Watching,
                };
                let status = if state == DeskState::Paused {
                    "paused".to_owned()
                } else {
                    script.armed().status_line()
                };
                DeskRow {
                    tab_id,
                    pane_id,
                    id: InstanceRef::Script(script.id),
                    name: script.script.clone(),
                    state,
                    link: match script.placement.drawing() {
                        Some(drawing) => DeskLink::Drawing {
                            drawing,
                            name: name_of(pane_id, drawing),
                        },
                        None => DeskLink::Chart,
                    },
                    action: DeskAction::Alarm,
                    status,
                }
            }
        }
    }

    /// Carry out `command` on `tab`, warming whatever it arms on
    /// `warmup(pane, depth)` — closed bars with their footprint POC, oldest
    /// first.
    ///
    /// # Errors
    ///
    /// The sentence the caller reads: an id no instance has, a pause of an
    /// ended instance, an arm of a watching one, a save of a drawn one, or —
    /// for a caller that `may_start_orders` not — a resume, arm or duplicate
    /// of a drawn instance that places paper orders. Such a caller's asset
    /// resume leaves those instances paused.
    pub fn execute_desk(
        &mut self,
        tab: &DeskTab<'_>,
        command: StrategyDeskCommand<D>,
        may_start_orders: bool,
        warmup: impl FnMut(u64, usize) -> Vec<(Bar, Option<Decimal>)>,
    ) -> Result<DeskOutcome<D>, String> {
        self.number_instances();
        if !may_start_orders && self.starts_orders(&command) {
            return Err(STARTS_ORDERS.to_owned());
        }
        match command {
            StrategyDeskCommand::Pause(id) => self.set_paused(id, true).map(DeskOutcome::Changed),
            StrategyDeskCommand::Resume(id) => self.set_paused(id, false).map(DeskOutcome::Changed),
            StrategyDeskCommand::Remove(id) => self.remove(id).map(|()| DeskOutcome::Changed(1)),
            StrategyDeskCommand::PauseAsset(asset) => Ok(DeskOutcome::Changed(
                self.pause_asset(tab, &asset, true, true),
            )),
            StrategyDeskCommand::ResumeAsset(asset) => {
                let changed = self.pause_asset(tab, &asset, false, may_start_orders);
                Ok(DeskOutcome::Changed(changed))
            }
            StrategyDeskCommand::Arm(id) => {
                self.arm_again(id, warmup).map(|()| DeskOutcome::Changed(1))
            }
            StrategyDeskCommand::SaveAs { instance, name } => {
                let InstanceRef::Script(script) = instance else {
                    return Err(NO_SCRIPT.to_owned());
                };
                self.locate(instance).ok_or(NO_SUCH)?;
                let source = self.desk.scripts.get(&script).ok_or(NO_SUCH)?;
                let source = source.source().to_owned();
                Ok(DeskOutcome::Save { name, source })
            }
            StrategyDeskCommand::Duplicate(id) => {
                self.duplicate(id, warmup).map(DeskOutcome::Duplicated)
            }
        }
    }

    /// Whether `command` lets a drawn instance that places paper orders
    /// start placing them again.
    fn starts_orders(&self, command: &StrategyDeskCommand<D>) -> bool {
        let (StrategyDeskCommand::Resume(id)
        | StrategyDeskCommand::Arm(id)
        | StrategyDeskCommand::Duplicate(id)) = command
        else {
            return false;
        };
        match self.locate(*id) {
            Some(Located::Drawn { pane_id, index }) => {
                let instances = &self.pane(pane_id).expect("located").anchors.instances;
                !instances[index].alarm_only()
            }
            Some(Located::Script { .. }) | None => false,
        }
    }

    /// Pause or resume one instance; how many changed.
    fn set_paused(&mut self, id: InstanceRef<D>, paused: bool) -> Result<usize, String> {
        match self.locate(id).ok_or(NO_SUCH)? {
            Located::Script { pane_id, index } => {
                let script = &self.pane(pane_id).expect("located").scripts[index];
                if !script.watching() {
                    return Err(ENDED.to_owned());
                }
                let script = script.id;
                let changed = if paused {
                    self.desk.paused_scripts.insert(script)
                } else {
                    self.desk.paused_scripts.remove(&script)
                };
                Ok(usize::from(changed))
            }
            Located::Drawn { pane_id, index } => {
                let instance = &mut self.pane_at(pane_id).anchors.instances[index];
                if !instance.watching() {
                    return Err(ENDED.to_owned());
                }
                Ok(usize::from(pause_drawn(instance, paused)))
            }
        }
    }

    /// Pause or resume every watching instance of `asset` on `tab`; how many
    /// changed.
    fn pause_asset(
        &mut self,
        tab: &DeskTab<'_>,
        asset: &Asset,
        paused: bool,
        may_start_orders: bool,
    ) -> usize {
        let on_asset = |pane_id: u64| tab.asset(pane_id).as_ref() == Some(asset);
        let mut changed = 0;
        let mut scripts = Vec::new();
        for (pane_id, pane) in self.panes_mut() {
            if !on_asset(pane_id) {
                continue;
            }
            for instance in &mut pane.anchors.instances {
                let starts_orders = !paused && !instance.alarm_only();
                if instance.watching() && (may_start_orders || !starts_orders) {
                    changed += usize::from(pause_drawn(instance, paused));
                }
            }
            let watching = pane.scripts.iter().filter(|script| script.watching());
            scripts.extend(watching.map(|script| script.id));
        }
        for script in scripts {
            let flipped = if paused {
                self.desk.paused_scripts.insert(script)
            } else {
                self.desk.paused_scripts.remove(&script)
            };
            changed += usize::from(flipped);
        }
        changed
    }

    fn remove(&mut self, id: InstanceRef<D>) -> Result<(), String> {
        match self.locate(id).ok_or(NO_SUCH)? {
            Located::Script { pane_id, index } => {
                let script = self.pane(pane_id).expect("located").scripts[index].id;
                self.disarm_script(script);
            }
            Located::Drawn { pane_id, index } => {
                self.pane_at(pane_id).remove_instance_at(index);
                self.desk.forget(id);
            }
        }
        Ok(())
    }

    fn arm_again(
        &mut self,
        id: InstanceRef<D>,
        mut warmup: impl FnMut(u64, usize) -> Vec<(Bar, Option<Decimal>)>,
    ) -> Result<(), String> {
        match self.locate(id).ok_or(NO_SUCH)? {
            Located::Script { pane_id, index } => {
                let held = &self.pane(pane_id).expect("located").scripts[index];
                if held.watching() {
                    return Err(STILL_WATCHING.to_owned());
                }
                let (script, side, placement) = (held.id, held.side(), held.placement);
                let source = self.desk.scripts.get(&script).ok_or(NO_SUCH)?;
                let warm = warmup(pane_id, WARMUP_BARS);
                let armed = ScriptInstance::arm(script, source, side, placement, &warm);
                self.pane_at(pane_id).scripts[index] = armed;
                self.desk.paused_scripts.remove(&script);
            }
            Located::Drawn { pane_id, index } => {
                let instance = &mut self.pane_at(pane_id).anchors.instances[index];
                if !instance.rearmable() {
                    return Err(STILL_WATCHING.to_owned());
                }
                rearm_instance(instance, |depth| bars_of(warmup(pane_id, depth)));
                instance.armed.resume();
            }
        }
        Ok(())
    }

    fn duplicate(
        &mut self,
        id: InstanceRef<D>,
        mut warmup: impl FnMut(u64, usize) -> Vec<(Bar, Option<Decimal>)>,
    ) -> Result<InstanceRef<D>, String> {
        match self.locate(id).ok_or(NO_SUCH)? {
            Located::Script { pane_id, index } => {
                let held = &self.pane(pane_id).expect("located").scripts[index];
                let (side, placement) = (held.side(), held.placement);
                let source = self.desk.scripts.get(&held.id).ok_or(NO_SUCH)?.clone();
                let copy = self.next_script_id;
                self.next_script_id += 1;
                let warm = warmup(pane_id, WARMUP_BARS);
                let armed = ScriptInstance::arm(copy, &source, side, placement, &warm);
                self.pane_at(pane_id).scripts.push(armed);
                self.desk.record_script(copy, source);
                self.desk.paused_scripts.insert(copy);
                Ok(InstanceRef::Script(copy))
            }
            Located::Drawn { pane_id, index } => {
                let pane = self.pane_at(pane_id);
                let held = &pane.anchors.instances[index];
                let (drawing, spec, preset) =
                    (held.drawing, held.spec.clone(), held.preset.clone());
                let warm = |depth| bars_of(warmup(pane_id, depth));
                pane.arm(drawing, &spec, preset, || Ok(()), warm)?;
                let copy = pane.anchors.instances.last_mut().expect("just armed");
                pause_drawn(copy, true);
                self.number_instances();
                let copy = self
                    .pane(pane_id)
                    .and_then(|pane| pane.anchors.instances.last());
                Ok(drawn_ref(pane_id, copy.expect("just armed")).expect("just numbered"))
            }
        }
    }

    /// Instance `id` in full, `None` when the tab has no such instance. Its
    /// history replays `bars(pane)` — the chart's loaded closed bars with
    /// their POC, slot zero first — against `region(pane, drawing, slot)`,
    /// the band a drawing marks there and whether it covers the slot.
    pub fn detail(
        &mut self,
        tab: &DeskTab<'_>,
        id: InstanceRef<D>,
        name_of: impl Fn(u64, D) -> Option<String>,
        bars: impl FnOnce(u64) -> Vec<(Bar, Option<Decimal>)>,
        mut region: impl FnMut(u64, D, usize) -> Option<(Decimal, Decimal, bool)>,
    ) -> Option<StrategyDetail<D>> {
        self.number_instances();
        let located = self.locate(id)?;
        let row = self.row(tab.tab_id, &located, &name_of);
        let pane_id = row.pane_id;
        let history = self.history_markers(id, &bars(pane_id), |drawing, slot| {
            region(pane_id, drawing, slot)
        })?;
        let fires_today = self.desk.fires_today(row.id);
        Some(match located {
            Located::Script { index, .. } => {
                let instance = &self.pane(pane_id)?.scripts[index];
                let script = self.desk.scripts.get(&instance.id)?;
                StrategyDetail {
                    row,
                    description: script.title().to_owned(),
                    side: instance.side(),
                    placement: instance.placement,
                    inputs: script.inputs(),
                    fires_today,
                    code: Some(script.source().to_owned()),
                    history,
                }
            }
            Located::Drawn { index, .. } => {
                let instance = &self.pane(pane_id)?.anchors.instances[index];
                let reading = ArmedReading::of(instance, String::new());
                StrategyDetail {
                    row,
                    description: format!(
                        "{}: a force bar inside its region, {}",
                        instance.preset,
                        if instance.alarm_only() {
                            "rings an alarm"
                        } else {
                            "places a paper order"
                        }
                    ),
                    side: reading.side,
                    placement: ScriptPlacement::Region(instance.drawing),
                    inputs: drawn_inputs(&reading),
                    fires_today,
                    code: None,
                    history,
                }
            }
        })
    }

    /// The slots of `bars` (slot zero first, each with its POC) a fresh copy
    /// of instance `id` fires on, armed on the first and judged against
    /// `region(drawing, slot)`; `None` when the tab has no such instance.
    pub fn history_markers(
        &self,
        id: InstanceRef<D>,
        bars: &[(Bar, Option<Decimal>)],
        mut region: impl FnMut(D, usize) -> Option<(Decimal, Decimal, bool)>,
    ) -> Option<Vec<usize>> {
        let mut band =
            |drawing, slot| region(drawing, slot).map(|(a, b, active)| (Region::new(a, b), active));
        let mut fired = Vec::new();
        match self.locate(id)? {
            Located::Script { pane_id, index } => {
                let held = &self.pane(pane_id)?.scripts[index];
                let script = self.desk.scripts.get(&held.id)?;
                let mut copy = ScriptInstance::arm(0, script, held.side(), held.placement, &[]);
                for (slot, (bar, poc)) in bars.iter().enumerate() {
                    let resolved = held.placement.resolve(|drawing| band(drawing, slot));
                    let now_ms = u64::try_from(bar.close_time).unwrap_or_default();
                    if copy
                        .on_closed_bar(bar, *poc, resolved.as_ref(), now_ms)
                        .is_some()
                    {
                        fired.push(slot);
                    }
                }
            }
            Located::Drawn { pane_id, index } => {
                let held = &self.pane(pane_id)?.anchors.instances[index];
                let CompiledPreset { params, force, .. } = held.spec.to_kernel()?;
                let mut copy = ArmedStrategy::new(params, Box::new(ForceTrigger::new(force)));
                for (slot, (bar, _)) in bars.iter().enumerate() {
                    let resolved = band(held.drawing, slot);
                    let (placement, active) = placement_or_hold(resolved.as_ref());
                    let _ = copy.on_closed_bar(bar, placement, active, true);
                    if copy.last_close_opportunity().is_some() {
                        fired.push(slot);
                    }
                }
            }
        }
        Some(fired)
    }
}

/// Pause or resume a drawn instance; whether that changed it. A pause also
/// withdraws a standing preview mark.
fn pause_drawn<D>(instance: &mut AnchoredInstance<D>, paused: bool) -> bool {
    if instance.armed.paused() == paused {
        return false;
    }
    if paused {
        instance.armed.pause();
        instance.reset_alarm();
    } else {
        instance.armed.resume();
    }
    true
}

fn bars_of(warm: Vec<(Bar, Option<Decimal>)>) -> Vec<Bar> {
    warm.into_iter().map(|(bar, _)| bar).collect()
}

/// A drawn preset's parameters as the desk lists them.
fn drawn_inputs<D>(reading: &ArmedReading<D>) -> Vec<(String, String)> {
    let side = match reading.side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    };
    [
        ("Side", side.to_owned()),
        ("Quantity", reading.quantity.to_string()),
        ("Take profit ×", reading.tp_mult.to_string()),
        ("Stop loss ×", reading.sl_mult.to_string()),
        ("On break", reading.on_break.to_owned()),
        ("Execution", reading.execution.to_owned()),
        ("Re-arm", reading.rearm.to_owned()),
    ]
    .into_iter()
    .map(|(label, value)| (label.to_owned(), value))
    .collect()
}

#[cfg(test)]
#[path = "strategy_desk_tests.rs"]
mod tests;
