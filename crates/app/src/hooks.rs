//! Discovery for declared startup configuration and opt-in harness hooks.
//!
//! Each owner declares its `QUANTICK_*` names in `HOOKS`, beside the read;
//! [`owners`] joins those slices into the catalog. Two kinds exist.
//! *Configuration* is what an operator is told about in the README and docs;
//! the composition root (`crate::launch`) reads it once and it is always
//! compiled. Everything else — capture, demo, automation and fault hooks — is
//! a *harness* hook: it compiles only under its family's Cargo feature
//! (`scenario-harness`, `control-harness`, `drawing-harness`,
//! `quick-range-harness`; `harness` enables all four) or `cfg(test)`, and its
//! declaration is gated with it. A default binary therefore names, reads and
//! registers configuration and nothing else.
//!
//! `docs/ui-harness/hook-prose.md` owns each hook's behavior and required
//! feature. A `--features harness` build's `quantick-app --dump-hook-registry`
//! joins those descriptions with owner paths into
//! `.claude/skills/ui-harness/references/hook-registry.md`; any other build
//! refuses, because its declarations are not the whole registry. Authored
//! prose stays under `docs/`; the generated catalog is the skill's reference.
//!
//! The guard checks declarations, source names and documented names for
//! parity, reading source text, so a gated declaration still counts. Adding
//! an owner requires a declaration slice, one registration in its family's
//! table here and authored descriptions, followed by regeneration.
//!
//! Historical drift included the unimplemented `QUANTICK_DRAWING_MANAGER`
//! spelling alongside the real `QUANTICK_DRAWINGS_MANAGER`. Declaration
//! parity detects that mismatch; it does not establish runtime availability.
//!
//! At startup the composition root (`crate::launch::persistence_refusal`)
//! logs every name this build does not register, except the documented
//! [`NOT_HOOKS`] entries — a harness hook set against a build without its
//! feature included — and runs that session writing no store (decision DS7).

use std::collections::BTreeSet;

// The declaration half — the `HookSpec` type and the `declare_hooks!` macro
// that writes a module's slice — is defined in `quantick-feed` and re-exported
// here, so every module in the workspace declares its hooks the same way and
// [`owners`] below can join them all into one table.
//
// It sits there rather than here because four of that crate's adapters read a
// hook and it cannot depend on this one; the graph runs the other way. This
// module is still where the registry is: the owner tables, `NOT_HOOKS` and the
// startup warning are all below, and this is the file to open to find out
// which hooks exist. The registry's markdown rendering, pure string work, sits
// beside the type in `quantick_feed::hooks::registry`.
pub(crate) use quantick_feed::hooks::{HookSpec, declare_hooks};

/// Scenario hook values the composition root captured; owners ask it by name
/// rather than reading the process environment.
#[cfg(any(feature = "scenario-harness", test))]
pub(crate) use quantick_feed::hooks::captured;

/// Every scenario hook's name, for the composition root to capture once.
#[cfg(feature = "scenario-harness")]
pub(crate) fn scenario_names() -> impl Iterator<Item = &'static str> {
    SCENARIO_OWNERS
        .iter()
        .flat_map(|(_, specs)| specs.iter().map(|spec| spec.name))
}

/// `QUANTICK_*` variables that are deliberately **not** launch hooks.
///
/// One definition, two readers. [`unknown_hooks`] skips them, so a build
/// that sets `QUANTICK_GIT_COMMIT` is not warned about its own build metadata;
/// and `crates/guards/src/generated.rs` parses this same table out of this
/// file, so the guard cannot demand a harness row for something the
/// application never reads. A second copy kept by hand in the guard would be
/// the duplicated truth this module exists to end, and it would drift the
/// first time either side gained an entry.
///
/// Each carries its reason, because an allowlist is how a parity guard is
/// quietly defeated: a reader who disagrees with an entry has something to
/// disagree with. A fixture that lives inside a `#[cfg(test)]` module needs no
/// entry: no build that ships reads it, and the guard skips those modules.
pub(crate) const NOT_HOOKS: &[(&str, &str)] = &[(
    "QUANTICK_GIT_COMMIT",
    "build metadata, read through `option_env!` at compile time and \n         reported in the control plane's system info. Setting it at runtime \n         does nothing.",
)];

/// Every module that owns hooks, with the path a reader should open to find
/// them, grouped by what compiles them.
///
/// The path is written out rather than derived because `module_path!()` gives
/// a Rust path and the registry has to name a file someone can open. The guard
/// checks the two agree: a slice registered under the wrong path, or a file
/// that reads a `QUANTICK_*` without registering a slice at all, is a finding.
///
/// [`CONFIGURATION`] is the composition root's operator configuration and is
/// always compiled. Every other table is a harness family and exists only in
/// a build with that family's feature (or under test) — as do the modules it
/// names — so a default build registers configuration and nothing else.
pub(crate) fn owners() -> Vec<(&'static str, &'static [HookSpec])> {
    #[allow(unused_mut)]
    let mut out = CONFIGURATION.to_vec();
    #[cfg(any(feature = "scenario-harness", test))]
    out.extend_from_slice(SCENARIO_OWNERS);
    #[cfg(any(feature = "control-harness", test))]
    out.extend_from_slice(CONTROL_OWNERS);
    #[cfg(any(feature = "drawing-harness", test))]
    out.extend_from_slice(DRAWING_OWNERS);
    #[cfg(any(feature = "quick-range-harness", test))]
    out.extend_from_slice(QUICK_RANGE_OWNERS);
    out
}

/// Whether every harness family is compiled into this build: the only build
/// whose declarations are the whole registry.
pub(crate) const ALL_FAMILIES: bool = cfg!(all(
    feature = "scenario-harness",
    feature = "control-harness",
    feature = "drawing-harness",
    feature = "quick-range-harness"
));

/// Operator configuration, read once by the composition root.
const CONFIGURATION: &[(&str, &[HookSpec])] = &[("crates/app/src/launch.rs", crate::launch::HOOKS)];

/// Launch scenarios, scripted menus, demo stagers, capture isolation of the
/// stores, and the feed's fault injection: `scenario-harness`.
#[cfg(any(feature = "scenario-harness", test))]
const SCENARIO_OWNERS: &[(&str, &[HookSpec])] = &[
    (
        "crates/app/src/app/launch_hooks.rs",
        crate::app::launch_hooks::HOOKS,
    ),
    (
        "crates/app/src/deal_recording.rs",
        crate::deal_recording::HOOKS,
    ),
    (
        "crates/feed/src/metatrader.rs",
        quantick_feed::metatrader::HOOKS,
    ),
    ("crates/feed/src/lib.rs", quantick_feed::HOOKS),
    ("crates/feed/src/stall.rs", quantick_feed::stall::HOOKS),
    ("crates/app/src/feed_notice.rs", crate::feed_notice::HOOKS),
    (
        "crates/app/src/footprint_config.rs",
        crate::footprint_config::HOOKS,
    ),
    (
        "crates/app/src/footprint_render.rs",
        crate::footprint_render::HOOKS,
    ),
    ("crates/app/src/frvp.rs", crate::frvp::HOOKS),
    ("crates/app/src/harness.rs", crate::harness::HOOKS),
    ("crates/app/src/store_home.rs", crate::store_home::HOOKS),
    (
        "crates/app/src/launch/window.rs",
        crate::launch::window::HOOKS,
    ),
    (
        "crates/app/src/paper_account.rs",
        crate::paper_account::HOOKS,
    ),
    (
        "crates/app/src/paper_trading.rs",
        crate::paper_trading::HOOKS,
    ),
    ("crates/app/src/replay_view.rs", crate::replay_view::HOOKS),
    (
        "crates/app/src/surfaces/agent_popup.rs",
        crate::surfaces::agent_popup::HOOKS,
    ),
    (
        "crates/app/src/surfaces/footprint_settings.rs",
        crate::surfaces::footprint_settings::HOOKS,
    ),
    (
        "crates/app/src/surfaces/indicator_preview.rs",
        crate::surfaces::indicator_preview::HOOKS,
    ),
    (
        "crates/app/src/surfaces/source_picker.rs",
        crate::surfaces::source_picker::HOOKS,
    ),
    (
        "crates/app/src/surfaces/style_panel.rs",
        crate::surfaces::style_panel::HOOKS,
    ),
    (
        "crates/app/src/surfaces/toast.rs",
        crate::surfaces::toast::HOOKS,
    ),
    (
        "crates/app/src/surfaces/workspace_name.rs",
        crate::surfaces::workspace_name::HOOKS,
    ),
    ("crates/app/src/tab.rs", crate::tab::HOOKS),
];

/// The control plane's launch scenarios: `control-harness`.
#[cfg(any(feature = "control-harness", test))]
const CONTROL_OWNERS: &[(&str, &[HookSpec])] = &[(
    "crates/app/src/app/control_host.rs",
    crate::app::control_host::HOOKS,
)];

/// The drawing rail and the drawing chrome's demos: `drawing-harness`.
#[cfg(any(feature = "drawing-harness", test))]
const DRAWING_OWNERS: &[(&str, &[HookSpec])] = &[
    ("crates/app/src/toolrail.rs", crate::toolrail::HOOKS),
    (
        "crates/app/src/surfaces/drawing_chrome/mod.rs",
        crate::surfaces::drawing_chrome::HOOKS,
    ),
];

/// The quick-range demo: `quick-range-harness`.
#[cfg(any(feature = "quick-range-harness", test))]
const QUICK_RANGE_OWNERS: &[(&str, &[HookSpec])] = &[(
    "crates/app/src/surfaces/drawing_chrome/quick_range/launch.rs",
    crate::surfaces::drawing_chrome::QUICK_RANGE_HOOKS,
)];

/// The scenario hooks the launch phase and the frame's scripted stages read,
/// captured once by the composition root and handed to the app.
///
/// Holds every name its owners declare, set or not, so asking for a name no
/// owner declares is a bug caught in a debug build rather than a hook that
/// silently never fires.
#[cfg(any(feature = "scenario-harness", test))]
#[derive(Debug, Default, Clone)]
pub(crate) struct ScenarioInputs(std::collections::BTreeMap<&'static str, Option<String>>);

#[cfg(any(feature = "scenario-harness", test))]
impl ScenarioInputs {
    /// The owners whose hooks travel in these inputs.
    const OWNERS: &[&[HookSpec]] = &[
        crate::app::launch_hooks::HOOKS,
        crate::harness::HOOKS,
        crate::deal_recording::HOOKS,
        crate::replay_view::HOOKS,
        crate::surfaces::toast::HOOKS,
    ];

    /// Read every scenario input, once, through `lookup`.
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<std::ffi::OsString>) -> Self {
        Self(
            Self::OWNERS
                .iter()
                .flat_map(|specs| specs.iter())
                .map(|spec| {
                    let value = lookup(spec.name).and_then(|value| value.into_string().ok());
                    (spec.name, value)
                })
                .collect(),
        )
    }

    /// Inputs a test states outright, as `(name, value)` pairs.
    #[cfg(test)]
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self::capture(|name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).into())
        })
    }

    /// The captured value of `name`, when the launch set one.
    pub fn var(&self, name: &str) -> Option<String> {
        debug_assert!(
            self.0.is_empty() || self.0.contains_key(name),
            "{name} is not a scenario input; declare it with its owner"
        );
        self.0.get(name).cloned().flatten()
    }
}

/// The names a default build registers: the composition root's configuration.
#[cfg(test)]
pub(crate) fn configuration_names() -> BTreeSet<&'static str> {
    CONFIGURATION
        .iter()
        .flat_map(|(_, specs)| specs.iter().map(|spec| spec.name))
        .collect()
}

/// Every declared hook, with the file that owns it, in name order.
pub(crate) fn all() -> Vec<(&'static str, &'static HookSpec)> {
    let mut out: Vec<(&'static str, &'static HookSpec)> = owners()
        .into_iter()
        .flat_map(|(path, specs)| specs.iter().map(move |spec| (path, spec)))
        .collect();
    out.sort_by_key(|(_, spec)| spec.name);
    out
}

/// Every declared hook name.
pub(crate) fn declared_names() -> BTreeSet<&'static str> {
    owners()
        .into_iter()
        .flat_map(|(_, specs)| specs.iter().map(|spec| spec.name))
        .collect()
}

/// The `QUANTICK_*` variables set in this environment that no slice declares
/// and [`NOT_HOOKS`] does not excuse; the comparison is
/// [`quantick_feed::hooks::undeclared`], with the environment injected.
pub(crate) fn unknown_hooks<'a>(
    environment: impl Iterator<Item = &'a str>,
    declared: &BTreeSet<&'static str>,
) -> Vec<String> {
    quantick_feed::hooks::undeclared(environment, declared, NOT_HOOKS)
}

/// The authored half, relative to the workspace root.
pub(crate) const PROSE_PATH: &str = "docs/ui-harness/hook-prose.md";

/// Render `.claude/skills/ui-harness/references/hook-registry.md`.
///
/// Declarations supply names and owner paths; only a build with every harness
/// family compiles them all, so any other build refuses rather than write a
/// partial registry over the committed one. The authored `Reaches` cells
/// supply behavior and feature requirements; they are copied unchanged,
/// without reflow, truncation or summarizing.
pub(crate) fn hook_registry_markdown() -> Result<String, String> {
    if !ALL_FAMILIES && !cfg!(test) {
        return Err("this build compiles only part of the hook registry; run \
             `cargo run -p quantick-app --features harness -- --dump-hook-registry`"
            .to_owned());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("crates/app sits two levels below the workspace root")?;
    let prose = std::fs::read_to_string(root.join(PROSE_PATH))
        .map_err(|error| format!("{PROSE_PATH}: {error}"))?;
    Ok(quantick_feed::hooks::registry::render_registry(
        &all(),
        &prose,
    ))
}

#[cfg(test)]
mod tests {
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
        assert!(
            unknown_hooks(["PATH", "QUANTICKISH", "RUST_LOG"].into_iter(), &declared).is_empty()
        );
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
}
