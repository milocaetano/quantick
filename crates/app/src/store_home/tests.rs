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
    let catalog = capture_override(crate::paper_state::INSTRUMENTS_FILE, |env| {
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
    let mut keys: Vec<_> = COCKPIT_STORES.iter().map(|store| store.key).collect();
    let mut files: Vec<_> = COCKPIT_STORES.iter().map(|store| store.file).collect();
    let (before_keys, before_files) = (keys.len(), files.len());
    keys.sort_unstable();
    keys.dedup();
    files.sort_unstable();
    files.dedup();
    assert_eq!(keys.len(), before_keys, "two stores share a bundle key");
    assert_eq!(files.len(), before_files, "two stores share a file name");
}
