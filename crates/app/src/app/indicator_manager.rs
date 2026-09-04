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
use smallvec::SmallVec;

use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::indicator_legend;
use crate::indicator_panel::{self, SettingsDialog, SettingsOutcome};
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::IndicatorView;
use crate::indicators::state_file::{SavedInput, SavedKind};
use crate::pane::PaneSide;
use crate::style::CandlePreset;

use super::{QuantickApp, TabSlot};

/// How often the hot-reload poll checks script files for changes.
const SCRIPT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(1_000);

impl QuantickApp {
    /// Put one Quantick Pine script on the focused pane behind a fresh slot.
    ///
    /// The one door: the script library's click arrives here, and so does an
    /// authorized agent's `indicator.script.attach`. Returns the tab, the
    /// pane and the slot, which is what a caller needs to detach it again.
    pub(crate) fn attach_script_indicator(
        &mut self,
        name: String,
        text: String,
        by_operator: bool,
    ) -> (u64, crate::control::PaneSideDto, SlotId) {
        let slot = self
            .focused_pane_mut()
            .add_indicator(IndicatorSource::Script {
                name: name.clone(),
                text,
            });
        let owner = self.target_slot(slot);
        let kind = SavedKind::Script { name };
        self.slot_kinds.push((owner, kind.clone()));
        // Whose slot this is decides who may take it away again: the annotate
        // tier removes what it attached, never what the trader put there. An
        // operator's overlay stays on the one pane it was attached to and
        // out of the layout; the trader's script is a layout edit and goes
        // onto every pane.
        if by_operator {
            self.operator_slots.insert(owner);
        } else {
            self.mirror_add(owner, &kind);
        }
        self.note_indicator_edit_at(owner.tab, owner.side);
        (owner.tab, owner.side.into(), slot)
    }

    /// Take one slot **an operator attached** off the chart, through the same
    /// removal the indicator legend's own button uses.
    ///
    /// `Err` when the slot is one the trader put there: this tier adds and
    /// takes back its own, and never removes work done by hand (plan §2.6).
    /// `Ok(false)` when there is no such slot at all.
    pub(crate) fn detach_script_indicator(&mut self, slot: u64) -> Result<bool, ()> {
        // A slot number is allocated per pane, so several panes can carry the
        // same one: the operator's own is the one to take, and matching on the
        // number alone would remove whichever pane happened to be registered
        // first — the trader's, as often as not.
        let mut known = false;
        let mut target = None;
        for (owner, _) in &self.slot_kinds {
            if owner.slot.0 != slot {
                continue;
            }
            known = true;
            if self.operator_slots.contains(owner) {
                target = Some(*owner);
                break;
            }
        }
        let Some(target) = target else {
            // A slot that exists but belongs to the trader is refused; one
            // that exists nowhere simply was not there.
            return if known { Err(()) } else { Ok(false) };
        };
        self.remove_indicator_at(target);
        self.operator_slots.remove(&target);
        Ok(true)
    }

    /// Open the dialog for whichever indicator a gesture on a pane asked
    /// about: the pane header, a collapsed strip, or an overlay's own line.
    ///
    /// Drained here rather than acted on where it was read, because the dialog
    /// belongs to the window and the gestures are read deep inside a pane's
    /// input pass. Both sides of a split are asked, and each request resolves
    /// against the pane that raised it — the legend's rule (MAJOR-4), applied
    /// to the same problem one layer down.
    pub(super) fn open_requested_indicator_settings(&mut self) {
        let tab_id = self.active_tab().id;
        // The harness hook waits for the view it names, exactly as the restored
        // hidden flags do: the indicator is born from the worker's first
        // Rebuilt, which is several frames after the window opens.
        if let Some((index, tab)) = self.harness.settings_autostart() {
            // The focused pane first, then the flow pane. A split tab can open
            // with the *time* pane focused while every indicator — the ones the
            // other autostart hook adds, and the ones the state file restores —
            // lives on the flow pane, and a hook that only asked the focused
            // side then waited for a view that was never coming. Silence is the
            // worst failure a validation hook can have: the run captures a
            // chart with no dialog on it and nothing says why.
            let focused = self.active_tab().focused_side();
            let found = [focused, PaneSide::Flow].into_iter().find_map(|side| {
                self.tabs
                    .iter()
                    .find(|candidate| candidate.id == tab_id)
                    .map(|candidate| candidate.pane(side))
                    .and_then(|pane| pane.indicators.all().get(index))
                    .map(|view| (side, view.slot))
            });
            if let Some((side, slot)) = found {
                self.harness.settings_autostart_opened();
                self.open_indicator_settings_at(TabSlot {
                    tab: tab_id,
                    side,
                    slot,
                });
                if let Some(dialog) = self.indicator_settings.as_mut() {
                    dialog.tab = tab;
                }
            }
        }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return;
        };
        let requests: SmallVec<[(PaneSide, SlotId); MAX_CANVAS_PANES]> = tab
            .panes_with_sides_mut()
            .filter_map(|(pane, side)| pane.take_settings_request().map(|slot| (side, slot)))
            .collect();
        for (side, slot) in requests {
            {
                self.open_indicator_settings_at(TabSlot {
                    tab: tab_id,
                    side,
                    slot,
                });
            }
        }
    }

    /// Fold or unfold one pane's indicator legend.
    ///
    /// The one place the state changes. The chevron, the menu entry, the
    /// hotkey and the harness hook all arrive here with the outcome they want,
    /// so none of them can leave a pane in a state the others would not have
    /// produced — and an operator that is not holding the mouse names this
    /// call rather than a click.
    fn set_legend_collapsed(&mut self, tab_id: u64, side: PaneSide, collapsed: bool) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return;
        };
        tab.pane_mut(side).legend_collapsed = collapsed;
    }

    /// Whether the focused pane's legend is folded — what the menu label and
    /// the hotkey read before deciding which way to flip it.
    pub(super) fn focused_legend_collapsed(&self) -> bool {
        let tab = self.active_tab();
        tab.pane(tab.focused_side()).legend_collapsed
    }

    /// Fold or unfold the legend of the pane the chrome speaks for. The menu
    /// entry and the hotkey both act through this, so "which chart did that
    /// affect" has one answer: the focused one, the same pane every other
    /// chrome control acts on.
    pub(super) fn set_focused_legend_collapsed(&mut self, collapsed: bool) {
        let tab_id = self.active_tab().id;
        let side = self.active_tab().focused_side();
        self.set_legend_collapsed(tab_id, side, collapsed);
    }

    /// Flip a slot's render-side eye, wherever the slot lives. Addressed by
    /// [`TabSlot`], never by focus: the legend acts on the pane it is drawn
    /// on, and the toolbar path builds its target from focus before calling.
    pub(super) fn toggle_indicator_hidden_at(&mut self, target: TabSlot) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) else {
            return;
        };
        tab.pane_mut(target.side)
            .indicators
            .toggle_hidden(target.slot);
        self.mirror_hidden(target);
        self.note_indicator_edit_at(target.tab, target.side);
    }

    /// Remove a slot, wherever it lives. UI first (the entry vanishes this
    /// frame), worker second; events already in flight for the slot are
    /// dropped on apply.
    pub(super) fn remove_indicator_at(&mut self, target: TabSlot) {
        if !self.tabs.iter().any(|tab| tab.id == target.tab) {
            return;
        }
        // The mirrors first, while the slot's layout position can still be
        // read off the bookkeeping.
        self.mirror_remove(target);
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) else {
            return;
        };
        let pane = tab.pane_mut(target.side);
        pane.indicators.remove(target.slot);
        pane.indicator_worker
            .send(IndicatorCommand::Remove(target.slot));
        self.slot_kinds.retain(|(owner, _)| *owner != target);
        self.operator_slots.remove(&target);
        self.script_files.retain(|(owner, ..)| *owner != target);
        self.pending_hidden.retain(|owner| *owner != target);
        self.pending_styles.retain(|(owner, _)| *owner != target);
        self.note_indicator_edit_at(target.tab, target.side);
    }

    /// Open the settings dialog for a slot, wherever it lives.
    pub(super) fn open_indicator_settings_at(&mut self, target: TabSlot) {
        let Some(view) = self
            .tabs
            .iter()
            .find(|tab| tab.id == target.tab)
            .map(|tab| tab.pane(target.side))
            .and_then(|pane| {
                pane.indicators
                    .all()
                    .iter()
                    .find(|view| view.slot == target.slot)
            })
        else {
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

    /// Draw each visible pane's indicator legend and run what its rows asked
    /// for. Actions resolve against the pane the legend was drawn on — the
    /// legend must never act on the chart beside it (the audit's MAJOR-4
    /// trap, avoided by construction).
    pub(super) fn draw_indicator_legends(&mut self, ctx: &egui::Context) {
        let tab_id = self.active_tab().id;
        // Whether a context chart is on screen — from what the layout holds
        // and whether the column is collapsed, never from one variant. Matched
        // against `TimeAndFlow` alone, this drew no legend at all on the
        // stacked charts of `time+time+flow`, and drew one over the flow chart
        // against a stale rect while the column was put away.
        let split = self.active_tab().shows_context_charts();
        let mut pending: Vec<(PaneSide, indicator_legend::LegendAction)> = Vec::new();
        let shown = self.active_tab().context_panes_shown();
        let sides: SmallVec<[PaneSide; MAX_CANVAS_PANES]> = self.active_tab().sides().collect();
        for side in sides {
            // Only a context chart on screen draws a legend: the stack may
            // hold a pane the layout no longer shows.
            if let PaneSide::Time(slot) = side
                && (!split || slot >= shown)
            {
                continue;
            }
            let pane = self.active_tab().pane(side);
            // The rect is last frame's, like every anchor the input path
            // reads; a pane not yet drawn has none and draws no legend.
            let Some(mut rect) = pane.last_chart_area else {
                continue;
            };
            // The position HUD owns the very corner of the pane it paints on;
            // this legend rides just below it there, and nowhere else. That
            // pane is the one holding the HUD anchor — the focused one, not
            // the flow one, so a split with the time pane focused drops these
            // chips on the *time* pane. The order-flow key stacks under this
            // legend and reads the same offset from the same place, so the two
            // cannot drift apart.
            rect.min.y += indicator_legend::hud_offset_px(
                pane.paper_hud_anchor().is_some()
                    && self.active_tab().paper.position_summary().is_some(),
            );
            // The slot being live-previewed by the settings dialog, if it
            // lives on this pane: its legend row wears a "preview" chip.
            let preview_slot = self
                .indicator_settings
                .as_ref()
                .filter(|dialog| dialog.previewed)
                .filter(|_| {
                    self.indicator_settings_target.tab == tab_id
                        && self.indicator_settings_target.side == side
                })
                .map(|dialog| dialog.slot);
            for action in indicator_legend::draw(
                ctx,
                pane.id,
                rect,
                pane.indicators.all(),
                preview_slot,
                pane.legend_collapsed,
            ) {
                pending.push((side, action));
            }
        }
        for (side, action) in pending {
            let at = |slot| TabSlot {
                tab: tab_id,
                side,
                slot,
            };
            match action {
                indicator_legend::LegendAction::ToggleHidden(slot) => {
                    self.toggle_indicator_hidden_at(at(slot));
                }
                indicator_legend::LegendAction::OpenSettings(slot) => {
                    self.open_indicator_settings_at(at(slot));
                }
                indicator_legend::LegendAction::Remove(slot) => {
                    self.remove_indicator_at(at(slot));
                }
                indicator_legend::LegendAction::SetCollapsed(collapsed) => {
                    self.set_legend_collapsed(tab_id, side, collapsed);
                }
            }
        }
    }

    /// Draw the settings dialog and execute its outcome. Apply goes through
    /// the worker (construct anew, replace, replay) — the same path every
    /// input change takes, UI or not.
    pub(super) fn draw_indicator_settings(&mut self, ctx: &egui::Context) {
        // The boot hook's deferred half: fire once a slot can actually show
        // a dialog (see `harness::Harness::wants_indicator_settings_dialog`).
        if self.harness.wants_indicator_settings_dialog() && self.indicator_settings.is_none() {
            let tab_id = self.active_tab().id;
            if let Some(slot) = self
                .active_tab()
                .flow_pane
                .indicators
                .all()
                .iter()
                .find(|view| !view.input_values.is_empty())
                .map(|view| view.slot)
            {
                self.open_indicator_settings_at(TabSlot {
                    tab: tab_id,
                    side: PaneSide::Flow,
                    slot,
                });
                self.harness.indicator_settings_dialog_opened();
            }
        }
        let target = self.indicator_settings_target;
        // The preset shelf for this slot's kind. A slot with no registered
        // kind (the natives autostart hook deliberately registers none) has
        // nowhere to save to, and the dialog hides the picker.
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
        let outcome = {
            let Self {
                indicator_settings,
                tabs,
                ..
            } = self;
            let Some(dialog) = indicator_settings.as_mut() else {
                return;
            };
            // The tab the dialog was opened on may have been closed under it.
            let Some(view) = tabs
                .iter_mut()
                .find(|tab| tab.id == target.tab)
                .map(|tab| tab.pane_mut(target.side))
                .and_then(|pane| pane.indicators.view_mut(dialog.slot))
            else {
                // The indicator was removed under the dialog.
                *indicator_settings = None;
                return;
            };
            // Follow the live label: an applied edit can retitle the
            // indicator (`EMA(9)` → `EMA(21)`), and a dialog that stays open
            // must not keep announcing the old one.
            dialog.title = view.label().to_owned();
            // The descriptor is read while the style layer beside it is
            // written, so the two halves are split off the same view here
            // rather than borrowed twice.
            let IndicatorView {
                descriptor, style, ..
            } = view;
            indicator_panel::draw(
                ctx,
                dialog,
                &descriptor.inputs,
                &descriptor.plots,
                style,
                preset_names.as_deref(),
            )
        };
        match outcome {
            SettingsOutcome::Open => {}
            SettingsOutcome::Close => {
                self.revert_indicator_settings_preview();
                self.indicator_settings = None;
            }
            SettingsOutcome::Apply => self.apply_indicator_settings_draft(),
            SettingsOutcome::Preview => self.preview_indicator_settings_draft(),
            SettingsOutcome::LoadPreset(name) => self.load_indicator_preset(name),
            SettingsOutcome::SavePreset(name) => self.save_indicator_preset(&name),
            SettingsOutcome::DeletePreset(name) => self.delete_indicator_preset(&name),
            // Style is render-side: the chart already shows it and there is
            // nothing to preview or commit, so all that is owed is the file.
            // Deliberately unlike an input edit — it takes no rebuild and no
            // replay, and it follows the legend's eye, which is also instant
            // and also persisted without an Apply.
            SettingsOutcome::StyleChanged => {
                let target = self.indicator_settings_target;
                self.mirror_style(target);
                self.note_indicator_edit_at(target.tab, target.side);
            }
        }
    }

    /// Replace the dialog's draft with a preset (`None` = the declared
    /// defaults) and preview it — the same nudge-and-look contract as a
    /// slider: Apply commits, Discard reverts. A saved value whose type no
    /// longer matches its input (the script evolved under the preset) falls
    /// back to that input's default rather than being silently coerced.
    fn load_indicator_preset(&mut self, name: Option<String>) {
        let target = self.indicator_settings_target;
        let Some(specs) = self
            .tabs
            .iter()
            .find(|tab| tab.id == target.tab)
            .map(|tab| tab.pane(target.side))
            .and_then(|pane| {
                pane.indicators
                    .all()
                    .iter()
                    .find(|view| view.slot == target.slot)
            })
            .map(|view| view.descriptor.inputs.clone())
        else {
            return;
        };
        let saved: Option<Vec<Option<quantick_indicators::InputValue>>> = match &name {
            None => Some(Vec::new()),
            Some(name) => {
                let Some(kind) = self
                    .slot_kinds
                    .iter()
                    .find(|(owner, _)| *owner == target)
                    .map(|(_, kind)| kind)
                else {
                    return;
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
            return;
        };
        // The same binder the worker binds a saved state file with: one rule
        // for what a saved value means, in one place.
        let bound = quantick_indicators::bind_by_position(&specs, &saved);
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
        self.preview_indicator_settings_draft();
    }

    /// Save the dialog's current draft under `name` for this slot's kind.
    /// Saving is a file write, never a chart change — the draft on screen
    /// stays exactly as it was.
    fn save_indicator_preset(&mut self, name: &str) {
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
            self.indicator_presets
                .save(self.workspace.indicator_presets_path());
            dialog.preset_label = Some(name.trim().to_owned());
            dialog.preset_name_draft.clear();
        }
    }

    /// Forget a preset of this slot's kind. The values on screen stay —
    /// deleting a name never touches the chart.
    fn delete_indicator_preset(&mut self, name: &str) {
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
            self.indicator_presets
                .save(self.workspace.indicator_presets_path());
            if let Some(dialog) = self.indicator_settings.as_mut()
                && dialog.preset_label.as_deref() == Some(name)
            {
                dialog.preset_label = None;
            }
        }
    }

    /// Send the open dialog's draft to the worker and keep the dialog open
    /// (audit M2): tuning is a nudge-and-look loop, and a dialog that dies
    /// on every Apply makes each attempt four clicks. The slot is the one
    /// the dialog was opened on, not whatever has focus now — clicking
    /// Apply must not retarget the edit.
    pub(super) fn apply_indicator_settings_draft(&mut self) {
        let target = self.indicator_settings_target;
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return;
        };
        dialog.committed = dialog.draft.clone();
        dialog.previewed = false;
        let (slot, values) = (dialog.slot, dialog.draft.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
            tab.pane_mut(target.side)
                .indicator_worker
                .send(IndicatorCommand::SetInputs {
                    slot,
                    values: values.clone(),
                });
        }
        self.mirror_inputs(target, &values);
        self.note_indicator_edit_at(target.tab, target.side);
    }

    /// Show the draft on the chart without committing it: same worker path
    /// as Apply, but the state file is never marked dirty — what survives a
    /// restart is always the last Apply, never a slider mid-drag. The worker
    /// coalesces a burst of these to its own cadence.
    pub(super) fn preview_indicator_settings_draft(&mut self) {
        let target = self.indicator_settings_target;
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return;
        };
        dialog.previewed = true;
        let (slot, values) = (dialog.slot, dialog.draft.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
            tab.pane_mut(target.side)
                .indicator_worker
                .send(IndicatorCommand::SetInputs { slot, values });
        }
    }

    /// Put the last committed values back on the chart. Close's half of the
    /// preview contract: a dialog dismissed mid-tuning leaves no trace.
    fn revert_indicator_settings_preview(&mut self) {
        let target = self.indicator_settings_target;
        let Some(dialog) = self.indicator_settings.as_ref() else {
            return;
        };
        if !dialog.previewed {
            return;
        }
        let (slot, values) = (dialog.slot, dialog.committed.clone());
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == target.tab) {
            tab.pane_mut(target.side)
                .indicator_worker
                .send(IndicatorCommand::SetInputs { slot, values });
        }
    }

    /// Load a library script behind a fresh slot. A file that no longer
    /// reads or a script that no longer compiles becomes the slot's error —
    /// shown with lines and codes, never silently dropped.
    ///
    /// Returns the slot it claimed, so a caller that needs to address the new
    /// indicator (restoring saved inputs, say) does not have to guess which
    /// one it is.
    pub(super) fn add_script_indicator(&mut self, index: usize) -> Option<SlotId> {
        let entry = self.script_library.entries().get(index)?;
        let name = entry.name.clone();
        match self.script_library.read(index) {
            Some(Ok(text)) => {
                let (_, _, slot) = self.attach_script_indicator(name, text, false);
                let owner = self.target_slot(slot);
                // Watch the file so a save reloads it. Registered here, with
                // the add, so the two cannot drift apart.
                if let Some((_, mtime)) = self.script_library.file_info(index) {
                    self.script_files.push((owner, index, mtime));
                }
                Some(slot)
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
                let pane = self.focused_pane_mut();
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
                Some(slot)
            }
            None => None,
        }
    }

    pub(super) fn emit_style_changed(&mut self, applied_preset: Option<CandlePreset>) {
        let candles = &self.style.candles;
        let preset = applied_preset
            .or_else(|| CandlePreset::detect(candles))
            .map_or("custom", CandlePreset::log_value);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANDLE_STYLE_CHANGED",
            revision = self.style_revision,
            preset,
            body_mode = ?candles.body_mode,
            fill_opacity = candles.fill_opacity,
            outline_opacity = candles.outline_opacity,
            outline_width_px = candles.outline_width,
            body_width_fraction = candles.body_width_frac,
            wick_mode = ?candles.wick_color_mode,
            wick_width_px = candles.wick_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            action = "redraw_only",
            "candle appearance changed"
        );
    }

    /// Hot reload: about once a second, compare each file-backed script's
    /// mtime; a changed file is re-read and sent as a Reload — recompiled
    /// and replayed on success, or flagged stale (the last good version
    /// keeps running) on errors. The mtime updates even when the compile
    /// fails, so a broken save does not re-fire every second.
    pub(super) fn poll_script_files(&mut self) {
        if self.script_files.is_empty()
            || self.last_script_poll.elapsed() < SCRIPT_RELOAD_POLL_INTERVAL
        {
            return;
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
        for (owner, name, text) in reloads {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_SCRIPT_RELOAD",
                script = %name,
                tab = owner.tab,
                pane = ?owner.side,
                action = "recompile_and_replay",
                "indicator script changed on disk"
            );
            // To the worker that owns the slot: the same script loaded on two
            // panes is two slots, and a Reload sent to the wrong one addresses
            // whatever indicator happens to share its number there.
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == owner.tab) {
                tab.pane_mut(owner.side)
                    .indicator_worker
                    .send(IndicatorCommand::Reload {
                        slot: owner.slot,
                        source: IndicatorSource::Script { name, text },
                    });
            }
        }
    }

    /// Add one of the built-in indicators to the focused pane and register how
    /// it restores.
    ///
    /// The kind travels to the worker as-is: an id no build ships becomes an
    /// error slot naming it, which is why there is no fallback arm here. The
    /// one this replaced turned every unrecognised kind into an EMA, so a
    /// mistyped id put an indicator on the chart that nobody had asked for.
    pub(super) fn add_native_indicator(&mut self, id: &str) -> SlotId {
        let kind = SavedKind::native(id);
        let source = IndicatorSource::Native {
            id: id.to_owned(),
            values: Vec::new(),
        };
        let slot = self.focused_pane_mut().add_indicator(source);
        let owner = self.target_slot(slot);
        self.slot_kinds.push((owner, kind.clone()));
        // Every other pane of every tab gets the same indicator now; the
        // settled reconciliation binds the layout's copy of it.
        self.mirror_add(owner, &kind);
        self.note_indicator_edit_at(owner.tab, owner.side);
        slot
    }

    /// An indicator edit happened on the focused pane. Edits that know their
    /// pane call [`Self::note_indicator_edit_at`] directly.
    pub(super) fn mark_indicator_state_dirty(&mut self) {
        let (tab, side) = {
            let tab = self.active_tab();
            (tab.id, tab.focused_side())
        };
        self.note_indicator_edit_at(tab, side);
    }

    /// Undo the dirty mark an add just set — for indicators an env var asked
    /// for rather than the user. The slot still exists and still works; it
    /// simply does not enter the persisted set.
    pub(super) fn forget_last_indicator_state_change(&mut self) {
        self.slot_kinds.pop();
    }

    /// Apply layout-placed hide flags and styles once their views exist.
    ///
    /// The layout itself is written by the edit that changed it — see
    /// `layout_wiring` — so nothing here reads a view back.
    pub(super) fn maintain_indicator_state(&mut self) {
        self.apply_pending_indicator_state();
    }
}
