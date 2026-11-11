//! Select the chart's sphere calibration without replacing the tape look.
use super::BubbleLook;
use crate::config::AssetMode;
use quantick_orderflow::{TickBubbleStyle, TickTuning};

impl BubbleLook {
    /// The mode the config switches on for the bound asset, if any.
    #[must_use]
    pub fn asset_mode(&self) -> Option<&AssetMode> {
        self.asset().and_then(|asset| asset.mode())
    }

    /// What a quantity on the bound asset counts, when its mode names it.
    #[must_use]
    pub fn quantity_unit(&self) -> Option<&str> {
        self.asset().and_then(|asset| asset.quantity_unit())
    }

    /// The sphere presets this asset offers: every preset carrying tick
    /// spheres, and none unless the config switches the asset's mode on.
    #[must_use]
    pub fn tick_presets(&self) -> Vec<String> {
        if self.asset_mode().is_none() {
            return Vec::new();
        }
        self.presets()
            .presets
            .iter()
            .filter(|preset| preset.tick_bubbles.is_some())
            .map(|preset| preset.name.clone())
            .collect()
    }

    /// The spheres `preset` names, or none: dropping spheres is always
    /// allowed, whatever put them on (review #29); a named preset must be one
    /// this asset offers.
    pub fn tick_style(&self, preset: Option<&str>) -> Result<Option<TickBubbleStyle>, String> {
        let Some(name) = preset else {
            return Ok(None);
        };
        if self.asset_mode().is_none() {
            return Err(
                "Tick sphere presets require an asset whose config asset mode is on".to_owned(),
            );
        }
        if !self.tick_presets().iter().any(|choice| choice == name) {
            return Err(format!("Unknown tick sphere preset: {name}"));
        }
        Ok(self
            .presets()
            .get(name)
            .and_then(|preset| preset.tick_bubbles))
    }

    /// The spheres `preset` names with `tuning` applied; a field `tuning`
    /// leaves out keeps what `worn` wears when `worn` is that preset already
    /// (`orderflow.bubbles.tick.set` setting one field never resets the
    /// other), and is the preset's own otherwise.
    pub fn tick_style_tuned(
        &self,
        preset: Option<&str>,
        tuning: TickTuning,
        worn: Option<TickBubbleStyle>,
    ) -> Result<Option<TickBubbleStyle>, String> {
        let same = preset.is_some() && self.tick_preset_worn(worn).as_deref() == preset;
        let tuning = tuning.or_worn(worn.filter(|_| same));
        Ok(self.tick_style(preset)?.map(|style| tuning.apply(style)))
    }

    /// A saved look from before peak spheres (`peak_spheres` absent) that
    /// otherwise wears a preset takes that preset's peak switch;
    /// a look that says `false` keeps it.
    pub fn adopt_preset_peaks(&self, config: &mut quantick_orderflow::HeatmapConfig) {
        let Some(tick) = &mut config.volume_dots.tick_bubbles else {
            return;
        };
        if tick.peak_spheres.is_some() {
            return;
        }
        let bare = |style: TickBubbleStyle| TickBubbleStyle {
            peak_spheres: None,
            ..style
        };
        let preset = self
            .presets()
            .presets
            .iter()
            .filter_map(|preset| preset.tick_bubbles)
            .find(|style| bare(*style) == bare(*tick));
        if let Some(preset) = preset {
            tick.peak_spheres = preset.peak_spheres;
        }
    }

    /// The offered sphere preset `current` is, if it is one. A pane's own
    /// tuning of a preset (peak spheres, opacity) still wears that preset.
    #[must_use]
    pub fn tick_preset_worn(&self, current: Option<TickBubbleStyle>) -> Option<String> {
        let tuned = |style: Option<TickBubbleStyle>| {
            style.map(|style| match current {
                Some(current) => TickTuning::worn(current).apply(style),
                None => style,
            })
        };
        self.tick_presets()
            .into_iter()
            .find(|name| self.tick_style(Some(name)).ok().map(tuned) == Some(current))
    }

    /// The settings line under the sphere picker: the unit the asset's mode
    /// counts in, and never one it does not name.
    #[must_use]
    pub fn tick_hint(&self) -> String {
        match self.quantity_unit() {
            Some(unit) => {
                format!("Tick chart spheres · quantity in {unit} · tape settings stay as selected")
            }
            None => "Tick chart spheres · tape settings stay as selected".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bubble_asset_store::AssetBubblesStore, bubble_presets, config::AppConfig};
    use quantick_orderflow::HeatmapConfig;

    #[test]
    fn tick_choices_refuse_other_venues_assets_and_unknown_names() {
        let mut config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        config
            .feeds
            .iter_mut()
            .find(|feed| feed.id == "hyperliquid")
            .unwrap()
            .symbols
            .push("BTCUSDT".to_owned());
        for (feed, symbol, allowed) in [
            ("binance", "BTCUSDT", true),
            ("binance-futures", "BTCUSDT.p", true),
            ("binance", "ETHUSDT", false),
            ("hyperliquid", "BTCUSDT", false),
            ("hyperliquid", "BTC", false),
            ("metatrader-b3", "WINV26", true),
            ("metatrader-b3", "WDO$N", false),
        ] {
            let presets = bubble_presets::embedded();
            let (binding, _) = crate::bubble_asset_store::AssetBinding::bind(
                AssetBubblesStore::default().shared(),
                &config,
                (feed, symbol),
                &presets,
                false,
            );
            let mut look = BubbleLook::open(
                (presets, bubble_presets::PresetSource::Embedded, None),
                &mut HeatmapConfig::default(),
            );
            look.bind(binding);
            assert_eq!(!look.tick_presets().is_empty(), allowed);
            assert_eq!(look.asset_mode().is_some(), allowed);
            assert_eq!(
                look.tick_style(Some("Binance BTC Spot spheres")).is_ok(),
                allowed
            );
            assert_eq!(
                look.tick_style(None),
                Ok(None),
                "review #29: dropping spheres is always allowed"
            );
            assert!(look.tick_style(Some("missing")).is_err());
        }
    }

    /// Review finding 6: a look saved before peak spheres that otherwise
    /// wears the mini index preset takes the preset's switch and radius; an
    /// untick and an edited look stay as they are.
    #[test]
    fn a_saved_look_from_before_peak_spheres_wears_the_presets_switch() {
        let presets = bubble_presets::embedded();
        let preset = presets
            .get("mini index regions")
            .and_then(|preset| preset.tick_bubbles)
            .unwrap();
        assert!(preset.peaks());
        let look = BubbleLook::open(
            (presets, bubble_presets::PresetSource::Embedded, None),
            &mut HeatmapConfig::default(),
        );
        let wear = |style: TickBubbleStyle| {
            let mut config = HeatmapConfig::default();
            config.volume_dots.tick_bubbles = Some(style);
            look.adopt_preset_peaks(&mut config);
            config.volume_dots.tick_bubbles.unwrap()
        };
        let saved = TickBubbleStyle {
            peak_spheres: None,
            ..preset
        };
        assert_eq!(wear(saved), preset);
        let unticked = TickBubbleStyle {
            peak_spheres: Some(false),
            ..saved
        };
        assert_eq!(wear(unticked), unticked);
        let edited = TickBubbleStyle {
            opacity: 0.5,
            ..saved
        };
        assert_eq!(wear(edited), edited);
    }

    /// Review finding 7: setting one field of the preset a pane wears keeps
    /// the other as worn; from another look the preset's own value applies.
    #[test]
    fn an_omitted_tuning_keeps_what_the_pane_wears_of_that_preset() {
        let config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        let presets = bubble_presets::embedded();
        let (binding, _) = crate::bubble_asset_store::AssetBinding::bind(
            AssetBubblesStore::default().shared(),
            &config,
            ("metatrader-b3", "WINV26"),
            &presets,
            false,
        );
        let mut look = BubbleLook::open(
            (presets, bubble_presets::PresetSource::Embedded, None),
            &mut HeatmapConfig::default(),
        );
        look.bind(binding);
        let name = Some("mini index regions");
        let preset = look.tick_style(name).unwrap().unwrap();
        let unticked = TickTuning {
            peak_spheres: Some(false),
            ..TickTuning::default()
        }
        .apply(preset);
        let opacity = TickTuning {
            opacity: Some(0.6),
            ..TickTuning::default()
        };
        let worn = look.tick_style_tuned(name, opacity, Some(unticked));
        let worn = worn.unwrap().unwrap();
        assert_eq!(worn.peak_spheres, Some(false), "the untick is kept");
        assert_eq!(worn.opacity, 0.6);
        let other = TickBubbleStyle {
            max_radius: 40.0,
            ..unticked
        };
        let fresh = look.tick_style_tuned(name, opacity, Some(other));
        assert_eq!(fresh.unwrap().unwrap().peak_spheres, preset.peak_spheres);
    }

    #[test]
    fn a_recording_keeps_its_configured_owner_when_the_tab_does_not_offer_it() {
        let config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        assert_eq!(
            config.bubble_asset("hyperliquid", "BTCUSDT"),
            config.bubble_asset("binance", "BTCUSDT")
        );
        assert_ne!(config.bubble_asset("hyperliquid", "BTC").key, "BTCUSDT");
    }
}
