//! A crate that exactly one other crate links, signed for.
//!
//! Extracting code from `app` into its own crate is how the refactor sprints
//! scored the architecture, and a crate is only as good as the boundary it
//! draws. A crate whose one consumer is `app` is `app` with a manifest: it
//! moves the lines out of the trunk's count while nothing else in the
//! workspace can reuse it, and backtest and the bot — the other two consumers
//! `CLAUDE.md` promises — stay exactly as far from it as before. Crate count
//! is a proxy that can be gamed by splitting; a second consumer is the thing
//! the split was meant to buy.
//!
//! So a crate under `crates/` whose only consumer is one other workspace crate
//! fails, unless [`EXEMPTIONS_FILE`] carries a `<crate> <reason>` line for it.
//! The list is short on purpose and the teeth run both ways: an exemption for
//! a crate that gained a second consumer, lost its last one, or no longer
//! exists permits nothing and is itself a finding — delete the line.
//!
//! "Consumer" means a shipped dependency, as [`graph::consumers`] reads the
//! manifests: `[dependencies]` and its target-specific variants. A crate a
//! test links through `[dev-dependencies]` is not consumed by production code,
//! and counting it would let a one-line dev-dependency buy the exemption.
//!
//! There is no recorded count. Like the headless allowlist, every entry is an
//! argument somebody has to make again to add the next one.

use std::fs;
use std::path::Path;

use crate::{Finding, graph};

/// The signed exemptions: `<crate> <reason>` per line.
pub const EXEMPTIONS_FILE: &str = "crates/guards/single-consumer-exemptions.txt";

/// What the guard asks for when a crate has one consumer and no exemption.
pub const REMEDY: &str = "A crate linked by exactly one other workspace crate is that crate with \
    a manifest: it moved lines without drawing a boundary anything else can reuse. Give it a \
    second shipped consumer (backtest, the bot, another domain crate), fold it back into its \
    consumer, or sign for it with one `<crate> <reason>` line in \
    crates/guards/single-consumer-exemptions.txt saying why one consumer is the design.";

/// What the guard asks for when an exemption line is malformed or stale.
pub const EXEMPTION_REMEDY: &str = "Every line in crates/guards/single-consumer-exemptions.txt is \
    blank, a `#` comment, or `<crate> <reason>`: a directory under crates/ with exactly one \
    shipped consumer, and why. An exemption for a crate that is gone, or no longer has exactly \
    one consumer, permits nothing — delete the line.";

/// One signed exemption.
struct Exemption {
    krate: String,
    /// One-based line in [`EXEMPTIONS_FILE`].
    line: usize,
}

/// Read the exemptions. A malformed line is a finding and no exemption, so a
/// typo can never widen the list.
fn exemptions(root: &Path) -> Result<(Vec<Exemption>, Vec<Finding>), String> {
    let text = fs::read_to_string(root.join(EXEMPTIONS_FILE))
        .map_err(|e| format!("  {EXEMPTIONS_FILE} is unreadable: {e}"))?;
    let mut found: Vec<Exemption> = Vec::new();
    let mut findings = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let (krate, reason) = content
            .split_once(char::is_whitespace)
            .unwrap_or((content, ""));
        let problem = if reason.trim().is_empty() {
            Some(format!("exempts {krate} with no reason"))
        } else {
            found
                .iter()
                .find(|known| known.krate == krate)
                .map(|first| format!("{krate} is already exempt on line {}", first.line))
        };
        match problem {
            Some(problem) => findings.push(Finding::new(
                format!("  {EXEMPTIONS_FILE}:{line}: {problem}"),
                EXEMPTION_REMEDY,
            )),
            None => found.push(Exemption {
                krate: krate.to_owned(),
                line,
            }),
        }
    }
    Ok((found, findings))
}

/// Every crate with exactly one shipped consumer, with that consumer, sorted.
pub fn single_consumers(root: &Path) -> Vec<(String, String)> {
    graph::consumers(root)
        .into_iter()
        .filter_map(|(krate, by)| match by.as_slice() {
            [only] => Some((krate, only.clone())),
            _ => None,
        })
        .collect()
}

/// How many crates have exactly one consumer, exempt or not, for
/// [`crate::report`]: the list is the finding, and its length is the number
/// a merge moves.
pub fn count(root: &Path) -> usize {
    single_consumers(root).len()
}

/// Every single-consumer crate without an exemption, and every exemption
/// that is malformed or permits nothing.
pub fn check(root: &Path) -> Vec<Finding> {
    let (exempt, mut findings) = match exemptions(root) {
        Ok(read) => read,
        Err(problem) => return vec![Finding::new(problem, EXEMPTION_REMEDY)],
    };
    let consumers = graph::consumers(root);
    for (krate, by) in &consumers {
        if let [only] = by.as_slice()
            && !exempt.iter().any(|e| &e.krate == krate)
        {
            findings.push(Finding::new(
                format!("  crates/{krate}: its only consumer is `{only}`, and it is not exempt"),
                REMEDY,
            ));
        }
    }
    for exemption in &exempt {
        let stale = match consumers
            .iter()
            .find(|(krate, _)| krate == &exemption.krate)
        {
            None => Some("is not a crate under crates/".to_owned()),
            Some((_, by)) if by.len() != 1 => Some(format!("has {} consumers", by.len())),
            Some(_) => None,
        };
        if let Some(stale) = stale {
            findings.push(Finding::new(
                format!(
                    "  {EXEMPTIONS_FILE}:{}: {} {stale} — delete the line",
                    exemption.line, exemption.krate
                ),
                EXEMPTION_REMEDY,
            ));
        }
    }
    findings
}

/// The edit-time question: a manifest is the only way to add or remove a
/// consumer, and the exemption file is the only way to sign for one.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    let in_scope = relative == EXEMPTIONS_FILE
        || (relative.starts_with("crates/") && relative.ends_with("/Cargo.toml"));
    if in_scope { check(root) } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch_dir::ScratchDir;

    /// A scratch workspace: one manifest per `(crate, manifest body)`, and
    /// the exemption file.
    fn workspace(crates: &[(&str, String)], exemptions: &str) -> ScratchDir {
        let root = ScratchDir::new("single-consumer");
        for (name, body) in crates {
            let dir = root.join(format!("crates/{name}"));
            fs::create_dir_all(&dir).expect("crate dir is creatable");
            fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\n\n{body}"),
            )
            .expect("manifest is writable");
        }
        fs::create_dir_all(root.join("crates/guards")).expect("guards dir is creatable");
        fs::write(root.join(EXEMPTIONS_FILE), exemptions).expect("exemptions are writable");
        root
    }

    fn uses(names: &[&str]) -> String {
        let mut body = String::from("[dependencies]\n");
        for name in names {
            body.push_str(&format!("quantick-{name} = {{ path = \"../{name}\" }}\n"));
        }
        body
    }

    fn lines(findings: &[Finding]) -> String {
        findings
            .iter()
            .map(|f| f.line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `engine` has two consumers; `feed` has one; `app` and `backtest` are
    /// leaves with none.
    fn graph() -> Vec<(&'static str, String)> {
        vec![
            ("app", uses(&["engine", "feed"])),
            ("backtest", uses(&["engine"])),
            ("engine", String::new()),
            ("feed", uses(&["engine"])),
        ]
    }

    #[test]
    fn a_crate_with_one_consumer_and_no_exemption_fails() {
        let crates = graph();
        let root = workspace(&crates, "");
        let found = lines(&check(root.path()));
        assert_eq!(
            found, "  crates/feed: its only consumer is `app`, and it is not exempt",
            "leaves and multi-consumer crates are not findings"
        );
        assert_eq!(count(root.path()), 1);
    }

    #[test]
    fn a_signed_exemption_passes() {
        let crates = graph();
        let root = workspace(&crates, "# note\nfeed the venue host\n");
        assert!(check(root.path()).is_empty(), "{:?}", check(root.path()));
    }

    #[test]
    fn an_exemption_without_a_reason_permits_nothing() {
        let crates = graph();
        let root = workspace(&crates, "feed\n");
        let found = lines(&check(root.path()));
        assert!(found.contains(":1: exempts feed with no reason"), "{found}");
        assert!(found.contains("crates/feed: its only consumer"), "{found}");
    }

    #[test]
    fn a_duplicate_exemption_is_a_finding() {
        let crates = graph();
        let root = workspace(&crates, "feed a\nfeed b\n");
        let found = lines(&check(root.path()));
        assert!(found.contains("already exempt on line 1"), "{found}");
    }

    /// Teeth the other way: an exemption the graph no longer needs.
    #[test]
    fn an_exemption_for_a_crate_that_gained_a_consumer_is_stale() {
        let mut crates = graph();
        crates[1].1 = uses(&["engine", "feed"]);
        let root = workspace(&crates, "feed the venue host\n");
        let found = lines(&check(root.path()));
        assert!(
            found.contains("feed has 2 consumers — delete the line"),
            "{found}"
        );
    }

    #[test]
    fn an_exemption_for_a_missing_crate_is_stale() {
        let crates = graph();
        let root = workspace(&crates, "feed a\ngone b\n");
        let found = lines(&check(root.path()));
        assert!(
            found.contains("gone is not a crate under crates/"),
            "{found}"
        );
    }

    /// A test linking a crate is not a consumer: the dev-dependency cannot
    /// buy the second consumer.
    #[test]
    fn a_dev_dependency_is_not_a_consumer() {
        let mut crates = graph();
        crates[1].1 = format!(
            "{}\n[dev-dependencies]\nquantick-feed = {{ path = \"../feed\" }}\n",
            uses(&["engine"])
        );
        let root = workspace(&crates, "");
        let found = lines(&check(root.path()));
        assert!(
            found.contains("crates/feed: its only consumer is `app`"),
            "{found}"
        );
    }

    #[test]
    fn a_target_specific_dependency_is_a_consumer() {
        let mut crates = graph();
        crates[1].1 = format!(
            "{}\n[target.'cfg(windows)'.dependencies]\nquantick-feed = {{ path = \"../feed\" }}\n",
            uses(&["engine"])
        );
        let root = workspace(&crates, "");
        assert!(check(root.path()).is_empty(), "{:?}", check(root.path()));
    }

    #[test]
    fn manifests_and_the_exemptions_are_in_scope_at_edit_time() {
        let crates = graph();
        let root = workspace(&crates, "");
        assert!(!check_file(root.path(), "crates/app/Cargo.toml").is_empty());
        assert!(!check_file(root.path(), EXEMPTIONS_FILE).is_empty());
        assert!(check_file(root.path(), "crates/app/src/lib.rs").is_empty());
    }
}
