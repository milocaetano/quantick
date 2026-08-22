//! The registration assistant for Codex and Claude Code (contract §13).
//!
//! It prints the exact command that registers this binary as a local STDIO
//! MCP server, using the binary's own absolute path. It never writes a client
//! configuration file itself, and it never embeds a token, a user name or an
//! application launch: the adapter connects to an instance the user has
//! already opened.

use std::path::Path;

/// The clients the assistant knows how to register with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupClient {
    Codex,
    Claude,
}

impl SetupClient {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "codex" => Some(Self::Codex),
            "claude" | "claude-code" => Some(Self::Claude),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
        }
    }
}

/// The one-line registration command for a client.
pub fn registration_command(client: SetupClient, executable: &Path, profile: &str) -> String {
    let executable = executable.display();
    match client {
        SetupClient::Codex => {
            format!("codex mcp add quantick -- \"{executable}\" --profile {profile}")
        }
        SetupClient::Claude => format!(
            "claude mcp add --transport stdio --scope local quantick -- \"{executable}\" --profile {profile}"
        ),
    }
}

/// The full walkthrough the contract documents, with the command filled in.
pub fn walkthrough(client: SetupClient, executable: &Path, profile: &str) -> String {
    let command = registration_command(client, executable, profile);
    let verify = match client {
        SetupClient::Codex => "codex mcp list",
        SetupClient::Claude => "claude mcp get quantick   (or: claude mcp list)",
    };
    format!(
        "Register quantick-mcp with {label}\n\
         \n\
         1. Start Quantick and enable local agent access for this run\n   \
            (Tools > Local agent access > Enable observer access).\n\
         2. Register this binary as a local STDIO server:\n   \
            {command}\n\
         3. Verify the registration:\n   \
            {verify}\n\
         4. Start or restart the client and use /mcp to check the connection.\n\
         5. Call quantick_describe. With more than one instance running, pass\n   \
            the instance_id it lists before reading anything else.\n\
         \n\
         The adapter never starts Quantick and never stores a token: it discovers\n\
         the instance you opened through its private descriptor and connects with\n\
         the {profile} profile.\n",
        label = client.label(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commands_match_the_contract_and_carry_no_secret() {
        let exe = Path::new("C:\\tools\\quantick-mcp.exe");
        let codex = registration_command(SetupClient::Codex, exe, "observer");
        assert_eq!(
            codex,
            "codex mcp add quantick -- \"C:\\tools\\quantick-mcp.exe\" --profile observer"
        );
        let claude = registration_command(SetupClient::Claude, exe, "observer");
        assert!(claude.starts_with("claude mcp add --transport stdio --scope local quantick -- "));
        for text in [
            codex,
            claude,
            walkthrough(SetupClient::Claude, exe, "observer"),
        ] {
            assert!(!text.contains("bearer"));
            assert!(!text.contains("token="));
        }
        assert_eq!(SetupClient::parse("claude-code"), Some(SetupClient::Claude));
        assert_eq!(SetupClient::parse("cursor"), None);
    }
}
