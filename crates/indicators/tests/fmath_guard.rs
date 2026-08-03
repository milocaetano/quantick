//! Grep guard for the determinism rule on transcendentals.
//!
//! `f64::powf`/`exp`/`ln`/`log10` defer to the platform libm and round
//! differently across platforms — a silent cross-platform drift golden CSVs
//! would only catch on the *other* platform's CI. This test fails the moment
//! any source file except `fmath.rs` calls them, which is cheaper and earlier
//! than a golden mismatch.

use std::fs;
use std::path::Path;

/// Call patterns of the std transcendentals whose results may vary by
/// platform. The ones `crate::fmath` wraps come first; the rest are listed so
/// that reaching for a *new* transcendental fails the build here instead of
/// shipping a platform-dependent plot. `sqrt` is deliberately absent:
/// IEEE-754 requires correctly-rounded sqrt, so it is bit-exact everywhere.
const FORBIDDEN: &[&str] = &[
    ".powf(",
    ".powi(",
    ".exp(",
    ".ln(",
    ".log10(",
    ".log(",
    ".log2(",
    ".exp2(",
    ".exp_m1(",
    ".ln_1p(",
    ".cbrt(",
    ".hypot(",
    ".sin(",
    ".cos(",
    ".tan(",
    ".asin(",
    ".acos(",
    ".atan(",
    ".atan2(",
    ".sinh(",
    ".cosh(",
    ".tanh(",
    ".asinh(",
    ".acosh(",
    ".atanh(",
    // Fully-qualified forms of the same calls.
    "f64::powf(",
    "f64::powi(",
    "f64::exp(",
    "f64::ln(",
    "f64::log10(",
    "f64::log(",
    "f64::log2(",
];

fn scan(dir: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("src dir is readable") {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            scan(&path, violations);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        if path.file_name().is_some_and(|n| n == "fmath.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("source file is readable");
        for (line_no, line) in text.lines().enumerate() {
            for pattern in FORBIDDEN {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: `{}` — use crate::fmath instead",
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
fn transcendentals_only_through_fmath() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    scan(&src, &mut violations);
    assert!(
        violations.is_empty(),
        "std transcendental calls outside fmath.rs (platform-dependent rounding breaks \
         cross-platform determinism):\n{}",
        violations.join("\n")
    );
}
