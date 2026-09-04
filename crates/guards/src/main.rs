//! The guards as a command, for the two moments a `cargo test` run is the
//! wrong shape of answer.
//!
//! `--file <path>` is the edit-time question: one file, one baseline read,
//! milliseconds. The hook in `.claude/hooks/` calls it after every write so a
//! ceiling is crossed and reported in the same breath, instead of surfacing
//! four minutes later at the end of a full suite run.
//!
//! `--tighten` is the other one. A ratchet asks for a file that shrank
//! to have its entry lowered, and that direction is always good news with the
//! correct number already computed — there is nothing for a human to decide,
//! only something to type. So the command types it.
//!
//! Growth is deliberately not automated. Raising a ceiling is the decision a
//! reviewer must be able to argue with, and a tool that raised it silently
//! would give back exactly the invisibility the guard exists to remove.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use quantick_guards::{GUARDS, ratchet, remedies, report, workspace_root};

/// Turn whatever the caller typed into the workspace-relative spelling with
/// forward slashes that every guard is keyed on.
///
/// Without this, `--file ./crates/app/src/app.rs` and an absolute path — both
/// of which a shell tab-completes — fell out of scope in every guard and
/// exited 0 with no output. That is the silent all-clear this crate's own doc
/// comments spend paragraphs forbidding: a developer asks about a real file
/// and is told nothing, which reads exactly like clean.
///
/// A path that cannot be made relative is an error rather than a silence.
fn relative_to(root: &Path, argument: &str) -> Result<String, String> {
    let normalised = argument.replace('\\', "/");
    let trimmed = normalised.trim_start_matches("./").trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("--file was given an empty path".to_owned());
    }
    let candidate = Path::new(trimmed);
    if !candidate.is_absolute() {
        return Ok(trimmed.to_owned());
    }
    candidate
        .strip_prefix(root)
        .map(|rest| rest.to_string_lossy().replace('\\', "/"))
        .map_err(|_| {
            format!(
                "{argument} is not inside {} — the guards can only speak about files in the \
                 workspace they were pointed at",
                root.display()
            )
        })
}

// The modes are alternatives, not composable flags, and the usage says so.
// Written as an optional-flag grammar it read as though `--file x --tighten`
// would do both, while only the first argument was ever honoured — a mistyped
// invocation exiting 0 having done half of what was asked.
const USAGE: &str = "\
usage: quantick-guards (--file <path> | --tighten | --report)

  (no arguments)   run every guard over the repository
  --file <path>    run every guard over one workspace-relative path
  --tighten        lower any baseline entry whose file has shrunk, in every
                   ratchet, and the !budget totals to match -- only downward
  --report         print the repository's health numbers as a label<TAB>value
                   table; deterministic, so a diff between two runs is the
                   report of what changed between them

The three modes are alternatives; they cannot be combined.
Exit code 1 means a guard found something. --report exits 0: it measures the
tree, it does not judge it.";

fn main() -> ExitCode {
    // A set-but-empty variable is not a root. `var_os` hands back
    // `Some("")` for it, which resolves every later `join` against the
    // process working directory instead — the baseline read then fails while
    // the other guards report clean.
    let root = std::env::var_os("QUANTICK_GUARDS_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(workspace_root);
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None => run_guards(&root, None),
        Some("--tighten") if args.len() == 1 => tighten(&root),
        // Printed rather than returned as findings, and always exit 0. These
        // numbers describe the tree; none of them is ratcheted, and a
        // measurement that can fail a build is one people negotiate with
        // instead of reading.
        Some("--report") if args.len() == 1 => {
            print!("{}", report::render(&root));
            ExitCode::SUCCESS
        }
        Some("--file") if args.len() == 2 => match relative_to(&root, &args[1]) {
            Ok(relative) => run_guards(&root, Some(relative)),
            Err(problem) => {
                eprintln!("{problem}\n\n{USAGE}");
                ExitCode::FAILURE
            }
        },
        Some("--file") if args.len() < 2 => {
            eprintln!("--file needs a path\n\n{USAGE}");
            ExitCode::FAILURE
        }
        // Everything left over is refused rather than ignored. Silently
        // dropping a trailing argument is how a caller ends up trusting a
        // clean exit for a run that never did what they asked.
        Some(mode @ ("--file" | "--tighten" | "--report")) => {
            // `--file` consumes the path beside it; the other two consume
            // nothing. Naming only what is actually unconsumed keeps the
            // message from accusing the caller of the argument they got right.
            let consumed = if mode == "--file" { 2 } else { 1 };
            eprintln!(
                "unexpected extra arguments: {}\n\n{USAGE}",
                args[consumed..].join(" ")
            );
            ExitCode::FAILURE
        }
        Some(other) => {
            eprintln!("unrecognised argument `{other}`\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Run every guard, over the whole repository or over one file, and print
/// what each found.
fn run_guards(root: &std::path::Path, only: Option<String>) -> ExitCode {
    let mut clean = true;
    for guard in GUARDS {
        let violations = match &only {
            Some(path) => (guard.check_file)(root, path),
            None => (guard.check)(root),
        };
        if violations.is_empty() {
            continue;
        }
        clean = false;
        eprintln!("{}: {} finding(s)", guard.name, violations.len());
        for violation in &violations {
            eprintln!("{}", violation.line);
        }
        for remedy in remedies(&violations) {
            eprintln!("\n{remedy}\n");
        }
    }
    if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Lower every baseline entry whose file has shrunk past the slack, and the
/// `!budget` total with it. Both directions are down only.
///
/// Every ratchet in the registry runs, and the registry is the only list —
/// a ratchet named here as well would be one somebody could add to `GUARDS`
/// and forget, after which it tightens nothing and says so nowhere. A
/// command that tightened code and quietly left the context or cycle
/// ceilings stale would report the good news it happened to know about, and
/// the author would find the other half as a failing test later, which is
/// exactly the shape of surprise this flag exists to remove.
fn tighten(root: &std::path::Path) -> ExitCode {
    let mut failed = false;
    for guard in GUARDS {
        let Some(ratchet) = &guard.ratchet else {
            continue;
        };
        failed |= tighten_one(
            root,
            guard.name,
            (ratchet.tighten)(root),
            ratchet.policy.baseline_file,
            ratchet.policy.budget_slack,
        );
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Report one ratchet's tightening. Returns whether it failed.
fn tighten_one(
    root: &std::path::Path,
    name: &str,
    result: Result<Vec<String>, String>,
    baseline_file: &str,
    slack: usize,
) -> bool {
    match result {
        Err(problem) => {
            eprintln!("{problem}");
            true
        }
        Ok(applied) if applied.is_empty() => {
            println!(
                "nothing to tighten in the {name} ratchet: no tracked file has shrunk past its \
                 slack, and the tracked total is within {slack} of the {}",
                ratchet::BUDGET_DIRECTIVE
            );
            false
        }
        Ok(applied) => {
            // The resolved absolute path, not the relative spelling. The root
            // is compiled in from whichever worktree built this binary, and
            // this repo runs many at once — a relative path in the success
            // line reads as the caller's own tree while the edit landed in
            // someone else's, whose `git status` then carries a change nobody
            // made on purpose.
            println!(
                "tightened {} line(s) in {}:",
                applied.len(),
                root.join(baseline_file).display()
            );
            for line in &applied {
                println!("{line}");
            }
            false
        }
    }
}
