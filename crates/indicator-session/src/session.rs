use crate::{IndicatorEvent, IndicatorSource, SessionEffects, SlotId};
use quantick_engine::forming_run::FormingRun;
use quantick_indicators::{EvalError, IndicatorDescriptor, IndicatorHost, InputValue, InstanceId};
use std::collections::BTreeMap;
/// Live host, source bindings and publication history for one consumer.
/// Runtime, filesystem, UI and persistence state belong to the caller.
#[derive(Default)]
pub struct IndicatorSession {
    pub(crate) host: IndicatorHost,
    pub(crate) slots: BTreeMap<SlotId, SlotMirror>,
    pub(crate) lane_run: FormingRun,
    pub(crate) retired_folds: u64,
}
/// Worker-side bookkeeping for one slot: the host id plus what the UI is
/// known to have, so every event is an exact delta.
pub(crate) struct SlotMirror {
    /// `None`: the source never loaded (script compile error) — the slot
    /// exists only as a UI entry carrying its error.
    pub(crate) host_id: Option<InstanceId>,
    /// What to rebuild from when inputs change or a script reloads.
    pub(crate) source: IndicatorSource,
    /// Values currently bound to the declared inputs.
    pub(crate) values: Vec<InputValue>,
    /// Committed rows the UI has (via Rebuilt/Appended events).
    pub(crate) known_rows: usize,
    /// Whether the UI has been sent a `Rebuilt` for this slot's current
    /// shape. `known_rows == 0` cannot stand in for it: before the first bar
    /// closes — the whole first bar on a slow tape, and again after every
    /// reset or replay seek — that would re-send a full descriptor + column
    /// clone on every drained batch.
    pub(crate) synced: bool,
    /// Whether the UI has been told about the current error state.
    pub(crate) error_reported: bool,
    /// Set when a reload failed to compile: the instance still running is
    /// older than the file on disk. Cleared by the next good load.
    pub(crate) stale: Option<String>,
    /// Store revision last published to the UI (0 = nothing yet).
    pub(crate) objects_revision: u64,
}

impl IndicatorSession {
    pub fn new() -> Self {
        Self::default()
    }
    pub(crate) fn add(
        &mut self,
        slot: SlotId,
        source: IndicatorSource,
        effects: &mut impl SessionEffects,
    ) {
        let host = &mut self.host;
        let slots = &mut self.slots;
        match source.build(effects) {
            Ok(indicator) => {
                // Straight from the instance, not from the declared
                // defaults: `Ema::new(3, ..)` still *declares* 9, so
                // a mirror seeded from the schema would report a
                // value the indicator is not running, and Apply
                // without touching a widget would write it back.
                let values = indicator.input_values();
                let host_id = host.add(indicator);
                slots.insert(
                    slot,
                    SlotMirror {
                        host_id: Some(host_id),
                        source,
                        values,
                        known_rows: 0,
                        synced: false,
                        error_reported: false,
                        stale: None,
                        objects_revision: 0,
                    },
                );
            }
            Err(message) => {
                // The slot still exists UI-side, carrying its load
                // error: a script that does not compile is shown,
                // with lines and codes, never silently dropped.
                effects.event(IndicatorEvent::Rebuilt {
                    slot,
                    descriptor: IndicatorDescriptor {
                        title: source.fallback_title(),
                        short_title: None,
                        overlay: false,
                        plots: Vec::new(),
                        inputs: Vec::new(),
                        fills: Vec::new(),
                    },
                    columns: Vec::new(),
                    bar_paint: Vec::new(),
                    rows: 0,
                    inputs: Vec::new(),
                    stale: None,
                });
                effects.event(IndicatorEvent::Error {
                    slot,
                    error: EvalError {
                        bar_index: 0,
                        message,
                    },
                });
                slots.insert(
                    slot,
                    SlotMirror {
                        host_id: None,
                        source,
                        values: Vec::new(),
                        known_rows: 0,
                        synced: true,
                        error_reported: true,
                        stale: None,
                        objects_revision: 0,
                    },
                );
            }
        }
    }
    pub(crate) fn set_inputs(
        &mut self,
        slot: SlotId,
        values: Vec<InputValue>,
        effects: &mut impl SessionEffects,
    ) {
        let host = &mut self.host;
        let slots = &mut self.slots;
        if let Some(mirror) = slots.get_mut(&slot) {
            let Some(host_id) = mirror.host_id else {
                // No instance to rebuild: this slot's first
                // compile failed. The values are still the
                // trader's, though, and `Reload` builds from this
                // mirror — so keep them. Dropping them here is
                // what made repairing a broken script cost every
                // setting it had: the fixed script loaded at its
                // declared defaults, the slot's error cleared,
                // and the next state save wrote those defaults
                // over the tuned ones on disk.
                mirror.values = values;
                return;
            };
            match mirror.source.build_with(Some(&values), effects) {
                Ok(indicator) => {
                    // Mirror what the instance bound, not what
                    // was asked for: every fallback inside the
                    // build is silent, and a discarded input
                    // recorded as applied is exactly the
                    // "inferred data, silently patched" the
                    // honesty rule forbids.
                    mirror.values = indicator.input_values();
                    host.replace(host_id, indicator);
                    // Force a full Rebuilt on publish: the new
                    // instance replayed the whole history, and
                    // its descriptor may have changed too.
                    mirror.known_rows = 0;
                    mirror.synced = false;
                    mirror.error_reported = false;
                    mirror.objects_revision = 0;
                }
                Err(message) => {
                    effects.event(IndicatorEvent::Error {
                        slot,
                        error: EvalError {
                            bar_index: 0,
                            message,
                        },
                    });
                }
            }
        }
    }
    pub(crate) fn reload(
        &mut self,
        slot: SlotId,
        source: IndicatorSource,
        effects: &mut impl SessionEffects,
    ) {
        let host = &mut self.host;
        let slots = &mut self.slots;
        if let Some(mirror) = slots.get_mut(&slot) {
            // Build with the values the user set, not with the
            // declared defaults: editing a comment in a script
            // used to reset `len = 50` back to 20 with no
            // message. `build_with` falls back to the defaults
            // by itself when the input set changed.
            match source.build_with(Some(&mirror.values), effects) {
                Ok(indicator) => {
                    let values = indicator.input_values();
                    match mirror.host_id {
                        Some(host_id) => {
                            host.replace(host_id, indicator);
                        }
                        // The slot never loaded (its first
                        // compile failed): the reload is its
                        // first working version.
                        None => mirror.host_id = Some(host.add(indicator)),
                    }
                    mirror.source = source;
                    mirror.values = values;
                    // A reload can change the title, the plots
                    // and the input set, so the UI needs the
                    // whole slot again, not an append.
                    mirror.known_rows = 0;
                    mirror.synced = false;
                    mirror.error_reported = false;
                    mirror.objects_revision = 0;
                    // A good load is no longer stale.
                    mirror.stale = None;
                }
                Err(message) => {
                    mirror.stale = Some(message.clone());
                    effects.event(IndicatorEvent::ReloadFailed { slot, message });
                }
            }
        }
    }
    pub(crate) fn remove(&mut self, slot: SlotId) {
        if let Some(mirror) = self.slots.remove(&slot)
            && let Some(host_id) = mirror.host_id
        {
            self.host.remove(host_id);
        }
    }
}
