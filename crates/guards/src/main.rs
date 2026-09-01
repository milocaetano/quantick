//! The guards as a command, for the two moments a `cargo test` run is the
//! wrong shape of answer.
//!
//! `--file <path>` is the edit-time question: one file, one baseline read,
//! milliseconds. The hook in `.claude/hooks/` calls it after every write so a
//! ceiling is crossed and reported in the same breath, instead of surfacing
//! four minutes later at the end of a full suite run.
//!
//! `--tighten` is the other one. The size ratchet asks for a file that shrank
//! to have its entry lowered, and that direction is always good news with the
//! correct number already computed — there is nothing for a human to decide,
//! only something to type. So the command types it.
//!
//! Growth is deliberately not automated. Raising a ceiling is the decision a
//! reviewer must be able to argue with, and a tool that raised it silently
//! would give back exactly the invisibility the guard exists to remove.

use std::path::PathBuf;
use std::process::ExitCode;

use quantick_guards::{GUARDS, size, workspace_root};

// The modes are alternatives, not composable flags, and the usage says so.
// Written as an optional-flag grammar it read as though `--file x --tighten`
// would do both, while only the first argument was ever honoured — a mistyped
// invocation exiting 0 having done half of what was asked.
const USAGE: &str = "\
usage: quantick-guards (--file <path> | --tighten)

  (no arguments)   run every guard over the repository
  --file <path>    run every guard over one workspace-relative path
  --tighten        lower any size-baseline entry whose file has shrunk

The two modes are alternatives; they cannot be combined.
Exit code 1 means a guard found something.";

fn main() -> ExitCode {
    // A set-but-empty variable is not a root. `var_os` hands back
    // `Some("")` for it, which resolves every later `join` against the
    // process working directory instead — the baseline read then fails while
    // two of the three guards report clean.
    let root = std::env::var_os("QUANTICK_GUARDS_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(workspace_root);
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        None => report(&root, None),
        Some("--tighten") if args.len() == 1 => tighten(&root),
        Some("--file") if args.len() == 2 => report(&root, Some(args[1].replace('\\', "/"))),
        Some("--file") if args.len() < 2 => {
            eprintln!("--file needs a path\n\n{USAGE}");
            ExitCode::FAILURE
        }
        // Everything left over is refused rather than ignored. Silently
        // dropping a trailing argument is how a caller ends up trusting a
        // clean exit for a run that never did what they asked.
        Some(mode @ ("--file" | "--tighten")) => {
            // `--file` consumes the path beside it; `--tighten` consumes
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
fn report(root: &std::path::Path, only: Option<String>) -> ExitCode {
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
            eprintln!("{violation}");
        }
        eprintln!("\n{}\n", guard.remedy);
    }
    if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Lower every size-baseline entry whose file has shrunk past the slack.
fn tighten(root: &std::path::Path) -> ExitCode {
    match size::tighten(root) {
        Err(problem) => {
            eprintln!("{problem}");
            ExitCode::FAILURE
        }
        Ok(applied) if applied.is_empty() => {
            println!("nothing to tighten: no tracked file has shrunk past the slack");
            ExitCode::SUCCESS
        }
        Ok(applied) => {
            println!(
                "tightened {} entr(ies) in {}:",
                applied.len(),
                size::BASELINE_FILE
            );
            for line in &applied {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
    }
}
