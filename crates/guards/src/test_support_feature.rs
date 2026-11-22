//! Keep owner test helpers out of production dependency feature graphs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::Finding;

const REMEDY: &str = "Enable `test-support` only from dev-dependencies; production and build dependency feature paths must remain shipping code.";

#[derive(Default)]
struct Manifest {
    package: String,
    path: PathBuf,
    features: BTreeMap<String, Vec<String>>,
    production: Vec<Dependency>,
}

#[derive(Clone)]
struct Dependency {
    alias: String,
    package: String,
    features: Vec<String>,
    defaults: bool,
    inherited: bool,
    optional: bool,
}

fn quoted(value: &str) -> Result<(&str, String), String> {
    let value = value.trim_start();
    let Some(mark) = value
        .chars()
        .next()
        .filter(|mark| matches!(mark, '\'' | '"'))
    else {
        return Err(format!("unsupported manifest string `{value}`"));
    };
    let rest = &value[mark.len_utf8()..];
    for (at, character) in rest.char_indices() {
        if mark == '"' && character == '\\' {
            return Err(format!(
                "unsupported escaped manifest string `{value}`; use an unescaped Cargo package or feature name"
            ));
        }
        if character == mark {
            return Ok((&rest[at + mark.len_utf8()..], rest[..at].to_owned()));
        }
    }
    Err(format!("unterminated manifest string `{value}`"))
}

fn manifest_key(value: &str) -> Result<Vec<String>, String> {
    let characters: Vec<char> = value.chars().collect();
    let mut at = 0;
    let mut parts = Vec::new();
    loop {
        while characters.get(at).is_some_and(|c| matches!(c, ' ' | '\t')) {
            at += 1;
        }
        let Some(&first) = characters.get(at) else {
            return Err(format!("empty manifest key `{value}`"));
        };
        let mut part = String::new();
        if matches!(first, '\'' | '"') {
            let quote = first;
            at += 1;
            loop {
                let Some(&character) = characters.get(at) else {
                    return Err(format!("unterminated manifest key `{value}`"));
                };
                at += 1;
                if character == quote {
                    break;
                }
                if quote == '"' && character == '\\' {
                    let Some(&escaped) = characters.get(at) else {
                        return Err(format!("unterminated manifest escape `{value}`"));
                    };
                    at += 1;
                    match escaped {
                        'b' => part.push('\u{0008}'),
                        't' => part.push('\t'),
                        'n' => part.push('\n'),
                        'f' => part.push('\u{000c}'),
                        'r' => part.push('\r'),
                        '"' => part.push('"'),
                        '\\' => part.push('\\'),
                        'u' | 'U' => {
                            let digits = if escaped == 'u' { 4 } else { 8 };
                            let mut scalar = 0_u32;
                            for _ in 0..digits {
                                let digit = characters
                                    .get(at)
                                    .and_then(|c| c.to_digit(16))
                                    .ok_or_else(|| format!("invalid manifest escape `{value}`"))?;
                                at += 1;
                                scalar = scalar * 16 + digit;
                            }
                            part.push(char::from_u32(scalar).ok_or_else(|| {
                                format!("invalid manifest scalar escape `{value}`")
                            })?);
                        }
                        _ => return Err(format!("unsupported manifest escape `{value}`")),
                    }
                } else {
                    part.push(character);
                }
            }
        } else {
            while let Some(&character) = characters.get(at) {
                if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                    part.push(character);
                    at += 1;
                } else {
                    break;
                }
            }
            if part.is_empty() {
                return Err(format!("unsupported manifest key `{value}`"));
            }
        }
        parts.push(part);
        while characters.get(at).is_some_and(|c| matches!(c, ' ' | '\t')) {
            at += 1;
        }
        match characters.get(at) {
            None => return Ok(parts),
            Some('.') => at += 1,
            _ => return Err(format!("unsupported manifest key tail `{value}`")),
        }
    }
}

fn table_key(header: &str) -> Result<Option<Vec<String>>, String> {
    let header = header.trim();
    if header.starts_with("[[") {
        return Ok(None);
    }
    let Some(body) = header.strip_prefix('[') else {
        return Ok(None);
    };
    let mut quote = None;
    let mut escaped = false;
    let mut close = None;
    for (at, character) in body.char_indices() {
        if escaped {
            escaped = false;
        } else if quote == Some('"') && character == '\\' {
            escaped = true;
        } else if quote == Some(character) {
            quote = None;
        } else if quote.is_none() && matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if quote.is_none() && character == ']' {
            close = Some(at);
            break;
        }
    }
    let close = close.ok_or_else(|| format!("unterminated manifest table header `{header}`"))?;
    let tail = body[close + 1..].trim_start();
    if !tail.is_empty() && !tail.starts_with('#') {
        return Err(format!("unsupported manifest table header tail `{header}`"));
    }
    manifest_key(&body[..close]).map(Some)
}

fn strings(value: &str) -> Result<Vec<String>, String> {
    let Some(open) = value.find('[') else {
        return Err(format!("expected a manifest string array in `{value}`"));
    };
    let Some(close) = value.rfind(']').filter(|close| *close > open) else {
        return Err(format!("unterminated manifest string array `{value}`"));
    };
    let mut rest = value[open + 1..close].trim();
    let mut result = Vec::new();
    while !rest.is_empty() {
        let (after, item) = quoted(rest)?;
        result.push(item);
        rest = after.trim_start();
        if rest.is_empty() {
            break;
        }
        rest = rest
            .strip_prefix(',')
            .ok_or_else(|| format!("unsupported manifest array tail `{rest}`"))?
            .trim_start();
    }
    Ok(result)
}

fn field_tail<'a>(value: &'a str, field: &str) -> Option<&'a str> {
    value.match_indices(field).find_map(|(at, _)| {
        let boundary = at == 0
            || !value[..at].chars().next_back().is_some_and(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            });
        let rest = &value[at + field.len()..];
        (boundary && rest.trim_start().starts_with('=')).then_some(rest)
    })
}

fn quoted_field(value: &str, field: &str) -> Result<Option<String>, String> {
    let Some(rest) = field_tail(value, field) else {
        return Ok(None);
    };
    let rest = rest.trim_start().trim_start_matches('=').trim_start();
    quoted(rest).map(|(_, parsed)| Some(parsed))
}

fn package_section(header: &str) -> bool {
    table_key(header).is_ok_and(|parts| parts == Some(vec!["package".to_owned()]))
}

fn features_section(header: &str) -> bool {
    table_key(header).is_ok_and(|parts| parts == Some(vec!["features".to_owned()]))
}

fn shipping_dependencies(header: &str) -> bool {
    table_key(header).is_ok_and(|parts| {
        parts.is_some_and(|parts| {
            matches!(parts.as_slice(), [kind] if matches!(kind.as_str(), "dependencies" | "build-dependencies"))
                || matches!(parts.as_slice(), [workspace, kind] if workspace == "workspace" && kind == "dependencies")
                || matches!(parts.as_slice(), [target, _, kind] if target == "target" && matches!(kind.as_str(), "dependencies" | "build-dependencies"))
        })
    })
}

fn dependency_specific_section(header: &str) -> bool {
    table_key(header).is_ok_and(|parts| {
        parts.is_some_and(|parts| {
            matches!(parts.as_slice(), [kind, _] if matches!(kind.as_str(), "dependencies" | "build-dependencies"))
                || matches!(parts.as_slice(), [workspace, kind, _] if workspace == "workspace" && kind == "dependencies")
                || matches!(parts.as_slice(), [target, _, kind, _] if target == "target" && matches!(kind.as_str(), "dependencies" | "build-dependencies"))
        })
    })
}

fn parse(path: PathBuf) -> Result<Manifest, String> {
    let source =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut out = Manifest {
        path,
        ..Manifest::default()
    };
    for header in source
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('['))
    {
        table_key(header).map_err(|error| format!("{}: {error}", out.path.display()))?;
    }
    if let Some((section, key, _)) =
        crate::graph::manifest_entries_with_sections(&source, dependency_specific_section)
            .into_iter()
            .next()
    {
        return Err(format!(
            "{}: dependency-specific table {section} entry `{key}` is unsupported and cannot be checked safely",
            out.path.display()
        ));
    }
    if let Some((_, value)) = crate::graph::manifest_entries_in(&source, package_section)
        .into_iter()
        .find(|(key, _)| manifest_key(key).is_ok_and(|parts| parts == ["name"]))
    {
        out.package = quoted(&value)
            .map_err(|error| format!("{}: {error}", out.path.display()))?
            .1;
    }
    for (key, value) in crate::graph::manifest_entries_in(&source, features_section) {
        let key = manifest_key(&key).map_err(|error| format!("{}: {error}", out.path.display()))?;
        if key.len() != 1 {
            return Err(format!("{}: unsupported feature key", out.path.display()));
        }
        out.features.insert(
            key[0].clone(),
            strings(&value).map_err(|error| format!("{}: {error}", out.path.display()))?,
        );
    }
    for (_, key, value) in
        crate::graph::manifest_entries_with_sections(&source, shipping_dependencies)
    {
        let key = manifest_key(&key).map_err(|error| format!("{}: {error}", out.path.display()))?;
        let inherited_key = matches!(key.as_slice(), [_, field] if field == "workspace");
        if key.len() != 1 && !inherited_key {
            return Err(format!(
                "{}: unsupported dependency key",
                out.path.display()
            ));
        }
        let alias = key[0].clone();
        let package = quoted_field(&value, "package")
            .map_err(|error| format!("{}: {error}", out.path.display()))?
            .unwrap_or_else(|| alias.clone());
        out.production.push(Dependency {
            alias,
            package,
            features: if value.trim_start().starts_with('{') {
                if let Some(features) = field_tail(&value, "features") {
                    strings(features).map_err(|error| format!("{}: {error}", out.path.display()))?
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            },
            defaults: !value.contains("default-features = false"),
            inherited: inherited_key || value.contains("workspace = true"),
            optional: value.contains("optional = true"),
        });
    }
    Ok(out)
}

fn manifests(root: &Path) -> Result<Vec<Manifest>, String> {
    let root_manifest = root.join("Cargo.toml");
    let workspace = if root_manifest.is_file() {
        parse(root_manifest)?
    } else {
        Manifest::default()
    };
    let crates = root.join("crates");
    let mut result = Vec::new();
    for entry in fs::read_dir(&crates).map_err(|error| format!("{}: {error}", crates.display()))? {
        let path = entry
            .map_err(|error| error.to_string())?
            .path()
            .join("Cargo.toml");
        if path.is_file() {
            let mut manifest = parse(path)?;
            for dependency in &mut manifest.production {
                if !dependency.inherited {
                    continue;
                }
                if let Some(shared) = workspace
                    .production
                    .iter()
                    .find(|item| item.alias == dependency.alias)
                {
                    dependency.package.clone_from(&shared.package);
                    dependency.features.extend(shared.features.iter().cloned());
                    dependency.defaults &= shared.defaults;
                    dependency.optional |= shared.optional;
                }
            }
            result.push(manifest);
        }
    }
    Ok(result)
}

fn reaches_test_support<'a>(
    member: &'a Manifest,
    by_package: &BTreeMap<&'a str, &'a Manifest>,
) -> Option<(String, String)> {
    let mut active_crates = BTreeSet::from([member.package.clone()]);
    let mut active_edges = BTreeSet::new();
    let mut features = BTreeMap::<(String, String), String>::new();
    features.insert(
        (member.package.clone(), "default".to_owned()),
        format!("{} default", member.package),
    );
    loop {
        let before = (active_crates.len(), active_edges.len(), features.len());
        for package in active_crates.clone() {
            let Some(manifest) = by_package.get(package.as_str()) else {
                continue;
            };
            for edge in &manifest.production {
                if !edge.optional {
                    active_edges.insert((package.clone(), edge.alias.clone()));
                }
            }
        }
        for (package, alias) in active_edges.clone() {
            let Some(manifest) = by_package.get(package.as_str()) else {
                continue;
            };
            let Some(edge) = manifest.production.iter().find(|edge| edge.alias == alias) else {
                continue;
            };
            let Some(target) = by_package.get(edge.package.as_str()) else {
                continue;
            };
            active_crates.insert(target.package.clone());
            if edge.defaults {
                features
                    .entry((target.package.clone(), "default".to_owned()))
                    .or_insert_with(|| format!("{package} -> {alias} default"));
            }
            for feature in &edge.features {
                features
                    .entry((target.package.clone(), feature.clone()))
                    .or_insert_with(|| format!("{package} -> {alias}/{feature}"));
            }
        }
        for ((package, feature), path) in features.clone() {
            let Some(manifest) = by_package.get(package.as_str()) else {
                continue;
            };
            for next in manifest.features.get(&feature).into_iter().flatten() {
                if let Some(alias) = next.strip_prefix("dep:") {
                    active_edges.insert((package.clone(), alias.to_owned()));
                } else if let Some((alias, forwarded)) = next.split_once("?/") {
                    if active_edges.contains(&(package.clone(), alias.to_owned()))
                        && let Some(edge) =
                            manifest.production.iter().find(|edge| edge.alias == alias)
                    {
                        features
                            .entry((edge.package.clone(), forwarded.to_owned()))
                            .or_insert_with(|| format!("{path} -> {next}"));
                    }
                } else if let Some((alias, forwarded)) = next.split_once('/') {
                    active_edges.insert((package.clone(), alias.to_owned()));
                    if let Some(edge) = manifest.production.iter().find(|edge| edge.alias == alias)
                    {
                        features
                            .entry((edge.package.clone(), forwarded.to_owned()))
                            .or_insert_with(|| format!("{path} -> {next}"));
                    }
                } else if manifest
                    .production
                    .iter()
                    .any(|edge| edge.alias == *next && edge.optional)
                {
                    active_edges.insert((package.clone(), next.clone()));
                } else {
                    features
                        .entry((package.clone(), next.clone()))
                        .or_insert_with(|| format!("{path} -> {next}"));
                }
            }
        }
        if before == (active_crates.len(), active_edges.len(), features.len()) {
            break;
        }
    }
    features.into_iter().find_map(|((package, feature), path)| {
        (feature == "test-support").then_some((package, path))
    })
}

pub fn check(root: &Path) -> Vec<Finding> {
    let parsed = match manifests(root) {
        Ok(value) => value,
        Err(error) => return vec![Finding::new(error, REMEDY)],
    };
    let by_package: BTreeMap<&str, &Manifest> = parsed
        .iter()
        .map(|item| (item.package.as_str(), item))
        .collect();
    let mut findings = Vec::new();
    for member in &parsed {
        if let Some((support_owner, path)) = reaches_test_support(member, &by_package) {
            findings.push(Finding::new(
                format!(
                    "{}: production feature graph enables {} through {path}",
                    member.path.display(),
                    support_owner
                ),
                REMEDY,
            ));
        }
    }
    findings.sort_by(|left, right| left.line.cmp(&right.line));
    findings
}

pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if relative.ends_with("Cargo.toml") {
        check(root)
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::scratch_dir::ScratchDir;
    use std::fs;

    fn write(root: &ScratchDir, name: &str, body: &str) {
        let path = root.join(format!("crates/{name}"));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("Cargo.toml"), body).unwrap();
    }

    #[test]
    fn production_paths_follow_aliases_targets_defaults_and_forwarding() {
        let root = ScratchDir::new("test-support-features");
        write(
            &root,
            "owner",
            "[package]\nname = \"quantick-owner\"\n[features]\ntest-support = []\n",
        );
        write(
            &root,
            "middle",
            "[package]\nname = \"quantick-middle\"\n[features]\ndefault = [\"fixtures\"]\nfixtures = [\"builder/test-support\"]\n[dependencies]\nbuilder = { package = \"quantick-owner\", path = \"../owner\" }\n",
        );
        write(
            &root,
            "app",
            "[package]\nname = \"quantick-app\"\n[target.'cfg(windows)'.dependencies]\nquantick-middle = { path = \"../middle\" }\n",
        );
        let findings = check(&root);
        assert!(
            findings
                .iter()
                .any(|finding| finding.line.contains("builder/test-support")),
            "{findings:?}"
        );
    }

    #[test]
    fn dev_only_and_unreachable_features_pass() {
        let root = ScratchDir::new("test-support-dev-only");
        write(
            &root,
            "owner",
            "[package]\nname = \"quantick-owner\"\n[features]\ntest-support = []\n",
        );
        write(
            &root,
            "app",
            "[package]\nname = \"quantick-app\"\n[features]\nunused = [\"owner/test-support\"]\n[dev-dependencies]\nowner = { package = \"quantick-owner\", path = \"../owner\", features = [\"test-support\"] }\n",
        );
        assert!(check(&root).is_empty());
    }

    #[test]
    fn workspace_inherited_production_features_fail() {
        let root = ScratchDir::new("test-support-workspace");
        fs::write(root.join("Cargo.toml"), "[workspace.dependencies]\nowner = { package = \"quantick-owner\", features = [\"test-support\"] }\n").unwrap();
        write(
            &root,
            "owner",
            "[package]\nname = \"quantick-owner\"\n[features]\ntest-support = []\n",
        );
        write(
            &root,
            "app",
            "[package]\nname = \"quantick-app\"\n[dependencies]\nowner.workspace = true\n",
        );
        let findings = check(&root);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].line.contains("test-support"));
    }

    #[test]
    fn optional_dependency_activation_and_weak_forwarding_are_followed() {
        for (label, middle_features) in [
            ("dep-default", "fixture = [\"dep:builder\"]"),
            ("strong-default", "fixture = [\"builder/ordinary\"]"),
            (
                "weak-after-activation",
                "fixture = [\"dep:builder\", \"builder?/bridge\"]",
            ),
        ] {
            let root = ScratchDir::new(label);
            write(
                &root,
                "owner",
                "[package]\nname = \"quantick-owner\"\n[features]\ndefault = [\"test-support\"]\ntest-support = []\nordinary = []\nbridge = [\"test-support\"]\n",
            );
            write(
                &root,
                "middle",
                &format!(
                    "[package]\nname = \"quantick-middle\"\n[features]\n{middle_features}\n[dependencies]\nbuilder = {{ package = \"quantick-owner\", path = \"../owner\", optional = true }}\n"
                ),
            );
            write(
                &root,
                "app",
                "[package]\nname = \"quantick-app\"\n[dependencies]\nquantick-middle = { path = \"../middle\", features = [\"fixture\"] }\n",
            );
            let findings = check(&root);
            assert!(
                findings.iter().any(|finding| finding.line.contains("app")),
                "{label}: {findings:?}"
            );
        }

        let root = ScratchDir::new("weak-unactivated");
        write(
            &root,
            "owner",
            "[package]\nname = \"quantick-owner\"\n[features]\ntest-support = []\nbridge = [\"test-support\"]\n",
        );
        write(
            &root,
            "middle",
            "[package]\nname = \"quantick-middle\"\n[features]\nfixture = [\"builder?/bridge\"]\n[dependencies]\nbuilder = { package = \"quantick-owner\", path = \"../owner\", optional = true }\n",
        );
        write(
            &root,
            "app",
            "[package]\nname = \"quantick-app\"\n[dependencies]\nquantick-middle = { path = \"../middle\", features = [\"fixture\"] }\n",
        );
        assert!(check(&root).is_empty());
    }

    #[test]
    fn root_optional_activation_and_forwarding_follow_cargo_semantics() {
        for (label, root_features, expected) in [
            ("strong-direct", "default = [\"owner/test-support\"]", true),
            ("strong-bridge", "default = [\"owner/bridge\"]", true),
            ("weak-only", "default = [\"owner?/bridge\"]", false),
            (
                "weak-after-activation",
                "default = [\"dep:owner\", \"owner?/bridge\"]",
                true,
            ),
        ] {
            let root = ScratchDir::new(label);
            write(
                &root,
                "owner",
                "[package]\nname = \"quantick-owner\"\n[features]\ntest-support = []\nbridge = [\"test-support\"]\n",
            );
            write(
                &root,
                "app",
                &format!(
                    "[package]\nname = \"quantick-app\"\n[features]\n{root_features}\n[dependencies]\nowner = {{ package = \"quantick-owner\", path = \"../owner\", optional = true, default-features = false }}\n"
                ),
            );
            let findings = check(&root);
            assert_eq!(!findings.is_empty(), expected, "{label}: {findings:?}");
        }
    }

    #[test]
    fn literal_strings_have_the_same_feature_graph_semantics() {
        let root = ScratchDir::new("test-support-literal-strings");
        write(
            &root,
            "owner",
            "[package]\nname = 'quantick-owner'\n[features]\ntest-support = []\n",
        );
        write(
            &root,
            "app",
            "[package]\nname = 'quantick-app'\n[dependencies]\nowner = { package = 'quantick-owner', path = '../owner', features = ['test-support'] }\n",
        );
        let findings = check(&root);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].line.contains("quantick-owner"));
    }

    #[test]
    fn semantic_headers_and_quoted_aliases_keep_shipping_graph_semantics() {
        let root = ScratchDir::new("test-support-semantic-headers");
        write(
            &root,
            "owner",
            "[ 'package' ]\n'name' = 'quantick-owner'\n[ \"features\" ]\n\"test-support\" = []\n",
        );
        write(
            &root,
            "app",
            "[[bin]]\nname = 'fixture-app'\npath = 'src/main.rs'\n[ \"package\" ]\nname = 'quantick-app'\n[ \"dependencies\" ] # shipping\n\"builder\" = { package = 'quantick-owner', path = '../owner', features = ['test-support'] }\n",
        );
        let findings = check(&root);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].line.contains("quantick-owner"));
    }

    #[test]
    fn dependency_specific_tables_fail_closed_in_every_shipping_form() {
        for (label, header, body) in [
            (
                "direct",
                "[ dependencies . owner ]",
                "features = ['test-support']",
            ),
            (
                "renamed",
                "[ \"dependencies\" . \"builder\" ]",
                "package = 'quantick-owner'\nfeatures = ['test-support']",
            ),
            (
                "build",
                "[ \"build-depend\\u0065ncies\" . owner ]",
                "features = ['test-support']",
            ),
            (
                "target",
                "[ target . 'cfg(windows)' . build-dependencies . builder ]",
                "features = ['test-support']",
            ),
        ] {
            let root = ScratchDir::new(label);
            write(
                &root,
                "app",
                &format!("[package]\nname = 'quantick-app'\n{header}\n{body}\n"),
            );
            let findings = check(&root);
            assert_eq!(findings.len(), 1, "{label}: {findings:?}");
            assert!(findings[0].line.contains("unsupported"));
        }

        let root = ScratchDir::new("workspace-specific");
        fs::write(
            root.join("Cargo.toml"),
            "[[bench]]\nname = 'fixture'\n[ workspace . \"dependencies\" . owner ]\nfeatures = ['test-support']\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("crates")).unwrap();
        let findings = check(&root);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].line.contains("dependency-specific table"));
        assert!(findings[0].line.contains("owner"));
    }

    #[test]
    fn semantic_dev_dependency_specific_tables_remain_outside_shipping() {
        let root = ScratchDir::new("test-support-semantic-dev-specific");
        write(
            &root,
            "app",
            "[[bench]]\nname = 'fixture'\n[ 'package' ]\nname = 'quantick-app'\n[ target . 'cfg(windows)' . \"dev-dependencies\" . builder ]\npackage = 'quantick-owner'\nfeatures = ['test-support']\n",
        );
        assert!(check(&root).is_empty());
    }

    #[test]
    fn multiline_dependency_and_feature_arrays_keep_the_same_verdict() {
        let root = ScratchDir::new("test-support-multiline");
        write(
            &root,
            "owner",
            "[package]\nname = \"quantick-owner\"\n[features]\ntest-support = []\n",
        );
        write(
            &root,
            "middle",
            "[package]\nname = \"quantick-middle\"\n[features]\nfixtures = [\n  \"builder/test-support\",\n]\n[dependencies]\nbuilder = {\n  package = \"quantick-owner\",\n  path = \"../owner\",\n}\n",
        );
        write(
            &root,
            "app",
            "[package]\nname = \"quantick-app\"\n[dependencies]\nquantick-middle = {\n  path = \"../middle\",\n  features = [\n    \"fixtures\",\n  ],\n}\n",
        );
        let findings = check(&root);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].line.contains("builder/test-support"));
    }
}
