//! Ignored tests carry a nonempty reason, including conditional attributes.

use std::{fs, path::Path};

use crate::{Finding, extension_boundary::lex};

const REMEDY: &str = "Give each ignored test an explicit nonempty reason in its ignore attribute.";

fn reason(literal: &str) -> bool {
    if literal.starts_with('r') {
        return literal.find('"').is_some_and(|start| {
            let end = literal.rfind('"').unwrap_or(start);
            !literal[start + 1..end].trim().is_empty()
        });
    }
    let Some(body) = literal.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return false;
    };
    let mut chars = body.chars().peekable();
    while let Some(mut ch) = chars.next() {
        if ch == '\\' {
            ch = match chars.next() {
                Some('n' | 'r' | 't') => ' ',
                Some('\n') => {
                    while chars.peek().is_some_and(|c| c.is_whitespace()) {
                        chars.next();
                    }
                    continue;
                }
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or(' ')
                }
                Some('u') => {
                    chars.next(); // opening brace
                    let hex: String = chars
                        .by_ref()
                        .take_while(|&c| c != '}')
                        .filter(|&c| c != '_')
                        .collect();
                    u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or(' ')
                }
                Some(c) => c,
                None => return false,
            };
        }
        if !ch.is_whitespace() {
            return true;
        }
    }
    false
}

fn inspect_attribute(parts: &[(&str, usize)], invalid: &mut Vec<usize>) {
    let parts = if parts
        .get(..2)
        .is_some_and(|p| p[0].0 == "r" && p[1].0 == "#")
    {
        &parts[2..]
    } else {
        parts
    };
    if parts.first().is_some_and(|p| p.0 == "ignore") {
        if parts.len() != 3 || parts[1].0 != "=" || !reason(parts[2].0) {
            invalid.push(parts[0].1);
        }
    } else if parts.first().is_some_and(|p| p.0 == "cfg_attr")
        && parts.get(1).is_some_and(|p| p.0 == "(")
        && parts.last().is_some_and(|p| p.0 == ")")
    {
        let mut depth = 0;
        let mut start = None;
        for i in 2..parts.len() - 1 {
            match parts[i].0 {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth -= 1,
                "," if depth == 0 => {
                    if let Some(begin) = start {
                        inspect_attribute(&parts[begin..i], invalid);
                    }
                    start = Some(i + 1);
                }
                _ => {}
            }
        }
        if let Some(begin) = start {
            inspect_attribute(&parts[begin..parts.len() - 1], invalid);
        }
    }
}

fn violations(source: &str) -> Result<Vec<usize>, String> {
    let tokens = lex::lex(source)?;
    let parts: Vec<_> = tokens
        .iter()
        .map(|token| (token.text.as_str(), token.line + 1))
        .collect();
    let mut invalid = Vec::new();
    let mut i = 0;
    while i + 1 < parts.len() {
        let bracket = i + if parts[i + 1].0 == "!" { 2 } else { 1 };
        if parts[i].0 == "#" && parts.get(bracket).is_some_and(|p| p.0 == "[") {
            let end = tokens[bracket].pair.ok_or("unclosed attribute")?;
            inspect_attribute(&parts[bracket + 1..end], &mut invalid);
            i = end;
        }
        i += 1;
    }
    Ok(invalid)
}

/// Check every Rust source, including test-only modules and integration tests.
pub fn check(root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut pending = vec![root.join("crates")];
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                findings.push(Finding::new(format!("{}: {error}", dir.display()), REMEDY));
                continue;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) if entry.path().is_dir() => pending.push(entry.path()),
                Ok(entry) => {
                    let path = entry.path();
                    match path.strip_prefix(root) {
                        Ok(relative) => findings.extend(check_file(
                            root,
                            &relative.to_string_lossy().replace('\\', "/"),
                        )),
                        Err(error) => findings
                            .push(Finding::new(format!("{}: {error}", path.display()), REMEDY)),
                    }
                }
                Err(error) => findings.push(Finding::new(error.to_string(), REMEDY)),
            }
        }
    }
    findings.sort_by(|a, b| a.line.cmp(&b.line));
    findings
}

/// Inspect a single source through the same edit-time guard contract.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if !relative.starts_with("crates/") || !relative.ends_with(".rs") {
        return Vec::new();
    }
    match fs::read_to_string(root.join(relative))
        .map_err(|error| error.to_string())
        .and_then(|source| violations(&source))
    {
        Ok(lines) => lines
            .into_iter()
            .map(|line| {
                Finding::new(
                    format!("{relative}:{line}: ignored test needs a nonempty reason"),
                    REMEDY,
                )
            })
            .collect(),
        Err(error) => vec![Finding::new(format!("{relative}: {error}"), REMEDY)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_tests_require_reasons() {
        for annotation in [
            "#[ignore]",
            "#![ignore]",
            "#[r#ignore]",
            "#[r#cfg_attr(test, r#ignore)]",
            "#[ignore = \"\"]",
            "#[ignore = \" \\n\\t\"]",
            "#[ignore = r#\"  \"#]",
            "#[ignore = \"\\u{20}\\x20\"]",
            "#[cfg_attr(windows, ignore)]",
            "#[cfg_attr(any(unix, windows), inline, ignore = \"\")]",
            "#[cfg_attr(unix, cfg_attr(test, ignore))]",
        ] {
            assert_eq!(violations(annotation).unwrap(), vec![1], "{annotation}");
        }
        for annotation in [
            "#[ignore = \"manual benchmark\"]",
            "#[ignore = r#\"live service\"#]",
            "#[cfg_attr(windows, ignore = \"needs Unix socket\")]",
            "#[cfg_attr(ignore, inline)]",
            "#[cfg_attr(test, cfg_attr(unix, ignore = \"quiet host\"))]",
            "// #[ignore]\n/* nested /* #[ignore] */ comment */",
            "let example = r##\"#[ignore]\"##;",
            "let example = \"#[ignore]\";",
            "let c = '\"'; #[ignore = \"manual\"]",
        ] {
            assert!(violations(annotation).unwrap().is_empty(), "{annotation}");
        }
        assert!(violations("#[ignore").is_err());
        assert_eq!(
            violations("#[cfg_attr(test, ignore, ignore = \"\")]").unwrap(),
            vec![1, 1]
        );
        assert_eq!(
            violations("#[cfg_attr(\n test,\n ignore\n)]").unwrap(),
            vec![3]
        );
    }

    #[test]
    fn unreadable_sources_fail_and_the_repository_has_reasons() {
        let root = crate::workspace_root();
        assert_eq!(check_file(&root, "crates/missing.rs").len(), 1);
        assert!(check(&root).is_empty(), "{:?}", check(&root));
    }
}
