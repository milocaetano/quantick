//! Generate published control artifacts without linking the desktop application.
use quantick_control_handlers::generation::{Artifact, BuildIdentity, artifacts};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("regen-control: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut mode = None;
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" | "--write" | "--list" if mode.is_none() => mode = Some(arg),
            "--root" => {
                root = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--root needs a directory")?
            }
            "--help" => {
                println!("regen-control --check|--write|--list [--root DIRECTORY]");
                return Ok(());
            }
            _ => return Err(format!("unknown or repeated option: {arg}")),
        }
    }
    let mode = mode.ok_or("choose --check, --write, or --list (see --help)")?;
    let artifacts = artifacts(BuildIdentity {
        application_version: env!("CARGO_PKG_VERSION"),
        git_commit: None,
    })?;
    if mode == "--list" {
        for artifact in artifacts {
            println!("{}", artifact.path);
        }
        return Ok(());
    }
    if mode == "--write" {
        return write(&root, &artifacts);
    }
    let mut stale = Vec::new();
    for artifact in artifacts {
        match fs::read(root.join(&artifact.path)) {
            Ok(bytes) if bytes == artifact.contents.as_bytes() => (),
            Ok(_) => stale.push(format!("{}: bytes differ", artifact.path)),
            Err(error) => stale.push(format!("{}: {error}", artifact.path)),
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(stale.join("\n"))
    }
}

fn write(root: &Path, artifacts: &[Artifact]) -> Result<(), String> {
    // Stage every document before replacing any destination; a render/write
    // failure leaves existing artifacts intact. Rename never truncates a file.
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| {
        for artifact in artifacts {
            let destination = root.join(&artifact.path);
            let parent = destination
                .parent()
                .ok_or("artifact has no parent directory")?;
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
            let temporary = destination.with_extension("regen-control.tmp");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| format!("{}: {error}", temporary.display()))?;
            staged.push((temporary, destination));
            file.write_all(artifact.contents.as_bytes())
                .map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
        }
        for (temporary, destination) in &staged {
            fs::rename(temporary, destination)
                .map_err(|error| format!("{}: {error}", destination.display()))?;
        }
        Ok(())
    })();
    for (temporary, _) in staged {
        let _ = fs::remove_file(temporary);
    }
    result
}
