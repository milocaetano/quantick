//! Grep guard for the repository's one language.
//!
//! `CLAUDE.md` states the rule and owns its scope: everything written into a
//! tracked file is English. Nothing the compiler does can see a Portuguese
//! comment — fmt, clippy, build and the whole suite stay green while half the
//! codebase becomes unreadable to the next contributor — so the rule is
//! enforced here, the way [`crate::encoding`] enforces an encoding the
//! compiler also cannot see.
//!
//! Detection is by **word**, not by accented character. A blanket
//! accented-letter scan was tried first and rejected: it fires on `López de
//! Prado`, cited by name in `engine/src/imbalance.rs`, `feed-mt5/src/map.rs`
//! and `replay/examples/imbalance_audit.rs`, and on
//! `control/src/canonical.rs`, whose test proves UTF-8 is *not* escaped by
//! round-tripping `{"é": "naïve"}`. Neither is a language violation — one is
//! a person's name and the other is the data under test — so the guard looks
//! for Portuguese vocabulary instead, in both its accented and unaccented
//! spellings.
//!
//! Words are compared after two normalisations a shell one-liner does not do:
//!
//! 1. **Unicode-aware lowercasing.** `grep -i` does not case-fold multi-byte
//!    characters in this environment (GNU grep 3.0, `LC_CTYPE=C.UTF-8`), so
//!    `PREÇO` walks straight past the obvious recipe while `preço` is caught.
//!    Rust's `to_lowercase` folds both.
//! 2. **Splitting on `_` and on camelCase humps**, so `preco_medio` and
//!    `precoMedio` are both caught. A `grep -w` sees neither, because `_` is
//!    a word character and a camelCase hump is no boundary at all.
//!
//! What predates the rule is grandfathered in [`ALLOWED`], listed rather than
//! silently skipped so the debt stays visible and cannot grow.

use std::fs;
use std::path::Path;

use crate::Finding;

/// Portuguese vocabulary distinctive enough to be safe as whole words in an
/// English codebase, in both spellings a keyboard produces. Deliberately
/// short: a word that could plausibly turn up inside English prose or an
/// identifier — `para`, `com`, `no`, `da` — is left out, because a guard that
/// cries wolf is a guard somebody disables. It catches the common case, not
/// every case; reading the prose is still dimension 8's job.
const KEYWORDS: &[&str] = &[
    "não", "nao", "então", "entao", "também", "tambem", "porque", "você", "voce", "isso", "aqui",
    "quando", "arquivo", "pasta", "senha", "usuário", "usuario", "preço", "preco", "tela", "erro",
    "botão", "botao", "janela", "altura", "largura", "gráfico", "grafico", "precisa", "mensagem",
    "tamanho", "linha", "barra",
];

// Not listed, and worth saying why: `índice`. B3 names its own contracts
// *mini índice* (WIN) and *mini dólar* (WDO), so the word is a proper noun in
// this domain — `crates/app/config/feeds.toml` and
// `feed-mt5/tests/fixture_replay.rs` both carry it correctly. Renaming a real
// financial product to keep a guard quiet is the wrong trade.

/// Directories scanned, relative to the workspace root. Product code, the
/// scripts shipped with it, the prose that documents it, and the operating
/// instructions agents read. `.claude/GOAL-archive-*.md` is deliberately out
/// of scope: those are session records of work already done, not artifacts the
/// next contributor has to read.
const SCANNED_DIRS: &[&str] = &["crates", "docs", ".claude/skills", ".claude/hooks"];

/// Extensions worth scanning inside those directories.
///
/// `.txt` is here because of `crates/guards/size-baseline.txt`. Sixty lines of
/// hand-written rationale moved into it when the size ceilings left Rust for a
/// data file, and those comments are the signed justifications the whole
/// ratchet doctrine rests on — exactly the "comments inside config files"
/// `CLAUDE.md` names. Without this the branch that wrote them put them
/// somewhere no guard opens.
const SCANNED_EXTS: &[&str] = &["rs", "pine", "md", "html", "toml", "txt"];

/// Paths that already carried non-English prose when this guard was written.
/// Grandfathered by `CLAUDE.md`'s rule, which grades the lines a diff authors.
/// Translating any of them is welcome as its own change; until then they are
/// named here so the debt is visible and bounded.
const ALLOWED: &[&str] = &[
    // This guard itself. Its word list and its own test fixtures are the
    // mechanism, not prose — the same exemption `CLAUDE.md` grants a
    // localisation resource.
    "crates/guards/src/language.rs",
    // A full UX specification written in Portuguese, ~46 non-English lines.
    "docs/ux/drawing-tools-ux-spec.html",
    // Two doc comments quoting the trader's own bug reports verbatim. These
    // are exempt under the quotation rule anyway — translating a report would
    // put words in the reporter's mouth — and are listed so a future reader
    // does not have to rediscover why.
    "crates/app/src/app.rs",
    "crates/app/src/drawings/fib.rs",
];

/// Split an identifier or sentence into comparable words: on anything that is
/// not alphanumeric, and again at each camelCase hump. `preco_medio`,
/// `precoMedio` and `preco medio` all yield `preco`.
pub fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in line.chars() {
        if ch.is_alphanumeric() {
            if prev_lower && ch.is_uppercase() && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.extend(ch.to_lowercase());
            prev_lower = ch.is_lowercase();
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            prev_lower = false;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn scan(dir: &Path, root: &Path, violations: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            // `target/` holds vendored crate sources in every language there
            // is; it is build output, not this repository's writing.
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            scan(&path, root, violations);
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if !in_scope(&relative) {
            continue;
        }
        inspect(&path, &relative, violations);
    }
}

/// Whether a workspace-relative path is one this guard reads. The single
/// owner of that question, called by the walker and by [`check_file`], so the
/// suite and the edit-time hook can never disagree about what is in scope —
/// the hook used to report Portuguese inside `target/`, which the whole-repo
/// scan skips as build output, leaving an author an advisory that running the
/// suite could not clear.
fn in_scope(relative: &str) -> bool {
    SCANNED_DIRS
        .iter()
        .any(|dir| relative.starts_with(&format!("{dir}/")))
        && !relative.split('/').any(|part| part == "target")
        && relative
            .rsplit_once('.')
            .is_some_and(|(_, ext)| SCANNED_EXTS.contains(&ext))
}

/// The per-file half of the scan, shared with [`check_file`] so the
/// whole-repo run and the edit-time hook read the same file the same way.
fn inspect(path: &Path, relative: &str, violations: &mut Vec<String>) {
    if ALLOWED.contains(&relative) {
        return;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for (line_no, line) in text.lines().enumerate() {
        if let Some(word) = words(line)
            .into_iter()
            .find(|w| KEYWORDS.contains(&w.as_str()))
        {
            violations.push(format!(
                "{relative}:{}: `{word}` — this repo writes in English",
                line_no + 1
            ));
        }
    }
}

/// What the guard asks for beyond the list of violations.
pub const REMEDY: &str = "See the English rule in CLAUDE.md; if the foreign text is the data — a \
                          localisation resource, a fixture reproducing a real system's string, an \
                          attributed quotation — say so in a comment and add the path to ALLOWED \
                          in crates/guards/src/language.rs.";

/// Every non-English word found in a scanned file.
pub fn check(root: &Path) -> Vec<Finding> {
    let mut violations = Vec::new();
    for dir in SCANNED_DIRS {
        scan(&root.join(dir), root, &mut violations);
    }
    // One class of violation, one remedy: every finding this guard raises is
    // fixed the same way, so the mapping is a wrap rather than a decision.
    violations
        .into_iter()
        .map(|v| Finding::new(v, REMEDY))
        .collect()
}

/// The same check for one file. A path outside the scanned directories, or
/// with an extension the guard does not read, reports nothing.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if !in_scope(relative) {
        return Vec::new();
    }
    let mut violations = Vec::new();
    inspect(&root.join(relative), relative, &mut violations);
    // One class of violation, one remedy: every finding this guard raises is
    // fixed the same way, so the mapping is a wrap rather than a decision.
    violations
        .into_iter()
        .map(|v| Finding::new(v, REMEDY))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ways a Portuguese word hides from a shell `grep -w`: inside a
    /// snake_case identifier, where `_` is a word character, and inside a
    /// camelCase one, where there is no boundary at all.
    #[test]
    fn words_splits_identifiers_a_word_boundary_match_would_miss() {
        assert!(words("let preco_medio = 2;").contains(&"preco".to_string()));
        assert!(words("let precoMedio = 2;").contains(&"preco".to_string()));
        assert!(words("fn erro_handler() {}").contains(&"erro".to_string()));
    }

    /// English words that merely start with a listed keyword must not match,
    /// which is what splitting on word boundaries rather than on substrings
    /// buys.
    #[test]
    fn words_does_not_split_inside_english_words() {
        assert!(!words("let error = compute();").contains(&"erro".to_string()));
        assert!(!words("a paragraph of prose").contains(&"para".to_string()));
    }

    /// `grep -i` does not case-fold multi-byte characters in this
    /// environment, so an accented uppercase spelling is exactly what a shell
    /// recipe loses. Rust folds it, and this pins the difference the guard
    /// exists to close.
    #[test]
    fn words_folds_accented_uppercase() {
        assert!(words("panic!(\"PREÇO INVÁLIDO\")").contains(&"preço".to_string()));
        assert!(words("log::error!(\"NÃO CONECTOU\")").contains(&"não".to_string()));
    }
}
