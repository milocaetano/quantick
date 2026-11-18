use super::*;

/// Every QA hook and autostart run leans on this: an explicit ask wins,
/// so validation never reads or writes the trader's real cockpit.
///
/// The lookup is injected, so no test touches the process environment.
#[test]
fn an_explicit_environment_ask_beats_the_home() {
    let asked = capture_override("ui-state.toml", |env| {
        (env == "QUANTICK_UI_STATE").then(|| "D:/somewhere/else.toml".into())
    });
    assert_eq!(asked, Some("D:/somewhere/else.toml".into()));
    assert_eq!(capture_override("unknown.toml", |_| Some("x".into())), None);
    let catalog = capture_override(quantick_paper::state::INSTRUMENTS_FILE, |env| {
        (env == "QUANTICK_INSTRUMENTS").then(|| "D:/scratch/instruments.toml".into())
    });
    assert_eq!(
        catalog,
        Some("D:/scratch/instruments.toml".into()),
        "a harness run reads its own instrument catalog, never the trader's"
    );
}

/// Two stores may never share a file or a key: the bundle keys sections
/// by one and the rescue copies by the other.
#[test]
fn every_store_is_named_once() {
    let stores = cockpit_stores(StorePaths {
        alerts: PathBuf::new,
        appearance: PathBuf::new,
        ui_state: PathBuf::new,
        indicators: PathBuf::new,
        layouts: PathBuf::new,
        workspaces: PathBuf::new,
        market_settings: PathBuf::new,
        indicator_presets: PathBuf::new,
        bubble_assets: PathBuf::new,
        chart_layers: PathBuf::new,
        drawing_presets: PathBuf::new,
        footprint_settings: PathBuf::new,
        footprint_presets: PathBuf::new,
        candle_presets: PathBuf::new,
        paper_state: PathBuf::new,
        symbols: PathBuf::new,
    });
    let mut keys: Vec<_> = stores.iter().map(|store| store.key).collect();
    let mut files: Vec<_> = stores.iter().map(|store| store.file).collect();
    let (before_keys, before_files) = (keys.len(), files.len());
    keys.sort_unstable();
    keys.dedup();
    files.sort_unstable();
    files.dedup();
    assert_eq!(keys.len(), before_keys, "two stores share a bundle key");
    assert_eq!(files.len(), before_files, "two stores share a file name");
}

#[test]
fn resolve_override_skips_the_platform_and_failed_shelf_uses_the_launch_path() {
    let home = StoreHome::new();
    assert_eq!(
        home.resolve(
            "ui-state.toml",
            || panic!("explicit path needs no shelf"),
            |_| Some("capture.toml".into())
        ),
        PathBuf::from("capture.toml")
    );
    let dir = crate::scratch::ScratchDir::new("unavailable-shelf");
    let file = dir.path().join("regular-file");
    std::fs::write(&file, "leave this file alone").unwrap();
    assert_eq!(
        home.resolve("layouts.toml", || Some(file.clone()), |_| None),
        PathBuf::from("layouts.toml")
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "leave this file alone"
    );
    assert_eq!(home.home(|| panic!("shelf failure is cached")), None);
}
