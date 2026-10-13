//! Select the chart's sphere calibration without replacing the tape look.
use super::BubbleLook;
use quantick_orderflow::TickBubbleStyle;

impl BubbleLook {
    #[must_use]
    pub fn tick_presets(&self) -> Vec<String> {
        if !self
            .asset()
            .is_some_and(|asset| matches!(asset.key(), "BTCUSDT" | "binance-futures:BTCUSDT.p"))
        {
            return Vec::new();
        }
        self.presets()
            .presets
            .iter()
            .filter(|preset| preset.tick_bubbles.is_some())
            .map(|preset| preset.name.clone())
            .collect()
    }

    pub fn tick_style(&self, preset: Option<&str>) -> Result<Option<TickBubbleStyle>, String> {
        let choices = self.tick_presets();
        if choices.is_empty() {
            return Err("Tick sphere presets require a Binance BTC asset".to_owned());
        }
        preset
            .map(|name| {
                if !choices.iter().any(|choice| choice == name) {
                    return Err(format!("Unknown tick sphere preset: {name}"));
                }
                Ok(self
                    .presets()
                    .get(name)
                    .and_then(|preset| preset.tick_bubbles))
            })
            .unwrap_or(Ok(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bubble_asset_store::AssetBubblesStore, bubble_presets, config::AppConfig};
    use quantick_orderflow::HeatmapConfig;

    #[test]
    fn tick_choices_refuse_other_venues_assets_and_unknown_names() {
        let config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        for (feed, symbol, allowed) in [
            ("binance", "BTCUSDT", true),
            ("binance-futures", "BTCUSDT.p", true),
            ("binance", "ETHUSDT", false),
            ("hyperliquid", "BTCUSDT", false),
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
            assert_eq!(
                look.tick_style(Some("Binance BTC Spot spheres")).is_ok(),
                allowed
            );
            assert_eq!(look.tick_style(None).is_ok(), allowed);
            assert!(look.tick_style(Some("missing")).is_err());
        }
    }
}
