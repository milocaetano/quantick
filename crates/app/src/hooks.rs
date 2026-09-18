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
//! is known, so it is not `UNKNOWN_HOOK`. This warning does not enable hooks
//! or refuse startup: availability follows the owner's feature and behavior.

use std::collections::BTreeSet;

// The declaration half — the `HookSpec` type and the `declare_hooks!` macro
// that writes a module's slice — is defined in `quantick-feed` and re-exported
// here, so every module in the workspace declares its hooks the same way and
// `OWNERS` below can hold them all in one array.
//
// It sits there rather than here because four of that crate's adapters read a
// hook and it cannot depend on this one; the graph runs the other way. This
// module is still where the registry is: `OWNERS`, `NOT_HOOKS` and the
// startup warning are all below, and this is the file to open to find out
// which hooks exist.
use quantick_feed::hooks::registry::{self, render_registry};
pub(crate) use quantick_feed::hooks::{HookSpec, declare_hooks};

/// Every declared hook, with the file that owns it, in name order.
pub(crate) fn all() -> Vec<(&'static str, &'static HookSpec)> {
    registry::all(OWNERS)
}

/// Every declared hook name.
pub(crate) fn declared_names() -> BTreeSet<&'static str> {
    registry::declared_names(OWNERS)
}

/// The `QUANTICK_*` variables set in this environment that no slice
/// declares and [`NOT_HOOKS`] does not excuse.
pub(crate) fn unknown_hooks<'a>(
    environment: impl Iterator<Item = &'a str>,
    declared: &BTreeSet<&'static str>,
) -> Vec<String> {
    registry::unknown_hooks(environment, declared, NOT_HOOKS)
}

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

/// Warn once at startup about undeclared, non-exempt `QUANTICK_*` names.
pub(crate) fn log_unknown_hooks() {
    let declared = declared_names();
    let environment: Vec<String> = std::env::vars().map(|(name, _)| name).collect();
    for name in unknown_hooks(environment.iter().map(String::as_str), &declared) {
        tracing::warn!(
            target: "quantick::app",
            event_code = "UNKNOWN_HOOK",
            hook = %name,
            "no launch hook by this name is registered; it will do nothing. \
             Check the spelling against .claude/skills/ui-harness/references/hook-registry.md"
        );
    }
}

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
        for (path, _) in OWNERS {
            assert!(root.join(path).is_file(), "{path} is not a file");
        }
    }
}
