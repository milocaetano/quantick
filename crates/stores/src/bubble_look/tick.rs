//! Select the chart's sphere calibration without replacing the tape look.
use super::BubbleLook;
use crate::config::AssetMode;
use quantick_orderflow::TickBubbleStyle;

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
            ("metatrader-b3", "WINV26", false),
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
