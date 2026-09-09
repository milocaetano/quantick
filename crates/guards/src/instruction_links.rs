//! File-existence checks for local Markdown links in instruction entrypoints.
//! This is not an anchor linter. Inline links and reference definitions are
//! checked; code examples, remote URLs and mission evidence are excluded.

use std::fs;
use std::path::Path;

use crate::Finding;

const REMEDY: &str = "Restore the linked instruction file or update its relative Markdown link in the same change. Paths resolve from the document containing the link.";
const ROOT_FILES: [&str; 4] = [
    "CLAUDE.md",
    "AGENTS.md",
    "CONTRIBUTING.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
];
const DIRS: [&str; 3] = [".claude/skills", ".agents", "docs"];

/// Scan the bounded instruction and documentation trees.
pub fn check(root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in ROOT_FILES {
        scan_file(root, &root.join(file), &mut findings);
    }
    for dir in DIRS {
        walk(root, &root.join(dir), &mut findings);
    }
    findings.sort_by(|a, b| a.line.cmp(&b.line));
    findings
}

/// Rescan incoming links even when the edited target has been removed.
pub fn check_file(root: &Path, _relative: &str) -> Vec<Finding> {
    check(root)
}

fn walk(root: &Path, dir: &Path, findings: &mut Vec<Finding>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            findings.push(Finding::new(
                format!("  {}: cannot inspect links: {error}", dir.display()),
                REMEDY,
            ));
            return;
        }
    };
    for entry in entries {
        let result = entry.and_then(|entry| Ok((entry.path(), entry.file_type()?)));
        match result {
            Ok((path, kind)) if kind.is_dir() => {
                // Do not follow symlinks or recurse into historical evidence.
                if path != root.join("docs/workflow/evidence") {
                    walk(root, &path, findings);
                }
            }
            Ok((path, kind))
                if kind.is_file() && path.extension().is_some_and(|ext| ext == "md") =>
            {
                scan_file(root, &path, findings)
            }
            Ok(_) => {}
            Err(error) => findings.push(Finding::new(
                format!("  {}: cannot inspect entry: {error}", dir.display()),
                REMEDY,
            )),
        }
    }
}

fn scan_file(root: &Path, path: &Path, findings: &mut Vec<Finding>) {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            findings.push(Finding::new(
                format!("  {relative}: cannot read links: {error}"),
                REMEDY,
            ));
            return;
        }
    };
    let mut fence: Option<(char, usize)> = None;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().unwrap_or(' ');
        let run = trimmed.chars().take_while(|c| *c == marker).count();
        if matches!(marker, '`' | '~') && run >= 3 {
            if let Some((open, length)) = fence {
                if marker == open && run >= length && trimmed[run..].trim().is_empty() {
                    fence = None;
                }
            } else {
                fence = Some((marker, run));
            }
            continue;
        }
        if fence.is_some() || line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        for destination in destinations(line) {
            let target = destination.split(['#', '?']).next().unwrap_or("");
            if target.is_empty() || target.starts_with('/') || target.contains(':') {
                continue;
            }
            let target = target.replace("%20", " ");
            if !path
                .parent()
                .expect("document has parent")
                .join(&target)
                .exists()
            {
                findings.push(Finding::new(
                    format!(
                        "  {relative}:{}: missing relative link target `{target}`",
                        index + 1
                    ),
                    REMEDY,
                ));
            }
        }
    }
}

// Skip code spans outside labels; a code-formatted label is still a link.
fn destinations(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
            let delimiter = &line[i..i + run];
            i += run;
            if let Some(end) = line[i..].find(delimiter) {
                i += end + run;
            }
        } else if bytes[i] == b'[' {
            let Some(end) = line[i + 1..].find(']') else {
                break;
            };
            let after = i + end + 2;
            if (bytes.get(after) == Some(&b'(')
                || (bytes.get(after) == Some(&b':') && line[..i].trim().is_empty()))
                && let Some(target) = destination(line[after + 1..].trim_start())
            {
                found.push(target);
            }
            i = after;
        } else {
            i += 1;
        }
    }
    found
}

fn destination(rest: &str) -> Option<&str> {
    if let Some(angle) = rest.strip_prefix('<') {
        return angle.find('>').map(|end| &angle[..end]);
    }
    let mut depth = 0;
    let mut end = rest.len();
    for (index, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ')' => {
                end = index;
                break;
            }
            c if c.is_whitespace() => {
                end = index;
                break;
            }
            _ => {}
        }
    }
    (end > 0).then_some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> crate::scratch_dir::ScratchDir {
        let root = crate::scratch_dir::ScratchDir::new(name);
        for dir in DIRS {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::create_dir_all(root.join(".github")).unwrap();
        for file in ROOT_FILES {
            fs::write(root.join(file), "").unwrap();
        }
        fs::create_dir_all(root.join("docs/workflow")).unwrap();
        fs::write(root.join("docs/workflow/delivery.md"), "Contract").unwrap();
        root
    }

    #[test]
    fn deletion_and_rename_break_incoming_contract_links() {
        let root = fixture("instruction-links-rename");
        fs::write(
            root.join("CLAUDE.md"),
            "[contract](docs/workflow/delivery.md#validation)",
        )
        .unwrap();
        assert!(check(&root).is_empty());
        fs::rename(
            root.join("docs/workflow/delivery.md"),
            root.join("docs/workflow/moved.md"),
        )
        .unwrap();
        let findings = check_file(&root, "docs/workflow/delivery.md");
        assert_eq!(findings.len(), 1);
        assert!(findings[0].line.contains("CLAUDE.md:1"));
        fs::write(root.join("CLAUDE.md"), "[contract](docs/workflow/moved.md)").unwrap();
        assert!(check(&root).is_empty());
        fs::remove_file(root.join("docs/workflow/moved.md")).unwrap();
        assert_eq!(check(&root).len(), 1);
    }

    #[test]
    fn each_surface_resolves_from_its_own_directory() {
        let root = fixture("instruction-links-relative");
        for file in [
            "CONTRIBUTING.md",
            ".github/PULL_REQUEST_TEMPLATE.md",
            ".claude/skills/guide.md",
            ".agents/guide.md",
            "docs/guide.md",
        ] {
            fs::write(root.join(file), "[missing](absent.md)").unwrap();
        }
        assert_eq!(check(&root).len(), 5);
        fs::write(
            root.join(".claude/skills/guide.md"),
            "[contract](../../docs/workflow/delivery.md)",
        )
        .unwrap();
        assert_eq!(check(&root).len(), 4);
        fs::write(
            root.join(".github/PULL_REQUEST_TEMPLATE.md"),
            "<!-- Read [contract](../docs/workflow/delivery.md). -->",
        )
        .unwrap();
        assert_eq!(check(&root).len(), 3);
    }

    #[test]
    fn examples_remote_links_and_evidence_are_not_dependencies() {
        let root = fixture("instruction-links-examples");
        fs::write(root.join("CLAUDE.md"), "[remote](https://example.com/missing.md) [anchor](#local)\n`[example](missing.md)`\n```md\n[example](missing.md)\n```\n~~~md\n[example](missing.md)\n~~~\n    [example](missing.md)\n").unwrap();
        fs::create_dir_all(root.join("docs/workflow/evidence")).unwrap();
        fs::write(
            root.join("docs/workflow/evidence/review.md"),
            "[historical](missing.md)",
        )
        .unwrap();
        assert!(check(&root).is_empty());
    }

    #[test]
    fn a_missing_entrypoint_or_source_tree_is_not_a_clean_scan() {
        let root = fixture("instruction-links-missing-source");
        fs::remove_file(root.join("CONTRIBUTING.md")).unwrap();
        fs::remove_dir(root.join(".agents")).unwrap();
        let findings = check(&root);
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().any(|f| f.line.contains("CONTRIBUTING.md")));
        assert!(findings.iter().any(|f| f.line.contains(".agents")));
    }

    #[test]
    fn reference_definitions_titles_angles_and_code_labels_are_checked() {
        assert_eq!(
            destinations("[`contract`](../delivery.md#section) [other](<a b.md> \"Title\")"),
            vec!["../delivery.md#section", "a b.md"]
        );
        assert_eq!(
            destinations("[contract]: delivery.md \"Contract\""),
            vec!["delivery.md"]
        );
        assert_eq!(destinations("[contract](name(v2).md)"), vec!["name(v2).md"]);
    }
}
