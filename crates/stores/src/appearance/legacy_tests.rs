use super::{DARK_SOURCES, LIGHT_SOURCES};
use crate::appearance::{AppearanceStore, Builtin, load};
use crate::scratch::ScratchDir;
use quantick_appearance::{LIGHT, ResolvedTheme, Token};

#[test]
fn historical_dark_choice_retains_builtin_identity_and_rendered_values() {
    for source in DARK_SOURCES
        .into_iter()
        .flat_map(|known| [known.to_owned(), known.replace('\n', "\r\n")])
    {
        let scratch = ScratchDir::new("legacy-dark-choice");
        let path = scratch.join("appearance.toml");
        let choice_path = scratch.join("appearance.toml.choice.toml");
        std::fs::write(&choice_path, &source).unwrap();
        let store = AppearanceStore::open(path.clone());
        assert_eq!(store.builtin(), Some(Builtin::Dark));
        assert_eq!(store.source(), Some(quantick_appearance::DARK));
        assert_eq!(store.resolved(), &ResolvedTheme::parse(&source).unwrap());
        assert_eq!(std::fs::read_to_string(&choice_path).unwrap(), source);
        let custom = format!("# Authored copy\n{source}");
        std::fs::write(&choice_path, &custom).unwrap();
        let store = AppearanceStore::open(path.clone());
        assert_eq!(store.builtin(), None);
        assert_eq!(store.source(), Some(custom.as_str()));
        std::fs::remove_file(&choice_path).unwrap();
        std::fs::write(&path, &source).unwrap();
        let store = AppearanceStore::open(path);
        assert_eq!(store.builtin(), None);
        assert_eq!(store.source(), Some(source.as_str()));
    }
}

#[test]
fn historical_builtin_choices_load_current_light_without_rewriting_files() {
    let dark = ResolvedTheme::default();
    for known in LIGHT_SOURCES {
        for source in [known.to_owned(), known.replace('\n', "\r\n")] {
            let scratch = ScratchDir::new("legacy-light-choice");
            let path = scratch.join("appearance.toml");
            let choice_path = scratch.join("appearance.toml.choice.toml");
            let authored = "version=1\nname='Authored'\n";
            std::fs::write(&path, authored).unwrap();
            std::fs::write(&choice_path, &source).unwrap();
            for _ in 0..2 {
                let store = AppearanceStore::open(path.clone());
                assert_eq!(store.builtin(), Some(Builtin::Light));
                assert_eq!(store.source(), Some(LIGHT));
                assert_eq!(
                    store.resolved().color(Token::ChartGrid).to_array(),
                    [186, 187, 187, 255]
                );
                assert!(store.durable());
                assert!(!store.load_failed());
                assert_eq!(
                    store.resolved().bookmap().color(Token::Canvas),
                    ResolvedTheme::parse(LIGHT)
                        .unwrap()
                        .bookmap()
                        .color(Token::Canvas)
                );
                assert_eq!(
                    store.resolved().bookmap().candle_fill_opacity,
                    dark.candle_fill_opacity
                );
                assert_eq!(store.resolved().bookmap().orderflow, dark.orderflow);
                assert_ne!(
                    store.resolved().color(Token::Canvas),
                    dark.color(Token::Canvas)
                );
                assert_eq!(std::fs::read_to_string(&choice_path).unwrap(), source);
                assert_eq!(std::fs::read_to_string(&path).unwrap(), authored);
            }
            let mut store = AppearanceStore::open(path.clone());
            store.apply_builtin(Builtin::Light, true).unwrap();
            assert_eq!(std::fs::read_to_string(&choice_path).unwrap(), LIGHT);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), authored);
        }
    }
}

#[test]
fn authored_historical_sources_and_modified_choices_remain_exact() {
    for known in LIGHT_SOURCES {
        let scratch = ScratchDir::new("legacy-light-authored");
        let path = scratch.join("appearance.toml");
        std::fs::write(&path, known).unwrap();
        let store = AppearanceStore::open(path.clone());
        assert_eq!(store.source(), Some(known));
        assert_eq!(store.builtin(), None);
        // A source from before `chart_grid` existed draws its grid in the
        // control color; a later one authors its own.
        if !known.contains("chart_grid") {
            assert_eq!(
                store.resolved().color(Token::ChartGrid),
                store.resolved().color(Token::Control)
            );
        }
        assert_eq!(load(&path).unwrap().source(), Some(known));
        let choice_path = scratch.join("appearance.toml.choice.toml");
        for custom in [
            format!("# Authored copy\n{known}"),
            known.replacen("name = \"Light\"", "name = \"Custom\"", 1),
            known.replacen("canvas =", "canvas  =", 1),
            known.replacen("#FFFFFF", "#FAFAFA", 1),
            known.trim_end().to_owned(),
        ] {
            assert_ne!(custom, known);
            std::fs::write(&choice_path, &custom).unwrap();
            let store = AppearanceStore::open(path.clone());
            assert!(!store.load_failed());
            assert_eq!(store.source(), Some(custom.as_str()));
            assert_eq!(store.builtin(), None);
            assert_eq!(std::fs::read_to_string(&choice_path).unwrap(), custom);
        }
    }
}

/// An authored Light file older than the strip, brand and replay tokens keeps
/// Light for all three: the strip from its own inset, the others from Light.
#[test]
fn authored_light_before_the_strip_tokens_resolves_light_values() {
    let known = include_str!("legacy-light-d4a42dc1.toml");
    assert!(!known.contains("tab_strip") && !known.contains("brand"));
    let scratch = ScratchDir::new("legacy-light-strip-tokens");
    let path = scratch.join("appearance.toml");
    std::fs::write(&path, known).unwrap();
    let store = AppearanceStore::open(path);
    assert_eq!(store.builtin(), None);
    let (resolved, light) = (store.resolved(), ResolvedTheme::parse(LIGHT).unwrap());
    let dark = ResolvedTheme::default();
    assert_eq!(
        resolved.color(Token::TabStrip).to_array(),
        [0xE6, 0xE6, 0xE6, 255],
        "the file's own inset"
    );
    for token in [Token::Brand, Token::ReplayInk] {
        assert_eq!(resolved.color(token), light.color(token), "{}", token.key());
        assert_ne!(resolved.color(token), dark.color(token), "{}", token.key());
    }
}

/// A Light choice saved before the beige canvas and Blue-ink order flow
/// loads as today's Light, not as an authored white and slate theme.
#[test]
fn light_choice_before_beige_canvas_migrates_to_current_light() {
    let old = include_str!("legacy-light-ab24b5e3.toml");
    let (old_theme, light) = (
        ResolvedTheme::parse(old).unwrap(),
        ResolvedTheme::parse(LIGHT).unwrap(),
    );
    assert_eq!(old_theme.color(Token::Canvas).to_array(), [255; 4]);
    assert_eq!(
        old_theme.bookmap().color(Token::Canvas).to_array(),
        [0x43, 0x4A, 0x52, 255]
    );
    let scratch = ScratchDir::new("legacy-light-beige");
    let path = scratch.join("appearance.toml");
    let choice_path = scratch.join("appearance.toml.choice.toml");
    std::fs::write(&choice_path, old).unwrap();
    let store = AppearanceStore::open(path);
    assert_eq!(store.builtin(), Some(Builtin::Light));
    assert_eq!(store.source(), Some(LIGHT));
    for (resolved, expected, stale) in [
        (store.resolved(), &light, &old_theme),
        (
            store.resolved().bookmap(),
            light.bookmap(),
            old_theme.bookmap(),
        ),
    ] {
        assert_eq!(resolved.color(Token::Canvas), expected.color(Token::Canvas));
        assert_ne!(resolved.color(Token::Canvas), stale.color(Token::Canvas));
    }
    assert_eq!(std::fs::read_to_string(&choice_path).unwrap(), old);
}

#[test]
fn historical_choice_loads_with_writes_refused_and_invalid_choice_stays_untouched() {
    use quantick_workspace::write_refusal::{WritesRefused, refuse_writes_on_this_thread};
    let scratch = ScratchDir::new("legacy-light-refused");
    let path = scratch.join("appearance.toml");
    let choice_path = scratch.join("appearance.toml.choice.toml");
    std::fs::write(&choice_path, LIGHT_SOURCES[2]).unwrap();
    refuse_writes_on_this_thread(Some(WritesRefused {
        hooks: vec!["QUANTICK_UNKNOWN_TEST".into()],
    }));
    let mut store = AppearanceStore::open(path.clone());
    let failed_save = store.apply_builtin(Builtin::Light, true);
    refuse_writes_on_this_thread(None);
    assert_eq!(store.builtin(), Some(Builtin::Light));
    assert_eq!(store.source(), Some(LIGHT));
    assert!(failed_save.is_err());
    assert_eq!(
        std::fs::read_to_string(&choice_path).unwrap(),
        LIGHT_SOURCES[2]
    );
    let invalid = "version = 99\nname = 'Light'\n";
    std::fs::write(&choice_path, invalid).unwrap();
    assert!(AppearanceStore::open(path).load_failed());
    assert_eq!(std::fs::read_to_string(choice_path).unwrap(), invalid);
}

/// A Light choice saved before the title strip loads as today's Light, slate strip included.
#[test]
fn light_choice_before_the_title_strip_migrates_to_current_light() {
    let old = include_str!("legacy-light-ee7ea6b5.toml");
    assert!(!old.contains("[title_strip"));
    let scratch = ScratchDir::new("legacy-light-title-strip");
    let path = scratch.join("appearance.toml");
    std::fs::write(scratch.join("appearance.toml.choice.toml"), old).unwrap();
    let store = AppearanceStore::open(path);
    assert_eq!(store.builtin(), Some(Builtin::Light));
    let light = ResolvedTheme::parse(LIGHT).unwrap();
    assert_eq!(store.resolved().title_strip(), light.title_strip());
    for token in [Token::TabIdleFill, Token::ToolbarSheet, Token::TabBand] {
        assert_eq!(
            store.resolved().color(token),
            light.color(token),
            "{}",
            token.key()
        );
    }
    let stale = ResolvedTheme::parse(old).unwrap();
    assert_ne!(
        stale.title_strip(),
        light.title_strip(),
        "the old file has no table"
    );
}

/// A Light choice saved before the TradingView chart opens on it: white plot,
/// time grid, black contours. A Dark choice keeps being Dark.
#[test]
fn light_choice_before_the_tradingview_chart_migrates_to_current_light() {
    let old = include_str!("legacy-light-f32407a0.toml");
    let scratch = ScratchDir::new("legacy-light-tradingview");
    let path = scratch.join("appearance.toml");
    std::fs::write(scratch.join("appearance.toml.choice.toml"), old).unwrap();
    let store = AppearanceStore::open(path);
    assert_eq!(store.builtin(), Some(Builtin::Light));
    assert_eq!(store.resolved(), &ResolvedTheme::parse(LIGHT).unwrap());
    assert!(store.resolved().chart_time_grid);
    assert_eq!(
        store.resolved().color(Token::ChartCanvas).to_array(),
        [255; 4]
    );
    let scratch = ScratchDir::new("legacy-dark-tradingview");
    let path = scratch.join("appearance.toml");
    let dark = include_str!("legacy-dark-f32407a0.toml");
    std::fs::write(scratch.join("appearance.toml.choice.toml"), dark).unwrap();
    let store = AppearanceStore::open(path);
    assert_eq!(store.builtin(), Some(Builtin::Dark));
    assert_eq!(store.resolved(), &ResolvedTheme::parse(dark).unwrap());
}

/// A Light choice saved with the warm toolbar row and the blue VWAP opens on
/// the grey toolbar row, the slate VWAP drawing and the slate VWAP indicator.
#[test]
fn light_choice_before_the_slate_vwap_migrates_to_current_light() {
    let old = include_str!("legacy-light-297976ae.toml");
    let scratch = ScratchDir::new("legacy-light-slate-vwap");
    let path = scratch.join("appearance.toml");
    std::fs::write(scratch.join("appearance.toml.choice.toml"), old).unwrap();
    let store = AppearanceStore::open(path);
    assert_eq!(store.builtin(), Some(Builtin::Light));
    let theme = store.resolved();
    assert_eq!(theme, &ResolvedTheme::parse(LIGHT).unwrap());
    let slate = [0x2B, 0x36, 0x40, 255];
    assert_eq!(
        theme.color(Token::ToolbarSheet).to_array(),
        [0xED, 0xED, 0xED, 255]
    );
    assert_eq!(theme.color(Token::DrawingVwap).to_array(), slate);
    let ink = theme.plot_ink("VWAP (cumulative)", "vwap");
    assert_eq!(ink.map(|ink| ink.to_array()), Some(slate));
    let old = ResolvedTheme::parse(old).unwrap();
    assert_eq!(old.plot_ink("VWAP (cumulative)", "vwap"), None);
}
