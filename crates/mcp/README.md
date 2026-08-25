# quantick-mcp

The MCP adapter for the Quantick control plane: a local STDIO server that an
MCP client (Codex, Claude Code, …) launches as a subprocess. It discovers a
Quantick instance that is **already running** with local agent access
enabled, authenticates with the descriptor that instance published, and
exposes a small, fixed tool set over the gateway's observer capabilities.

It never starts Quantick. With no running instance, `quantick_describe` lists
none and says what to do next.

## Tools

| Tool | Capability | What it answers |
| --- | --- | --- |
| `quantick_describe` | `control.describe` | Without `instance_id`: the live instances. With one: version, profile, scopes, modules, capabilities and their availability, snapshot scopes, limits. |
| `quantick_get_snapshot` | `snapshot.read` | One coherent capture of the requested scopes with a capture revision. |
| `quantick_get_chart_window` | `chart.window.read` | A paginated, append-only page of closed bars for one pane. |
| `quantick_get_diagnostics` | `health.diagnostics.read` | The bounded health view. |
| `quantick_read_events` | `events.read` | A page of the semantic event journal after a cursor or from `oldest`/`latest`, with `dropped_before` when retention passed the cursor. |
| `quantick_wait_for_change` | `events.wait` | Parks (≤ 30 s) until the journal moves past the cursor, then the page that completes the call. |
| `quantick_search_capabilities` | `control.describe`, filtered | Capabilities and scopes by substring or module, with availability. |
| `quantick_invoke` | any registered capability | The long tail, under the same authority checks as the named tools. |

Every instance-bound tool takes an optional routing `instance_id`, removed
before the payload reaches the instance. With one live instance it is
selected; with several and no choice the call fails with
`control.instance_ambiguous` and the choices — never the newest window.

## Running

```text
quantick-mcp [serve] [--profile observer] [--instance <id>] [--instances-dir <path>]
quantick-mcp setup --client codex|claude
```

`stdout` carries MCP frames only; diagnostics go to `stderr`. `setup` prints
the registration command for the client (see
`docs/control-plane/control-contract.md` §13).

## Shape

A leaf crate: `quantick-mcp` → `quantick-control-local` → `quantick-control`.
Never the application. `link::ControlLink` is the port to a running instance;
`link::LocalLink` implements it over the local transport and `fake::FakeLink`
for tests. The tool input schemas are the committed contract documents under
`schemas/control/`, embedded so the tool list describes exactly what the
instance validates.
