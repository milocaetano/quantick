//! One explicit manifest owns family membership; every readback declaration must dock.
use std::{collections::BTreeSet, fs, path::Path};

use crate::{Finding, extension_boundary::lex};

const MANIFEST: &str = "crates/control/src/families.rs";
const AGGREGATE: &str = "crates/control-schema/src/retry_matrix/rows.rs";
const REMEDY: &str = "Dock each family once in control_families!, in strictly increasing ID order; declare every READBACKS owner in its readbacks binding.";

fn manifest(source: &str) -> Result<BTreeSet<String>, String> {
    let tokens = lex::lex(source)?;
    let start = tokens
        .windows(3)
        .position(|row| row[0].text == "macro_rules" && row[2].text == "control_families")
        .ok_or("missing control_families! declaration")?;
    let end = tokens[start + 3].pair.ok_or("unclosed family manifest")?;
    let mut previous = None;
    let mut readbacks = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    let mut index = start + 4;
    while index < end {
        let token = &tokens[index];
        if !token.text.starts_with('"') || tokens[index + 1].text != "{" {
            index += 1;
            continue;
        }
        let id = token.text.trim_matches('"');
        if previous.is_some_and(|previous| previous >= id) {
            return Err(format!("family `{id}` is duplicate or out of order"));
        }
        previous = Some(id);
        let row_end = tokens[index + 1].pair.ok_or("unclosed family row")?;
        let mut group = index + 2;
        while group < row_end {
            let name = &tokens[group].text;
            if tokens.get(group + 2).is_none_or(|token| token.text != "[") {
                return Err(format!("family `{id}` has a malformed `{name}` binding"));
            }
            let group_end = tokens[group + 2].pair.ok_or("unclosed binding group")?;
            for binding in &tokens[group + 3..group_end] {
                if binding.text == "," {
                    continue;
                }
                if !bindings.insert((name.clone(), binding.text.clone())) {
                    return Err(format!("duplicate {name} binding `{}`", binding.text));
                }
                if name == "readbacks" {
                    readbacks.insert(format!("crates/control-schema/src/{}.rs", binding.text));
                }
            }
            group = group_end + 2;
        }
        index = row_end + 1;
    }
    if previous.is_none() {
        return Err("family manifest has no rows".into());
    }
    Ok(readbacks)
}

fn readback_declarations(source: &str) -> Result<Vec<(String, bool)>, String> {
    let tokens = lex::lex(source)?;
    let mut declarations = Vec::new();
    for (index, pair) in tokens.windows(2).enumerate() {
        if !matches!(pair[0].text.as_str(), "const" | "static") || pair[1].text != "READBACKS" {
            continue;
        }
        let mut module = String::new();
        for (start, head) in tokens[..index].windows(3).enumerate() {
            if head[0].text == "mod"
                && head[2].text == "{"
                && tokens[start + 2].pair.is_some_and(|end| end > index)
            {
                module.push_str("::");
                module.push_str(&head[1].text);
            }
        }
        let declaration: String = tokens[index..]
            .iter()
            .take_while(|token| token.text != ";")
            .map(|token| token.text.as_str())
            .collect();
        declarations.push((module, declaration == "constREADBACKS:&[Readback]=&JOINED"));
    }
    Ok(declarations)
}

/// Scan all crates, including inline and test declarations. Only the single
/// aggregate export is exempt, and its expression must still be `&JOINED`.
pub fn check(root: &Path) -> Vec<Finding> {
    if !root.join("crates/control").exists() {
        return Vec::new();
    }
    let mut findings = Vec::new();
    let expected = match fs::read_to_string(root.join(MANIFEST))
        .map_err(|error| error.to_string())
        .and_then(|source| manifest(&source))
    {
        Ok(expected) => expected,
        Err(error) => return vec![Finding::new(format!("{MANIFEST}: {error}"), REMEDY)],
    };
    let mut actual = BTreeSet::new();
    let mut pending = vec![root.join("crates")];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                findings.push(Finding::new(
                    format!("{}: {error}", directory.display()),
                    REMEDY,
                ));
                continue;
            }
        };
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    findings.push(Finding::new(error.to_string(), REMEDY));
                    continue;
                }
            };
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let source = match fs::read_to_string(&path) {
                Ok(source) => source,
                Err(error) => {
                    findings.push(Finding::new(format!("{relative}: {error}"), REMEDY));
                    continue;
                }
            };
            if !source.contains("READBACKS") {
                continue;
            }
            match readback_declarations(&source) {
                Ok(declarations) if relative == AGGREGATE => {
                    if declarations != [(String::new(), true)] {
                        findings.push(Finding::new(
                            format!("{relative}: aggregate must export &JOINED"),
                            REMEDY,
                        ));
                    }
                }
                Ok(declarations) => {
                    for (module, _) in declarations {
                        actual.insert(format!("{relative}{module}"));
                    }
                }
                Err(error) => findings.push(Finding::new(format!("{relative}: {error}"), REMEDY)),
            }
        }
    }
    for missing in actual.difference(&expected) {
        findings.push(Finding::new(
            format!("{missing}: READBACKS is absent from the family manifest"),
            REMEDY,
        ));
    }
    for absent in expected.difference(&actual) {
        findings.push(Finding::new(
            format!("{absent}: manifest binding has no READBACKS declaration"),
            REMEDY,
        ));
    }
    findings.sort_by(|left, right| left.line.cmp(&right.line));
    findings
}

pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if relative.starts_with("crates/") && relative.ends_with(".rs") {
        check(root)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "control_families_tests.rs"]
mod tests;
