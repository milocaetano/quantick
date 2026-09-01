//! Runs the Python bridge's own test suites as part of `cargo test`.
//!
//! The bridge is a shipped component of this feature and it is where the live
//! bugs have been — its candle paging, and then where its opening block
//! started — so its tests belong in the same four checks as everything else,
//! not in a file someone has to remember to run. They cannot be written in
//! Rust: the logic under test is Python, and the terminal it talks to exists
//! only on Windows beside an installed MetaTrader. The Python side stubs that
//! away (`sys.modules`); this shells out to it.
//!
//! Every `test_*.py` under `bridge/mt5/tests/` is discovered and run, rather
//! than one script being named here. A suite added beside the others is then
//! covered the moment it exists — which is the difference between a check and
//! a check someone has to remember to wire up.
//!
//! No interpreter, no pass: skipping would hide exactly the regressions this
//! exists to catch, and every environment that builds this workspace can run a
//! Python script — CI's ubuntu image ships one, and the bridge itself is
//! unusable without one.

use std::path::PathBuf;
use std::process::Command;

/// Interpreter names tried in order, matching how the app launches the bridge:
/// `python3` where it is the only spelling, `python` on Windows.
const INTERPRETERS: [&str; 2] = ["python3", "python"];

/// Whether `interpreter` is really a Python and not a name that merely resolves.
///
/// Windows ships an "app execution alias" at `python3.exe` that is not Python
/// at all: it spawns successfully, prints "Python was not found; run without
/// arguments to install from the Microsoft Store" and exits non-zero. A shim
/// that only asks whether the spawn worked reads that as *the suites failed*,
/// and reports a red test on a machine whose real interpreter is one name
/// further down the list. Asking it to evaluate something first is what tells
/// a missing interpreter apart from a failing suite.
fn is_really_python(interpreter: &str) -> bool {
    Command::new(interpreter)
        .args(["-c", "print(1)"])
        .output()
        .is_ok_and(|output| output.status.success() && output.stdout.starts_with(b"1"))
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/feed-mt5.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Every bridge suite, by path, sorted so a failure names them in a stable
/// order.
///
/// `harness.py` is deliberately not among them: it holds the fake terminal the
/// suites share and has no tests of its own, so running it would pass
/// vacuously and imply coverage that is not there.
fn suites() -> Vec<PathBuf> {
    let dir = repo_root().join("bridge/mt5/tests");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| {
            panic!("the bridge's tests are missing: {}: {error}", dir.display())
        })
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "py")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("test_"))
        })
        .collect();
    found.sort();
    assert!(
        !found.is_empty(),
        "no test_*.py found under {} — discovery would then pass by finding \
         nothing, which is the one way this check can lie",
        dir.display()
    );
    found
}

#[test]
fn the_bridge_python_tests_pass() {
    let suites = suites();
    let mut tried = Vec::new();
    for interpreter in INTERPRETERS {
        if !is_really_python(interpreter) {
            tried.push(interpreter);
            continue;
        }
        let mut failures = Vec::new();
        let mut missing_interpreter = false;
        for suite in &suites {
            // An interpreter that is not installed has to be told apart from
            // one that ran and reported failures, and only the spawn error can
            // do that.
            let output = match Command::new(interpreter).arg(suite).output() {
                Ok(output) => output,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tried.push(interpreter);
                    missing_interpreter = true;
                    break;
                }
                Err(error) => panic!("could not run {interpreter}: {error}"),
            };
            if !output.status.success() {
                failures.push(format!(
                    "--- {} ---\n--- stdout ---\n{}\n--- stderr ---\n{}",
                    suite.display(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ));
            }
        }
        if missing_interpreter {
            continue;
        }
        assert!(
            failures.is_empty(),
            "{} of the bridge's {} Python suites failed:\n{}",
            failures.len(),
            suites.len(),
            failures.join("\n")
        );
        return;
    }
    panic!(
        "no Python interpreter found (tried {}), so the bridge's tests could \
         not run. The bridge needs one to work at all; install Python.",
        tried.join(" or ")
    );
}
