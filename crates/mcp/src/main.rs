//! `quantick-mcp`: the local STDIO MCP server for a running Quantick.
//!
//! Standard output carries MCP frames and nothing else; every diagnostic goes
//! to standard error. The binary never starts Quantick.

use std::{
    collections::BTreeSet,
    io::{self, BufReader, Write as _},
    path::PathBuf,
    process::ExitCode,
};

use quantick_control::id::{InstanceId, PermissionId};
use quantick_control_local::client::ConnectOptions;
use quantick_mcp::{
    link::LocalLink,
    server::McpServer,
    setup::{SetupClient, walkthrough},
};

/// The only profile ceiling this release grants. Later profiles arrive with
/// their threat-model extensions, not with a flag.
const AVAILABLE_PROFILES: &[&str] = &["observer"];

/// The scopes an observer connection asks for. The gateway intersects them
/// with the user's grant; asking for a scope never grants it.
const OBSERVER_SCOPES: &[&str] = &[
    "observe",
    "observe.system",
    "observe.workspace",
    "observe.market",
    "observe.chart",
    "observe.indicators",
    "observe.drawings",
    "observe.orderflow",
    "observe.replay",
    "observe.health",
    "observe.attention",
    "observe.events",
];

const USAGE: &str = "usage:
  quantick-mcp [serve] [--profile observer] [--instance <instance_id>] [--instances-dir <path>]
  quantick-mcp setup --client <codex|claude> [--profile observer]
  quantick-mcp --help

serve (default)  run the MCP server over standard input/output; stdout carries
                 MCP frames only, diagnostics go to stderr.
  --profile      the authority ceiling to request; only `observer` exists yet.
  --instance     pin every call to one running instance by its instance_id.
  --instances-dir
                 read descriptors from this directory instead of the platform's
                 private runtime directory (tests and development only).
setup            print the command that registers this binary with a client.";

enum Command {
    Serve {
        profile: String,
        instance: Option<InstanceId>,
        instances_dir: Option<PathBuf>,
    },
    Setup {
        client: SetupClient,
        profile: String,
    },
    Help,
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut rest = args.iter().map(String::as_str).peekable();
    let mode = match rest.peek().copied() {
        Some("setup") => {
            rest.next();
            "setup"
        }
        Some("serve") => {
            rest.next();
            "serve"
        }
        Some("--help" | "-h") => return Ok(Command::Help),
        _ => "serve",
    };
    let mut profile = "observer".to_owned();
    let mut instance = None;
    let mut instances_dir = None;
    let mut client = None;
    while let Some(flag) = rest.next() {
        let value = |rest: &mut std::iter::Peekable<_>| -> Result<String, String> {
            rest.next()
                .map(str::to_owned)
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag {
            "--profile" => profile = value(&mut rest)?,
            "--instance" => {
                let raw = value(&mut rest)?;
                instance = Some(
                    InstanceId::new(raw)
                        .map_err(|_| "--instance is not a Quantick instance_id".to_owned())?,
                );
            }
            "--instances-dir" => instances_dir = Some(PathBuf::from(value(&mut rest)?)),
            "--client" => {
                let raw = value(&mut rest)?;
                client = Some(
                    SetupClient::parse(&raw)
                        .ok_or_else(|| format!("unknown client `{raw}`; use codex or claude"))?,
                );
            }
            "--help" | "-h" => return Ok(Command::Help),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    // A flag of the other mode is a mistake worth naming, not ignoring.
    match mode {
        "setup" if instance.is_some() || instances_dir.is_some() => {
            return Err("setup takes --client and --profile only".to_owned());
        }
        "serve" if client.is_some() => {
            return Err("--client belongs to `setup`".to_owned());
        }
        _ => {}
    }
    if !AVAILABLE_PROFILES.contains(&profile.as_str()) {
        return Err(format!(
            "profile `{profile}` is not available in this release; available: {}",
            AVAILABLE_PROFILES.join(", ")
        ));
    }
    match mode {
        "setup" => Ok(Command::Setup {
            client: client.ok_or_else(|| "setup needs --client codex|claude".to_owned())?,
            profile,
        }),
        _ => Ok(Command::Serve {
            profile,
            instance,
            instances_dir,
        }),
    }
}

fn main() -> ExitCode {
    // `args()` panics on a non-Unicode argument; a bad path is a usage error.
    let args: Result<Vec<String>, _> = std::env::args_os()
        .skip(1)
        .map(std::ffi::OsString::into_string)
        .collect();
    let Ok(args) = args else {
        eprintln!("quantick-mcp: an argument is not valid Unicode\n{USAGE}");
        return ExitCode::from(2);
    };
    let command = match parse_args(&args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("quantick-mcp: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => {
            // Help is the one thing that belongs on stdout outside a session.
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Setup { client, profile } => {
            let executable =
                std::env::current_exe().unwrap_or_else(|_| PathBuf::from("quantick-mcp"));
            print!("{}", walkthrough(client, &executable, &profile));
            ExitCode::SUCCESS
        }
        Command::Serve {
            profile,
            instance,
            instances_dir,
        } => serve(&profile, instance, instances_dir),
    }
}

fn serve(profile: &str, instance: Option<InstanceId>, instances_dir: Option<PathBuf>) -> ExitCode {
    let scopes: BTreeSet<PermissionId> = OBSERVER_SCOPES
        .iter()
        .map(|id| PermissionId::new(*id).expect("static observer scopes are valid"))
        .collect();
    let options = ConnectOptions::observer(
        format!("quantick-mcp {}", env!("CARGO_PKG_VERSION")),
        env!("CARGO_PKG_VERSION"),
        scopes,
    );
    let link = LocalLink::new(options, instances_dir, instance);
    let mut server = McpServer::new(Box::new(link), profile);
    eprintln!(
        "quantick-mcp {} serving over stdio (profile {profile}); it never starts Quantick",
        env!("CARGO_PKG_VERSION")
    );
    let stdin = io::stdin();
    let stdout = io::stdout();
    let result = server.serve(BufReader::new(stdin.lock()), stdout.lock());
    let _ = io::stderr().flush();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("quantick-mcp: stdio transport ended with an error: {error}");
            ExitCode::FAILURE
        }
    }
}
