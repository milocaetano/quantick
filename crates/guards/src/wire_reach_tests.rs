//! The wire shapes stay low in the graph after `control-schema` folded into
//! `control-handlers`: production code under `wire/` names only the sibling
//! crates the old crate could link, so a wire DTO cannot quietly take on a
//! chart, drawing, paper or sim type because its host crate links one.

/// `control-schema`'s sibling dependencies when it folded, by package suffix.
const WIRE_MAY_REACH: &[&str] = &[
    "control",
    "control_host",
    "engine",
    "indicators",
    "layers",
    "operability",
    "orderbook",
    "orderflow",
    "pine",
    "session",
    "stores",
    "trading",
];

/// Every `quantick_<name>` a line of code names, comments excluded.
fn crate_names(line: &str) -> Vec<String> {
    let code = line.split("//").next().unwrap_or(line);
    let mut names = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("quantick_") {
        let preceded = rest[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let tail = &rest[at + "quantick_".len()..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(tail.len());
        if !preceded && tail[end..].starts_with("::") {
            names.push(tail[..end].to_owned());
        }
        rest = &tail[end..];
    }
    names
}

#[test]
fn a_crate_path_is_read_from_code_only() {
    assert_eq!(
        crate_names("use quantick_chart::Bar; // quantick_sim::Order"),
        vec!["chart"]
    );
    assert!(crate_names("const X: &str = \"my_quantick_x\";").is_empty());
}

#[test]
fn wire_names_only_the_crates_control_schema_could_link() {
    let root = crate::workspace_root();
    let prefix = "crates/control-handlers/src/wire/";
    let source = crate::size::measure(&root);
    let mut scanned = 0;
    for (path, _) in source.counts.iter().filter(|(path, _)| {
        path.starts_with(prefix) || path == "crates/control-handlers/src/wire.rs"
    }) {
        scanned += 1;
        let contents = std::fs::read_to_string(root.join(path)).unwrap();
        for line in crate::size::production_source(&contents) {
            for name in crate_names(line) {
                assert!(
                    WIRE_MAY_REACH.contains(&name.as_str()),
                    "{path} names quantick_{name}, which the wire shapes may not link: \
                     keep the type in its owner and mirror it as a wire DTO with a `From`"
                );
            }
        }
    }
    assert!(scanned > 50, "the wire tree was not found: {scanned} files");
}
