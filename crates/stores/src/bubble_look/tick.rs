//! Select the chart's sphere calibration without replacing the tape look.
use super::BubbleLook;
use quantick_orderflow::{TickBubbleStyle, TickQuantityUnit};

impl BubbleLook {
    #[must_use]
    pub fn tick_preset_label(&self, style: Option<TickBubbleStyle>) -> String {
        self.tick_presets()
            .into_iter()
            .find(|name| self.tick_style(Some(name)).ok() == Some(style))
            .unwrap_or_else(|| {
                if style.is_some() {
                    "Custom spheres"
                } else {
                    "Regional FLOW"
                }
                .to_owned()
            })
    }

    #[must_use]
    pub fn tick_presets(&self) -> Vec<String> {
        let Some(unit) = self.asset().and_then(|asset| match asset.key() {
            "BTCUSDT" | "binance-futures:BTCUSDT.p" => Some(TickQuantityUnit::Btc),
            "WIN*" => Some(TickQuantityUnit::Contracts),
            _ => None,
        }) else {
            return Vec::new();
        };
        self.presets()
            .presets
            .iter()
            .filter(|preset| {
                preset
                    .tick_bubbles
                    .is_some_and(|style| style.quantity_unit == unit)
            })
            .map(|preset| preset.name.clone())
            .collect()
    }

    pub fn tick_style(&self, preset: Option<&str>) -> Result<Option<TickBubbleStyle>, String> {
        let choices = self.tick_presets();
        if choices.is_empty() {
            return Err("No tick sphere calibration is available for this asset".to_owned());
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
        let mut config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        config
            .feeds
            .iter_mut()
            .find(|feed| feed.id == "hyperliquid")
            .unwrap()
            .symbols
            .push("BTCUSDT".to_owned());
        for (feed, symbol, name) in [
            ("binance", "BTCUSDT", Some("Binance BTC Spot spheres")),
            (
                "binance-futures",
                "BTCUSDT.p",
                Some("Binance BTC perpetual spheres"),
            ),
            ("binance", "ETHUSDT", None),
            ("hyperliquid", "BTCUSDT", None),
            ("hyperliquid", "BTC", None),
            ("metatrader-b3", "WINV26", Some("B3 WIN tick spheres")),
            ("metatrader-b3", "WINZ26", Some("B3 WIN tick spheres")),
            ("metatrader-b3", "WDO$N", None),
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
            assert_eq!(!look.tick_presets().is_empty(), name.is_some());
            assert_eq!(look.tick_style(None).is_ok(), name.is_some());
            if let Some(name) = name {
                assert!(look.tick_style(Some(name)).unwrap().is_some());
                let other = if name == "B3 WIN tick spheres" {
                    "Binance BTC Spot spheres"
                } else {
                    "B3 WIN tick spheres"
                };
                assert!(look.tick_style(Some(other)).is_err());
            }
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
