//! The headless rule, checked instead of remembered.
//!
//! `CLAUDE.md` states it in one sentence: **everything below `app` is
//! headless — no UI, no network, no async, no wall clock**, and it names the
//! eleven crates it binds. Until this guard existed the sentence was enforced
//! by review alone, and review is exactly what it survived: `quantick-orderflow`
//! was extracted *as a headless crate* and arrived carrying `Instant::now()`.
//! Nothing failed. fmt, clippy, build and the whole suite stayed green, which
//! is the signature of every rule this crate exists to make mechanical.
//!
//! A clock in a domain crate is not a style problem. The determinism rule
//! downstream of it — same trades in, same bars out — is what makes a golden
//! test meaningful and what lets the chart, the backtest and the bot share one
//! aggregator. A crate that reads the wall clock cannot promise it, and the
//! promise fails silently: the golden fixture still passes, because the clock
//! only changes an answer under a load the fixture does not reproduce.
//!
//! # What is scanned, and what is deliberately not
//!
//! Only the crates `CLAUDE.md` names, and only their **production source** —
//! [`size::production_source`], the same definition the size ratchet rations
//! by, so there is one answer in this crate to "where does the test code
//! start?". A bench harness timing itself and a test that spells `Instant::now()`
//! to stand in for a caller's argument are not what the rule forbids; they
//! never ship, and reporting them would teach an author to write fewer tests.
//!
//! `feed` and the `feed-*` venue crates are out of scope by the same sentence
//! that puts the others in: the feed level *owns* the runtimes, the threads and
//! the clock, and stamps arrival. So are `app`, which owns the UI, and
//! `backtest` and `mcp`, whose single wall-clock reads `CLAUDE.md` documents by
//! name. Widening the scan to them would produce findings the rule does not
//! ask for, which is how a guard gets switched off.
//!
//! # Identifiers, not substrings
//!
//! The first version of this scan matched text, and `pine` — a crate whose
//! whole job is refusing `timeframe.*` — lit up on every error message it
//! writes about clocks it does not have. So the scan tokenises: a hit is a
//! *sequence of identifiers*, so `Instant :: now` and `Instant::now` are the
//! same hit and `timeframe` is not one at all.
//!
//! Comments are stripped first, deliberately. Every `egui` and `HashMap` in
//! these crates today sits in a doc comment explaining the very rule this
//! guard enforces — "this crate never depends on egui", "`HashMap`'s
//! iteration order would fail that check". Documentation stays free to name
//! what it forbids, and the allowlist stays a record of real code rather than
//! of prose.
//!
//! # The allowlist
//!
//! A remaining hit is legitimate only with a signed reason, in
//! `crates/guards/headless-allowlist.txt`: the file, the identifier, and the
//! sentence saying why. A stale entry — one naming a hit that no longer
//! exists — is itself a finding, because an allowlist nobody prunes becomes a
//! list of permissions granted to code that is gone, and the next author reads
//! it as precedent.
//!
//! There is no ratchet here and no recorded count. The tree is clean from the
//! commit that adds this guard, and a number to negotiate with is the one
//! thing that would let it drift back.

use std::fs;
use std::path::Path;

use crate::{Finding, size};

/// The crates `CLAUDE.md`'s headless sentence names, in the order it names
/// them.
///
/// A constant rather than a walk of `crates/`, because the rule is a list and
/// not a shape: `feed` and the venues sit in the same directory and are
/// exempt. `the_scanned_crates_are_the_ones_the_rule_names` checks this list
/// against that sentence, so a crate added to one and forgotten in the other
/// fails rather than going quietly unguarded.
pub const HEADLESS_CRATES: &[&str] = &[
    "engine",
    "orderbook",
    "orderflow",
    "trading",
    "control",
    "control-local",
    "indicators",
    "pine",
    "replay",
    "sim",
    "strategy",
];

/// One thing the headless rule forbids: how a finding spells it, and the
/// identifier sequence that finds it.
pub struct Forbidden {
    /// How the finding names it — the spelling an author recognises.
    pub name: &'static str,
    /// The consecutive identifiers that constitute a hit.
    ///
    /// Deliberately shorter than the display name where a call has more than
    /// one spelling: `use std::thread;` then `thread::spawn(…)` is the same
    /// call as `std::thread::spawn(…)`, and a needle insisting on the full
    /// path would miss the spelling this workspace actually writes. Matching
    /// the call rather than one way of writing it is the rule
    /// [`crate::scratch`] already settled on.
    pub identifiers: &'static [&'static str],
    /// Which half of the sentence this belongs to, for the finding's prose.
    pub because: &'static str,
}

/// Everything the scan looks for.
///
/// The four clauses of one sentence — no UI, no network, no async, no wall
/// clock — plus the determinism rule's `HashMap`, which belongs here rather
/// than in a guard of its own: an iteration order that differs between runs
/// breaks "same trades in, same bars out" exactly as a clock read does, and
/// these are the same crates.
pub const FORBIDDEN: &[Forbidden] = &[
    Forbidden {
        name: "tokio",
        identifiers: &["tokio"],
        because: "an async runtime",
    },
    Forbidden {
        name: "async fn",
        identifiers: &["async", "fn"],
        because: "an async function",
    },
    Forbidden {
        name: "std::thread::spawn",
        identifiers: &["thread", "spawn"],
        because: "a thread",
    },
    Forbidden {
        name: "SystemTime::now",
        identifiers: &["SystemTime", "now"],
        because: "the wall clock",
    },
    Forbidden {
        name: "Instant::now",
        identifiers: &["Instant", "now"],
        because: "the wall clock",
    },
    Forbidden {
        name: "egui",
        identifiers: &["egui"],
        because: "the UI toolkit",
    },
    Forbidden {
        name: "eframe",
        identifiers: &["eframe"],
        because: "the UI toolkit",
    },
    Forbidden {
        name: "HashMap",
        identifiers: &["HashMap"],
        because: "an iteration order that is not deterministic",
    },
    Forbidden {
        name: "HashSet",
        identifiers: &["HashSet"],
        because: "an iteration order that is not deterministic",
    },
];

/// Where the signed exceptions live.
pub const ALLOWLIST_FILE: &str = "crates/guards/headless-allowlist.txt";

/// The rule every finding from this guard cites.
const RULE: &str = "CLAUDE.md: Architecture, headless";

/// What the guard asks for when production source in a headless crate reaches
/// something the rule forbids.
pub const REMEDY: &str = "Production source in a crate `CLAUDE.md` calls headless reaches a runtime, a thread, the \
     wall clock, the UI toolkit or a non-deterministic map. Invert it: let the caller in `app` \
     or `feed` own the runtime and *tell* this crate what it needs, the way `replay` and \
     `strategy` are told how much time passed, and use `BTreeMap`/`Vec` where order must be \
     stable. If the site genuinely cannot ship a wrong answer, sign it in \
     `crates/guards/headless-allowlist.txt` with the file, the identifier and the reason.";

/// What the guard asks for when the allowlist names something that is gone.
pub const REMEDY_STALE: &str = "An entry in `crates/guards/headless-allowlist.txt` permits something no longer in the \
     source. Delete the line. An allowlist nobody prunes becomes a list of permissions granted \
     to code that is gone, which the next author reads as precedent.";

/// One signed exception.
struct Allowed {
    /// Workspace-relative path, with forward slashes.
    path: String,
    /// The [`Forbidden::name`] this line permits in that file.
    identifier: String,
    /// Line number in the allowlist, so a stale entry names itself.
    line: usize,
}

/// Read the allowlist. A missing file is an empty allowlist and no error: the
/// guard's clean state is zero exceptions, and refusing to run without the
/// file would make deleting it the cheapest way past the check.
///
/// Every other malformed shape *is* reported, through the returned findings —
/// a line naming an identifier the guard does not scan for, or a line with no
/// reason, is an exception nobody can audit.
fn allowlist(root: &Path) -> (Vec<Allowed>, Vec<Finding>) {
    let mut allowed = Vec::new();
    let mut findings = Vec::new();
    let Ok(text) = fs::read_to_string(root.join(ALLOWLIST_FILE)) else {
        return (allowed, findings);
    };
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = trimmed.splitn(3, char::is_whitespace);
        let (Some(path), Some(identifier), Some(reason)) =
            (fields.next(), fields.next(), fields.next())
        else {
            findings.push(Finding::new(
                format!(
                    "{ALLOWLIST_FILE}:{number}: `{trimmed}` is not `<path> <identifier> <reason>` \
                     — an exception with no reason is one nobody can audit — {RULE}"
                ),
                REMEDY_STALE,
            ));
            continue;
        };
        if reason.trim().is_empty() {
            findings.push(Finding::new(
                format!(
                    "{ALLOWLIST_FILE}:{number}: allows `{identifier}` in {path} with no reason — \
                     {RULE}"
                ),
                REMEDY_STALE,
            ));
            continue;
        }
        if !FORBIDDEN.iter().any(|f| f.name == identifier) {
            findings.push(Finding::new(
                format!(
                    "{ALLOWLIST_FILE}:{number}: allows `{identifier}`, which this guard does not \
                     scan for — {RULE}"
                ),
                REMEDY_STALE,
            ));
            continue;
        }
        allowed.push(Allowed {
            path: path.to_owned(),
            identifier: identifier.to_owned(),
            line: number,
        });
    }
    (allowed, findings)
}

/// One hit: a file, the line it is on, and what it reaches.
struct Hit {
    path: String,
    line: usize,
    forbidden: &'static Forbidden,
}

/// Every workspace-relative Rust file this guard reads, sorted, so findings
/// come out in the same order on every platform.
///
/// A crate directory that will not list is reported rather than skipped: a
/// green verdict over sources nobody opened is the failure this whole family
/// exists to remove.
fn sources(root: &Path, findings: &mut Vec<Finding>) -> Vec<String> {
    let mut paths = Vec::new();
    for name in HEADLESS_CRATES {
        let dir = root.join("crates").join(name).join("src");
        collect(&dir, root, &mut paths, findings);
    }
    paths.sort();
    paths
}

fn collect(dir: &Path, root: &Path, paths: &mut Vec<String>, findings: &mut Vec<Finding>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // A directory that is not there is not this guard's finding to
        // make. A renamed or removed crate is what `graph`'s coverage check
        // and `the_scanned_crates_are_the_ones_the_rule_names` are for, and
        // reporting it twice would give the tree two voices on one problem.
        // Every other error still speaks: a locked or unreadable directory
        // must not produce a green verdict over sources nobody opened.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            findings.push(Finding::new(
                format!(
                    "{}/: directory could not be listed: {e} — {RULE}",
                    relative_to(dir, root)
                ),
                REMEDY,
            ));
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, root, paths, findings);
            continue;
        }
        let relative = relative_to(&path, root);
        // Through `in_scope`, so the walk and `check_file` cannot disagree --
        // which they did while this arm carried its own `.rs` test.
        if in_scope(&relative) {
            paths.push(relative);
        }
    }
}

/// A workspace-relative path with forward slashes, so a finding reads the
/// same on either platform.
fn relative_to(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Whether a workspace-relative path is one this guard reads.
///
/// The single owner of that question, called by the walk and by
/// [`check_file`], so the whole-repo run and the edit-time hook can never
/// disagree about scope.
///
/// A path under a `tests/` directory, and a `*_tests.rs` file, are out of
/// scope, for the reason this module's own header gives: test code never
/// ships, so reporting it would teach an author to write fewer tests.
/// [`size::production_source`] already drops an *inline* `#[cfg(test)]`
/// module, which covered every test in the repository while every test lived
/// inside its host. A test module that moves out to `<host>/tests/mod.rs` is
/// a whole file the slice cannot see into, and without this the guard would
/// read a `#[test]` fn that reaches `Instant::now` as shipping production
/// code -- a finding against a file compiled out of the binary entirely.
///
/// Both spellings, because both are this repository's: `app/tests/` holds
/// twelve `*_tests.rs` files beside its `mod.rs`. [`crate::cycle::module_of`]
/// is the existing owner of that pair and the rule copied here; excluding
/// only the directory would leave the guard blind to the other half the day
/// a headless crate splits a suite the way `app` did.
fn in_scope(relative: &str) -> bool {
    relative.ends_with(".rs")
        && !is_test_path(relative)
        && HEADLESS_CRATES
            .iter()
            .any(|name| relative.starts_with(&format!("crates/{name}/src/")))
}

/// Whether a crate-relative path holds test code rather than shipping source.
///
/// The same pair [`crate::cycle::module_of`] excludes. Stated once here so
/// the two arms of `in_scope` read as one rule.
fn is_test_path(relative: &str) -> bool {
    relative.split('/').any(|part| part == "tests") || relative.ends_with("_tests.rs")
}

/// Every hit in one file's production source.
///
/// The line numbers are the file's own, not the production slice's: an author
/// given the wrong line goes looking in the wrong place, and the slice drops
/// whole test modules out of the middle of a file.
///
/// Recovered from each retained line's byte offset rather than by walking the
/// file and the slice in step. Stepping looked simpler and was wrong: the
/// slice is a *subsequence*, and a blank line inside a test module matches the
/// next blank line of the slice, after which every number is off by the size
/// of the module. Offsets cannot desync, and they keep
/// [`size::production_source`] the one owner of where test code starts.
fn hits(source: &str, path: &str) -> Vec<Hit> {
    let base = source.as_ptr() as usize;
    // Where each line of the file begins, strictly increasing.
    let starts: Vec<usize> = source
        .lines()
        .map(|line| line.as_ptr() as usize - base)
        .collect();
    let mut found = Vec::new();
    for line in size::production_source(source) {
        let offset = line.as_ptr() as usize - base;
        let number = starts.partition_point(|start| *start <= offset);
        let tokens = identifiers(without_comments(line));
        for forbidden in FORBIDDEN {
            if contains_sequence(&tokens, forbidden.identifiers) {
                found.push(Hit {
                    path: path.to_owned(),
                    line: number,
                    forbidden,
                });
            }
        }
    }
    found
}

/// A line with its comment removed, quotes respected.
///
/// Tracking the quote state costs ten lines and buys the direction the
/// approximation errs in: without it, a `//` inside a string literal would
/// blind the scan to everything after it on that line, which is an
/// under-report — the one direction a guard may not be wrong in.
///
/// Block comments are not tracked. That is a false positive on paper and none
/// in practice — this workspace's Rust holds a handful of block comments, none
/// of them naming a forbidden identifier — and the alternative is carrying
/// comment state across lines, which is a parser, in the crate that is allowed
/// no dependencies. If one ever appears, the finding says which line and the
/// fix is to reword it or to sign it.
fn without_comments(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if in_string => index += 1,
            // A char literal holding a quote -- `'"'`, and `b'"'` -- would
            // otherwise flip the string state and leave the scanner inside a
            // string it never entered, so a `//` after it on the same line
            // stops being stripped and the comment is scanned as code. Eight
            // lines in the headless crates carry one today, `control`'s
            // canonical JSON writer and `pine`'s lexer among them; the scan
            // was clean only because none of them also held a comment naming
            // a forbidden identifier.
            //
            // Only a literal that actually closes is consumed: `'a` is a
            // lifetime, and swallowing it would be the same desync wearing
            // the other hat.
            b'\'' if !in_string => {
                let after_escape = if bytes.get(index + 1) == Some(&b'\\') {
                    index + 2
                } else {
                    index + 1
                };
                if bytes.get(after_escape + 1) == Some(&b'\'') {
                    index = after_escape + 1;
                }
            }
            b'"' => in_string = !in_string,
            b'/' if !in_string && bytes.get(index + 1) == Some(&b'/') => return &line[..index],
            _ => {}
        }
        index += 1;
    }
    line
}

/// The identifiers on a line, in order. Everything else — punctuation,
/// operators, the `::` between two halves of a path — is separator.
fn identifiers(line: &str) -> Vec<&str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|token| !token.is_empty())
        .collect()
}

/// Whether `needle` appears as consecutive elements of `tokens`.
fn contains_sequence(tokens: &[&str], needle: &[&str]) -> bool {
    if needle.is_empty() || needle.len() > tokens.len() {
        return false;
    }
    tokens.windows(needle.len()).any(|window| window == needle)
}

/// Turn one hit into its finding.
fn finding(hit: &Hit) -> Finding {
    Finding::new(
        format!(
            "{}:{}: reaches `{}` — {} in a crate the headless rule names — {RULE}",
            hit.path, hit.line, hit.forbidden.name, hit.forbidden.because
        ),
        REMEDY,
    )
}

/// Every unsigned hit in the headless crates, plus every stale or malformed
/// allowlist entry.
pub fn check(root: &Path) -> Vec<Finding> {
    let (allowed, mut findings) = allowlist(root);
    let mut used = vec![false; allowed.len()];
    let paths = sources(root, &mut findings);

    for path in &paths {
        let Ok(source) = fs::read_to_string(root.join(path)) else {
            findings.push(Finding::new(
                format!("{path}: could not be read — {RULE}"),
                REMEDY,
            ));
            continue;
        };
        for hit in hits(&source, path) {
            match allowed
                .iter()
                .position(|a| a.path == hit.path && a.identifier == hit.forbidden.name)
            {
                Some(index) => used[index] = true,
                None => findings.push(finding(&hit)),
            }
        }
    }

    for (entry, was_used) in allowed.iter().zip(used) {
        if was_used {
            continue;
        }
        findings.push(Finding::new(
            format!(
                "{ALLOWLIST_FILE}:{}: allows `{}` in {}, which no longer reaches it — {RULE}",
                entry.line, entry.identifier, entry.path
            ),
            REMEDY_STALE,
        ));
    }
    findings
}

/// The same scan for one file, for the edit-time hook.
///
/// Stale allowlist entries are deliberately not reported here: the file being
/// edited says nothing about whether some *other* file still needs its
/// exception, and a hook that accused an author of an unrelated stale line
/// after every write is a hook that gets turned off. The whole-repo run owns
/// that half.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if !in_scope(relative) {
        return Vec::new();
    }
    let (allowed, mut findings) = allowlist(root);
    findings.retain(|f| f.remedy != REMEDY_STALE);
    let Ok(source) = fs::read_to_string(root.join(relative)) else {
        findings.push(Finding::new(
            format!("{relative}: could not be read — {RULE}"),
            REMEDY,
        ));
        return findings;
    };
    for hit in hits(&source, relative) {
        if !allowed
            .iter()
            .any(|a| a.path == hit.path && a.identifier == hit.forbidden.name)
        {
            findings.push(finding(&hit));
        }
    }
    findings
}

/// How many findings the repository carries today. [`crate::report`] prints
/// it; the number is expected to be zero and is not ratcheted, because a
/// number to negotiate with is what would let the rule drift back.
pub fn findings(root: &Path) -> usize {
    check(root).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch_dir::ScratchDir;

    /// A scratch workspace holding one source file in one headless crate.
    fn workspace(test: &str, file: &str, source: &str) -> ScratchDir {
        let root = ScratchDir::new(test);
        let path = root.join(file);
        fs::create_dir_all(path.parent().expect("the file has a directory"))
            .expect("scratch dirs are creatable");
        fs::write(&path, source).expect("the source is writable");
        root
    }

    fn lines(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.line.as_str()).collect()
    }

    #[test]
    fn a_clock_read_in_a_headless_crate_is_a_finding_that_names_the_rule() {
        let root = workspace(
            "headless-clock",
            "crates/sim/src/fill.rs",
            "fn fill() {\n    let t = std::time::SystemTime::now();\n}\n",
        );
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "one clock read: {:#?}", lines(&findings));
        assert_eq!(
            findings[0].line,
            format!(
                "crates/sim/src/fill.rs:2: reaches `SystemTime::now` — the wall clock in a crate \
                 the headless rule names — {RULE}"
            ),
            "the finding names the file, the line, the identifier and the rule"
        );
    }

    /// The five other clauses of the sentence, each on its own line, so a
    /// needle that stops matching is a named failure rather than a quiet one.
    #[test]
    fn every_forbidden_identifier_is_found() {
        let source = "\
use tokio::runtime;
async fn go() {}
fn t() { std::thread::spawn(|| {}); }
fn u() { let a = std::time::Instant::now(); }
fn v() { let b = std::time::SystemTime::now(); }
fn w(c: egui::Color32) {}
fn x(d: eframe::Frame) {}
fn y() { let e = std::collections::HashMap::new(); }
fn z() { let f = std::collections::HashSet::new(); }
";
        let root = workspace("headless-all", "crates/engine/src/lib.rs", source);
        let findings = check(root.path());
        let found = lines(&findings);
        assert_eq!(
            found.len(),
            FORBIDDEN.len(),
            "one per forbidden identifier: {found:#?}"
        );
        for forbidden in FORBIDDEN {
            assert!(
                found
                    .iter()
                    .any(|line| line.contains(&format!("reaches `{}`", forbidden.name))),
                "`{}` was not found: {found:#?}",
                forbidden.name
            );
        }
    }

    /// A thread spawned through an imported `thread` is the same call as one
    /// spelled in full, and both are hits. Matching the call rather than one
    /// way of writing it is the rule this guard shares with `scratch`.
    #[test]
    fn a_shortened_spelling_is_the_same_hit() {
        let root = workspace(
            "headless-spelling",
            "crates/trading/src/lib.rs",
            "use std::thread;\nfn go() { thread::spawn(|| {}); }\n",
        );
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "{:#?}", lines(&findings));
        assert!(findings[0].line.contains("crates/trading/src/lib.rs:2"));
    }

    /// The false positive that made a substring scan unusable: `pine` refuses
    /// `timeframe.*` and says so in prose and in error text on many lines.
    #[test]
    fn a_word_that_merely_contains_a_needle_is_not_a_hit() {
        let root = workspace(
            "headless-timeframe",
            "crates/pine/src/error.rs",
            "const M: &str = \"timeframe.* is not supported: bars are not on a clock\";\n\
             fn f() -> &'static str { \"no timeframes here\" }\n",
        );
        assert_eq!(check(root.path()), Vec::new());
    }

    /// Documentation stays free to name what the rule forbids.
    #[test]
    fn a_forbidden_identifier_inside_a_comment_is_not_a_hit() {
        let root = workspace(
            "headless-comment",
            "crates/indicators/src/output.rs",
            "//! Nothing here depends on egui, a runtime or Instant::now.\n\
             /// A color, UI-toolkit-agnostic (this crate never depends on egui).\n\
             fn c() -> u32 { 0 } // no HashMap either\n",
        );
        assert_eq!(check(root.path()), Vec::new());
    }

    /// A char literal holding a quote must not leave the scanner believing it
    /// is inside a string: the `//` after it would stop being stripped, and
    /// the comment would be scanned as code. Eight lines in the headless
    /// crates carry one today.
    #[test]
    fn a_char_literal_holding_a_quote_does_not_desync_the_scanner() {
        let root = workspace(
            "headless-char-literal",
            "crates/control/src/canonical.rs",
            "fn w(o: &mut String) { o.push('\"'); } // no Instant::now here\n\
             fn b(c: u8) -> bool { c == b'\"' } // nor egui\n",
        );
        assert_eq!(check(root.path()), Vec::new());
    }

    /// The converse: a lifetime is not a char literal and must not be
    /// swallowed, or the same desync arrives wearing the other hat.
    #[test]
    fn a_lifetime_is_not_read_as_a_char_literal() {
        let root = workspace(
            "headless-lifetime",
            "crates/engine/src/lib.rs",
            "fn f<'a>(s: &'a str) -> &'a str { let t = Instant::now(); s }\n",
        );
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "{:#?}", lines(&findings));
        assert!(findings[0].line.contains("Instant::now"));
    }

    /// A `//` inside a string literal must not blind the scan to the rest of
    /// the line: that would be an under-report, the one direction a guard may
    /// not be wrong in.
    #[test]
    fn a_slash_pair_inside_a_string_does_not_hide_the_rest_of_the_line() {
        let root = workspace(
            "headless-url",
            "crates/control/src/lib.rs",
            "fn f() { let u = \"https://example.test\"; let t = Instant::now(); }\n",
        );
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "{:#?}", lines(&findings));
        assert!(findings[0].line.contains("Instant::now"));
    }

    /// Test code is not what the rule forbids: it never ships. The bench
    /// harness and the test shorthand that stand in for a caller's argument
    /// are exactly the shape `orderflow` carries today.
    #[test]
    fn a_clock_read_inside_a_test_module_is_not_a_hit() {
        let root = workspace(
            "headless-test-module",
            "crates/orderflow/src/engine.rs",
            "fn project_at(now: Instant) {}\n\
             \n\
             #[cfg(test)]\n\
             mod tests {\n\
             fn project() { project_at(Instant::now()); }\n\
             }\n",
        );
        assert_eq!(check(root.path()), Vec::new());
    }

    /// The line number is the file's own, not the production slice's — an
    /// author sent to the wrong line looks in the wrong place.
    #[test]
    fn the_line_number_survives_a_test_module_in_the_middle() {
        let root = workspace(
            "headless-line-number",
            "crates/replay/src/lib.rs",
            "fn a() {}\n\
             #[cfg(test)]\n\
             mod early {\n\
             fn b() {}\n\
             }\n\
             fn c() { let t = Instant::now(); }\n",
        );
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "{:#?}", lines(&findings));
        assert!(
            findings[0].line.starts_with("crates/replay/src/lib.rs:6:"),
            "the sixth line of the file: {}",
            findings[0].line
        );
    }

    /// A signed entry silences its own hit and nothing else.
    #[test]
    fn a_signed_entry_silences_exactly_its_own_hit() {
        let root = workspace(
            "headless-allowed",
            "crates/replay/src/scratch.rs",
            "fn dir() { let n = std::time::SystemTime::now(); }\n",
        );
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::write(
            root.join(ALLOWLIST_FILE),
            "# a comment\ncrates/replay/src/scratch.rs SystemTime::now test-only helper; seeds a \
             directory name, never a result\n",
        )
        .expect("the allowlist is writable");
        assert_eq!(check(root.path()), Vec::new());

        // The same signature does not cover a different file.
        fs::write(
            root.join("crates/replay/src/other.rs"),
            "fn dir() { let n = std::time::SystemTime::now(); }\n",
        )
        .expect("the source is writable");
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "{:#?}", lines(&findings));
        assert!(
            findings[0]
                .line
                .starts_with("crates/replay/src/other.rs:1:")
        );
    }

    #[test]
    fn an_entry_that_no_longer_matches_anything_is_itself_a_finding() {
        let root = workspace("headless-stale", "crates/sim/src/lib.rs", "fn a() {}\n");
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::write(
            root.join(ALLOWLIST_FILE),
            "crates/sim/src/lib.rs Instant::now a reason nobody needs any more\n",
        )
        .expect("the allowlist is writable");
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "{:#?}", lines(&findings));
        assert!(
            findings[0].line.contains("which no longer reaches it")
                && findings[0].line.ends_with(RULE),
            "the finding names the stale entry and the rule: {}",
            findings[0].line
        );
    }

    #[test]
    fn an_entry_with_no_reason_is_refused() {
        let root = workspace("headless-unsigned", "crates/sim/src/lib.rs", "fn a() {}\n");
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::write(
            root.join(ALLOWLIST_FILE),
            "crates/sim/src/lib.rs Instant::now\ncrates/sim/src/lib.rs Elapsed::now a reason\n",
        )
        .expect("the allowlist is writable");
        let findings = check(root.path());
        let found = lines(&findings);
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(
            found[0].contains("is not `<path> <identifier> <reason>`"),
            "the unsigned line: {}",
            found[0]
        );
        assert!(
            found[1].contains("which this guard does not scan for"),
            "the unknown identifier: {}",
            found[1]
        );
    }

    /// A missing allowlist is an empty allowlist, not an error: refusing to
    /// run without the file would make deleting it the way past the guard.
    #[test]
    fn a_missing_allowlist_still_reports_the_hits() {
        let root = workspace(
            "headless-no-allowlist",
            "crates/strategy/src/lib.rs",
            "fn a() { let t = Instant::now(); }\n",
        );
        assert_eq!(check(root.path()).len(), 1);
    }

    #[test]
    fn the_hook_reads_headless_sources_and_nothing_else() {
        assert!(in_scope("crates/engine/src/lib.rs"));
        assert!(in_scope("crates/control-local/src/client.rs"));
        // A test module that moved out of its host is still test code, in
        // either spelling the repository uses for one.
        assert!(!in_scope("crates/orderflow/src/projection/tests/mod.rs"));
        assert!(!in_scope("crates/engine/src/tests/bars.rs"));
        assert!(!in_scope("crates/orderflow/src/projection_tests.rs"));
        assert!(!in_scope("crates/engine/src/bars/golden_tests.rs"));
        assert!(!in_scope("crates/app/src/app.rs"));
        assert!(!in_scope("crates/feed/src/lib.rs"));
        assert!(!in_scope("crates/feed-mt5/src/lib.rs"));
        assert!(!in_scope("crates/backtest/src/main.rs"));
        assert!(!in_scope("crates/mcp/src/link.rs"));
        assert!(!in_scope("crates/engine/tests/golden.rs"));
        assert!(!in_scope("crates/engine/src/notes.md"));
    }

    /// The hook answers about the file in front of it and never accuses its
    /// author of an unrelated stale line elsewhere.
    #[test]
    fn the_hook_does_not_report_a_stale_entry_for_another_file() {
        let root = workspace(
            "headless-hook-stale",
            "crates/sim/src/lib.rs",
            "fn a() { let t = Instant::now(); }\n",
        );
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::write(
            root.join(ALLOWLIST_FILE),
            "crates/trading/src/gone.rs Instant::now a file that no longer exists\n",
        )
        .expect("the allowlist is writable");
        let findings = check_file(root.path(), "crates/sim/src/lib.rs");
        assert_eq!(findings.len(), 1, "{:#?}", lines(&findings));
        assert!(findings[0].line.contains("crates/sim/src/lib.rs:1:"));
    }

    /// The crates named in `CLAUDE.md`'s headless sentence, from the list it
    /// opens with `That is` and closes before `, and it binds`.
    ///
    /// Read out of the sentence rather than trusted, because the check below
    /// runs both ways and a one-way check is what let this guard be written
    /// with a hole in it: asserting only that every scanned crate is named
    /// leaves a crate *added to the sentence* and forgotten here silently
    /// unscanned — unguarded, which looks green and is worse than a failure.
    /// That is the same reason `graph`'s `every_crate_is_covered` exists.
    fn crates_the_sentence_names(sentence: &str) -> Vec<String> {
        let list = sentence
            .split_once("That is ")
            .expect("the headless sentence introduces its crate list with `That is`")
            .1
            .split_once(", and it binds")
            .expect("the crate list ends before `, and it binds`")
            .0;
        list.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    /// The list of crates is `CLAUDE.md`'s list, in both directions.
    #[test]
    fn the_scanned_crates_are_the_ones_the_rule_names() {
        let doc = fs::read_to_string(crate::workspace_root().join("CLAUDE.md"))
            .expect("CLAUDE.md is readable");
        let sentence = doc
            .lines()
            .find(|line| line.contains("Everything below") && line.contains("is headless"))
            .expect("CLAUDE.md states the headless rule on one line");
        let named = crates_the_sentence_names(sentence);

        for name in HEADLESS_CRATES {
            assert!(
                named.iter().any(|listed| listed == name),
                "the headless sentence does not name `{name}`, which this guard scans"
            );
        }
        for name in &named {
            assert!(
                HEADLESS_CRATES.contains(&name.as_str()),
                "`{name}` is named by the headless sentence and is not in HEADLESS_CRATES, so                  nothing scans it -- add it there, or take it out of the sentence"
            );
        }
    }

    /// The repository this crate ships in obeys the rule, with every
    /// remaining site signed. Zero from the first commit, and not ratcheted.
    #[test]
    fn this_workspace_is_headless() {
        assert_eq!(check(&crate::workspace_root()), Vec::new());
    }
}
