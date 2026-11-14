//! The indicator manager: what the window does about the slots on its panes.
//!
//! Registry, library, preset storage, script polling and addressed worker/layout
//! effects remain host-owned. The registered settings model owns the editor
//! lifecycle; its view borrows these domain services for each paint pass.

#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use eframe::egui;

use crate::indicator_legend;
#[cfg(any(feature = "scenario-harness", test))]
use crate::indicator_panel;
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorWorker, SlotId};
use crate::indicators::IndicatorView;
use crate::indicators::library::ScriptLibrary;
use crate::indicators::preset_file;
use crate::pane::ChartPane;
use crate::pane::PaneSide;
use crate::surfaces::indicator_settings::IndicatorSettingsSurface;
use quantick_chart::indicator_registry::{IndicatorAttachment, IndicatorHost, IndicatorRegistry};
use quantick_ui_model::indicator_settings::{Intent as SettingsIntent, State as SettingsState};

use super::TabSlot;

/// The indicator feature adapter: library, presets and polling clock.
/// The headless registry owns identities, provenance and deferred view state.
pub(super) struct IndicatorState {
    pub(super) registry: IndicatorRegistry,
    /// Loadable `.pine` scripts (embedded + indicators dir), scanned at
    /// startup. A file-backed script then follows its file: `poll_script_files`
    /// checks mtimes on a debounce and reloads on a save.
    pub(super) script_library: ScriptLibrary,

    /// Last hot-reload poll instant (the poll runs about once a second;
    /// file metadata every frame would be waste).
    pub(super) last_script_poll: Instant,
    // Named input setups per indicator kind, offered by the settings
    // dialog's preset picker.
    pub(super) indicator_presets: preset_file::PresetStore,
}

impl IndicatorState {
    /// Initialize the feature at its existing startup phase: scan, clock, presets.
    pub(super) fn new(
        workspace: quantick_workspace::lifetime::WorkspaceId,
        presets_path: &std::path::Path,
    ) -> Self {
        Self {
            registry: IndicatorRegistry::new(workspace),
            script_library: crate::indicators::library::scan(),
            last_script_poll: Instant::now(),
            indicator_presets: preset_file::PresetStore::load(presets_path),
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

impl IndicatorEdit {
    pub(super) fn target(&self) -> TabSlot {
        match self {
            Self::Attached(attachment) => attachment.target,
            Self::Hidden(target)
            | Self::Remove(target)
            | Self::Inputs(target, _)
            | Self::Style(target) => *target,
        }
    }
}

/// The settings owner distinguishes a worker preview from a persisted edit.
pub(super) enum SettingsChange {
    Inputs {
        target: TabSlot,
        slot: SlotId,
        values: Vec<quantick_indicators::InputValue>,
        commit: bool,
    },
    Style(TabSlot),
}

impl From<quantick_ui_model::indicator_settings::Change> for SettingsChange {
    fn from(change: quantick_ui_model::indicator_settings::Change) -> Self {
        match change {
            quantick_ui_model::indicator_settings::Change::Inputs {
                target,
                values,
                commit,
            } => Self::Inputs {
                target,
                slot: target.slot,
                values,
                commit,
            },
            quantick_ui_model::indicator_settings::Change::Style(target) => Self::Style(target),
        }
    }
}

impl SettingsChange {
    pub(super) fn target(&self) -> TabSlot {
        match self {
            Self::Inputs { target, .. } | Self::Style(target) => *target,
        }
    }
    pub(super) fn worker_target(&self) -> Option<TabSlot> {
        match self {
            Self::Inputs { target, .. } => Some(*target),
            _ => None,
        }
    }

    pub(super) fn deliver(self, worker: Option<&IndicatorWorker>) -> Option<IndicatorEdit> {
        match self {
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
        self.registry
            .attach_script(pane, target, name, text, by_operator)
    }

    pub(super) fn attach_native(
        &mut self,
        pane: &mut impl IndicatorHost,
        target: (u64, PaneSide),
        id: &str,
    ) -> IndicatorAttachment {
        self.registry.attach_native(pane, target, id)
    }

    pub(super) fn operator_target(&mut self, slot: u64) -> Result<Option<TabSlot>, ()> {
        self.registry.operator_target(slot).map_err(|_| ())
    }

    /// Called only after the layout mirror has read this slot's position.
    pub(super) fn remove(&mut self, pane: Option<&mut impl IndicatorHost>, target: TabSlot) {
        self.registry.remove(pane, target);
    }

    pub(super) fn open_settings(
        settings: &mut IndicatorSettingsSurface,
        target: TabSlot,
        view: Option<&IndicatorView>,
    ) {
        if let Some(view) = view {
            settings.update(SettingsIntent::Open {
                target,
                title: view.label().to_owned(),
                values: view.input_values.clone(),
            });
        }
    }

    /// Resolve the indexed launch request on the focused pane, then the flow pane.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) fn open_autostart(
        &self,
        settings: &mut IndicatorSettingsSurface,
        tab: u64,
        focused: (PaneSide, u64, &[IndicatorView]),
        flow: (u64, &[IndicatorView]),
        request: Option<(usize, indicator_panel::SettingsTab)>,
    ) -> bool {
        let Some((index, settings_tab)) = request else {
            return false;
        };
        let Some((side, pane, view)) = [focused, (PaneSide::Flow, flow.0, flow.1)]
            .into_iter()
            .find_map(|(side, pane, views)| views.get(index).map(|view| (side, pane, view)))
        else {
            return false;
        };
        Self::open_settings(
            settings,
            TabSlot {
                workspace: self.registry.workspace(),
                pane,
                tab,
                side,
                slot: view.slot,
            },
            Some(view),
        );
        settings.update(SettingsIntent::SelectTab(settings_tab));
        true
    }

    /// A deferred launch opens only when the worker has produced editable inputs.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) fn open_first_editable(
        &self,
        settings: &mut IndicatorSettingsSurface,
        tab: u64,
        flow: (u64, &[IndicatorView]),
        requested: bool,
    ) -> bool {
        if !requested || settings.model_state.dialog().is_some() {
            return false;
        }
        let Some(view) = flow.1.iter().find(|view| !view.input_values.is_empty()) else {
            return false;
        };
        Self::open_settings(
            settings,
            TabSlot {
                workspace: self.registry.workspace(),
                pane: flow.0,
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
        settings: &mut IndicatorSettingsSurface,
        pane: &mut ChartPane,
        (tab, side): (u64, PaneSide),
        action: indicator_legend::LegendAction,
    ) -> Option<IndicatorEdit> {
        let workspace = self.registry.workspace();
        let pane_id = pane.id;
        let at = |slot| TabSlot {
            workspace,
            pane: pane_id,
            tab,
            side,
            slot,
        };
        match action {
            indicator_legend::LegendAction::OpenSettings(slot) => {
                Self::open_settings(
                    settings,
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
        settings: &SettingsState,
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
        let preview = settings
            .dialog()
            .filter(|dialog| {
                dialog.previewed && dialog.target.tab == tab && dialog.target.side == side
            })
            .map(|dialog| dialog.target.slot);
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

    /// Preserve peer-before-origin watch registration after layout mirroring.
    pub(super) fn watch_attachment(&mut self, watch: Option<(TabSlot, usize)>) {
        if let Some((owner, index)) = watch
            && let Some((_, mtime)) = self.script_library.file_info(index)
        {
            self.registry.watch_script(owner, index, mtime);
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
                        title: quantick_stores::script_library::script_title(&name),
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
}

#[cfg(test)]
#[path = "tests/indicator_owner_baselines.rs"]
mod owner_baselines;
