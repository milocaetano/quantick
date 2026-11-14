use super::*;

/// The committed registry is what the generator emits today.
///
/// The authoritative half of the pair: `crates/guards` compares name sets
/// textually in a second, which catches the common mistake; this compares
/// the whole file, which catches every mistake — a reworded prose cell
/// that was never regenerated moves no name and is invisible to a set
/// comparison.
#[test]
fn the_committed_registry_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/app sits two levels below the workspace root")
        .join(".claude/skills/ui-harness/references/hook-registry.md");
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let generated = hook_registry_markdown().expect("the prose half is readable");
    assert_eq!(
        committed, generated,
        "the committed hook registry is stale. Regenerate it:\n  \
             cargo run -p quantick-app -- --dump-hook-registry > \
             .claude/skills/ui-harness/references/hook-registry.md"
    );
}

/// A `QUANTICK_*` nothing declares is reported.
///
/// This is the `QUANTICK_DRAWING_MANAGER` case: the registry documented a
/// singular spelling nothing has ever read, so a capture run setting it
/// got a window that simply did not open the object manager — which reads
/// as a defect in the surface, not as a typo in the script.
#[test]
fn an_unrecognised_hook_is_reported_rather_than_ignored() {
    let declared = declared_names();
    let environment = [
        "PATH",
        "QUANTICK_DRAWING_MANAGER",
        "QUANTICK_DRAWINGS_MANAGER",
        "RUST_LOG",
    ];
    assert_eq!(
        unknown_hooks(environment.into_iter(), &declared),
        vec!["QUANTICK_DRAWING_MANAGER".to_owned()],
        "the misspelt hook must be named and the real one left alone"
    );
}

/// The root reads each scenario input once, by its declared name, and
/// nothing it was not told about: a name no owner declares is never
/// looked up, and a value changed after capture changes nothing.
#[test]
fn scenario_inputs_capture_declared_names_once() {
    let mut asked = Vec::new();
    let mut value = Some(std::ffi::OsString::from("1"));
    let inputs = ScenarioInputs::capture(|name| {
        asked.push(name.to_owned());
        (name == "QUANTICK_INVERTED")
            .then(|| value.take())
            .flatten()
    });
    assert!(asked.iter().any(|name| name == "QUANTICK_INVERTED"));
    assert!(asked.iter().any(|name| name == "QUANTICK_POINTER"));
    assert!(!asked.iter().any(|name| name == "QUANTICK_CONFIG"));
    let mut unique = asked.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), asked.len(), "each input is read once");
    assert_eq!(inputs.var("QUANTICK_INVERTED").as_deref(), Some("1"));
    assert_eq!(inputs.var("QUANTICK_POINTER"), None);
    assert_eq!(
        ScenarioInputs::from_pairs(&[("QUANTICK_TAPE", "off")])
            .var("QUANTICK_TAPE")
            .as_deref(),
        Some("off")
    );
}

/// A default build registers the composition root's configuration and
/// nothing else; this test build compiles every family, so every table
/// is present and no name is claimed twice across them.
#[test]
fn configuration_is_declared_by_the_composition_root_alone() {
    let configuration: Vec<&str> = CONFIGURATION
        .iter()
        .flat_map(|(_, specs)| specs.iter().map(|spec| spec.name))
        .collect();
    assert!(configuration.contains(&"QUANTICK_CONFIG"));
    assert!(configuration.len() <= 20, "{configuration:?}");
    assert_eq!(
        owners().len(),
        1 + SCENARIO_OWNERS.len()
            + CONTROL_OWNERS.len()
            + DRAWING_OWNERS.len()
            + QUICK_RANGE_OWNERS.len()
    );
}

/// A default build registers the composition root's configuration and
/// nothing else, so a scenario hook set against it is reported by name at
/// startup rather than acted on or silently dropped. (That it is not acted
/// on is structural — the reader is compiled out — and CI's "Default
/// binary names no harness hook" step reads the shipped bytes to prove it.)
#[test]
fn a_default_build_reports_a_scenario_hook_it_does_not_compile() {
    let default_build: BTreeSet<&'static str> = CONFIGURATION
        .iter()
        .flat_map(|(_, specs)| specs.iter().map(|spec| spec.name))
        .collect();
    let environment = ["QUANTICK_CONFIG", "QUANTICK_TAPE", "QUANTICK_GIT_COMMIT"];
    assert_eq!(
        unknown_hooks(environment.into_iter(), &default_build),
        vec!["QUANTICK_TAPE".to_owned()],
        "configuration and build metadata are known; the scenario hook is reported"
    );
    assert!(
        unknown_hooks(["QUANTICK_TAPE"].into_iter(), &declared_names()).is_empty(),
        "a build with the scenario harness knows it"
    );
}

/// Nothing outside the prefix is ever reported, however odd it looks.
#[test]
fn variables_outside_the_prefix_are_not_this_guard_s_business() {
    let declared = declared_names();
    assert!(unknown_hooks(["PATH", "QUANTICKISH", "RUST_LOG"].into_iter(), &declared).is_empty());
}

/// Every hook the application actually declares is silent at startup.
///
/// Without this the diagnostic could be trivially satisfied by declaring
/// nothing and warning about everything.
#[test]
fn every_declared_hook_is_accepted() {
    let declared = declared_names();
    let names: Vec<&str> = declared.iter().copied().collect();
    assert!(!names.is_empty(), "no hooks are declared at all");
    assert!(unknown_hooks(names.into_iter(), &declared).is_empty());
}

/// Two modules must not both claim the same hook: the registry would then
/// name one owner and the reader would open the wrong file.
#[test]
fn no_hook_is_declared_by_two_modules() {
    let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for (path, spec) in all() {
        if let Some(first) = seen.insert(spec.name, path) {
            panic!("{} is declared in both {first} and {path}", spec.name);
        }
    }
}

/// Every registered owner path names a file that exists.
#[test]
fn every_owner_path_is_a_real_file() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/app sits two levels below the workspace root");
    for (path, _) in owners() {
        assert!(root.join(path).is_file(), "{path} is not a file");
    }
}
