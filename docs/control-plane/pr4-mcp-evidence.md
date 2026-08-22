# PR 4 MCP adapter evidence

**Branch:** `feat/mcp-observer`

**Rate class:** Per tool call, in a separate process. Nothing in this PR runs
inside the application or on any of its threads.

## Result

`quantick-mcp` is the first adapter of the control plane: a local STDIO MCP
server that an MCP client launches as a subprocess. It discovers a Quantick
instance that is already running with local agent access enabled through
`quantick-control-local`, authenticates with the descriptor that instance
published, and exposes the fixed observer tool set over the gateway's
capabilities. It never starts Quantick.

The crate is a leaf: `quantick-mcp` → `quantick-control-local` →
`quantick-control`. It has no dependency on the application, no async runtime
and no MCP SDK; the JSON-RPC and MCP shapes it needs are a few hundred lines
it owns, which is what keeps standard output provably clean.

## Tool surface

Plan §7.1 and contract §8: a small fixed list, never one tool per capability.

| Tool | Capability | Annotations |
| --- | --- | --- |
| `quantick_describe` | `control.describe` (or the instance list without an ID) | read-only |
| `quantick_get_snapshot` | `snapshot.read` | read-only |
| `quantick_get_chart_window` | `chart.window.read` | read-only |
| `quantick_get_diagnostics` | `health.diagnostics.read` | read-only |
| `quantick_search_capabilities` | `control.describe`, filtered locally | read-only |
| `quantick_invoke` | any registered capability by ID | the ceiling's conservative hints |

`quantick_get_scene`, `quantick_read_events`, `quantick_wait_for_change` and
`quantick_capture_evidence` are not listed: their capabilities land with the
scene module, PR 5a and PR 5c, and adding a tool later is additive.

The tool input schemas are the committed contract documents
(`observer-snapshot-read-input-v1`, `observer-chart-window-input-v1`) with one
routing property added; the output schemas wrap the committed result documents
(`observer-describe-result-v1`, `observer-snapshot-capture-v1`,
`observer-chart-window-page-v1`) in the envelope fields the adapter returns
(instance ID, capture revision, module revisions, warnings). A test validates
every tool's input and output schema as a draft 2020-12 document.

## Instance selection

Every instance-bound tool accepts an optional `instance_id` that the adapter
removes before the capability payload is validated (contract §8). With exactly
one live instance it is selected; with none the call is
`control.instance_gone` with the discovery report's next steps; with several
and no choice it is `control.instance_ambiguous` listing the choices in
`(published_at_unix_ms, instance_id)` order — never the newest window.
`--instance <id>` pins every call and refuses a contradicting routing ID.
`quantick_describe` without an ID lists the live instances.

## Authority

The adapter requests the observer profile and the observer read scopes; the
gateway intersects them with the user's grant and asking never grants. Only
`observer` is accepted by `--profile` in this release; any other profile is
refused before serving with exit status 2 and nothing on stdout. Under the
observer ceiling `quantick_invoke` is annotated read-only and a write
capability ID is refused by the instance (`control.permission_denied` or
`control.capability_unknown`), which the adapter reports as a tool execution
error with the code.

## Standard output

`stdout` carries MCP frames only. The binary installs no tracing subscriber,
prints diagnostics with `eprintln!`, and the smoke test over real pipes feeds
initialize, tools/list, tools/call, a garbage line and ping, then closes stdin:
every stdout line parses as a JSON-RPC 2.0 frame, the process exits 0, and the
instances directory it was pointed at is never created.

## Setup assistant

`quantick-mcp setup --client codex|claude` prints the registration command of
contract §13 with the binary's own absolute path and the walkthrough. No
token, user name or application launch appears in it.

## Verification

- `cargo test -p quantick-mcp`: 21 unit tests (JSON-RPC parsing and frames,
  version negotiation, annotations, the fixed tool list, schema validity,
  routing-ID stripping, the fake link end to end through the server, the
  instructions' first 512 characters); 3 integration tests against a fake
  loopback gateway that publishes a real descriptor and accepts the contract's
  handshake with the reference registry (discovery, handshake, describe,
  snapshot, search, refused write, wrong instance, two instances ordered and
  never chosen silently, a pinned adapter); 3 stdio smoke tests over the built
  binary.
- The four workspace checks, recorded in the pull request.

## Acceptance against the plan

| PR 4 criterion | Status |
| --- | --- |
| Codex and Claude run describe, get_snapshot, get_chart_window, get_diagnostics against the same instance | Not run in this session: the live check needs a running desktop instance, which this session is not authorized to launch. The adapter is registered with the exact commands of contract §13 and the same calls are exercised against the fake gateway and the fake link. |
| Under the observer ceiling `quantick_invoke` is read-only, no write capability is available, attempted write IDs are denied | Annotations pinned by test; refusal proven against the fake gateway and the fake link |
| Disconnecting a client does not change application state | The adapter only reads; the gateway's own tests prove observer reads leave every module revision unchanged |
| Startup, errors and shutdown emit no non-MCP bytes on stdout | Smoke test over real pipes |
| All four repository checks pass | Recorded in the pull request |
