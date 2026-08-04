//! Runs the Python bridge's paging tests as part of `cargo test`.
//!
//! The bridge is a shipped component of this feature and its candle paging is
//! where the live bug was, so its tests belong in the same four checks as
//! everything else — not in a file someone has to remember to run. They cannot
//! be written in Rust: the logic under test is Python, and the terminal it
//! talks to exists only on Windows beside an installed MetaTrader. The Python
//! side stubs that away (`sys.modules`); this shells out to it.
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

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/feed-mt5.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn the_bridge_paging_tests_pass() {
    let script = repo_root().join("bridge/mt5/tests/test_paging.py");
    assert!(
        script.is_file(),
        "the bridge's paging tests are missing: {}",
        script.display()
    );

    let mut tried = Vec::new();
    for interpreter in INTERPRETERS {
        let output = match Command::new(interpreter).arg(&script).output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tried.push(interpreter);
                continue;
            }
            Err(error) => panic!("could not run {interpreter}: {error}"),
        };
        assert!(
            output.status.success(),
            "the bridge's paging tests failed.\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }
    panic!(
        "no Python interpreter found (tried {}), so the bridge's paging tests \
         could not run. The bridge needs one to work at all; install Python.",
        tried.join(" or ")
    );
}
