//! Select the chart's sphere calibration without replacing the tape look.
use super::BubbleLook;
use quantick_orderflow::TickBubbleStyle;

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
        self.presets()
            .presets
            .iter()
            .filter(|preset| self.tick_calibration_available(preset))
            .map(|preset| preset.name.clone())
            .collect()
    }

    pub(super) fn tick_calibration_available(
        &self,
        preset: &crate::bubble_presets::BubblePreset,
    ) -> bool {
        preset.tick_bubbles.is_some()
            && self
                .asset()
                .is_some_and(|asset| preset.tick_asset_keys.iter().any(|key| key == asset.key()))
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
    fn configured_asset_calibrations_dock_without_a_production_asset_match() {
        let mut config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        let feed = config
            .feeds
            .iter_mut()
            .find(|feed| feed.id == "metatrader-b3")
            .unwrap();
        feed.symbols.push("SYNZ26".to_owned());
        feed.symbol_bubble_presets
            .insert("SYN*".to_owned(), "default".to_owned());
        let mut presets = bubble_presets::embedded();
        let mut synthetic = presets.get("B3 WIN tick spheres").unwrap().clone();
        synthetic.name = "Synthetic contract calibration".to_owned();
        synthetic.tick_asset_keys = vec!["SYN*".to_owned()];
        synthetic.tick_bubbles.as_mut().unwrap().reference_quantity = 1234.0;
        presets.upsert(synthetic);
        let serialized = toml::to_string(&presets).unwrap();
        let presets = bubble_presets::parse(&serialized).unwrap();
        let (binding, _) = crate::bubble_asset_store::AssetBinding::bind(
            AssetBubblesStore::default().shared(),
            &config,
            ("metatrader-b3", "SYNZ26"),
            &presets,
            false,
        );
        assert_eq!(binding.key(), "SYN*");
        let mut current = HeatmapConfig::default();
        let mut look = BubbleLook::open(
            (presets, bubble_presets::PresetSource::Embedded, None),
            &mut current,
        );
        look.bind(binding);
        assert_eq!(look.tick_presets(), ["Synthetic contract calibration"]);
        assert!(
            look.preset_page(0, 64)
                .unwrap()
                .iter()
                .any(|name| name == "Synthetic contract calibration")
        );
        assert!(
            !look
                .preset_page(0, 64)
                .unwrap()
                .iter()
                .any(|name| name == "B3 WIN tick spheres")
        );
        assert!(look.tick_style(Some("B3 WIN tick spheres")).is_err());
        assert!(
            look.apply_named_preset("B3 WIN tick spheres", &mut current)
                .is_err()
        );
        let tape = current.clone();
        assert!(
            look.apply_named_preset("Synthetic contract calibration", &mut current)
                .unwrap()
        );
        assert_eq!(
            current.volume_dots.tick_bubbles.unwrap().reference_quantity,
            1234.0
        );
        assert!(current.same_tape_settings(&tape));
        *look.name_draft_mut() = "Saved synthetic calibration".to_owned();
        look.save_preset(&current, |_| {
            Ok(std::path::PathBuf::from("in-memory-only.toml"))
        });
        assert!(
            look.tick_presets()
                .iter()
                .any(|name| name == "Saved synthetic calibration")
        );
    }

    #[test]
    fn saved_win_calibration_keeps_asset_availability_after_reload() {
        let config: AppConfig = toml::from_str(crate::config::EMBEDDED_DEFAULT).unwrap();
        let presets = bubble_presets::embedded();
        let (binding, _) = crate::bubble_asset_store::AssetBinding::bind(
            AssetBubblesStore::default().shared(),
            &config,
            ("metatrader-b3", "WINZ26"),
            &presets,
            false,
        );
        let mut current = HeatmapConfig::default();
        let mut look = BubbleLook::open(
            (presets, bubble_presets::PresetSource::Embedded, None),
            &mut current,
        );
        look.bind(binding);
        assert!(
            look.apply_named_preset("B3 WIN tick spheres", &mut current)
                .unwrap()
        );
        let expected = current.volume_dots.tick_bubbles;
        *look.name_draft_mut() = "My WIN spheres".to_owned();
        let mut stored = String::new();
        look.save_preset(&current, |document| {
            stored = toml::to_string(document).unwrap();
            Ok(std::path::PathBuf::from("in-memory-only.toml"))
        });
        let reloaded = bubble_presets::parse(&stored).unwrap();
        assert_eq!(
            reloaded.get("My WIN spheres").unwrap().tick_asset_keys,
            ["WIN*"]
        );
        let (binding, _) = crate::bubble_asset_store::AssetBinding::bind(
            AssetBubblesStore::default().shared(),
            &config,
            ("metatrader-b3", "WINZ26"),
            &reloaded,
            false,
        );
        let mut restored = HeatmapConfig::default();
        let mut reopened = BubbleLook::open(
            (reloaded, bubble_presets::PresetSource::Embedded, None),
            &mut restored,
        );
        reopened.bind(binding);
        assert!(
            reopened
                .tick_presets()
                .iter()
                .any(|name| name == "My WIN spheres")
        );
        assert_eq!(
            reopened.tick_style(Some("My WIN spheres")).unwrap(),
            expected
        );
        assert!(
            reopened
                .apply_named_preset("My WIN spheres", &mut restored)
                .unwrap()
        );
        assert_eq!(restored.volume_dots.tick_bubbles, expected);
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
