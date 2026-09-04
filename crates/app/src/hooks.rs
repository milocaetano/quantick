//! Where every launch hook declares that it exists.
//!
//! A *hook* is a `QUANTICK_*` environment variable the application reads to
//! put itself into a state a hand would otherwise have to click it into.
//! `ui-harness` documents them, `visual-qa` and `trader-ux-review` drive the
//! application through them, and until this module existed the documentation
//! was the only record that a hook was real.
//!
//! That record was wrong in both directions. Three hooks the application reads
//! had no row at all. One row — `QUANTICK_DRAWING_MANAGER`, singular — named a
//! variable nothing has ever read; the code reads `QUANTICK_DRAWINGS_MANAGER`,
//! and the same file spells it correctly two rows further down. A capture run
//! setting the documented spelling got a window that simply did not open the
//! object manager, which reads exactly like a defect in the surface.
//!
//! # The two halves, and why they are apart
//!
//! **The code owns which hooks exist.** Each module that reads one declares it
//! in its own `HOOKS` slice, beside the read, and [`OWNERS`] below carries one
//! line per module. Adding a hook is a slice entry where the read is; adding a
//! module that owns hooks is one line here.
//!
//! **The prose owns what each hook means.** `docs/ui-harness/hook-prose.md`
//! holds the long `Reaches` cells — the paragraphs that explain which class of
//! defect is invisible without that hook, which is the most valuable content in
//! the harness and is deliberately not compressed. It stays prose because
//! prose is what it is, and it stays under `docs/` rather than
//! `.claude/skills/` because the context ratchet weighs that tree and a second
//! seventy-kilobyte file there would cost every session what the generated one
//! already costs it.
//!
//! `.claude/skills/ui-harness/references/hook-registry.md` is neither: it is
//! **generated** by fusing the two, through
//! `quantick-app --dump-hook-registry`. A hook missing from either half fails
//! `cargo test -p quantick-guards`, so the pair cannot drift apart the way the
//! single hand-kept file drifted from the code.
//!
//! # `UNKNOWN_HOOK`
//!
//! [`log_unknown_hooks`] runs once at startup and warns about any `QUANTICK_*`
//! in the environment that no slice declares. That is the other half of the
//! `QUANTICK_DRAWING_MANAGER` story: a dead hook used to present as a surface
//! that did not open, which sends the reader looking at the surface. Now it
//! says so on the first line of the log.
//!
//! It warns rather than exits. A typo in a capture script should be loud, but
//! an unbootable application is a worse failure than the one being fixed, and
//! the variable may belong to something else entirely.

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
pub(crate) use quantick_feed::hooks::{HookSpec, declare_hooks};

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
    ("crates/app/src/app.rs", crate::app::HOOKS),
    (
        "crates/app/src/bubble_presets.rs",
        crate::bubble_presets::HOOKS,
    ),
    ("crates/app/src/chart_layers.rs", crate::chart_layers::HOOKS),
    ("crates/app/src/config.rs", crate::config::HOOKS),
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
    ("crates/app/src/main.rs", crate::MAIN_HOOKS),
    ("crates/app/src/paper_home.rs", crate::paper_home::HOOKS),
    ("crates/app/src/paper_state.rs", crate::paper_state::HOOKS),
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

/// The `QUANTICK_*` variables set in this environment that no slice declares.
///
/// Takes the environment as an iterator rather than reading it, so the test
/// can exercise the real comparison without touching process state — setting
/// an environment variable is `unsafe` in this edition and racy under a
/// threaded test runner.
pub(crate) fn unknown_hooks<'a>(
    environment: impl Iterator<Item = &'a str>,
    declared: &BTreeSet<&'static str>,
) -> Vec<String> {
    let mut out: Vec<String> = environment
        .filter(|name| name.starts_with("QUANTICK_"))
        .filter(|name| !declared.contains(name))
        .filter(|name| !NOT_HOOKS.iter().any(|(known, _)| known == name))
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Warn, once at startup, about every `QUANTICK_*` nothing reads.
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

/// The marker the generated registry opens with.
pub(crate) const GENERATED_MARKER: &str =
    "<!-- generated by `quantick-app --dump-hook-registry`; do not edit -->";

/// The authored half, relative to the workspace root.
pub(crate) const PROSE_PATH: &str = "docs/ui-harness/hook-prose.md";

/// Render `.claude/skills/ui-harness/references/hook-registry.md`.
///
/// The index comes from the specs, so it cannot name a hook the application
/// does not read. The prose comes from [`PROSE_PATH`] and is copied through
/// **unaltered** — no reflow, no truncation, no summarising. The long cells
/// are the point of the file: each says which class of defect is invisible
/// without that hook, which is the one thing a grep of the source cannot tell
/// you.
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
/// One table, not two. An index beside the prose would repeat every hook name
/// a second time; adding the owner *to the row that already describes the
/// hook* puts the new fact where a reader is already looking and costs the
/// context budget a column instead of a page.
///
/// Prose lines that are not table rows pass through untouched, and a row's
/// `Reaches` cell is never rewritten — [`PROSE_PATH`] is the authored half and
/// this function is not allowed an opinion about it.
fn render_registry(hooks: &[(&'static str, &'static HookSpec)], prose: &str) -> String {
    let owners: std::collections::BTreeMap<&str, &str> = hooks
        .iter()
        .map(|(path, spec)| (spec.name, path.strip_prefix(OWNER_PREFIX).unwrap_or(path)))
        .collect();

    let mut out = String::new();
    out.push_str("# Hook registry\n\n");
    out.push_str(GENERATED_MARKER);
    out.push_str("\n\n");
    out.push_str(concat!(
        "Every `QUANTICK_*` the application reads, what it reaches, and where it
",
        "is declared (paths relative to `crates/app/src/`).
",
        "
",
        "Generated: existence from the `declare_hooks!` line beside each read,
",
        "prose from `docs/ui-harness/hook-prose.md` — edit there, then
",
        "`cargo run -p quantick-app -- --dump-hook-registry > <this file>`.
",
        "`cargo test -p quantick-guards` fails when a hook is read but not
",
        "described, or described but not read; an unrecognised `QUANTICK_*` in the
",
        "environment is logged at startup as `UNKNOWN_HOOK`.
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

    for line in body.lines() {
        if line.starts_with("| Hook ") {
            out.push_str("| Hook | Declared in | Reaches |\n");
        } else if line.starts_with("| --- ") {
            out.push_str("| --- | --- | --- |\n");
        } else if let Some(fused) = fuse_row(line, &owners) {
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

/// Turn `| <hook cell> | <reaches> |` into `| <hook cell> | <owners> | <reaches> |`.
///
/// A row may name more than one hook — several of them do, because the hooks
/// are used together — so the owner cell lists each distinct file once, in the
/// order the names appear.
fn fuse_row(line: &str, owners: &std::collections::BTreeMap<&str, &str>) -> Option<String> {
    let body = line.strip_prefix("| ")?.strip_suffix(" |")?;
    let (hook_cell, reaches) = body.split_once(" | ")?;
    let mut paths: Vec<&str> = Vec::new();
    for name in hook_names(hook_cell) {
        if let Some(path) = owners.get(name.as_str())
            && !paths.contains(path)
        {
            paths.push(path);
        }
    }
    let owner_cell = if paths.is_empty() {
        "—".to_owned()
    } else {
        paths
            .iter()
            .map(|path| format!("`{path}`"))
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
