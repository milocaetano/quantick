//! The bubble look on one view's screen: the presets it is picked from, the
//! name it started from, the asset it is kept for and what launch hooks hold
//! over it — every decision the panel and the asset make about it, apart from
//! the window that draws it.
//!
//! The owner never holds the view's [`HeatmapConfig`]: each call is handed
//! the screen it reads or dresses, and a look worn from an asset comes back
//! to the caller, who puts it on screen and tells its worker
//! ([`BubbleLook::follow_declared`], [`BubbleLook::sync`]). Saving and
//! reloading take the presets file's writer and reader from the caller,
//! which resolves where the file lives.

use std::path::PathBuf;

use quantick_orderflow::HeatmapConfig;

use crate::bubble_asset_store::{Adoption, AssetBinding, RunHold, SaveSwitch};
use crate::bubble_assets::AssetBubbles;
use crate::bubble_presets::{BubblePreset, BubblePresetFile, PresetSource};

/// The presets file as a loader reads it: the document, where it came from
/// and why it could not be read, if it could not.
pub type LoadedPresets = (BubblePresetFile, PresetSource, Option<String>);

/// An asset's settings for the caller to put on screen, and whether the
/// lane's width and window must stay where the view's own gestures put them.
pub type Worn = (AssetBubbles, bool);

/// The look one view shows, and everything that decides it.
#[derive(Debug)]
pub struct BubbleLook {
    /// Named bubble looks, loaded from the versionable presets file.
    presets: BubblePresetFile,
    /// Where those presets came from, shown in the panel.
    source: PresetSource,
    /// Name being typed for the next save.
    name_draft: String,
    /// Last preset action (or failure), shown verbatim in the panel.
    status: Option<String>,
    /// Name of the preset the look on screen started from; empty once the
    /// panel's defaults replaced it. `presets.active` stays the file's own.
    look_name: String,
    /// The asset these settings belong to, bound by the tab.
    asset: Option<AssetBinding>,
    /// What launch hooks hold for this run, never filed: put back whenever
    /// the view wears an asset's settings, so it reaches the asset a replay
    /// autostart switches to. Empty outside a capture run.
    held: RunHold,
}

impl BubbleLook {
    /// Open on `loaded`, dressing `config` in its active preset: the presets
    /// file is the record of how the tape should look, so the chart opens on
    /// it instead of the compiled defaults.
    #[must_use]
    pub fn open((presets, source, load_error): LoadedPresets, config: &mut HeatmapConfig) -> Self {
        let status = load_error.map(|message| {
            log_unreadable(&message);
            format!("presets not loaded — {message}")
        });
        let look_name = presets
            .get(&presets.active)
            .map_or_else(String::new, |active| {
                active.apply_to(config);
                active.name.clone()
            });
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "BUBBLE_PRESETS_LOADED",
            source = %source,
            stored = presets.presets.len(),
            active = presets.active.as_str(),
            "bubble presets resolved"
        );
        let name_draft = presets.active.clone();
        Self {
            presets,
            source,
            name_draft,
            status,
            look_name,
            asset: None,
            held: RunHold::default(),
        }
    }

    /// The stored presets, which an asset's declared look names.
    #[must_use]
    pub fn presets(&self) -> &BubblePresetFile {
        &self.presets
    }

    /// Where the presets came from.
    #[must_use]
    pub fn source(&self) -> &PresetSource {
        &self.source
    }

    /// The last preset action or failure, for the panel.
    #[must_use]
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Name of the preset the look on screen started from.
    #[must_use]
    pub fn look_name(&self) -> &str {
        &self.look_name
    }

    /// The name typed for the next save.
    #[must_use]
    pub fn name_draft(&self) -> &str {
        &self.name_draft
    }

    /// The name field the panel's next save and delete read.
    pub fn name_draft_mut(&mut self) -> &mut String {
        &mut self.name_draft
    }

    /// The asset this view's settings belong to, once a tab has bound one.
    #[must_use]
    pub fn asset(&self) -> Option<&AssetBinding> {
        self.asset.as_ref()
    }

    /// Whether the name field names a stored preset the panel may delete.
    #[must_use]
    pub fn draft_is_stored(&self) -> bool {
        let name = self.name_draft.trim();
        !name.is_empty() && self.presets.get(name).is_some()
    }

    /// Hold what a launch hook asks for this run ([`RunHold`]) and put it on
    /// `config`; never filed.
    pub fn hold_for_run(&mut self, hold: impl FnOnce(&mut RunHold), config: &mut HeatmapConfig) {
        self.held.add(hold, config);
    }

    /// A setting changed from `before` to `after`: a held switch the trader
    /// set is theirs from now on, and the asset on screen files the change
    /// at the next sync.
    pub fn note_config_change(&mut self, before: &HeatmapConfig, after: &HeatmapConfig) {
        self.held.release(before, after);
        if after != before {
            self.note_edit();
        }
    }

    /// A setting outside the config changed: the look's name, the candles'
    /// opening scale.
    pub fn note_edit(&mut self) {
        if let Some(binding) = &mut self.asset {
            binding.note_edit();
        }
    }

    /// The lane's width or window was set on purpose: a menu, the panel, a
    /// preset or the panel's reset ([`AssetBinding::note_lane_set`]).
    pub fn note_lane_set(&mut self) {
        if let Some(binding) = &mut self.asset {
            binding.note_lane_set();
        }
    }

    /// The settings on `config`, with the flow pane's `candle_aggression`
    /// and its opening rule.
    #[must_use]
    pub fn settings(
        &self,
        config: &HeatmapConfig,
        candle_aggression: bool,
        flow_ignore_opening: bool,
    ) -> AssetBubbles {
        AssetBubbles {
            bubbles: self.held.filed(config.show_aggressions, self.asset()),
            candle_aggression,
            flow_ignore_opening,
            look: BubblePreset::capture(&self.look_name, config),
        }
    }

    /// Keep this view's settings for the asset `binding` names. The caller
    /// then wears the settings the binding opened with.
    pub fn bind(&mut self, binding: AssetBinding) {
        self.asset = Some(binding);
    }

    /// Put `settings` on `config` without counting it as an edit;
    /// `keep_navigation` leaves the lane's width and window where this
    /// view's own gestures put them, and what a launch hook holds stays. The
    /// flow pane's opening rule is the caller's to apply.
    pub fn wear(
        &mut self,
        settings: &AssetBubbles,
        keep_navigation: bool,
        config: &mut HeatmapConfig,
    ) {
        let (window, width_share) = (config.live_lane.window, config.live_lane.width_share);
        settings.look.apply_to(config);
        if keep_navigation {
            config.live_lane.window = window;
            config.live_lane.width_share = width_share;
        }
        config.show_aggressions = settings.bubbles;
        self.held.wear(config);
        self.look_name.clone_from(&settings.look.name);
        self.name_draft.clone_from(&settings.look.name);
    }

    /// Take the presets another view reloaded. `None` when no asset is
    /// bound; `Some(true)` when presets arrived, and the caller follows the
    /// declared look ([`Self::follow_declared`]).
    pub fn take_reloaded_presets(&mut self) -> Option<bool> {
        let Some(presets) = self.asset.as_mut()?.take_reloaded_presets() else {
            return Some(false);
        };
        self.presets = presets;
        Some(true)
    }

    /// Re-read the asset's declared look from the presets: an asset nobody
    /// tuned wears the new one — returned for the caller to wear — and one
    /// with settings of its own keeps them. Reloaded here, they go to every
    /// view on the store first.
    pub fn follow_declared(&mut self, reloaded_here: bool) -> Option<Worn> {
        let binding = self.asset.as_mut()?;
        if reloaded_here {
            binding.publish_presets(&self.presets);
        }
        match binding.refresh_declared(&self.presets) {
            Some((settings, lane_moved)) => {
                self.status = Some(format!(
                    "presets reloaded · '{}' applied",
                    settings.look.name
                ));
                Some((settings, !lane_moved))
            }
            None => {
                self.status = Some(format!(
                    "presets reloaded · {} keeps its own settings",
                    binding.key()
                ));
                None
            }
        }
    }

    /// File what this view changed, then hand back what another view filed
    /// since ([`AssetBinding::file`], [`AssetBinding::adoption`]) for the
    /// caller to wear: filing first, so an edit here is never dropped for
    /// one filed elsewhere in the same pass. `config` is the screen as it
    /// stands, with the flow pane's `candle_aggression` and opening rule.
    pub fn sync(
        &mut self,
        config: &HeatmapConfig,
        candle_aggression: bool,
        flow_ignore_opening: bool,
    ) -> Option<Adoption> {
        let binding = self.asset.as_ref()?;
        if binding.edited() || binding.on_screen().candle_aggression != candle_aggression {
            let current = self.settings(config, candle_aggression, flow_ignore_opening);
            self.asset.as_mut()?.file(current);
        }
        let adopted = self.asset.as_mut()?.adoption()?;
        self.status = adopted.notice().or_else(|| self.status.take());
        Some(adopted)
    }

    /// Switch "Save changes for this asset" ([`AssetBinding::set_save_changes`]):
    /// switched on, what this view shows is filed at the next sync — or gives
    /// way to stored settings newer than it. `None` when no asset is bound.
    pub fn set_save_changes(&mut self, on: bool) -> Option<SaveSwitch> {
        let switch = self.asset.as_mut()?.set_save_changes(on);
        if let Some(status) = switch.status() {
            self.status = Some(status.to_owned());
        }
        Some(switch)
    }

    /// Apply the stored preset called `name` to `config`, reporting whether
    /// it exists.
    ///
    /// The choice is the asset's own: it is filed for the asset on screen.
    /// An unknown name changes nothing and returns `false`; the caller
    /// decides how loudly to say so.
    pub fn apply_preset(&mut self, name: &str, config: &mut HeatmapConfig) -> bool {
        let Some(preset) = self.presets.get(name).cloned() else {
            return false;
        };
        preset.apply_to(config);
        self.look_name = preset.name.clone();
        self.name_draft = preset.name.clone();
        self.status = Some(format!("'{}' applied", preset.name));
        self.note_lane_set();
        true
    }

    /// Save the look on `config` under the name field's name, through
    /// `writer`, as a preset any asset can pick — never over a look other
    /// assets open on.
    pub fn save_preset(
        &mut self,
        config: &HeatmapConfig,
        writer: impl FnOnce(&BubblePresetFile) -> Result<PathBuf, String>,
    ) {
        let name = self.name_draft.trim().to_owned();
        if name.is_empty() {
            self.status = Some("name the preset before saving".to_owned());
            return;
        }
        if self.refused_preset_name(&name) {
            return;
        }
        self.presets.upsert(BubblePreset::capture(&name, config));
        self.look_name = name.clone();
        self.note_edit();
        self.persist(format!("'{name}' saved"), writer);
    }

    /// Remove the preset the name field holds from the presets file, through
    /// `writer`, unless some asset opens on it.
    pub fn delete_preset(
        &mut self,
        writer: impl FnOnce(&BubblePresetFile) -> Result<PathBuf, String>,
    ) {
        let name = self.name_draft.trim().to_owned();
        if self.refused_preset_name(&name) {
            return;
        }
        self.presets.remove(&name);
        self.persist(format!("preset '{name}' removed"), writer);
    }

    /// Take the presets file `loaded` again. A bound asset follows its
    /// declared look — returned for the caller to wear — and an unbound view
    /// re-applies the preset it wears to `config`.
    pub fn reload(
        &mut self,
        (presets, source, error): LoadedPresets,
        config: &mut HeatmapConfig,
    ) -> Option<Worn> {
        self.presets = presets;
        self.source = source;
        let worn = self.follow_declared(true);
        match error {
            Some(message) => {
                log_unreadable(&message);
                self.status = Some(format!("presets not loaded — {message}"));
            }
            None if self.asset.is_some() => {}
            None => {
                let active = self.look_name.clone();
                if active.is_empty() || self.presets.get(&active).is_none() {
                    self.status = Some("presets reloaded".to_owned());
                } else {
                    self.apply_preset(&active, config);
                    self.status = Some(format!("reloaded · '{active}' applied"));
                }
            }
        }
        worn
    }

    /// The panel restored the bubble defaults: no stored preset is on screen
    /// any more, so the picker must not keep claiming one, and the asset on
    /// screen keeps them.
    pub fn defaults_restored(&mut self) {
        self.look_name.clear();
        self.note_lane_set();
        self.status = Some("bubble defaults restored for the asset on screen".to_owned());
    }

    /// Refuse saving over or removing `name` when other assets open on it.
    fn refused_preset_name(&mut self, name: &str) -> bool {
        let asset = self.asset.as_ref();
        let refusal = asset.and_then(|asset| asset.refuses_preset_name(name, &self.presets));
        refusal.map(|refusal| self.status = Some(refusal)).is_some()
    }

    fn persist(
        &mut self,
        success: String,
        writer: impl FnOnce(&BubblePresetFile) -> Result<PathBuf, String>,
    ) {
        // `active` stays the file's: the look an undeclared asset opens on,
        // never the asset on screen's choice.
        match writer(&self.presets) {
            Ok(path) => {
                self.source = PresetSource::WorkingDir(path.clone());
                self.status = Some(format!("{success} → {}", path.display()));
            }
            Err(message) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BUBBLE_PRESETS_NOT_SAVED",
                    error = message.as_str(),
                    action = "keep_settings_in_memory_only",
                    "bubble presets could not be written; the current look is in memory only"
                );
                self.status = Some(format!("not saved — {message}"));
            }
        }
    }
}

fn log_unreadable(message: &str) {
    tracing::error!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "BUBBLE_PRESETS_UNREADABLE",
        error = message,
        action = "using_built_in_presets",
        "bubble presets file could not be read; built-in presets are in use"
    );
}

#[cfg(test)]
#[path = "bubble_look_tests.rs"]
mod bubble_look_tests;
