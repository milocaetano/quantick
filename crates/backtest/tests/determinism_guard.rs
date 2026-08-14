//! Grep guards for the determinism rules, in the mould of
//! `crates/indicators/tests/fmath_guard.rs`.
//!
//! That guard scans the `indicators` sources only, so it does not cover this
//! crate — the WP that specified the harness said so explicitly, and
//! replicating the rule is the expected answer. A run of the same recording
//! must produce the same report on every machine and every day, which forbids
//! three things at the source level:
//!
//! - **platform transcendentals** — `f64::powf` & co. defer to the platform
//!   libm and round differently across targets;
//! - **wall clocks** — every time this crate reasons about comes from
//!   `Trade::timestamp_ms`;
//! - **unordered collections** — a `HashMap` iterated into a report leaks
//!   its seed into the numbers.
//!
//! Grep guards are blunt: they cannot see through a re-export or an alias.
//! They are also the earliest and cheapest place to catch the mistake, which
//! is why the workspace uses them anyway.

use std::fs;
use std::path::Path;

/// The `std` transcendentals whose results may vary by platform, copied from
/// the `indicators` guard so both crates forbid the same list.
const TRANSCENDENTALS: &[&str] = &[
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
    "f64::powf(",
    "f64::powi(",
    "f64::exp(",
    "f64::ln(",
    "f64::log10(",
    "f64::log(",
    "f64::log2(",
];

/// Anything that would make one run differ from the next.
const NON_DETERMINISM: &[&str] = &[
    "Instant::now",
    "SystemTime",
    "HashMap",
    "HashSet",
    "rand::",
    "thread_rng",
    // `library::scan` sorts by (symbol, date, path); a raw directory read
    // does not, and the order of sessions is the order of the aggregate.
    "read_dir",
];

/// The one file allowed a wall clock: the binary times its own run for the
/// stderr diagnostics. Those numbers never reach a report — the tests below
/// keep that true by letting the exception in here and nowhere else.
const STOPWATCH: &str = "main.rs";

fn scan(dir: &Path, patterns: &[&str], skip: Option<&str>, violations: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("src dir is readable") {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            scan(&path, patterns, skip, violations);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        if let Some(skip) = skip
            && path.file_name().is_some_and(|name| name == skip)
        {
            continue;
        }
        let text = fs::read_to_string(&path).expect("source file is readable");
        for (number, line) in text.lines().enumerate() {
            // The guards name their own forbidden strings; a doc comment
            // explaining the rule is not a violation of it.
            let code = line.split("//").next().unwrap_or(line);
            for pattern in patterns {
                if code.contains(pattern) {
                    violations.push(format!("{}:{}: `{pattern}`", path.display(), number + 1));
                }
            }
        }
    }
}

fn src() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn transcendentals_only_through_indicators_fmath() {
    let mut violations = Vec::new();
    scan(&src(), TRANSCENDENTALS, None, &mut violations);
    assert!(
        violations.is_empty(),
        "std transcendental calls (platform-dependent rounding breaks cross-platform \
         determinism — use `quantick_indicators::fmath`):\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_wall_clock_and_no_unordered_collections_in_the_logic() {
    let mut violations = Vec::new();
    scan(&src(), NON_DETERMINISM, Some(STOPWATCH), &mut violations);
    assert!(
        violations.is_empty(),
        "sources of run-to-run drift (time comes from `Trade::timestamp_ms`, ordered \
         collections from `BTreeMap`/`Vec`, session order from `library::scan`):\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_stopwatch_stays_inside_the_binary() {
    // The exception above is only safe while `main.rs` is a binary that
    // prints diagnostics. If the timing ever moves into the library, this
    // test is the tripwire: the file must not be reachable from `lib.rs`.
    let lib = fs::read_to_string(src().join("lib.rs")).expect("lib.rs is readable");
    assert!(
        !lib.contains("mod main"),
        "main.rs became part of the library, taking its wall clock with it"
    );
    let main = fs::read_to_string(src().join(STOPWATCH)).expect("main.rs is readable");
    assert!(
        main.contains("Instant"),
        "the stopwatch exception exists for a stopwatch that is no longer there — \
         drop `STOPWATCH` from this guard rather than leaving a hole open"
    );
}
