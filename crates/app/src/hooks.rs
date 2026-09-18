//! Discovery for declared startup configuration and opt-in scenario hooks.
//!
//! Each owner declares its `QUANTICK_*` names in `HOOKS`; [`OWNERS`] joins
//! those slices into the catalog. Declarations remain available when a
//! scenario's Cargo feature is disabled. A registered name therefore does
//! not prove that the current executable reads it or performs its scenario.
//!
//! `docs/ui-harness/hook-prose.md` owns each hook's behavior and any required
//! feature. `quantick-app --dump-hook-registry` joins those descriptions with
//! owner paths into `.claude/skills/ui-harness/references/hook-registry.md`.
//! The complete `Reaches` cells pass through unchanged. Authored prose stays
//! under `docs/`; the generated catalog is the skill's discovery reference.
//!
//! The guard checks declarations, source names and documented names for
//! parity. Disabled scenario implementations still exist in source and keep
//! their declarations; feature gating is not permission to omit a name or
//! weaken that check. Adding an owner requires a declaration slice, one
//! registration here and authored descriptions, followed by regeneration.
//!
//! Historical drift included the unimplemented `QUANTICK_DRAWING_MANAGER`
//! spelling alongside the real `QUANTICK_DRAWINGS_MANAGER`. Declaration
//! parity detects that mismatch; it does not establish runtime availability.
//!
//! [`log_unknown_hooks`] warns once at startup about undeclared names, except
//! for the documented [`NOT_HOOKS`] entries. A declared but disabled scenario
//! is known, so it is not `UNKNOWN_HOOK`; set without its feature it is
//! `HOOK_DISABLED` ([`FEATURE_GATED`]). Neither enables a hook or stops startup.

use std::collections::BTreeSet;

// `HookSpec`, `declare_hooks!` and the two pure environment comparisons live
// in `quantick-feed`, whose adapters read hooks and cannot depend on this
// crate. This module is still the registry: `OWNERS`, `NOT_HOOKS`,
// `FEATURE_GATED` and the startup warning are below.
use crate::surfaces::drawing_chrome;
pub(crate) use quantick_feed::hooks::{
    FeatureGate, HookSpec, compiled_out, declare_hooks, undeclared,
};

/// `QUANTICK_*` variables that are deliberately **not** launch hooks.
///
/// One definition, two readers. [`log_unknown_hooks`] skips them, so a build
/// that sets `QUANTICK_GIT_COMMIT` is not warned about its own build metadata;
/// and `crates/guards/src/generated.rs` parses this same table out of this
/// file, so the guard cannot demand a harness row for something the
/// application never reads. A second copy kept by hand in the guard would be
/// the duplicated truth this module exists to end, and it would drift the
/// first time either side gained an entry.
///
/// Each carries its reason, because an allowlist is how a parity guard is
/// quietly defeated: a reader who disagrees with an entry has something to
/// disagree with.
pub(crate) const NOT_HOOKS: &[(&str, &str)] = &[
    (
        "QUANTICK_GIT_COMMIT",
        "build metadata, read through `option_env!` at compile time and \n         reported in the control plane's system info. Setting it at runtime \n         does nothing.",
    ),
    (
        "QUANTICK_FAKE_STORE",
        "test plumbing inside `workspace_bundle`'s own `#[cfg(test)]` module. \n         Never read by a release build.",
    ),
    (
        "QUANTICK_TEST_STORE_HOME_ENV",
        "test plumbing inside `store_home`'s own `#[cfg(test)]` module, which \n         lets a test redirect the store home. Never read by a release build.",
    ),
];

/// Every module that owns hooks, with the path a reader should open to find
/// them.
///
/// The path is written out rather than derived because `module_path!()` gives
/// a Rust path and the registry has to name a file someone can open. The guard
/// checks the two agree: a slice registered under the wrong path, or a file
/// that reads a `QUANTICK_*` without registering a slice at all, is a finding.
pub(crate) const OWNERS: &[(&str, &[HookSpec])] = &[
    (
        "crates/app/src/app/control_host.rs",
        crate::app::control_host::HOOKS,
    ),
    ("crates/app/src/toolrail.rs", crate::toolrail::HOOKS),
    (
        "crates/app/src/app/launch_hooks.rs",
        crate::app::launch_hooks::HOOKS,
    ),
    (
        "crates/app/src/bubble_presets.rs",
        crate::bubble_presets::HOOKS,
    ),
    ("crates/app/src/chart_layers.rs", crate::chart_layers::HOOKS),
    ("crates/app/src/config.rs", crate::config::HOOKS),
    (
        "crates/app/src/deal_recording.rs",
        crate::deal_recording::HOOKS,
    ),
    (
        "crates/app/src/drawings/presets.rs",
        crate::drawings::presets::HOOKS,
    ),
    ("crates/feed/src/binance.rs", quantick_feed::binance::HOOKS),
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
        "crates/app/src/footprint_presets.rs",
        crate::footprint_presets::HOOKS,
    ),
    (
        "crates/app/src/footprint_render.rs",
        crate::footprint_render::HOOKS,
    ),
    ("crates/app/src/frvp.rs", crate::frvp::HOOKS),
    ("crates/app/src/harness.rs", crate::harness::HOOKS),
    (
        "crates/app/src/indicators/library.rs",
        crate::indicators::library::HOOKS,
    ),
    (
        "crates/app/src/indicators/preset_file.rs",
        crate::indicators::preset_file::HOOKS,
    ),
    (
        "crates/app/src/indicators/state_file.rs",
        crate::indicators::state_file::HOOKS,
    ),
    ("crates/app/src/layouts.rs", crate::layouts::HOOKS),
    ("crates/app/src/launch.rs", crate::launch::HOOKS),
    ("crates/app/src/paper_home.rs", crate::paper_home::HOOKS),
    ("crates/app/src/paper_state.rs", crate::paper_state::HOOKS),
    (
        "crates/app/src/paper_account.rs",
        crate::paper_account::HOOKS,
    ),
    (
        "crates/app/src/paper_trading.rs",
        crate::paper_trading::HOOKS,
    ),
    ("crates/app/src/replay_home.rs", crate::replay_home::HOOKS),
    ("crates/app/src/replay_view.rs", crate::replay_view::HOOKS),
    (
        "crates/app/src/strategy_presets.rs",
        crate::strategy_presets::HOOKS,
    ),
    (
        "crates/app/src/surfaces/agent_popup.rs",
        crate::surfaces::agent_popup::HOOKS,
    ),
    (
        "crates/app/src/surfaces/drawing_chrome/mod.rs",
        crate::surfaces::drawing_chrome::HOOKS,
    ),
    (
        "crates/app/src/surfaces/drawing_chrome/quick_range.rs",
        crate::surfaces::drawing_chrome::QUICK_RANGE_HOOKS,
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
    ("crates/app/src/symbols_file.rs", crate::symbols_file::HOOKS),
    ("crates/app/src/tab.rs", crate::tab::HOOKS),
    ("crates/app/src/ui_state.rs", crate::ui_state::HOOKS),
];

/// A family `main.rs` captures only under `$feature`, by that same `cfg!`.
macro_rules! gate {
    ($feature:literal, $hooks:expr) => {
        FeatureGate {
            feature: $feature,
            compiled: cfg!(feature = $feature),
            hooks: $hooks,
        }
    };
}

/// Declared hooks that are inert unless their feature is built in.
pub(crate) const FEATURE_GATED: &[FeatureGate] = &[
    gate!("control-harness", crate::app::control_host::HOOKS),
    gate!("drawing-harness", crate::toolrail::HOOKS),
    gate!("drawing-harness", crate::surfaces::drawing_chrome::HOOKS),
    gate!("quick-range-harness", drawing_chrome::QUICK_RANGE_HOOKS),
];

/// Every declared hook, with the file that owns it, in name order.
pub(crate) fn all() -> Vec<(&'static str, &'static HookSpec)> {
    let mut out: Vec<(&'static str, &'static HookSpec)> = OWNERS
        .iter()
        .flat_map(|(path, specs)| specs.iter().map(move |spec| (*path, spec)))
        .collect();
    out.sort_by_key(|(_, spec)| spec.name);
    out
}

/// Every declared hook name.
pub(crate) fn declared_names() -> BTreeSet<&'static str> {
    OWNERS
        .iter()
        .flat_map(|(_, specs)| specs.iter().map(|spec| spec.name))
        .collect()
}

/// Warn once at startup about undeclared, non-exempt `QUANTICK_*` names, and
/// about declared ones this build compiled out.
pub(crate) fn log_unknown_hooks() {
    let names: Vec<String> = std::env::vars().map(|(name, _)| name).collect();
    for (hook, feature) in compiled_out(names.iter().map(String::as_str), FEATURE_GATED) {
        tracing::warn!(target: "quantick::app", event_code = "HOOK_DISABLED", %hook, feature,
            "compiled out of this build, so it does nothing; add `--features {feature}`");
    }
    for name in undeclared(
        names.iter().map(String::as_str),
        &declared_names(),
        NOT_HOOKS,
    ) {
        tracing::warn!(
            target: "quantick::app",
            event_code = "UNKNOWN_HOOK",
            hook = %name,
            "no launch hook by this name is registered; it will do nothing. \
             Check the spelling against .claude/skills/ui-harness/references/hook-registry.md"
        );
    }
}

/// The marker the generated registry opens with.
pub(crate) const GENERATED_MARKER: &str =
    "<!-- generated by `quantick-app --dump-hook-registry`; do not edit -->";

/// The authored half, relative to the workspace root.
pub(crate) const PROSE_PATH: &str = "docs/ui-harness/hook-prose.md";

/// Render `.claude/skills/ui-harness/references/hook-registry.md`.
///
/// Declarations supply names and owner paths, including disabled scenarios.
/// The authored `Reaches` cells supply behavior and feature requirements;
/// they are copied unchanged, without reflow, truncation or summarizing.
pub(crate) fn hook_registry_markdown() -> Result<String, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("crates/app sits two levels below the workspace root")?;
    let prose = std::fs::read_to_string(root.join(PROSE_PATH))
        .map_err(|error| format!("{PROSE_PATH}: {error}"))?;
    Ok(render_registry(&all(), &prose))
}

/// The path a reader opens, with the part every hook shares taken off.
///
/// `crates/app/src/` in front of a hundred and twenty-nine rows is two
/// kilobytes of the context budget spent saying the same eleven words, in a
/// file whose whole argument is that a targeted run should cost less to
/// answer, not more.
const OWNER_PREFIX: &str = "crates/app/src/";

/// Fuse the declared hooks into the authored prose.
///
/// Hook rows retain their authored order and complete descriptions.
/// Repeated owner paths use readable keys; the legend resolves each key
/// to its full path. Single-use paths remain in their rows. The legend
/// contains owner paths only, so it never duplicates the hook inventory.
///
/// Prose lines that are not table rows pass through untouched, and a row's
/// `Reaches` cell is never rewritten — [`PROSE_PATH`] is the authored half and
/// this function is not allowed an opinion about it.
fn render_registry(hooks: &[(&'static str, &'static HookSpec)], prose: &str) -> String {
    let owners: std::collections::BTreeMap<&str, &'static str> = hooks
        .iter()
        .map(|(path, spec)| (spec.name, path.strip_prefix(OWNER_PREFIX).unwrap_or(path)))
        .collect();

    let mut out = String::new();
    out.push_str("# Hook registry\n\n");
    out.push_str(GENERATED_MARKER);
    out.push_str("\n\n");
    out.push_str(concat!(
        "Declared `QUANTICK_*` inputs: behavior, feature requirements and owners.
",
        "Paths relative to `crates/app/src/`; declaration does not enable a hook.
",
        "
",
        "Generated: owner `declare_hooks!` slices include disabled hooks,
",
        "prose from `docs/ui-harness/hook-prose.md` — edit there, then
",
        "`cargo run -p quantick-app -- --dump-hook-registry > <this file>`.
",
        "`cargo test -p quantick-guards` checks source/declaration/prose parity;
",
        "disabled hooks remain cataloged. Undeclared, non-exempt names in the
",
        "environment are logged at startup as `UNKNOWN_HOOK`; declared names
",
        "this build compiled out, as `HOOK_DISABLED` with the feature to add.
",
        "
",
    ));

    // The prose file opens with its own explanation of what it is and how to
    // regenerate from it. That is guidance for whoever edits it, not part of
    // the registry, and the generated file states both things in its own
    // words above — so the copy starts at the first table.
    let body = match prose.find("\n| Hook ") {
        Some(offset) => &prose[offset + 1..],
        None => prose,
    };

    let keys = owner_keys(&owners, body);
    if !keys.is_empty() {
        out.push_str(
            "\nOwner keys (other paths appear in full):\n\n| Key | Declared in |\n| --- | --- |\n",
        );
        for (path, key) in &keys {
            out.push_str(&format!("| {key} | `{path}` |\n"));
        }
        out.push('\n');
    }
    for line in body.lines() {
        if line.starts_with("| Hook ") {
            out.push_str("| Hook | Owner | Reaches |\n");
        } else if line.starts_with("| --- ") {
            out.push_str("| --- | --- | --- |\n");
        } else if let Some(fused) = fuse_row(line, &owners, &keys) {
            out.push_str(&fused);
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(&format!("\n{} hooks registered.\n", hooks.len()));
    out
}

/// Readable presentation keys; full paths remain canonical in the legend.
fn owner_keys(
    owners: &std::collections::BTreeMap<&str, &'static str>,
    body: &str,
) -> std::collections::BTreeMap<&'static str, String> {
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for line in body.lines() {
        if let Some((_, _, paths)) = row_paths(line, owners) {
            for path in paths {
                *counts.entry(path).or_default() += 1;
            }
        }
    }
    let mut assigned: std::collections::BTreeMap<_, _> = owners
        .values()
        .map(|path| (*path, (*path).to_owned()))
        .collect();
    for path in assigned.keys().copied().collect::<Vec<_>>() {
        let key = owner_key_candidates(path)
            .into_iter()
            .find(|key| {
                assigned
                    .iter()
                    .all(|(other, value)| *other == path || value != key)
            })
            .expect("the exact owner path is unique");
        assigned.insert(path, key);
    }
    assigned.retain(|path, _| counts.get(path).is_some_and(|count| *count > 1));
    assigned
}

fn owner_key_candidates(path: &str) -> Vec<String> {
    let stem = path
        .strip_suffix("/mod.rs")
        .or_else(|| path.strip_suffix("/src/lib.rs"))
        .unwrap_or_else(|| path.strip_suffix(".rs").unwrap_or(path));
    let parts: Vec<_> = stem.split('/').collect();
    let words: Vec<_> = parts
        .last()
        .expect("a path has a segment")
        .split('_')
        .collect();
    let mut candidates: Vec<_> = (1..=words.len())
        .map(|length| words[words.len() - length..].join("_"))
        .collect();
    candidates.extend((2..=parts.len()).map(|length| parts[parts.len() - length..].join("/")));
    candidates.push(path.to_owned());
    candidates
}

/// Keep complete cells and the order of distinct owners within each row.
fn row_paths<'a>(
    line: &'a str,
    owners: &std::collections::BTreeMap<&str, &'static str>,
) -> Option<(&'a str, &'a str, Vec<&'static str>)> {
    let body = line.strip_prefix("| ")?.strip_suffix(" |")?;
    let (hook_cell, reaches) = body.split_once(" | ")?;
    let mut paths = Vec::new();
    for name in hook_names(hook_cell) {
        if let Some(path) = owners.get(name.as_str())
            && !paths.contains(path)
        {
            paths.push(*path);
        }
    }
    Some((hook_cell, reaches, paths))
}

fn fuse_row(
    line: &str,
    owners: &std::collections::BTreeMap<&str, &'static str>,
    keys: &std::collections::BTreeMap<&str, String>,
) -> Option<String> {
    let (hook_cell, reaches, paths) = row_paths(line, owners)?;
    let owner_cell = if paths.is_empty() {
        "—".to_owned()
    } else {
        paths
            .iter()
            .map(|path| {
                keys.get(path)
                    .cloned()
                    .unwrap_or_else(|| format!("`{path}`"))
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(format!("| {hook_cell} | {owner_cell} | {reaches} |"))
}

/// Every `QUANTICK_*` named in a cell, in order of appearance.
pub(crate) fn hook_names(cell: &str) -> Vec<String> {
    let bytes = cell.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while let Some(offset) = cell[index..].find("QUANTICK_") {
        let start = index + offset;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_uppercase()
                || bytes[end].is_ascii_digit()
                || bytes[end] == b'_')
        {
            end += 1;
        }
        out.push(cell[start..end].to_owned());
        index = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unknown_hooks<'a>(
        environment: impl Iterator<Item = &'a str>,
        declared: &BTreeSet<&'static str>,
    ) -> Vec<String> {
        undeclared(environment, declared, NOT_HOOKS)
    }

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

    /// Nothing outside the prefix is ever reported, however odd it looks.
    #[test]
    fn variables_outside_the_prefix_are_not_this_guard_s_business() {
        let declared = declared_names();
        assert!(
            unknown_hooks(["PATH", "QUANTICKISH", "RUST_LOG"].into_iter(), &declared).is_empty()
        );
    }

    #[test]
    fn a_set_hook_whose_feature_is_off_is_reported_disabled() {
        let gates = [
            FeatureGate {
                feature: "control-harness",
                compiled: false,
                hooks: crate::app::control_host::HOOKS,
            },
            FeatureGate {
                feature: "drawing-harness",
                compiled: true,
                hooks: crate::toolrail::HOOKS,
            },
        ];
        let environment = [
            "QUANTICK_CONTROL_ACCESS",
            "QUANTICK_DRAWING_TOOL",
            "QUANTICK_TOAST",
            "QUANTICK_CONTROL_ACCESS",
        ];
        assert_eq!(
            compiled_out(environment.into_iter(), &gates),
            [("QUANTICK_CONTROL_ACCESS".to_owned(), "control-harness")]
        );
    }

    #[test]
    fn every_gated_hook_is_declared_and_its_flag_follows_the_feature() {
        let declared = declared_names();
        for gate in FEATURE_GATED {
            assert!(gate.hooks.iter().all(|spec| declared.contains(spec.name)));
            let expected = match gate.feature {
                "control-harness" => cfg!(feature = "control-harness"),
                "drawing-harness" => cfg!(feature = "drawing-harness"),
                "quick-range-harness" => cfg!(feature = "quick-range-harness"),
                other => panic!("{other} is not a quantick-app feature"),
            };
            assert_eq!(gate.compiled, expected, "{}", gate.feature);
        }
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
        for (path, _) in OWNERS {
            assert!(root.join(path).is_file(), "{path} is not a file");
        }
    }

    #[test]
    fn readable_owners_preserve_complete_cells_notes_modes_and_missing_owners() {
        static A: HookSpec = HookSpec::new("QUANTICK_ALPHA");
        static B: HookSpec = HookSpec::new("QUANTICK_BETA");
        let hooks = [
            ("crates/app/src/first_owner.rs", &A),
            ("crates/app/src/second_owner.rs", &B),
        ];
        let prose = "Intro\n| Hook | Reaches |\n| --- | --- |\n\
            | `QUANTICK_ALPHA=one` | Complete \\| escaped pipe, `code`, and Unicode — kept. |\n\
            Notes stay between these rows.\n\
            | `QUANTICK_BETA` / `QUANTICK_ALPHA=two` | Both owners, in this order. |\n\
            | `QUANTICK_MISSING` | Missing stays missing. |\n";
        let rendered = render_registry(&hooks, prose);
        assert!(rendered.contains("| owner | `first_owner.rs` |"));
        assert!(rendered.contains("| `QUANTICK_ALPHA=one` | owner | Complete \\| escaped pipe, `code`, and Unicode — kept. |"));
        assert!(rendered.contains("| `QUANTICK_BETA` / `QUANTICK_ALPHA=two` | `second_owner.rs`, owner | Both owners, in this order. |"));
        assert!(rendered.contains("| `QUANTICK_MISSING` | — | Missing stays missing. |"));
        let first = rendered.find("`QUANTICK_ALPHA=one`").unwrap();
        let note = rendered.find("Notes stay between these rows.").unwrap();
        let second = rendered.find("`QUANTICK_BETA` / ").unwrap();
        assert!(first < note && note < second);
        assert_eq!(rendered.matches("`QUANTICK_ALPHA=one`").count(), 1);
        assert_eq!(rendered.matches("`QUANTICK_ALPHA=two`").count(), 1);
    }

    #[test]
    fn owner_keys_are_deterministic_and_avoid_singleton_and_basename_collisions() {
        static A: HookSpec = HookSpec::new("QUANTICK_ALPHA");
        static B: HookSpec = HookSpec::new("QUANTICK_BETA");
        static C: HookSpec = HookSpec::new("QUANTICK_GAMMA");
        let hooks = [
            ("crates/app/src/a/shared_owner.rs", &A),
            ("crates/app/src/b/shared_owner.rs", &B),
            ("crates/app/src/c/shared_owner.rs", &C),
        ];
        let prose = "| Hook | Reaches |\n| --- | --- |\n\
            | `QUANTICK_ALPHA=one` | A one. |\n\
            | `QUANTICK_ALPHA=two` | A two. |\n\
            | `QUANTICK_BETA` | B singleton. |\n\
            | `QUANTICK_GAMMA=one` | C one. |\n\
            | `QUANTICK_GAMMA=two` | C two. |\n";
        let rendered = render_registry(&hooks, prose);
        assert_eq!(
            rendered,
            render_registry(&[hooks[2], hooks[0], hooks[1]], prose)
        );
        assert!(rendered.contains("| owner | `a/shared_owner.rs` |"));
        assert!(rendered.contains("| c/shared_owner | `c/shared_owner.rs` |"));
        assert!(rendered.contains("| `QUANTICK_BETA` | `b/shared_owner.rs` | B singleton. |"));
        assert!(
            !rendered.contains("| shared_owner |"),
            "the singleton reserves this readable key"
        );
    }

    #[test]
    fn owner_key_candidates_normalize_modules_and_keep_exact_fallback() {
        assert_eq!(
            owner_key_candidates("surfaces/drawing_chrome/mod.rs"),
            [
                "chrome",
                "drawing_chrome",
                "surfaces/drawing_chrome",
                "surfaces/drawing_chrome/mod.rs"
            ]
        );
        assert_eq!(
            owner_key_candidates("crates/feed/src/lib.rs"),
            ["feed", "crates/feed", "crates/feed/src/lib.rs"]
        );
        assert_eq!(owner_key_candidates("plain.rs"), ["plain", "plain.rs"]);
    }

    #[test]
    fn singleton_or_missing_owners_need_no_legend() {
        static A: HookSpec = HookSpec::new("QUANTICK_ALPHA");
        let rendered = render_registry(
            &[("crates/app/src/one.rs", &A)],
            "| Hook | Reaches |\n| --- | --- |\n| `QUANTICK_ALPHA` | One. |\n| `QUANTICK_MISSING` | None. |\n",
        );
        assert!(!rendered.contains("Owner keys"));
        assert!(rendered.contains("| `QUANTICK_ALPHA` | `one.rs` | One. |"));
        assert!(rendered.contains("| `QUANTICK_MISSING` | — | None. |"));
    }

    #[test]
    fn an_exact_path_remains_available_when_every_short_key_is_taken() {
        let owners = std::collections::BTreeMap::from([
            ("QUANTICK_ALPHA", "a/foo.rs"),
            ("QUANTICK_BETA", "foo.rs"),
        ]);
        let body = "| `QUANTICK_ALPHA=one` | A. |\n| `QUANTICK_ALPHA=two` | A. |\n\
            | `QUANTICK_BETA=one` | B. |\n| `QUANTICK_BETA=two` | B. |\n";
        let keys = owner_keys(&owners, body);
        assert_eq!(keys["a/foo.rs"], "foo");
        assert_eq!(keys["foo.rs"], "foo.rs");
    }

    #[test]
    fn keys_reconstruct_each_distinct_owner_in_original_row_order() {
        let owners = std::collections::BTreeMap::from([
            ("QUANTICK_ALPHA", "a/owner.rs"),
            ("QUANTICK_BETA", "b/owner.rs"),
        ]);
        let body = "| `QUANTICK_BETA` / `QUANTICK_ALPHA` | Both. |\n\
            | `QUANTICK_ALPHA` / `QUANTICK_BETA` | Reverse. |\n";
        let keys = owner_keys(&owners, body);
        let reverse: std::collections::BTreeMap<_, _> = keys
            .iter()
            .map(|(path, key)| (key.as_str(), *path))
            .collect();
        assert_eq!(reverse.len(), keys.len(), "no two paths share a key");
        for (line, expected) in body.lines().zip([
            vec!["b/owner.rs", "a/owner.rs"],
            vec!["a/owner.rs", "b/owner.rs"],
        ]) {
            let rendered = fuse_row(line, &owners, &keys).unwrap();
            let owner_cell = rendered.split(" | ").nth(1).unwrap();
            let reconstructed: Vec<_> = owner_cell.split(", ").map(|key| reverse[key]).collect();
            assert_eq!(reconstructed, expected);
        }
    }
}
