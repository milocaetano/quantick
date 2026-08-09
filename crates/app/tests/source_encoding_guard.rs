//! Grep guard for source-file encoding.
//!
//! Rust sources here are UTF-8 without a BOM, and their comments and UI
//! strings are full of `—`, `·`, `×` and `§`. A tool that rewrites a file in
//! the system codepage — a PowerShell `Set-Content`/`Out-File` without
//! `-Encoding utf8` is all it takes — turns every one of those into mojibake
//! (`â€"`, `Â·`, `Ã—`, `Â§`) and may prepend a BOM.
//!
//! Nothing else in the repo can see that: mojibake is valid UTF-8, so fmt,
//! clippy, build and the whole suite stay green while the chart shows
//! `"Native Â· 1Ã—"` and the diff fills with hundreds of lines of noise. It
//! happened once on this crate, which is why this test exists.

use std::fs;
use std::path::Path;

/// Byte sequences a codepage round-trip leaves behind. Each is the UTF-8
/// encoding of a character that cannot occur in this repo's sources on its
/// own: `Â`, `Ã`, `â` and `Ð` only ever appear here as the first byte of a
/// mangled multi-byte character.
const MOJIBAKE: &[&str] = &["Â", "Ã", "â€", "âœ", "â†", "Ð"];

/// The UTF-8 byte-order mark. Legal UTF-8, but nothing in this repo carries
/// one, and a tool that adds one has usually changed the encoding too.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

fn scan(dir: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("source dir is readable") {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            scan(&path, violations);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let bytes = fs::read(&path).expect("source file is readable");
        if bytes.starts_with(BOM) {
            violations.push(format!("{}: starts with a UTF-8 BOM", path.display()));
        }
        let Ok(text) = String::from_utf8(bytes) else {
            violations.push(format!("{}: is not valid UTF-8", path.display()));
            continue;
        };
        for (line_no, line) in text.lines().enumerate() {
            for pattern in MOJIBAKE {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: `{}` — a codepage round-trip mangled this line",
                        path.display(),
                        line_no + 1,
                        pattern
                    ));
                }
            }
        }
    }
}

#[test]
fn sources_are_utf8_without_a_bom_or_mojibake() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    scan(&src, &mut violations);
    assert!(
        violations.is_empty(),
        "source files were rewritten in the wrong encoding (write them as UTF-8 without a \
         BOM — in PowerShell, use [System.IO.File]::WriteAllBytes or an editor, never \
         Set-Content):\n{}",
        violations.join("\n")
    );
}
