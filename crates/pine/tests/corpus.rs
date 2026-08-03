//! Conformance corpus: every `tests/corpus/ok/*.pine` must lex and parse;
//! every `tests/corpus/err/*.pine` must fail with exactly the codes and
//! lines its first-line directive declares:
//!
//! ```text
//! // expect: PINE_SYNTAX@3, PINE_SYNTAX@4
//! ```
//!
//! The directive lists *all* expected errors — multi-error collection is
//! part of the contract (a script with three problems reports three).

use std::fs;
use std::path::PathBuf;

use quantick_pine::{PineError, lexer, parser};

fn corpus_dir(kind: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(kind)
}

fn compile(source: &str) -> Result<(), Vec<PineError>> {
    let lexed = lexer::lex(source)?;
    parser::parse(&lexed.tokens)?;
    Ok(())
}

#[test]
fn every_ok_script_parses() {
    let mut checked = 0;
    for entry in fs::read_dir(corpus_dir("ok")).expect("ok corpus exists") {
        let path = entry.expect("dir entry").path();
        let source = fs::read_to_string(&path).expect("script is readable");
        if let Err(errors) = compile(&source) {
            let rendered: Vec<String> = errors
                .iter()
                .map(|e| e.render(&path.display().to_string(), &source))
                .collect();
            panic!("{} must parse:\n{}", path.display(), rendered.join("\n"));
        }
        checked += 1;
    }
    assert!(
        checked >= 5,
        "corpus should not silently shrink ({checked})"
    );
}

#[test]
fn every_err_script_fails_exactly_as_declared() {
    let mut checked = 0;
    for entry in fs::read_dir(corpus_dir("err")).expect("err corpus exists") {
        let path = entry.expect("dir entry").path();
        let source = fs::read_to_string(&path).expect("script is readable");
        let directive = source
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("// expect:"))
            .unwrap_or_else(|| panic!("{} lacks an `// expect:` directive", path.display()));
        let mut expected: Vec<(String, usize)> = directive
            .split(',')
            .map(|item| {
                let (code, line) = item
                    .trim()
                    .split_once('@')
                    .unwrap_or_else(|| panic!("bad directive item `{item}` in {}", path.display()));
                (code.to_owned(), line.parse::<usize>().expect("line number"))
            })
            .collect();

        let errors = compile(&source).expect_err(&format!("{} must fail", path.display()));
        let mut actual: Vec<(String, usize)> = errors
            .iter()
            .map(|e| {
                let (line, _) = e.span.line_col(&source);
                (e.code.as_str().to_owned(), line)
            })
            .collect();
        expected.sort();
        actual.sort();
        assert_eq!(
            actual,
            expected,
            "{}: errors differ from the directive.\nrendered:\n{}",
            path.display(),
            errors
                .iter()
                .map(|e| e.render(&path.display().to_string(), &source))
                .collect::<Vec<_>>()
                .join("\n")
        );
        checked += 1;
    }
    assert!(
        checked >= 5,
        "corpus should not silently shrink ({checked})"
    );
}
