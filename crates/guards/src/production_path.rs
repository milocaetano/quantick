//! Production modules use Rust's conventional file layout.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Finding, extension_boundary::lex};

const REMEDY: &str =
    "Move the module to its conventional Rust path and declare it without #[path].";
const TEST_ROOT_REMEDY: &str =
    "Keep crates/app/src/app/tests/mod.rs as sorted module declarations only.";
const TEST_ROOT: &str = "crates/app/src/app/tests/mod.rs";

fn rust_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn module_dir(file: &Path) -> PathBuf {
    match file.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") => file.parent().unwrap_or(file).to_path_buf(),
        _ => file.with_extension(""),
    }
}

fn external_modules(source: &str, file: &Path) -> Result<Vec<PathBuf>, String> {
    let flags = crate::size::production_flags(source);
    let tokens = lex::lex(source)?;
    let mut modules = Vec::new();
    for declaration in tokens.windows(3) {
        let [module, name, semicolon] = declaration else {
            continue;
        };
        if module.text != "mod" || semicolon.text != ";" {
            continue;
        }
        if !flags.get(module.line).copied().unwrap_or(true) {
            continue;
        }
        if !name
            .text
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        {
            continue;
        }
        let base = module_dir(file);
        let direct = base.join(format!("{}.rs", name.text));
        let nested = base.join(&name.text).join("mod.rs");
        if direct.is_file() {
            modules.push(direct);
        } else if nested.is_file() {
            modules.push(nested);
        }
    }
    Ok(modules)
}

fn production_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut pending = Vec::new();
    let crates = root.join("crates");
    for crate_entry in
        fs::read_dir(&crates).map_err(|error| format!("{}: {error}", crates.display()))?
    {
        let crate_entry = crate_entry.map_err(|error| error.to_string())?;
        let src = crate_entry.path().join("src");
        for entry in [src.join("lib.rs"), src.join("main.rs")] {
            if entry.is_file() {
                pending.push(entry);
            }
        }
        let bin = src.join("bin");
        if bin.is_dir() {
            for entry in
                fs::read_dir(&bin).map_err(|error| format!("{}: {error}", bin.display()))?
            {
                let path = entry.map_err(|error| error.to_string())?.path();
                if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                    pending.push(path);
                }
            }
        }
    }
    let mut reached = BTreeSet::new();
    while let Some(file) = pending.pop() {
        let relative = file
            .strip_prefix(root)
            .map(rust_path)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        if !reached.insert(relative) {
            continue;
        }
        let source =
            fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        pending.extend(external_modules(&source, &file)?);
    }
    Ok(reached)
}

fn attribute_lines(source: &str) -> Result<Vec<usize>, String> {
    let tokens = lex::lex(source)?;
    let flags = crate::size::production_flags(source);
    let mut lines = Vec::new();
    let mut index = 0;
    while index + 1 < tokens.len() {
        let bracket = index
            + if tokens.get(index + 1).is_some_and(|token| token.text == "!") {
                2
            } else {
                1
            };
        if tokens[index].text == "#" && tokens.get(bracket).is_some_and(|token| token.text == "[") {
            let end = tokens[bracket].pair.ok_or("unclosed attribute")?;
            let body = &tokens[bracket + 1..end];
            if body.iter().any(|token| token.text == "path")
                && body.iter().any(|token| token.text == "=")
            {
                let line = tokens[index].line;
                if flags.get(line).copied().unwrap_or(true) {
                    lines.push(line + 1);
                }
            }
            index = end;
        }
        index += 1;
    }
    Ok(lines)
}

fn test_root_findings(root: &Path) -> Vec<Finding> {
    let source = match fs::read_to_string(root.join(TEST_ROOT)) {
        Ok(source) => source,
        Err(error) => {
            return vec![Finding::new(
                format!("{TEST_ROOT}: {error}"),
                TEST_ROOT_REMEDY,
            )];
        }
    };
    let mut names = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        let declaration = trimmed
            .strip_prefix("pub(crate) ")
            .or_else(|| trimmed.strip_prefix("pub(super) "))
            .unwrap_or(trimmed);
        let Some(name) = declaration
            .strip_prefix("mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        else {
            return vec![Finding::new(
                format!("{TEST_ROOT}:{}: expected a module declaration", index + 1),
                TEST_ROOT_REMEDY,
            )];
        };
        names.push(name);
    }
    if names.windows(2).any(|pair| pair[0] >= pair[1]) {
        return vec![Finding::new(
            format!("{TEST_ROOT}: module declarations are not strictly sorted"),
            TEST_ROOT_REMEDY,
        )];
    }
    Vec::new()
}

pub fn check(root: &Path) -> Vec<Finding> {
    let mut findings = test_root_findings(root);
    let files = match production_files(root) {
        Ok(files) => files,
        Err(error) => {
            findings.push(Finding::new(error, REMEDY));
            return findings;
        }
    };
    for relative in files {
        match fs::read_to_string(root.join(&relative))
            .map_err(|error| error.to_string())
            .and_then(|source| attribute_lines(&source))
        {
            Ok(lines) => findings.extend(lines.into_iter().map(|line| {
                Finding::new(
                    format!("{relative}:{line}: production #[path] attribute"),
                    REMEDY,
                )
            })),
            Err(error) => findings.push(Finding::new(format!("{relative}: {error}"), REMEDY)),
        }
    }
    findings.sort_by(|left, right| left.line.cmp(&right.line));
    findings
}

pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if relative == TEST_ROOT {
        return test_root_findings(root);
    }
    let files = match production_files(root) {
        Ok(files) => files,
        Err(error) => return vec![Finding::new(error, REMEDY)],
    };
    if !files.contains(relative) {
        return Vec::new();
    }
    match fs::read_to_string(root.join(relative))
        .map_err(|error| error.to_string())
        .and_then(|source| attribute_lines(&source))
    {
        Ok(lines) => lines
            .into_iter()
            .map(|line| {
                Finding::new(
                    format!("{relative}:{line}: production #[path] attribute"),
                    REMEDY,
                )
            })
            .collect(),
        Err(error) => vec![Finding::new(format!("{relative}: {error}"), REMEDY)],
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{attribute_lines, check_file};
    use crate::scratch_dir::ScratchDir;

    #[test]
    fn direct_cfg_attr_and_macro_paths_are_seen() {
        for source in [
            "#[path = \"other.rs\"] mod other;",
            "#[cfg_attr(unix, path = \"unix.rs\")] mod platform;",
            "macro_rules! modules { ($file:literal) => { #[path = $file] mod generated; } }",
        ] {
            assert_eq!(attribute_lines(source).unwrap(), vec![1], "{source}");
        }
        assert!(
            attribute_lines("#[cfg(test)]\n#[path = \"fixture.rs\"]\nmod fixture;")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn multiline_modules_are_reached_but_test_only_modules_are_not() {
        let root = ScratchDir::new("production-path-multiline");
        let source = root.join("crates/example/src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("lib.rs"),
            "pub(crate)\nmod\nreached\n;\n\n#[cfg(test)]\nmod\ntest_only\n;\n",
        )
        .unwrap();
        fs::write(
            source.join("reached.rs"),
            "#[path = \"elsewhere.rs\"]\nmod elsewhere;\n",
        )
        .unwrap();
        fs::write(
            source.join("test_only.rs"),
            "#[path = \"fixture.rs\"]\nmod fixture;\n",
        )
        .unwrap();

        let reached = check_file(&root, "crates/example/src/reached.rs");
        assert_eq!(reached.len(), 1, "{reached:?}");
        assert!(reached[0].line.contains("production #[path] attribute"));
        assert!(
            check_file(&root, "crates/example/src/test_only.rs").is_empty(),
            "a cfg(test) module is not production reachability"
        );
    }
}
