use super::LIGHT_SOURCES;
use crate::appearance::{AppearanceStore, Builtin, load};
use crate::scratch::ScratchDir;
use quantick_appearance::{LIGHT, ResolvedTheme, Token};

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
                assert!(store.durable());
                assert!(!store.load_failed());
                assert_eq!(
                    store.resolved().bookmap().color(Token::Canvas),
                    dark.color(Token::Canvas)
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
