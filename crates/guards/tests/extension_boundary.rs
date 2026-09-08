//! Independent source specimens exercise the registered guard and compiled CLI.

#[path = "../src/scratch_dir.rs"]
mod scratch_dir;

use std::fs;
use std::process::Command;

use quantick_guards::{GUARDS, extension_boundary as boundary};
use scratch_dir::ScratchDir;

const SOURCE: &str = "\
pub struct QuantickApp { window: usize, }
impl QuantickApp {
    fn existing(&self) {}
}
pub struct ChartState { bars: usize, }
impl ChartState {
    fn existing(&self) {}
}
pub(super) struct IndicatorSlots<'a> {
    pub kinds: &'a mut Vec<Kind>,
    pub operators: &'a mut Set<Target>,
    pub files: &'a mut Vec<File>,
    pub hidden: &'a mut Vec<Target>,
    pub styles: &'a mut Vec<Style>,
}
pub(super) trait IndicatorHost {
    fn add(&mut self, source: IndicatorSource) -> SlotId;
    fn remove(&mut self, slot: SlotId);
}
";

// Written independently of inventory(), including literal caps: one declaration
// line and three implementation lines per root in SOURCE.
const SHAPES: &str = "\
QuantickApp\tpub|struct |window : usize
ChartState\tpub|struct |bars : usize
IndicatorSlots\tpub ( super )|struct < 'a >|pub kinds : &'a mut Vec < Kind >|pub operators : &'a mut Set < Target >|pub files : &'a mut Vec < File >|pub hidden : &'a mut Vec < Target >|pub styles : &'a mut Vec < Style >
IndicatorHost\tpub ( super )|trait |fn add ( & mut self , source : IndicatorSource ) -> SlotId ;|fn remove ( & mut self , slot : SlotId ) ;
";

fn fixture() -> ScratchDir {
    let root = ScratchDir::new("extension-boundary");
    fs::create_dir_all(root.join("crates/app/src")).unwrap();
    fs::create_dir_all(root.join("crates/guards")).unwrap();
    fs::write(root.join("crates/guards/size-baseline.txt"), "!budget 0\n").unwrap();
    fs::write(root.join("crates/guards/cycle-baseline.txt"), "!budget 0\n").unwrap();
    fs::write(root.join("crates/app/src/app.rs"), SOURCE).unwrap();
    // Lexical tokens deliberately keep & and the lifetime separate.
    fs::write(
        root.join(boundary::SHAPES_FILE),
        SHAPES.replace("&'a", "& 'a"),
    )
    .unwrap();
    fs::write(
        root.join(boundary::BUDGET_FILE),
        "!budget 8\nQuantickApp 4\nChartState 4\n",
    )
    .unwrap();
    root
}

fn findings(root: &std::path::Path) -> String {
    let guard = GUARDS
        .iter()
        .find(|guard| guard.name == "extension-boundary")
        .unwrap();
    let full = (guard.check)(root);
    let file = (guard.check_file)(root, "crates/app/src/new_owner.rs");
    assert_eq!(
        full, file,
        "a new sibling file triggers the same whole-boundary check"
    );
    full.into_iter()
        .map(|finding| finding.line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_rejected(source: &str, evidence: &str) {
    let root = fixture();
    fs::write(root.join("crates/app/src/app.rs"), source).unwrap();
    let result = findings(&root);
    assert!(result.contains(evidence), "expected {evidence}: {result}");
    assert!(
        result.contains("crates/app/src"),
        "source path is evidence: {result}"
    );
    assert_cli_rejected(&root, "crates/app/src/app.rs", evidence);
}

fn assert_cli_rejected(root: &std::path::Path, relative: &str, evidence: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_quantick-guards"))
        .env("QUANTICK_GUARDS_ROOT", root)
        .args(["--file", relative])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("extension-boundary"), "{stderr}");
    assert!(stderr.contains(evidence), "{stderr}");
}

#[test]
fn independent_shape_contract_accepts_original_and_lexical_formatting() {
    let root = fixture();
    assert!(findings(&root).is_empty());
    let source = SOURCE.replace("window: usize", "window /* comment */ : usize");
    fs::write(root.join("crates/app/src/app.rs"), source).unwrap();
    assert!(findings(&root).is_empty());
}

#[test]
fn field_visibility_authority_and_host_effect_changes_are_rejected() {
    for (source, evidence) in [
        (
            SOURCE.replace("window: usize,", "window: usize, broad: bool,"),
            "QuantickApp shape differs",
        ),
        (
            SOURCE.replace(
                "pub kinds: &'a mut Vec<Kind>",
                "pub kinds: &'a mut QuantickApp",
            ),
            "IndicatorSlots shape differs",
        ),
        (
            SOURCE.replace("window: usize", "pub window: usize"),
            "QuantickApp shape differs",
        ),
        (
            SOURCE.replace(
                "fn remove(&mut self, slot: SlotId);",
                "fn remove(&mut self, slot: SlotId); fn focus(&mut self);",
            ),
            "IndicatorHost shape differs",
        ),
    ] {
        assert_rejected(&source, evidence);
    }
}

#[test]
fn deposits_below_file_ceiling_and_trait_or_qualified_growth_fail() {
    for header in [
        "impl QuantickApp",
        "impl crate::app::QuantickApp",
        "impl\n crate::window::Surface\n for crate::app::QuantickApp",
        "impl eframe::App for QuantickApp",
    ] {
        let source = format!(
            "{SOURCE}\n{header} {{\n fn add_native(&mut self) {{ self.window += 1; }}\n}}\n"
        );
        assert!(
            quantick_guards::size::production_lines(&source) < quantick_guards::size::THRESHOLD
        );
        assert_rejected(&source, "QuantickApp:");
    }
}

#[test]
fn moving_a_root_impl_preserves_union_and_new_sibling_deposits_fail() {
    let root = fixture();
    let existing = "impl QuantickApp {\n    fn existing(&self) {}\n}\n";
    fs::write(
        root.join("crates/app/src/app.rs"),
        SOURCE.replace(existing, ""),
    )
    .unwrap();
    fs::write(root.join("crates/app/src/owner.rs"), existing).unwrap();
    assert!(findings(&root).is_empty());
    let measured = boundary::inventory(&root).unwrap();
    assert_eq!(
        measured.counts,
        [("ChartState".into(), 4), ("QuantickApp".into(), 4)]
    );
    fs::write(
        root.join("crates/app/src/extra.rs"),
        "impl QuantickApp { fn another(&mut self) {} }\n",
    )
    .unwrap();
    let rejected = findings(&root);
    assert!(rejected.contains("QuantickApp: 5"), "{rejected}");
    assert!(rejected.contains("crates/app/src/extra.rs"), "{rejected}");
    assert_cli_rejected(&root, "crates/app/src/extra.rs", "QuantickApp: 5");
}

#[test]
fn cross_root_shrink_cannot_buy_growth_and_tighten_only_lowers() {
    let root = fixture();
    let source = SOURCE.replace("impl ChartState {\n    fn existing(&self) {}\n}\n", "")
        + "impl QuantickApp { fn deposit(&mut self) {} }\n";
    fs::write(root.join("crates/app/src/app.rs"), source).unwrap();
    assert!(findings(&root).contains("QuantickApp: 5"));
    assert_cli_rejected(&root, "crates/app/src/app.rs", "QuantickApp: 5");
    boundary::tighten(&root).unwrap();
    let baseline = fs::read_to_string(root.join(boundary::BUDGET_FILE)).unwrap();
    assert!(baseline.contains("QuantickApp 4"));
    assert!(baseline.contains("ChartState 1"));
    assert!(baseline.contains("!budget 5"));
    assert!(findings(&root).contains("QuantickApp: 5"));
}

const OWNED_NATIVE: &str = "\
impl IndicatorSlots<'_> {
    fn native(&mut self, host: &mut impl IndicatorHost, target: (u64, PaneSide), id: &str) -> TabSlot {
        let slot = host.add(IndicatorSource::Native { id: id.to_owned(), values: Vec::new() });
        let owner = TabSlot { tab: target.0, side: target.1, slot };
        self.kinds.push((owner, SavedKind::Native { id: id.to_owned() }));
        owner
    }
}
";

#[test]
fn realistic_native_owner_grows_in_same_or_new_file_but_root_deposit_fails() {
    for same_file in [false, true] {
        let root = fixture();
        if same_file {
            fs::write(
                root.join("crates/app/src/app.rs"),
                format!("{SOURCE}{OWNED_NATIVE}"),
            )
            .unwrap();
        } else {
            fs::write(root.join("crates/app/src/native_owner.rs"), OWNED_NATIVE).unwrap();
        }
        assert!(findings(&root).is_empty());
        let output = Command::new(env!("CARGO_BIN_EXE_quantick-guards"))
            .env("QUANTICK_GUARDS_ROOT", root.path())
            .args([
                "--file",
                if same_file {
                    "crates/app/src/app.rs"
                } else {
                    "crates/app/src/native_owner.rs"
                },
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let broad = OWNED_NATIVE.replace("impl IndicatorSlots<'_>", "impl QuantickApp");
    assert_rejected(&format!("{SOURCE}{broad}"), "QuantickApp:");
}

#[test]
fn skipped_directory_modules_require_test_classification_or_scanned_inline_bodies() {
    for name in ["tests", "target", "r#tests", "r#target"] {
        let root = fixture();
        let directory = name.trim_start_matches("r#");
        fs::create_dir_all(root.join(format!("crates/app/src/{directory}"))).unwrap();
        fs::write(
            root.join(format!("crates/app/src/{directory}/mod.rs")),
            "impl QuantickApp { fn hidden_deposit(&self) {} }\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/app/src/app.rs"),
            format!("{SOURCE}pub(crate) mod {name};\n"),
        )
        .unwrap();
        let evidence = "out-of-line production module";
        assert!(findings(&root).contains(evidence));
        assert_cli_rejected(&root, "crates/app/src/app.rs", evidence);

        fs::write(
            root.join("crates/app/src/app.rs"),
            format!("{SOURCE}#[cfg(test)]\nmod {name};\n"),
        )
        .unwrap();
        assert!(findings(&root).is_empty());
        let output = Command::new(env!("CARGO_BIN_EXE_quantick-guards"))
            .env("QUANTICK_GUARDS_ROOT", root.path())
            .args(["--file", "crates/app/src/app.rs"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");

        fs::write(
            root.join("crates/app/src/app.rs"),
            format!("{SOURCE}mod {name} {{ struct FocusedOwner; }}\n"),
        )
        .unwrap();
        assert!(findings(&root).is_empty());
        assert_rejected(
            &format!("{SOURCE}mod {name} {{ impl QuantickApp {{ fn deposit(&self) {{}} }} }}\n"),
            "QuantickApp: 5",
        );
    }
}

#[test]
fn malformed_missing_and_unsupported_source_never_clear() {
    for (suffix, message) in [
        (
            "unsafe impl Send for QuantickApp {}",
            "modified/attributed protected impl",
        ),
        (
            "#[cfg(feature = \"extra\")] impl QuantickApp {}",
            "modified/attributed protected impl",
        ),
        (
            "impl<T> QuantickApp {}",
            "unsupported protected impl header",
        ),
        (
            "impl Trait for &mut QuantickApp {}",
            "unsupported protected impl header",
        ),
        (
            "use crate::{QuantickApp as Alias};",
            "root-renaming import alias",
        ),
        ("type Alias = crate::ChartState;", "root type alias"),
        (
            "macro_rules! grow { () => { impl QuantickApp {} }; }",
            "macro/include",
        ),
        ("make! { impl ChartState {} }", "macro/include"),
        ("include!(\"external.rs\");", "macro/include"),
        ("#[path = \"../external.rs\"] mod hidden;", "external-path"),
        (
            "pub struct ChartState { other: bool, }",
            "duplicate target ChartState",
        ),
        ("fn broken() {", "unclosed delimiter"),
        (
            "const X: &str = r###\"unfinished",
            "unterminated raw string",
        ),
        ("/* unfinished", "unterminated block comment"),
        ("fn broken() { ] }", "mismatched"),
    ] {
        assert_rejected(&format!("{SOURCE}{suffix}"), message);
    }
    assert_rejected(
        &SOURCE.replace("pub struct ChartState { bars: usize, }", ""),
        "missing target ChartState",
    );
    assert_rejected(
        &SOURCE.replace(
            "pub struct ChartState { bars: usize, }",
            "pub struct ChartState(usize);",
        ),
        "named brace body",
    );
    assert_rejected(
        &SOURCE.replace("pub kinds:", "#[cfg(feature = \"wide\")] pub kinds:"),
        "attributed protected fields",
    );
}

#[test]
fn corrupt_or_unreadable_inputs_are_findings() {
    for (path, bytes, evidence) in [
        (
            boundary::SHAPES_FILE,
            b"bad\n".as_slice(),
            "expected target",
        ),
        (boundary::SHAPES_FILE, b"".as_slice(), "missing target"),
        (
            boundary::BUDGET_FILE,
            b"!budget 8\nQuantickApp oops\n".as_slice(),
            "not a count",
        ),
        (
            boundary::BUDGET_FILE,
            b"!budget 8\nQuantickApp 4\nQuantickApp 4\n".as_slice(),
            "already recorded",
        ),
        (
            "crates/app/src/app.rs",
            b"\xff".as_slice(),
            "unreadable source",
        ),
    ] {
        let root = fixture();
        fs::write(root.join(path), bytes).unwrap();
        assert!(findings(&root).contains(evidence));
        assert_cli_rejected(&root, boundary::BUDGET_FILE, evidence);
    }
    let root = fixture();
    fs::remove_file(root.join(boundary::SHAPES_FILE)).unwrap();
    assert!(findings(&root).contains("unreadable baseline"));
    let missing = ScratchDir::new("extension-missing-root");
    assert!(
        boundary::check(&missing)
            .iter()
            .any(|f| f.line.contains("unreadable source directory"))
    );
}
