
use super::*;

fn embedded(name: &str) -> &'static str {
    EMBEDDED_SCRIPTS
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("{name} is embedded"))
        .1
}

/// The copilot ships twice: embedded here and as the pine crate's
/// semantics fixture (`copilot_semantics.rs` proves the marker logic
/// against the corpus copy). If the copies drift, the semantics test
/// silently proves a script the app no longer embeds — so pin them.
#[test]
fn the_embedded_copilot_matches_its_semantics_fixture() {
    assert_eq!(
        embedded("copilot.pine"),
        include_str!("../../pine/tests/corpus/ok/copilot.pine"),
        "sync crates/pine/tests/corpus/ok/copilot.pine with crates/app/scripts/copilot.pine"
    );
}

/// Same pin, same reason: `force_bar_semantics.rs` proves the three
/// classifications against the corpus copy, and a drift would leave it
/// proving a script nobody ships.
#[test]
fn the_embedded_force_bar_matches_its_semantics_fixture() {
    assert_eq!(
        embedded("force_bar.pine"),
        include_str!("../../pine/tests/corpus/ok/force_bar.pine"),
        "sync crates/pine/tests/corpus/ok/force_bar.pine with crates/app/scripts/force_bar.pine"
    );
}

/// Same pin, same reason: `exhaustion_reversal_semantics.rs` proves the
/// force-bar / give-back pairs against the corpus copy, and a drift would
/// leave a suite of passing tests describing a script nobody ships.
#[test]
fn the_embedded_exhaustion_reversal_matches_its_semantics_fixture() {
    assert_eq!(
        embedded("exhaustion_reversal.pine"),
        include_str!("../../pine/tests/corpus/ok/exhaustion_reversal.pine"),
        "sync crates/pine/tests/corpus/ok/exhaustion_reversal.pine with \
             crates/app/scripts/exhaustion_reversal.pine"
    );
}

/// A private scratch folder that removes itself when the test ends.
fn scratch(tag: &str) -> crate::scratch::ScratchDir {
    crate::scratch::ScratchDir::new(tag)
}

#[test]
fn a_pine_file_joins_the_menu_in_a_deterministic_order() {
    let dir = scratch("order");
    std::fs::write(
        dir.join("zeta.pine"),
        "//@version=5
plot(close)
",
    )
    .unwrap();
    std::fs::write(
        dir.join("alpha.pine"),
        "//@version=5
plot(open)
",
    )
    .unwrap();
    // Not a script: neither belongs in the menu.
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();
    std::fs::create_dir_all(dir.join("folder.pine")).unwrap();

    let library = ScriptLibrary::scan_dir(&dir);
    let names: Vec<&str> = library.entries().iter().map(|e| e.name.as_str()).collect();
    let files: Vec<&&str> = names
        .iter()
        .filter(|n| **n == "alpha.pine" || **n == "zeta.pine")
        .collect();
    assert_eq!(
        files,
        vec![&"alpha.pine", &"zeta.pine"],
        "sorted, {names:?}"
    );
    assert!(!names.contains(&"notes.txt"), "only .pine files: {names:?}");
    assert!(
        !names.contains(&"folder.pine"),
        "a directory named like a script is not a script: {names:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_user_file_cannot_shadow_the_embedded_script_of_the_same_name() {
    let dir = scratch("shadow");
    let (embedded_name, _) = EMBEDDED_SCRIPTS[0];
    std::fs::write(
        dir.join(embedded_name),
        "//@version=5
plot(high)
",
    )
    .unwrap();

    let library = ScriptLibrary::scan_dir(&dir);
    let matching: Vec<&ScriptEntry> = library
        .entries()
        .iter()
        .filter(|e| e.name == embedded_name)
        .collect();
    assert_eq!(matching.len(), 1, "never two rows with the same label");
    assert!(
        matches!(matching[0].origin, ScriptOrigin::Embedded(_)),
        "built-in identities remain immutable"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreadable_directory_degrades_to_embedded_only() {
    // A folder that is not there: inside this thread's scratch directory,
    // so nothing has to create it and nothing is left behind.
    let missing = crate::scratch::thread_dir("script-library").join("missing");
    let library = ScriptLibrary::scan_dir(&missing);
    assert_eq!(
        library.entries().len(),
        EMBEDDED_SCRIPTS.len(),
        "the feature survives a folder that is not there"
    );
}

#[test]
fn a_file_that_disappears_reports_an_error_rather_than_nothing() {
    let dir = scratch("vanish");
    let path = dir.join("gone.pine");
    std::fs::write(
        &path,
        "//@version=5
plot(close)
",
    )
    .unwrap();
    let library = ScriptLibrary::scan_dir(&dir);
    let index = library
        .entries()
        .iter()
        .position(|e| e.name == "gone.pine")
        .expect("the file was scanned");
    std::fs::remove_file(&path).unwrap();
    assert!(
        matches!(library.read(index), Some(Err(_))),
        "a file that no longer reads is an error, not a silent skip"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn embedded_names_are_unique() {
    let mut names: Vec<&str> = EMBEDDED_SCRIPTS.iter().map(|(n, _)| *n).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), EMBEDDED_SCRIPTS.len());
}

#[test]
fn a_saved_name_loads_its_index_and_text_or_names_what_is_missing() {
    let dir = scratch("load");
    std::fs::write(dir.join("gone.pine"), "plot(close)").unwrap();
    let library = ScriptLibrary::scan_dir(&dir);
    let (name, source) = EMBEDDED_SCRIPTS[0];
    let index = library.index_of(name).expect("embedded entries are listed");
    assert_eq!(library.entries()[index].name, name);
    let text = source.to_owned();
    assert_eq!(library.load(name), LoadedScript::Read { index, text });
    assert_eq!(library.load_at(index), Some(library.load(name)));
    assert_eq!(library.load_at(library.entries().len()), None);
    let missing = library.load("nowhere.pine");
    assert_eq!(missing.index(), None);
    assert_eq!(
        missing,
        LoadedScript::Missing {
            message: "nowhere.pine is not in the script library".into()
        },
        "a name the library lacks still loads, as the message its error slot shows"
    );
    // Listed at scan time, deleted since: the index stays, the read fails.
    std::fs::remove_file(dir.join("gone.pine")).unwrap();
    let gone = library.load("gone.pine");
    assert!(matches!(gone, LoadedScript::Unreadable { .. }), "{gone:?}");
    assert_eq!(gone.index(), library.index_of("gone.pine"));
    assert!(gone.index().is_some());
}
