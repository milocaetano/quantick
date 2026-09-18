//! The indicator manager: what the window does about the slots on its panes.
//!
//! Attaching and detaching, the legends and their folds, the settings dialog
//! and its draft, the presets, the script hot-reload poll and the state file
//! that remembers it all. They are together because they are one feature seen
//! from several surfaces: the legend, the dialog and the control plane's
//! `indicator.script.attach` all reach the same slots through the same
//! [`crate::indicator_worker`] commands, and a change to what a slot is has
//! to be made in one file rather than found in six places in `app.rs`.

use std::time::{Duration, Instant};

use eframe::egui;

use super::indicator_operations::{IndicatorAttachment, IndicatorHost};
use crate::indicator_legend;
use crate::indicator_panel::{self, SettingsDialog, SettingsOutcome};
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorWorker, SlotId};
use crate::indicators::IndicatorView;
use crate::indicators::library::ScriptLibrary;
use crate::indicators::preset_file;
use crate::indicators::state_file::{SavedInput, SavedInputExt, SavedKind};
use crate::pane::ChartPane;
use crate::pane::PaneSide;

use super::TabSlot;

/// How often the hot-reload poll checks script files for changes.
const SCRIPT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(1_000);

/// The indicator persistence layer: what may be loaded, what is on screen,
/// what a layout still owes the slots it placed, and who put each there.
///
/// Owned by this module. `layout_wiring` places slots and drains the two
/// `pending_*` queues, `tabs` mirrors the live set onto a new tab, and
/// `workspace_save` persists it; nothing else names the group.
pub(super) struct IndicatorState {
    /// Loadable `.pine` scripts (embedded + indicators dir), scanned at
    /// startup. A file-backed script then follows its file: `poll_script_files`
    /// checks mtimes on a debounce and reloads on a save.
    pub(super) script_library: ScriptLibrary,

    /// The open indicator-settings dialog, if any (one at a time).
    pub(super) indicator_settings: Option<SettingsDialog>,

    /// The slot the open dialog edits. Held apart from the dialog so a tab or
    /// pane changing under it cannot retarget its Apply.
    pub(super) indicator_settings_target: TabSlot,

    /// File-backed script slots: (slot, library index, last seen mtime) —
    /// what the hot-reload poll walks.
    pub(super) script_files: Vec<(TabSlot, usize, std::time::SystemTime)>,

    /// How each live slot restores (the persistence identity per slot).
    ///
    /// Stays beside the library and the state file rather than moving into the
    /// panes with the slots themselves: one file records what the window had
    /// open, so one list records what is in it.
    pub(super) slot_kinds: Vec<(TabSlot, SavedKind)>,

    /// Slots placed hidden by a layout, applied when their Rebuilt lands —
    /// the view a hide acts on is born from the worker's first answer.
    pub(super) pending_hidden: Vec<TabSlot>,

    /// Per-plot style layers placed by a layout, applied when their Rebuilt
    /// lands — the same deferral [`Self::pending_hidden`] performs, for the
    /// same reason.
    pub(super) pending_styles: Vec<(TabSlot, crate::indicator_style::StyleOverride)>,
    /// Saved mouse guides waiting for their worker-created view.
    pub(super) pending_mouse_vertical_lines: Vec<TabSlot>,

    /// Last hot-reload poll instant (the poll runs about once a second;
    /// file metadata every frame would be waste).
    pub(super) last_script_poll: Instant,
    // Named input setups per indicator kind, offered by the settings
    // dialog's preset picker.
    pub(super) indicator_presets: preset_file::PresetStore,

    /// The indicator slots an operator other than the trader attached — the
    /// only ones the annotate tier may take back off the chart. Keyed by the
    /// whole [`TabSlot`]: a slot number is allocated per pane and is reused
    /// by every other pane, so the number alone would mark one tab's slot 0
    /// as an operator's because another tab's slot 0 was.
    pub(super) operator_slots: std::collections::BTreeSet<TabSlot>,
}

impl IndicatorState {
    /// Initialize the feature at its existing startup phase: scan, clock, presets.
    pub(super) fn new(first_tab: u64, presets_path: &std::path::Path) -> Self {
        Self {
            script_library: crate::indicators::library::scan(),
            indicator_settings: None,
            indicator_settings_target: TabSlot {
                tab: first_tab,
                side: PaneSide::Flow,
                slot: SlotId(0),
            },
            script_files: Vec::new(),
            slot_kinds: Vec::new(),
            pending_hidden: Vec::new(),
            pending_styles: Vec::new(),
            pending_mouse_vertical_lines: Vec::new(),
            last_script_poll: Instant::now(),
            operator_slots: std::collections::BTreeSet::new(),
            indicator_presets: preset_file::PresetStore::load(presets_path),
        }
    }

    /// A closed tab cannot leave persistence or deferred slot work behind.
    /// Slot numbers may be reused on other tabs, so the tab is the identity.
    pub(super) fn forget_tab(&mut self, tab: u64) {
        self.slot_kinds.retain(|(owner, _)| owner.tab != tab);
        self.operator_slots.retain(|owner| owner.tab != tab);
        self.script_files.retain(|(owner, ..)| owner.tab != tab);
        self.pending_hidden.retain(|owner| owner.tab != tab);
        self.pending_styles.retain(|(owner, _)| owner.tab != tab);
        self.pending_mouse_vertical_lines
            .retain(|owner| owner.tab != tab);
    }

    /// Lend only slot bookkeeping to operations; UI/library/poll state stays here.
    pub(super) fn slots_mut(&mut self) -> super::indicator_operations::IndicatorSlots<'_> {
        super::indicator_operations::IndicatorSlots {
            slot_kinds: &mut self.slot_kinds,
            operator_slots: &mut self.operator_slots,
            script_files: &mut self.script_files,
            pending_hidden: &mut self.pending_hidden,
            pending_styles: &mut self.pending_styles,
        }
    }
}

/// Layout persistence owed by an already applied indicator edit.
pub(super) enum IndicatorEdit {
    Attached(IndicatorAttachment),
    Hidden(TabSlot),
    Remove(TabSlot),
    Inputs(TabSlot, Vec<quantick_indicators::InputValue>),
    Style(TabSlot),
}

/// The settings owner distinguishes a worker preview from a persisted edit.
pub(super) enum SettingsChange {
    None,
    Inputs {
        target: TabSlot,
        slot: SlotId,
        values: Vec<quantick_indicators::InputValue>,
        commit: bool,
    },
    Style(TabSlot),
}

impl SettingsChange {
    pub(super) fn worker_target(&self) -> Option<TabSlot> {
        match self {
            Self::Inputs { target, .. } => Some(*target),
            _ => None,
        }
    }

    pub(super) fn deliver(self, worker: Option<&IndicatorWorker>) -> Option<IndicatorEdit> {
        match self {
            Self::None => None,
            Self::Style(target) => Some(IndicatorEdit::Style(target)),
            Self::Inputs {
                target,
                slot,
                values,
                commit: true,
            } => {
                if let Some(worker) = worker {
                    worker.send(IndicatorCommand::SetInputs {
                        slot,
                        values: values.clone(),
                    });
                }
                Some(IndicatorEdit::Inputs(target, values))
            }
            Self::Inputs {
                slot,
                values,
                commit: false,
                ..
            } => {
                if let Some(worker) = worker {
                    worker.send(IndicatorCommand::SetInputs { slot, values });
                }
                None
            }
        }
    }
}

pub(super) struct LibraryAttachment {
    pub attachment: Option<IndicatorAttachment>,
    pub watch: Option<(TabSlot, usize)>,
}

impl IndicatorState {
    pub(super) fn attach_script(
        &mut self,
        pane: &mut impl IndicatorHost,
        target: (u64, PaneSide),
        name: String,
        text: String,
        by_operator: bool,
    ) -> IndicatorAttachment {
        self.slots_mut()
            .attach_script(pane, target, name, text, by_operator)
    }

    pub(super) fn attach_native(
        &mut self,
        pane: &mut impl IndicatorHost,
        target: (u64, PaneSide),
        id: &str,
    ) -> IndicatorAttachment {
        self.slots_mut().attach_native(pane, target, id)
    }

    pub(super) fn operator_target(&mut self, slot: u64) -> Result<Option<TabSlot>, ()> {
        self.slots_mut().operator_target(slot)
    }

    /// Called only after the layout mirror has read this slot's position.
    pub(super) fn remove(&mut self, pane: Option<&mut impl IndicatorHost>, target: TabSlot) {
        self.slots_mut().remove(pane, target);
    }

    pub(super) fn open_settings(&mut self, target: TabSlot, view: Option<&IndicatorView>) {
        let Some(view) = view else {
            return;
        };
        self.indicator_settings = Some(SettingsDialog {
            slot: target.slot,
            title: view.label().to_owned(),
            tab: indicator_panel::SettingsTab::default(),
            draft: view.input_values.clone(),
            committed: view.input_values.clone(),
            previewed: false,
            settled: false,
            preset_label: None,
            preset_name_draft: String::new(),
        });
        self.indicator_settings_target = target;
    }

    /// Resolve the indexed launch request on the focused pane, then the flow pane.
    pub(super) fn open_autostart(
        &mut self,
        tab: u64,
        focused: (PaneSide, &[IndicatorView]),
        flow: &[IndicatorView],
        request: Option<(usize, indicator_panel::SettingsTab)>,
    ) -> bool {
        let Some((index, settings_tab)) = request else {
            return false;
        };
        let Some((side, view)) = [focused, (PaneSide::Flow, flow)]
            .into_iter()
            .find_map(|(side, views)| views.get(index).map(|view| (side, view)))
        else {
            return false;
        };
        self.open_settings(
            TabSlot {
                tab,
                side,
                slot: view.slot,
            },
            Some(view),
        );
        if let Some(dialog) = self.indicator_settings.as_mut() {
            dialog.tab = settings_tab;
        }
        true
    }

    /// A deferred launch opens only when the worker has produced editable inputs.
    pub(super) fn open_first_editable(
        &mut self,
        tab: u64,
        flow: &[IndicatorView],
        requested: bool,
    ) -> bool {
        if !requested || self.indicator_settings.is_some() {
            return false;
        }
        let Some(view) = flow.iter().find(|view| !view.input_values.is_empty()) else {
            return false;
        };
        self.open_settings(
            TabSlot {
                tab,
                side: PaneSide::Flow,
                slot: view.slot,
            },
            Some(view),
        );
        true
    }

    pub(super) fn set_legend_collapsed(pane: &mut ChartPane, collapsed: bool) {
        pane.legend_collapsed = collapsed;
    }

    pub(super) fn toggle_hidden(pane: &mut ChartPane, target: TabSlot) -> IndicatorEdit {
        pane.indicators.toggle_hidden(target.slot);
        IndicatorEdit::Hidden(target)
    }

    pub(super) fn legend_action(
        &mut self,
        pane: &mut ChartPane,
        (tab, side): (u64, PaneSide),
        action: indicator_legend::LegendAction,
    ) -> Option<IndicatorEdit> {
        let at = |slot| TabSlot { tab, side, slot };
        match action {
            indicator_legend::LegendAction::OpenSettings(slot) => {
                self.open_settings(
                    at(slot),
                    pane.indicators.all().iter().find(|view| view.slot == slot),
                );
                None
            }
            indicator_legend::LegendAction::SetCollapsed(collapsed) => {
                Self::set_legend_collapsed(pane, collapsed);
                None
            }
            indicator_legend::LegendAction::ToggleHidden(slot) => {
                Some(Self::toggle_hidden(pane, at(slot)))
            }
            indicator_legend::LegendAction::Remove(slot) => Some(IndicatorEdit::Remove(at(slot))),
        }
    }

    /// Draw one pane; the caller applies actions only after all panes draw.
    pub(super) fn draw_legend(
        &self,
        ctx: &egui::Context,
        pane: &mut ChartPane,
        (tab, side): (u64, PaneSide),
        visible: bool,
        position_open: bool,
    ) -> Vec<indicator_legend::LegendAction> {
        pane.frame.indicator_legend = None;
        if !visible {
            return Vec::new();
        }
        let Some(mut rect) = pane.frame.chart_area else {
            return Vec::new();
        };
        rect.min.y +=
            indicator_legend::hud_offset_px(pane.paper_hud_anchor().is_some() && position_open);
        let preview = self
            .indicator_settings
            .as_ref()
            .filter(|dialog| dialog.previewed)
            .filter(|_| {
                self.indicator_settings_target.tab == tab
                    && self.indicator_settings_target.side == side
            })
            .map(|dialog| dialog.slot);
        let (actions, footprint) = indicator_legend::draw(
            ctx,
            pane.id,
            rect,
            pane.indicators.all(),
            preview,
            pane.legend_collapsed,
        );
        pane.frame.indicator_legend = footprint;
        actions
    }

    pub(super) fn draw_settings(
        &mut self,
        ctx: &egui::Context,
        view: Option<&mut IndicatorView>,
        presets_path: &std::path::Path,
    ) -> SettingsChange {
        let target = self.indicator_settings_target;
        let preset_names: Option<Vec<String>> = self
            .slot_kinds
            .iter()
            .find(|(owner, _)| *owner == target)
            .map(|(_, kind)| {
                self.indicator_presets
                    .names_for(kind)
                    .map(str::to_owned)
                    .collect()
            });
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return SettingsChange::None;
        };
        let Some(view) = view else {
            self.indicator_settings = None;
            return SettingsChange::None;
        };
        dialog.title = view.label().to_owned();
        let IndicatorView {
            descriptor, style, ..
        } = view;
        let outcome = indicator_panel::draw(
            ctx,
            dialog,
            &descriptor.inputs,
            &descriptor.plots,
            style,
            preset_names.as_deref(),
        );
        match outcome {
            SettingsOutcome::Open => SettingsChange::None,
            SettingsOutcome::Close => {
                let change = self.discard_preview();
                self.indicator_settings = None;
                change
            }
            SettingsOutcome::Apply => self.apply_settings(),
            SettingsOutcome::Preview => self.preview_settings(),
            SettingsOutcome::LoadPreset(name) => self.load_preset(name, &descriptor.inputs),
            SettingsOutcome::SavePreset(name) => {
                self.save_indicator_preset(&name, presets_path);
                SettingsChange::None
            }
            SettingsOutcome::DeletePreset(name) => {
                self.delete_indicator_preset(&name, presets_path);
                SettingsChange::None
            }
            SettingsOutcome::StyleChanged => SettingsChange::Style(target),
        }
    }

    pub(super) fn apply_settings(&mut self) -> SettingsChange {
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return SettingsChange::None;
        };
        dialog.committed = dialog.draft.clone();
        dialog.previewed = false;
        SettingsChange::Inputs {
            target: self.indicator_settings_target,
            slot: dialog.slot,
            values: dialog.draft.clone(),
            commit: true,
        }
    }

    pub(super) fn preview_settings(&mut self) -> SettingsChange {
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return SettingsChange::None;
        };
        dialog.previewed = true;
        SettingsChange::Inputs {
            target: self.indicator_settings_target,
            slot: dialog.slot,
            values: dialog.draft.clone(),
            commit: false,
        }
    }

    pub(super) fn discard_preview(&self) -> SettingsChange {
        let Some(dialog) = self
            .indicator_settings
            .as_ref()
            .filter(|dialog| dialog.previewed)
        else {
            return SettingsChange::None;
        };
        SettingsChange::Inputs {
            target: self.indicator_settings_target,
            slot: dialog.slot,
            values: dialog.committed.clone(),
            commit: false,
        }
    }

    /// Legacy launch indicators remain live but do not join the saved set.
    pub(super) fn forget_last_indicator_state_change(&mut self) {
        self.slot_kinds.pop();
    }

    /// Preserve peer-before-origin watch registration after layout mirroring.
    pub(super) fn watch_attachment(&mut self, watch: Option<(TabSlot, usize)>) {
        if let Some((owner, index)) = watch
            && let Some((_, mtime)) = self.script_library.file_info(index)
        {
            self.script_files.push((owner, index, mtime));
        }
    }
    pub(super) fn load_preset(
        &mut self,
        name: Option<String>,
        specs: &[quantick_indicators::InputSpec],
    ) -> SettingsChange {
        let target = self.indicator_settings_target;
        let saved: Option<Vec<Option<quantick_indicators::InputValue>>> = match &name {
            None => Some(Vec::new()),
            Some(name) => {
                let Some(kind) = self
                    .slot_kinds
                    .iter()
                    .find(|(owner, _)| *owner == target)
                    .map(|(_, kind)| kind)
                else {
                    return SettingsChange::None;
                };
                self.indicator_presets
                    .get(kind, name)
                    // `map`, not `filter_map`: a stored cell that no longer
                    // reads (a source whose name the dialect dropped, say)
                    // still has to hold its index. Dropping it from the list
                    // would slide every value after it one input to the left,
                    // and same-typed neighbours would take each other's
                    // settings without a word.
                    .map(|inputs| inputs.iter().map(SavedInput::to_value).collect())
            }
        };
        let Some(saved) = saved else {
            return SettingsChange::None;
        };
        // The same binder the worker binds a saved state file with: one rule
        // for what a saved value means, in one place.
        let bound = quantick_indicators::bind_by_position(specs, &saved);
        if bound.kept < saved.len() {
            // The preset on screen is not the preset that was saved. Silence
            // here would show a chart that does not match the name above it.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_PRESET_REBOUND",
                preset = %name.as_deref().unwrap_or(indicator_panel::DEFAULT_PRESET),
                saved = saved.len(),
                declared = specs.len(),
                kept = bound.kept,
                action = "bound_by_position",
                "some of this preset's values no longer bind; those inputs took their defaults"
            );
        }
        let draft = bound.values;
        let label = name.unwrap_or_else(|| indicator_panel::DEFAULT_PRESET.to_owned());
        if let Some(dialog) = self.indicator_settings.as_mut() {
            dialog.draft = draft;
            dialog.preset_label = Some(label);
        }
        self.preview_settings()
    }
    fn save_indicator_preset(&mut self, name: &str, presets_path: &std::path::Path) {
        let target = self.indicator_settings_target;
        let Some(kind) = self
            .slot_kinds
            .iter()
            .find(|(owner, _)| *owner == target)
            .map(|(_, kind)| kind.clone())
        else {
            return;
        };
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return;
        };
        let inputs: Vec<SavedInput> = dialog.draft.iter().map(SavedInput::from_value).collect();
        if self.indicator_presets.insert(&kind, name, inputs) {
            self.indicator_presets.save(presets_path);
            dialog.preset_label = Some(name.trim().to_owned());
            dialog.preset_name_draft.clear();
        }
    }
    fn delete_indicator_preset(&mut self, name: &str, presets_path: &std::path::Path) {
        let target = self.indicator_settings_target;
        let Some(kind) = self
            .slot_kinds
            .iter()
            .find(|(owner, _)| *owner == target)
            .map(|(_, kind)| kind.clone())
        else {
            return;
        };
        if self.indicator_presets.remove(&kind, name) {
            self.indicator_presets.save(presets_path);
            if let Some(dialog) = self.indicator_settings.as_mut()
                && dialog.preset_label.as_deref() == Some(name)
            {
                dialog.preset_label = None;
            }
        }
    }
    pub(super) fn add_library(
        &mut self,
        pane: &mut ChartPane,
        target: (u64, PaneSide),
        index: usize,
    ) -> Option<(SlotId, LibraryAttachment)> {
        let entry = self.script_library.entries().get(index)?;
        let name = entry.name.clone();
        match self.script_library.read(index) {
            Some(Ok(text)) => {
                let attached = self.attach_script(pane, target, name, text, false);
                let slot = attached.target.slot;
                let watch = Some((attached.target, index));
                Some((
                    slot,
                    LibraryAttachment {
                        attachment: Some(attached),
                        watch,
                    },
                ))
            }
            Some(Err(message)) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNREADABLE",
                    script = %name,
                    error = %message,
                    action = "error_slot_shown",
                    "cannot read an indicator script"
                );
                // A click that produces nothing at all is the failure this
                // function's own doc comment rules out. The compile half of
                // that promise runs worker-side; the read half never leaves
                // the UI thread, so the error slot is built here, from the
                // same two events the worker would have sent.
                // The same kind string the healthy path derives from its
                // source, so an error slot the trader fixes and reloads keeps
                // whatever they had drawn on its pane.
                let slot = pane.indicators.allocate_slot(format!("script.{name}"));
                pane.indicators.apply(IndicatorEvent::Rebuilt {
                    slot,
                    descriptor: quantick_indicators::IndicatorDescriptor {
                        title: name,
                        short_title: None,
                        overlay: false,
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
                pane.indicators.apply(IndicatorEvent::Error {
                    slot,
                    error: quantick_indicators::EvalError {
                        bar_index: 0,
                        message,
                    },
                });
                Some((
                    slot,
                    LibraryAttachment {
                        attachment: None,
                        watch: None,
                    },
                ))
            }
            None => None,
        }
    }
    pub(super) fn log_reload(owner: TabSlot, name: &str) {
        tracing::info!(target: "quantick::app", schema_version = 1_u8,
            event_code = "INDICATOR_SCRIPT_RELOAD", script = %name, tab = owner.tab,
            pane = ?owner.side, action = "recompile_and_replay", "indicator script changed on disk");
    }

    pub(super) fn poll_script_files(&mut self) -> Vec<(TabSlot, String, String)> {
        if self.script_files.is_empty()
            || self.last_script_poll.elapsed() < SCRIPT_RELOAD_POLL_INTERVAL
        {
            return Vec::new();
        }
        self.last_script_poll = Instant::now();
        let mut reloads: Vec<(TabSlot, String, String)> = Vec::new();
        for (owner, index, seen_mtime) in &mut self.script_files {
            let Some((path, mtime)) = self.script_library.file_info(*index) else {
                continue;
            };
            if mtime == *seen_mtime {
                continue;
            }
            *seen_mtime = mtime;
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    reloads.push((*owner, name, text));
                }
                Err(error) => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNREADABLE",
                    script = %path.display(),
                    error = %error,
                    action = "reload_skipped",
                    "cannot re-read a changed indicator script"
                ),
            }
        }
        reloads
    }
}

#[cfg(test)]
#[path = "tests/indicator_owner_baselines.rs"]
mod owner_baselines;
