//! What the interface actually registers, walked out of the registries that
//! already exist.
//!
//! This is the half of the drift guard the table cannot fake. Nothing here
//! knows what a behaviour *means*; it only reports that the toolbar has an
//! action called `SetFootprint`, that the drawing registry holds a tool called
//! `brush`, that `menu_bar.rs` draws a button labelled `Save as…`. Pairing
//! each of those with a row is `super::drift`'s job, and an entry no row
//! claims is the finding.
//!
//! # Two kinds of walk
//!
//! **Live** walks read the registry itself — `DRAWING_TOOLS`, `DockTab::ALL`,
//! `LayerToggle::ALL`, `LAYOUT_PRESETS`, `ScriptedMenu::ALL`. They cannot be
//! wrong about what is registered because they are holding it.
//!
//! **Source** walks read the file. Four of the registries are plain `enum`s
//! with no `ALL` array — a `ToolbarAction` cannot be enumerated at runtime
//! because most variants carry data — and a keyboard shortcut is a `const`,
//! not a member of anything. For those, the variant names and the constant
//! names are parsed out of the source text, which is how
//! `crates/guards/src/generated.rs` already checks the capability inventory
//! and the hook registry: cheap, dependency-free, and loud when the parse
//! finds nothing, because a scan that silently matches zero entries is a guard
//! that silently passes.
//!
//! Test-only, and deliberately: reading `crates/app/src` at runtime is
//! meaningless in a shipped binary, and the generated matrix is a function of
//! the table alone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Registered, Source};

/// The workspace root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/app sits two levels below the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The variant names of one `enum`, parsed out of its declaration.
///
/// Panics when the declaration is not found or yields nothing: a scan that
/// matches zero variants would let every row in the table look claimed.
fn enum_variants(relative: &str, name: &str) -> Vec<String> {
    let text = read(relative);
    let header = format!("enum {name} {{");
    let start = text
        .find(&header)
        .unwrap_or_else(|| panic!("{relative} no longer declares `enum {name}`"))
        + header.len();
    let body = &text[start..];
    let end = body
        .find("\n}")
        .unwrap_or_else(|| panic!("`enum {name}` in {relative} has no closing brace"));
    let mut variants = Vec::new();
    for line in body[..end].lines() {
        let line = line.trim();
        // A variant line opens with the variant's own name: capital letter,
        // then identifier characters, then `,` `(` `{` or end of line.
        let Some(first) = line.chars().next() else {
            continue;
        };
        if !first.is_ascii_uppercase() {
            continue;
        }
        let variant: String = line
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect();
        if !variant.is_empty() {
            variants.push(variant);
        }
    }
    assert!(
        !variants.is_empty(),
        "`enum {name}` in {relative} parsed to no variants; the scan is broken, not the enum"
    );
    variants
}

/// Every production `.rs` file under `crates/app/src`, test trees excluded.
fn app_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let root = workspace_root().join("crates/app/src");
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()));
        for entry in entries {
            let path = entry.expect("a directory entry is readable").path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            if path.is_dir() {
                if name != "tests" {
                    stack.push(path);
                }
            } else if name.ends_with(".rs") && !name.ends_with("_tests.rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Every keyboard shortcut the application binds, by the name of the constant
/// that binds it.
///
/// Two shapes, because the interface has two. Most bindings are a
/// `KeyboardShortcut` constant. The layout numbers are an array of `egui::Key`
/// that two functions turn into `Ctrl+N` and `Alt+N` shortcuts, so the array's
/// name stands for both families — `layout.preset.apply` claims it, and
/// `layout.tab.switch` names the other family in its reach.
fn hotkey_constants() -> Vec<String> {
    let mut names = BTreeSet::new();
    for path in app_sources() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        for line in text.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("const ") {
                continue;
            }
            let binds_shortcut = trimmed.contains("KeyboardShortcut");
            let binds_keys =
                trimmed.contains("[egui::Key;") || trimmed.contains("[eframe::egui::Key;");
            if !binds_shortcut && !binds_keys {
                continue;
            }
            let after = trimmed
                .split("const ")
                .nth(1)
                .expect("the line contains `const `");
            let name: String = after
                .chars()
                .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                .collect();
            if !name.is_empty() {
                names.insert(name);
            }
        }
    }
    assert!(
        !names.is_empty(),
        "no keyboard shortcut constants found; the scan is broken"
    );
    names.into_iter().collect()
}

/// How many menu-bar entries carry a label the source computes rather than
/// writes — a label that says "Hide panels" or "Show panels" depending on
/// state, a bookmark's own name, a chart's number.
///
/// A computed label cannot be scanned, so it cannot be claimed by key. It is
/// counted instead: a new one changes this number and fails the guard, which
/// sends the author to this module to say which behaviour it belongs to. The
/// twelve are the panels, legend, drawing-rail and agent-access switches; the
/// layout, bookmark-open, bookmark-delete and recent-file lists; and the two
/// `Chart N up` / `Chart N down` entries.
pub(crate) const COMPUTED_MENU_LABELS: usize = 12;

/// The menu bar's own labels, and the count of the ones it computes.
///
/// Parsed from `menu_bar.rs` rather than declared beside it, for the reason
/// the hook registry gives: a list kept by hand beside the code is the
/// duplicated truth that drifts the first time either side gains an entry.
fn menu_labels() -> (Vec<String>, usize) {
    let text = read("crates/app/src/app/menu_bar.rs");
    let mut labels = Vec::new();
    let mut computed = 0usize;
    // `.menu_button(` is checked before `.button(` would match it; it cannot,
    // because the character before `button(` there is `_`, not `.`.
    for opener in [".button(", "egui::Button::new(", ".menu_button("] {
        let mut cursor = 0usize;
        while let Some(found) = text[cursor..].find(opener) {
            let argument = cursor + found + opener.len();
            cursor = argument;
            if text[argument..].starts_with('"') {
                let literal = &text[argument + 1..];
                let end = literal
                    .find('"')
                    .expect("a string literal in the menu bar is closed");
                labels.push(literal[..end].to_owned());
            } else {
                computed += 1;
            }
        }
    }
    // A checkbox takes its bound value first and its label second, so the
    // label is the next string literal after the opener.
    let mut cursor = 0usize;
    while let Some(found) = text[cursor..].find(".checkbox(") {
        let argument = cursor + found + ".checkbox(".len();
        cursor = argument;
        let separator = text[argument..]
            .find(", \"")
            .expect("a checkbox in the menu bar carries a literal label");
        let literal = &text[argument + separator + 3..];
        let end = literal
            .find('"')
            .expect("a string literal in the menu bar is closed");
        labels.push(literal[..end].to_owned());
    }
    assert!(
        !labels.is_empty(),
        "no menu labels found in menu_bar.rs; the scan is broken"
    );
    (labels, computed)
}

/// Everything the interface registers, as the drift guard sees it.
pub(crate) fn registered() -> Vec<Registered> {
    let mut entries = Vec::new();

    for variant in enum_variants("crates/app/src/toolbar.rs", "ToolbarAction") {
        entries.push(Registered::new(Source::ToolbarAction, variant));
    }
    for variant in enum_variants("crates/app/src/layout_strip.rs", "StripAction") {
        entries.push(Registered::new(Source::StripAction, variant));
    }
    for variant in enum_variants("crates/app/src/tabstrip.rs", "TabAction") {
        entries.push(Registered::new(Source::TabAction, variant));
    }
    for variant in enum_variants("crates/app/src/toolrail.rs", "ToolboxDock") {
        entries.push(Registered::new(Source::ToolboxDock, variant));
    }
    for variant in enum_variants("crates/app/src/feed_notice.rs", "NoticeAction") {
        entries.push(Registered::new(Source::NoticeAction, variant));
    }

    for tab in crate::dock::DockTab::ALL {
        entries.push(Registered::new(Source::DockTab, tab.id()));
    }
    for toggle in crate::toolbar::LayerToggle::ALL {
        entries.push(Registered::new(Source::LayerToggle, format!("{toggle:?}")));
    }
    for tool in crate::drawings::DRAWING_TOOLS {
        entries.push(Registered::new(Source::DrawingTool, tool.id()));
    }
    for tool in [
        crate::toolrail::Tool::Pointer,
        crate::toolrail::Tool::Crosshair,
    ] {
        entries.push(Registered::new(Source::RailTool, tool.id()));
    }
    for preset in crate::canvas_layout::LAYOUT_PRESETS {
        entries.push(Registered::new(Source::LayoutPreset, preset.id));
    }
    for (token, _) in crate::harness::ScriptedMenu::ALL {
        entries.push(Registered::new(Source::ScriptedMenu, token));
    }

    for name in hotkey_constants() {
        entries.push(Registered::new(Source::Hotkey, name));
    }
    let (labels, computed) = menu_labels();
    assert_eq!(
        computed, COMPUTED_MENU_LABELS,
        "the menu bar now computes {computed} labels rather than {COMPUTED_MENU_LABELS}. A \
         computed label cannot be claimed by key, so say which behaviour the new one belongs to \
         and update COMPUTED_MENU_LABELS"
    );
    for label in labels {
        entries.push(Registered::new(Source::MenuEntry, label));
    }

    entries.sort();
    entries.dedup();
    entries
}
