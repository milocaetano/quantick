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

/// The profile ceilings a client may ask for. Asking is not granting: the
/// instance intersects the request with what the trader ticked in its own
/// panel, so `--profile annotator` against a read-only grant connects as an
/// observer and every write is refused.
const AVAILABLE_PROFILES: &[&str] = &[
    quantick_control_local::client::OBSERVER_PROFILE_ID,
    quantick_control_local::client::ANNOTATOR_PROFILE_ID,
];

/// The scopes an annotator connection asks for on top of the observer's:
/// answering on the chart, interrupting, and attaching a script.
const ANNOTATOR_SCOPES: &[&str] = &[
    "annotate",
    "annotate.attention",
    "annotate.chart",
    "annotate.notification",
    "annotate.sound",
    "annotate.script",
];

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
    // Asked for and rarely granted: `observe.paper` is not one of the safe
    // defaults, so the panel starts with it off. Not asking at all would leave
    // `session.paper` refused even after the trader ticks it, which reads to a
    // client as the scope being broken rather than withheld.
    "observe.paper",
    // Asking is not granting: the trader still ticks it, and without the ask
    // the scope is refused even after they do.
    "observe.user_text",
    "observe.health",
    "observe.attention",
    "observe.events",
    // The evidence tier, for exactly the reason the two comments above give:
    // both are off in the panel until the trader ticks them, and a connection
    // that never asked is refused even after they do — which reads to a client
    // as `quantick_capture_evidence` being broken rather than withheld.
    "observe.evidence",
    "observe.screenshot",
];

const USAGE: &str = "usage:
  quantick-mcp [serve] [--profile <observer|annotator>] [--instance <instance_id>] [--instances-dir <path>]
  quantick-mcp setup --client <codex|claude> [--profile <observer|annotator>]
  quantick-mcp --help

serve (default)  run the MCP server over standard input/output; stdout carries
                 MCP frames only, diagnostics go to stderr.
  --profile      the authority ceiling to request: `observer` reads, `annotator`
                 also answers on the chart. The window grants it or it does not.
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
    let annotator = profile == quantick_control_local::client::ANNOTATOR_PROFILE_ID;
    let scopes: BTreeSet<PermissionId> = OBSERVER_SCOPES
        .iter()
        .chain(if annotator { ANNOTATOR_SCOPES } else { &[] })
        .map(|id| PermissionId::new(*id).expect("static scope IDs are valid"))
        .collect();
    let options = ConnectOptions::for_profile(
        profile,
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

#[cfg(test)]
mod tests {
    use super::OBSERVER_SCOPES;

    /// The committed capability catalog, as the application publishes it.
    const CATALOG: &str =
        include_str!("../../../schemas/control/observer-capability-catalog-v1.json");

    /// Every read this adapter can reach is a read it asked the scopes for.
    ///
    /// The gateway intersects the requested scopes with the trader's grant, so
    /// a scope this list forgets is refused *even after the trader ticks it* —
    /// which reads to a client as a broken tool rather than a withheld one.
    /// That is exactly how `quantick_capture_evidence` shipped in the first
    /// draft of this branch: registered, advertised, and permanently answering
    /// `control.scope_denied`.
    ///
    /// So the list is held against the registry rather than against memory. A
    /// module that registers a capability inside the observer ceiling and
    /// forgets to widen this list fails here, and is told which capability
    /// needs which scope.
    #[test]
    fn the_adapter_asks_for_every_scope_an_observer_capability_needs() {
        let catalog: serde_json::Value = serde_json::from_str(CATALOG).expect("the catalog parses");
        let observer_ceiling = catalog["permissions"]
            .as_array()
            .expect("the catalog lists permissions")
            .iter()
            .filter(|permission| {
                permission["profile_ceilings"]
                    .as_array()
                    .is_some_and(|ceilings| ceilings.iter().any(|id| id == "observer"))
            })
            .filter_map(|permission| permission["id"].as_str())
            .collect::<std::collections::BTreeSet<_>>();

        let mut missing = std::collections::BTreeSet::new();
        for capability in catalog["capabilities"]
            .as_array()
            .expect("the catalog lists capabilities")
        {
            let required = capability["required_permissions"]
                .as_array()
                .expect("a capability declares its permissions")
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>();
            // Only the reads an observer connection could ever reach: an
            // annotate action is outside this profile's ceiling by design.
            if !required.iter().all(|id| observer_ceiling.contains(id)) {
                continue;
            }
            for id in required {
                if !OBSERVER_SCOPES.contains(&id) {
                    missing.insert(format!("{id} (needed by {})", capability["id"]));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "the adapter never asks for these, so the gateway refuses them whatever the \
             trader grants: {missing:?}"
        );
    }
}
