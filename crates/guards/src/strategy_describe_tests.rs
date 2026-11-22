//! Provider dependencies and execution must stay outside the deterministic domain.
#[test]
fn provider_network_stays_in_app() {
    let root = crate::workspace_root();
    let source = crate::size::measure(&root);
    for name in crate::headless::HEADLESS_CRATES {
        let prefix = format!("crates/{name}/");
        let manifest = std::fs::read_to_string(root.join(&prefix).join("Cargo.toml")).unwrap();
        for dependency in ["reqwest", "ureq", "hyper", "hyper-util", "isahc", "surf"] {
            assert!(
                !manifest.lines().any(|l| l
                    .split_once('=')
                    .is_some_and(|(key, _)| key.trim() == dependency)),
                "{name} imports {dependency}"
            );
        }
        for (path, _) in source.counts.iter().filter(|(path, _)| {
            path.starts_with(&prefix)
                && path.contains("/src/")
                && (path.contains("strategy_describe") || path.contains("script_draft"))
        }) {
            let contents = std::fs::read_to_string(root.join(path)).unwrap();
            for token in [
                "reqwest::",
                "std::net::",
                "tokio::net::",
                "TcpStream",
                "TcpListener",
            ] {
                assert!(
                    !crate::size::production_source(&contents)
                        .iter()
                        .any(|line| line.contains(token)),
                    "{path} executes provider I/O"
                );
            }
        }
    }
}
