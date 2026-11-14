use super::*;

#[test]
fn captured_inputs_survive_changes_to_the_lookup_source() {
    let mut values = std::collections::BTreeMap::from([
        ("QUANTICK_LOG_FORMAT", OsString::from("JSON")),
        ("QUANTICK_CONFIG", OsString::from("feeds.toml")),
        ("QUANTICK_DEFAULT_SYMBOL", OsString::from("BTC")),
        ("QUANTICK_BOOK_DEPTH", OsString::from("250")),
        ("QUANTICK_TRADES_DIR", OsString::from("journal")),
        ("QUANTICK_BUBBLES", OsString::from("bubbles.toml")),
    ]);
    let launch = LaunchConfig::capture(|name| values.get(name).cloned());
    values.clear();
    assert_eq!(launch.log_format, LogFormat::Json);
    assert_eq!(launch.config_path, Some(PathBuf::from("feeds.toml")));
    assert!(launch.names_market());
    assert_eq!(
        launch.selection().expect("unicode"),
        (None, Some("BTC".to_owned()))
    );
    assert_eq!(launch.book_depth.as_deref(), Some("250"));
    assert_eq!(launch.paths.trades_dir, Some(OsString::from("journal")));
    assert_eq!(launch.paths.bubbles, Some(OsString::from("bubbles.toml")));
    assert_eq!(launch.paths.replay_dir, None);
}

/// Every name the root reads is declared: here as configuration, or in
/// `window` as a gated hook. Nothing is read that the registry cannot name.
#[test]
fn every_launch_read_is_declared() {
    let mut reads = Vec::new();
    let _ = LaunchConfig::capture(|name| {
        reads.push(name.to_owned());
        None
    });
    let declared: Vec<&str> = HOOKS
        .iter()
        .chain(window::HOOKS)
        .map(|spec| spec.name)
        .collect();
    for name in &reads {
        assert!(
            declared.contains(&name.as_str()),
            "{name} is read at launch but declared nowhere"
        );
    }
    for spec in HOOKS {
        assert!(reads.iter().any(|name| name == spec.name), "{}", spec.name);
    }
}

#[test]
fn absent_and_invalid_values_preserve_defaults() {
    for raw in [None, Some(""), Some("true"), Some(" 1 "), Some("JSON ")] {
        let launch = LaunchConfig::capture(|name| {
            (name == "QUANTICK_LOG_FORMAT")
                .then_some(raw)
                .flatten()
                .map(OsString::from)
        });
        assert_eq!(launch.log_format, LogFormat::Text);
        assert!(!launch.names_market());
    }
    for raw in ["JSON", "json", "JsOn"] {
        let launch =
            LaunchConfig::capture(|name| (name == "QUANTICK_LOG_FORMAT").then(|| raw.into()));
        assert_eq!(launch.log_format, LogFormat::Json);
    }
    let launch = LaunchConfig::default();
    assert_eq!(launch.window_size(None), DEFAULT_WINDOW_PX);
    assert_eq!(launch.window_size(Some([900.0, 700.0])), [900.0, 700.0]);
}

#[cfg(windows)]
#[test]
fn invalid_unicode_is_ignored_or_refused_by_name() {
    use std::os::windows::ffi::OsStringExt;
    let invalid = OsString::from_wide(&[0xD800]);
    let launch = LaunchConfig::capture(|_| Some(invalid.clone()));
    assert_eq!(launch.log_format, LogFormat::Text);
    assert_eq!(launch.book_depth, None);
    assert_eq!(launch.window_size(None), DEFAULT_WINDOW_PX);
    assert!(matches!(
        launch.selection(),
        Err(StartupSelectionError::NonUnicode {
            variable: "QUANTICK_DEFAULT_FEED"
        })
    ));
}

/// The window drags to nothing, but a session closed on nothing must not
/// reopen on nothing: the restore path is the one place a floor survives,
/// and it is a recovery floor, not a layout one.
#[test]
fn a_saved_sliver_reopens_on_something_the_trader_can_grab() {
    assert_eq!(restore_size(Some([0.0, 0.0])), REOPEN_FLOOR_PX);
    assert_eq!(
        restore_size(Some([4.0, 900.0])),
        [REOPEN_FLOOR_PX[0], 900.0]
    );
    assert_eq!(restore_size(Some([1280.0, 720.0])), [1280.0, 720.0]);
    assert_eq!(restore_size(None), DEFAULT_WINDOW_PX);
}
