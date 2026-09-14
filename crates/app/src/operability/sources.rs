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
//! **Source** walks read the file. Six of the registries are plain `enum`s
//! with no `ALL` array — a `ToolbarAction` cannot be enumerated at runtime
//! because most variants carry data — and a keyboard shortcut is a `const`,
//! not a member of anything. For those, the variant names and the constant
//! names are parsed out of the source text — by declaration name, never by
//! file path, so a module split moves a registry without breaking the walk
//! that holds it. That is how
//! `crates/guards/src/generated.rs` already checks the capability inventory
//! and the hook registry: cheap, dependency-free, and loud when the parse
//! finds nothing, because a scan that silently matches zero entries is a guard
//! that silently passes.
//!
//! Test-only, and deliberately: reading `crates/app/src` at runtime is
//! meaningless in a shipped binary, and the generated matrix is a function of
//! the table alone.

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
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    production(&text).to_owned()
}

/// The marker a file's test module opens with, and therefore where the
/// production half of the file ends.
///
/// The `mod` is part of the marker on purpose: `#[cfg(test)]` also sits on
/// individual items — `toolrail.rs` puts one on a constant two hundred lines
/// above the enum this scan needs — and cutting there would throw away most of
/// the file and report the registry as gone.
const TEST_MODULE_MARKER: &str = "\n#[cfg(test)]\nmod ";

/// A file with its test module cut off.
///
/// A test module is not the interface. This module's own tests hold fixture
/// source naming a `MARK_SHORTCUT` and a `LAYOUT_PRESET_KEYS` so they can
/// check the scan against the shapes rustfmt really produces — and until this
/// cut existed, the scan read its own fixtures back as two more registrations
/// and reported the real constants as duplicates of them.
fn production(text: &str) -> &str {
    match text.find(TEST_MODULE_MARKER) {
        Some(index) => &text[..index],
        None => text,
    }
}

/// The variant names of one `enum`, parsed out of its declaration wherever in
/// the crate that declaration lives.
///
/// By name and not by path, deliberately. Writing the file out would tie this
/// guard to the interface's current layout, and the layout is moving: the
/// toolbar and the tool rail are being split into sibling modules, and the day
/// `ToolbarAction` lands in `toolbar/actions.rs` a path-keyed walk would fail
/// with an address complaint rather than a behaviour finding — whose cheapest
/// reading is to delete the walk.
///
/// Panics when the declaration is not found or yields nothing: a scan that
/// matches zero variants would let every row in the table look claimed.
fn enum_variants(name: &str) -> Vec<String> {
    let header = format!("enum {name} {{");
    let declarations: Vec<(String, String, usize)> = app_sources()
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let text = production(&text).to_owned();
            let start = text.find(&header)? + header.len();
            let relative = path
                .strip_prefix(workspace_root())
                .unwrap_or(&path)
                .display()
                .to_string();
            Some((relative, text, start))
        })
        .collect();
    assert!(
        !declarations.is_empty(),
        "no file under crates/app/src declares `enum {name}`"
    );
    // Exactly one, or the name does not identify a registry. `Tool` is short
    // enough that a second `enum Tool` elsewhere in the crate would silently
    // decide which registry this walk is holding, by alphabetical order.
    assert!(
        declarations.len() == 1,
        "`enum {name}` is declared in more than one file, so the name does not say which registry \
         the walk means: {:?}",
        declarations
            .iter()
            .map(|(relative, _, _)| relative)
            .collect::<Vec<_>>()
    );
    let (relative, text, start) = &declarations[0];
    let body = &text[*start..];
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
///
/// A name is collected once per declaration rather than folded into a set:
/// the same constant name bound in two files is an ambiguous key, and the
/// duplicate check in `registered` says so rather than quietly keeping one.
///
/// Test trees, `*_tests.rs` and every file's `#[cfg(test)]` tail are cut
/// before the scan reads it: a shortcut a test declares is not one the
/// interface binds, and asking for a row it does not deserve would be as
/// wrong as missing one it does.
fn hotkey_constants() -> Vec<String> {
    let mut names = Vec::new();
    for path in app_sources() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        names.extend(shortcut_names(production(&text)));
    }
    assert!(
        !names.is_empty(),
        "no keyboard shortcut constants found; the scan is broken"
    );
    names
}

/// How far past `const ` the type annotation and the initialiser are looked
/// for, in bytes.
///
/// A window rather than the rest of the line, because rustfmt breaks a long
/// declaration after the name and the type lands on the next line. Reading one
/// line would skip such a binding in silence, which is the one thing a parity
/// guard may never do. Generous enough for the longest of either shape in the
/// tree and short enough that it cannot reach the *next* item's initialiser.
const DECLARATION_WINDOW_BYTES: usize = 200;

/// The names of the `KeyboardShortcut` and key-array constants one file
/// declares.
///
/// Split out of the read so it can be tested against the shapes rustfmt
/// actually produces rather than against the ones that happen to be in the
/// tree today.
fn shortcut_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (start, _) in text.match_indices("const ") {
        // A `const` inside a comment is prose, not a declaration.
        let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
        if text[line_start..start].trim_start().starts_with("//") {
            continue;
        }
        let end = (start + DECLARATION_WINDOW_BYTES).min(text.len());
        // Never split a multi-byte character: back off to the nearest boundary.
        let mut end = end;
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        // And it stops at the declaration's own end. A window that ran on into
        // the next item's doc comment would read that item's prose as this
        // one's type — which it did, and reported the window constant above as
        // a hotkey. A declaration in this tree holds neither a blank line nor a
        // comment line, so the first of either is past its end.
        let window = declaration(&text[start..end]);
        let binds_shortcut = window.contains("KeyboardShortcut");
        let binds_keys = window.contains("[egui::Key;") || window.contains("[eframe::egui::Key;");
        if !binds_shortcut && !binds_keys {
            continue;
        }
        let name: String = window["const ".len()..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

/// The head of `window` up to the first blank line or comment line — the end
/// of the declaration it opens with.
fn declaration(window: &str) -> &str {
    let mut length = 0usize;
    for line in window.split_inclusive('\n') {
        let trimmed = line.trim();
        if length > 0 && (trimmed.is_empty() || trimmed.starts_with("//")) {
            break;
        }
        length += line.len();
    }
    &window[..length]
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

    for variant in enum_variants("ToolbarAction") {
        entries.push(Registered::new(Source::ToolbarAction, variant));
    }
    for variant in enum_variants("StripAction") {
        entries.push(Registered::new(Source::StripAction, variant));
    }
    for variant in enum_variants("TabAction") {
        entries.push(Registered::new(Source::TabAction, variant));
    }
    for variant in enum_variants("ToolboxDock") {
        entries.push(Registered::new(Source::ToolboxDock, variant));
    }
    for variant in enum_variants("NoticeAction") {
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
    // By variant name, out of the `Tool` enum itself. A literal
    // `[Tool::Pointer, Tool::Crosshair]` here would let a third non-drawing
    // tool ship unclaimed with this guard green, which is the failure the
    // guard exists to catch.
    for variant in enum_variants("Tool") {
        entries.push(Registered::new(Source::RailTool, variant));
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
    let duplicates = duplicate_keys(&entries);
    assert!(
        duplicates.is_empty(),
        "two registrations share a source and a key, so the matrix cannot tell the two doors apart: {duplicates:?}. Give one of them a distinguishable name"
    );
    entries
}

/// Registrations that share a source and a key, in a sorted list.
///
/// Reported, never collapsed. Two such registrations are two doors the matrix
/// cannot tell apart — the same label drawn in two menus, the same constant
/// name bound in two files — and a row can only claim one of them. A `dedup`
/// here would hide the second from the guard instead of sending its author to
/// give it a distinguishable name.
fn duplicate_keys(sorted: &[Registered]) -> Vec<String> {
    sorted
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .map(|pair| format!("{}:{}", pair[0].source.as_str(), pair[0].key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scan reads a declaration, not a line. rustfmt breaks a long one
    /// after the name, and a binding whose type lands on the next line used to
    /// be skipped in silence — a new hotkey shipping unclaimed with the guard
    /// green, which is the failure this module exists to prevent.
    #[test]
    fn a_shortcut_whose_type_wrapped_to_the_next_line_is_still_found() {
        let wrapped = "pub(crate) const A_VERY_LONG_SHORTCUT_NAME_THAT_WRAPS:\n    \
                       egui::KeyboardShortcut =\n    \
                       egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Q);\n";
        assert_eq!(
            shortcut_names(wrapped),
            vec!["A_VERY_LONG_SHORTCUT_NAME_THAT_WRAPS".to_owned()]
        );
    }

    /// Both shapes the interface uses, and nothing else: a shortcut constant,
    /// a key array, a constant that is neither, and the word in a comment.
    #[test]
    fn the_scan_takes_both_shapes_and_leaves_the_rest() {
        let source = "// const NOT_A_SHORTCUT: egui::KeyboardShortcut = ...\n\
                      const MARK_SHORTCUT: egui::KeyboardShortcut =\n    \
                      egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::M);\n\
                      const LAYOUT_PRESET_KEYS: [egui::Key; 9] = [egui::Key::Num1];\n\
                      const MENU_BAR_HEIGHT: f32 = 28.0;\n";
        assert_eq!(
            shortcut_names(source),
            vec!["MARK_SHORTCUT".to_owned(), "LAYOUT_PRESET_KEYS".to_owned()]
        );
    }

    /// Two doors that look identical to the walk are named, not folded into
    /// one. Without this the second door is invisible: only one row can claim
    /// the key, and the guard would call that complete coverage.
    #[test]
    fn two_registrations_sharing_a_key_are_reported() {
        let mut entries = vec![
            Registered::new(Source::MenuEntry, "Delete"),
            Registered::new(Source::MenuEntry, "Delete"),
            Registered::new(Source::MenuEntry, "Open"),
        ];
        entries.sort();
        assert_eq!(
            duplicate_keys(&entries),
            vec!["menu_entry:Delete".to_owned()]
        );
    }

    /// And a walk with no repeats reports nothing, so the check above is
    /// measuring the repeat rather than the list.
    #[test]
    fn distinct_registrations_are_not_reported() {
        let mut entries = vec![
            Registered::new(Source::MenuEntry, "Delete"),
            Registered::new(Source::Hotkey, "DELETE_SHORTCUT"),
        ];
        entries.sort();
        assert!(duplicate_keys(&entries).is_empty());
    }
}
