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
    section: String,
    inherited: bool,
    optional: bool,
}

fn strings(value: &str) -> Vec<String> {
    let Some(open) = value.find('[') else {
        return Vec::new();
    };
    let Some(close) = value[open..].find(']').map(|at| open + at) else {
        return Vec::new();
    };
    value[open + 1..close]
        .split(',')
        .filter_map(|part| {
            part.trim()
                .strip_prefix('"')?
                .strip_suffix('"')
                .map(str::to_owned)
        })
        .collect()
}

fn quoted_field(value: &str, field: &str) -> Option<String> {
    let rest = value
        .split(field)
        .nth(1)?
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let rest = rest.strip_prefix('"')?;
    Some(rest.split('"').next()?.to_owned())
}

fn package_section(header: &str) -> bool {
    header == "[package]"
}

fn features_section(header: &str) -> bool {
    header == "[features]"
}

fn shipping_dependencies(header: &str) -> bool {
    header.ends_with("dependencies]") && !header.ends_with("dev-dependencies]")
}

fn parse(path: PathBuf) -> Result<Manifest, String> {
    let source =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut out = Manifest {
        path,
        ..Manifest::default()
    };
    if let Some((_, value)) = crate::graph::manifest_entries_in(&source, package_section)
        .into_iter()
        .find(|(key, _)| key == "name")
    {
        out.package = value.trim_matches('"').to_owned();
    }
    for (key, value) in crate::graph::manifest_entries_in(&source, features_section) {
        out.features
            .insert(key.trim_matches('"').to_owned(), strings(&value));
    }
    for (section, key, value) in
        crate::graph::manifest_entries_with_sections(&source, shipping_dependencies)
    {
        let alias = key.trim_matches('"').to_owned();
        let package = quoted_field(&value, "package").unwrap_or_else(|| alias.clone());
        out.production.push(Dependency {
            alias,
            package,
            features: strings(&value),
            defaults: !value.contains("default-features = false"),
            section: section.trim_matches(['[', ']']).to_owned(),
            inherited: value.contains("workspace = true"),
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
    owner: &'a Manifest,
    dependency: &Dependency,
    by_package: &BTreeMap<&'a str, &'a Manifest>,
) -> Option<(String, String)> {
    let mut active_crates = BTreeSet::from([owner.package.clone()]);
    let mut active_edges = BTreeSet::new();
    let mut features = BTreeMap::<(String, String), String>::new();
    if dependency.defaults {
        features.insert(
            (owner.package.clone(), "default".to_owned()),
            format!("{} default", dependency.alias),
        );
    }
    for feature in &dependency.features {
        features.insert(
            (owner.package.clone(), feature.clone()),
            format!("{} feature {feature}", dependency.alias),
        );
    }
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
                    if active_edges.contains(&(package.clone(), alias.to_owned())) {
                        if let Some(edge) =
                            manifest.production.iter().find(|edge| edge.alias == alias)
                        {
                            features
                                .entry((edge.package.clone(), forwarded.to_owned()))
                                .or_insert_with(|| format!("{path} -> {next}"));
                        }
                    }
                } else if let Some((alias, forwarded)) = next.split_once('/') {
                    active_edges.insert((package.clone(), alias.to_owned()));
                    if let Some(edge) = manifest.production.iter().find(|edge| edge.alias == alias)
                    {
                        features
                            .entry((edge.package.clone(), forwarded.to_owned()))
                            .or_insert_with(|| format!("{path} -> {next}"));
                    }
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
        for dependency in &member.production {
            if dependency.optional {
                continue;
            }
            let Some(owner) = by_package.get(dependency.package.as_str()) else {
                continue;
            };
            if let Some((support_owner, path)) =
                reaches_test_support(owner, dependency, &by_package)
            {
                findings.push(Finding::new(
                    format!(
                        "{}: {} dependency `{}` ({}) enables {} through {path}",
                        member.path.display(),
                        dependency.section,
                        dependency.alias,
                        dependency.package,
                        support_owner
                    ),
                    REMEDY,
                ));
            }
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
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].line.contains("builder/test-support"));
        assert!(
            findings[0]
                .line
                .contains("target.'cfg(windows)'.dependencies")
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
            "[package]\nname = \"quantick-app\"\n[dependencies]\nowner = { workspace = true }\n",
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
            assert_eq!(findings.len(), 1, "{label}: {findings:?}");
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
