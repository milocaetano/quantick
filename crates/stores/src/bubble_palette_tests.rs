//! Real file and view transitions pin palette ownership independently of RGB values.
use super::bubble_asset_store_tests::bind;
use super::*;
use crate::{
    bubble_look::BubbleLook,
    bubble_presets::{self, BubblePreset, BubblePresetFile, PresetSource},
    scratch::ScratchDir,
};
use quantick_appearance::ResolvedTheme;
use quantick_orderflow::config::theme::{OrderflowRenderStyle, PaletteChoice};

fn authored_theme() -> ResolvedTheme {
    ResolvedTheme::parse("version=1\nname='Authored palette'\n[orderflow.colors]\nbuy='#007752'")
        .unwrap()
}

fn colors(config: &HeatmapConfig, theme: &ResolvedTheme) -> [u8; 3] {
    config.palette.bubble_rgb(&theme.orderflow).buy
}

#[test]
fn automatic_default_and_legacy_file_have_distinct_ownership_after_real_reload() {
    let dir = ScratchDir::new("palette-source");
    let path = dir.join("bubbles.toml");
    bubble_presets::save_to(path.clone(), &bubble_presets::embedded()).unwrap();
    let (file, source, error) = bubble_presets::load_from(Some(path.clone()));
    assert!(error.is_none());
    let mut config = HeatmapConfig::default();
    let mut look = BubbleLook::open((file, source, error), &mut config);
    assert_eq!(config.palette, PaletteChoice::Inherited);
    let authored = authored_theme();
    assert_ne!(
        colors(&config, &authored),
        colors(&config, &ResolvedTheme::default())
    );
    assert!(
        look.apply_named_preset_with("default", &mut config, false, &authored.orderflow)
            .unwrap()
    );
    assert_eq!(config.palette, PaletteChoice::Snapshot(authored.orderflow));
    assert!(
        !look
            .apply_named_preset_with("default", &mut config, false, &authored.orderflow)
            .unwrap()
    );
    look.reload(bubble_presets::load_from(Some(path.clone())), &mut config);
    assert_eq!(config.palette, PaletteChoice::Snapshot(authored.orderflow));
    std::fs::write(&path, "active = 'default'\n[[presets]]\nname = 'default'\n").unwrap();
    let mut legacy = HeatmapConfig::default();
    let _ = BubbleLook::open(bubble_presets::load_from(Some(path)), &mut legacy);
    assert_eq!(legacy.palette, PaletteChoice::legacy());
    assert_eq!(
        colors(&legacy, &authored),
        colors(&legacy, &ResolvedTheme::default())
    );
    // Authored shipped RGB is never normalized away.
    assert_eq!(
        bubble_presets::embedded()
            .get("consumption focus")
            .map(|p| p.bubbles.front_color),
        Some(Some([255, 246, 205]))
    );
}

#[test]
fn named_authored_look_survives_save_reload_and_application_under_dark() {
    let dir = ScratchDir::new("palette-named-look");
    let path = dir.join("bubbles.toml");
    let authored = authored_theme();
    let mut config = HeatmapConfig::default();
    let mut look = BubbleLook::open(
        (bubble_presets::embedded(), PresetSource::Embedded, None),
        &mut config,
    );
    config.bubbles.max_radius = 31.0;
    config.bubbles.buy_color = Some([46, 224, 150]);
    *look.name_draft_mut() = "authored snapshot".into();
    look.save_preset_with(&config, &authored.orderflow, |file| {
        bubble_presets::save_to(path.clone(), file)
    });
    let (file, _, error) = bubble_presets::load_from(Some(path));
    assert!(error.is_none());
    let saved = file.get("authored snapshot").unwrap();
    let mut restored = HeatmapConfig::default();
    saved.apply_to(&mut restored);
    assert_eq!(
        restored.palette,
        PaletteChoice::Snapshot(authored.orderflow)
    );
    assert_eq!(restored.bubbles.buy_color, Some([46, 224, 150]));
    assert_eq!(restored.bubbles.sell_color, None);
    assert_eq!(restored.bubbles.max_radius, 31.0);
    let mut style = OrderflowRenderStyle::from_config(&restored, [0, 0, 0, 255]);
    style.resolve_palette(restored.palette, &ResolvedTheme::default().orderflow);
    assert_eq!(style.palette, Some(authored.orderflow));
    assert_eq!(
        config.palette,
        PaletteChoice::Inherited,
        "saving does not take ownership of ordinary screen defaults"
    );
}

#[test]
fn radius_edits_reload_inherited_and_explicit_equal_palette_is_not_elided() {
    let dir = ScratchDir::new("palette-asset");
    let path = dir.join(crate::bubble_assets::ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut first, mut settings) = bind(&store, "UNDECLARED");
    let (mut second, _) = bind(&store, "UNDECLARED");
    assert_eq!(settings.look.palette, PaletteChoice::Inherited);
    settings.look.bubbles.max_radius = 31.0;
    first.note_edit();
    assert!(first.file(settings.clone()));
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    assert_eq!(
        second.adoption().unwrap().settings.look.palette,
        PaletteChoice::Inherited
    );
    let reloaded = AssetBubblesStore::load(path.clone()).shared();
    let (mut first, mut settings) = bind(&reloaded, "UNDECLARED");
    assert_eq!(settings.look.palette, PaletteChoice::Inherited);
    assert_eq!(settings.look.bubbles.max_radius, 31.0);
    // Returning the numeric knob to default must not erase an explicit-equal palette.
    settings.look.bubbles.max_radius = HeatmapConfig::default().bubbles.max_radius;
    settings.look.palette = PaletteChoice::Snapshot(ResolvedTheme::default().orderflow);
    first.note_edit();
    assert!(first.file(settings));
    assert_eq!(reloaded.borrow_mut().flush(), Some(Ok(())));
    let reopened = AssetBubblesStore::load(path).shared();
    let (_, actual) = bind(&reopened, "UNDECLARED");
    assert_eq!(
        actual.look.palette,
        PaletteChoice::Snapshot(ResolvedTheme::default().orderflow)
    );
    assert!(reopened.borrow().get("UNDECLARED").is_some());
}

#[test]
fn palette_save_off_on_and_cross_tab_adoption_keep_the_same_snapshot() {
    let dir = ScratchDir::new("palette-save-switch");
    let path = dir.join(crate::bubble_assets::ASSETS_FILE);
    let store = AssetBubblesStore::load(path.clone()).shared();
    let (mut first, mut settings) = bind(&store, "UNDECLARED");
    let (mut other, _) = bind(&store, "UNDECLARED");
    let palette = PaletteChoice::Snapshot(authored_theme().orderflow);
    first.set_save_changes(false);
    settings.look.palette = palette;
    first.note_edit();
    assert!(!first.file(settings.clone()));
    assert!(other.adoption().is_none());
    assert_eq!(first.set_save_changes(true), SaveSwitch::ScreenStored);
    assert!(first.file(settings));
    assert_eq!(other.adoption().unwrap().settings.look.palette, palette);
    assert_eq!(store.borrow_mut().flush(), Some(Ok(())));
    let reopened = AssetBubblesStore::load(path).shared();
    assert_eq!(bind(&reopened, "UNDECLARED").1.look.palette, palette);
}

#[test]
fn saved_palette_with_unsupported_alpha_is_rejected() {
    let mut config = HeatmapConfig::default();
    let mut palette = ResolvedTheme::default().orderflow;
    palette.buy[3] = 80;
    config.palette = PaletteChoice::Snapshot(palette);
    let file = BubblePresetFile {
        active: "invalid".into(),
        presets: vec![BubblePreset::capture("invalid", &config)],
    };
    let text = toml::to_string(&file).unwrap();
    assert!(
        bubble_presets::parse(&text)
            .unwrap_err()
            .contains("orderflow.colors.buy")
    );
}

#[test]
fn shipped_feed_declared_looks_inherit_authored_and_manual_selection_captures_it() {
    let presets = bubble_presets::embedded();
    let authored = authored_theme();
    let feeds = super::bubble_asset_store_tests::shipped_config();
    let store = AssetBubblesStore::default().shared();
    for (feed, symbol, expected) in [
        ("binance", "BTCUSDT", "Binance BTC Spot spheres"),
        ("metatrader-b3", "WINV26", "mini index regions"),
    ] {
        let (_, settings) =
            AssetBinding::bind(store.clone(), &feeds, (feed, symbol), &presets, false);
        assert_eq!(settings.look.name, expected);
        assert_eq!(settings.look.palette, PaletteChoice::Inherited);
    }
    for name in [
        "Binance BTC Spot spheres",
        "Binance BTC perpetual spheres",
        "live lane pie",
        "mini index regions",
    ] {
        let mut config = HeatmapConfig::default();
        let mut look =
            BubbleLook::open((presets.clone(), PresetSource::Embedded, None), &mut config);
        presets.get(name).unwrap().apply_to(&mut config);
        assert_eq!(config.palette, PaletteChoice::Inherited, "{name}");
        assert_eq!(colors(&config, &authored), [0, 119, 82], "{name}");
        assert!(
            look.apply_named_preset_with(name, &mut config, true, &authored.orderflow)
                .unwrap()
        );
        assert_eq!(
            config.palette,
            PaletteChoice::Snapshot(authored.orderflow),
            "{name}"
        );
    }
}
