//! Keep owner test helpers out of production dependency feature graphs.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
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

fn parse(path: PathBuf) -> Result<Manifest, String> {
    let source =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut out = Manifest {
        path,
        ..Manifest::default()
    };
    let mut section = String::new();
    for raw in source.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        let value = value.trim();
        if section == "package" && key == "name" {
            out.package = value.trim_matches('"').to_owned();
        } else if section == "features" {
            out.features.insert(key.to_owned(), strings(value));
        } else if section.ends_with("dependencies") && !section.ends_with("dev-dependencies") {
            let package = quoted_field(value, "package").unwrap_or_else(|| key.to_owned());
            let features = if value.starts_with('{') {
                strings(value)
            } else {
                Vec::new()
            };
            let defaults = !value.contains("default-features = false");
            out.production.push(Dependency {
                alias: key.to_owned(),
                package,
                features,
                defaults,
                section: section.clone(),
                inherited: value.contains("workspace = true"),
            });
        }
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
                }
            }
            result.push(manifest);
        }
    }
    Ok(result)
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
            let Some(owner) = by_package.get(dependency.package.as_str()) else {
                continue;
            };
            let mut queue = VecDeque::new();
            if dependency.defaults {
                queue.push_back((
                    owner,
                    "default".to_owned(),
                    format!("{} default", dependency.alias),
                ));
            }
            for feature in &dependency.features {
                queue.push_back((
                    owner,
                    feature.clone(),
                    format!("{} feature {feature}", dependency.alias),
                ));
            }
            let mut seen = BTreeSet::new();
            while let Some((crate_manifest, feature, path)) = queue.pop_front() {
                if !seen.insert((crate_manifest.package.clone(), feature.clone())) {
                    continue;
                }
                if feature == "test-support" {
                    findings.push(Finding::new(
                        format!(
                            "{}: {} dependency `{}` ({}) enables {} through {path}",
                            member.path.display(),
                            dependency.section,
                            dependency.alias,
                            dependency.package,
                            crate_manifest.package
                        ),
                        REMEDY,
                    ));
                    break;
                }
                for next in crate_manifest.features.get(&feature).into_iter().flatten() {
                    if let Some((alias, forwarded)) = next.split_once('/') {
                        let Some(edge) = crate_manifest
                            .production
                            .iter()
                            .find(|edge| edge.alias == alias)
                        else {
                            continue;
                        };
                        let Some(target) = by_package.get(edge.package.as_str()) else {
                            continue;
                        };
                        queue.push_back((
                            target,
                            forwarded.to_owned(),
                            format!("{path} -> {alias}/{forwarded}"),
                        ));
                    } else if !next.starts_with("dep:") {
                        queue.push_back((
                            crate_manifest,
                            next.clone(),
                            format!("{path} -> {next}"),
                        ));
                    }
                }
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
}
