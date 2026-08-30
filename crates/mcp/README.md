# quantick-mcp

The MCP adapter for the Quantick control plane: a local STDIO server that an
MCP client (Codex, Claude Code, …) launches as a subprocess. It discovers a
Quantick instance that is **already running** with local agent access
enabled, authenticates with the descriptor that instance published, and
exposes a small, fixed tool set at the ceiling of the profile the trader
granted.

It never starts Quantick. With no running instance, `quantick_describe` lists
none and says what to do next.

## Tools

Every connection gets the observer set. A tool is a name for the instance's
own capabilities, never a second vocabulary beside them: most map to exactly
one, the routed ones name a fixed set and let a property pick which, and
`quantick_invoke` reaches whatever the instance registers.

| Tool | Capability | What it answers |
| --- | --- | --- |
| `quantick_describe` | `control.describe` | Without `instance_id`: the live instances. With one: version, profile, scopes, modules, capabilities and their availability, snapshot scopes, limits. Call it first. |
| `quantick_get_snapshot` | `snapshot.read` | One coherent capture of the requested scopes, taken in a single pass on the application thread, with one capture revision and every module revision it observed. |
| `quantick_get_chart_window` | `chart.window.read` | A paginated, append-only page of closed bars for one pane: OHLC, volume, delta, trade count and timestamps as exact decimal strings. The forming bar belongs to the snapshot's `chart.summary` scope, not to this series. |
| `quantick_get_scene` | `scene.read` | Every control on screen with a frame-stable ID, its owner, whether it is selected, and a coded reason when it cannot be operated. Chart canvases carry their rectangle in logical points, not device pixels. |
| `quantick_get_diagnostics` | `health.diagnostics.read` | The bounded health view: frame timing, feed arrival, order-flow engine state, worker and queue metrics, recent error counts. |
| `quantick_capture_evidence` | `evidence.capture` | A hashed, redacted bundle of the named scopes, the events around them and the effective configuration, held in memory for a bounded time. Answers with a manifest and says in codes — never prose — what it does *not* carry. Read it back through `quantick_invoke` on `evidence.read`. Nothing is written to disk. |
| `quantick_read_events` | `events.read` | A page of the semantic event journal after a cursor or from `oldest`/`latest`, with `dropped_before` when retention passed the cursor. |
| `quantick_wait_for_change` | `events.wait` | Parks (≤ 30 s) until the journal moves past the cursor, then the page that completes the call. |
| `quantick_search_capabilities` | `control.describe`, filtered | Capabilities and scopes by substring or module, with availability and the reason when one is unavailable. |
| `quantick_invoke` | any registered capability | The long tail, under the same authority checks as the named tools. |

With the **annotator** profile the trader granted, the tool set also carries
the half of the loop that answers on the chart:

| Tool | Capability | What it does |
| --- | --- | --- |
| `quantick_annotate` | `annotate.label.create`, `annotate.arrow.create`, `annotate.zone.create` — routed by `object` | Places a label, arrow or zone at market-time and price coordinates. Visibly attributed to this client wherever the trader sees it. |
| `quantick_remove_annotation` | `annotate.remove` | Removes one object *an operator* placed. An object the trader drew by hand is refused, whatever ID is asked for. |
| `quantick_notify` | `notify.popup`, `notify.toast`, `notify.sound` — routed by `channel` | Raises one attributed, rate-limited notification. `sound` needs a scope of its own, off by default. None of them can be taken back. |
| `quantick_attach_script` | `indicator.script.attach` | Compiles Quantick Pine and attaches the indicator to the focused pane. A script that does not compile is refused with structured diagnostics — code, byte span, line, column, message, notes — so the next attempt can fix the exact span. |
| `quantick_detach_script` | `indicator.script.detach` | Removes one slot this client attached, leaving the pane as it was. |

The **cockpit** profile is a superset of the annotator's: the same tools, plus
two modules reached through `quantick_invoke`. `cockpit.layout` unlocks the
`layout.*` capabilities — panes, layout tabs, presets and focus — and the
`cockpit` permission on its own unlocks `feed.reconnect`, which respawns the
live market transport. Neither is destructive.

`feed.reload` is, and it sits outside what this adapter can reach: it also
requires the separately sensitive `cockpit.recover`, declares
`reversible: false` with the risk flag `timeline_rebuilt`, and closes any open
paper position and disarms every strategy. `main.rs` asks for `cockpit` and
`cockpit.layout` only, so a cockpit connection through `quantick-mcp` cannot
invoke it — the contract comment on `COCKPIT_RECOVER_PERMISSION_ID` is the
reason it is a separate permission at all.

Every instance-bound tool takes an optional routing `instance_id`, removed
before the payload reaches the instance. With one live instance it is
selected; with several and no choice the call fails with
`control.instance_ambiguous` and the choices — never the newest window.

## Running

```text
quantick-mcp [serve] [--profile observer|annotator|cockpit] [--instance <id>] [--instances-dir <path>]
quantick-mcp setup --client codex|claude [--profile observer|annotator|cockpit]
```

`stdout` carries MCP frames only; diagnostics go to `stderr`. `setup` prints
the registration command for the client (see
`docs/control-plane/control-contract.md` §13) with this binary's absolute
path. It writes no configuration file, embeds no token and launches nothing.

## Shape

A leaf crate: `quantick-mcp` → `quantick-control-local` → `quantick-control`.
Never the application. `link::ControlLink` is the port to a running instance;
`link::LocalLink` implements it over the local transport and `fake::FakeLink`
for tests. The tool input schemas are the committed contract documents under
`schemas/control/`, embedded so the tool list describes exactly what the
instance validates.

The profiles are a chain, so a client that moves up a tier must never lose a
tool it had — a regression the cockpit ceiling once caused, and which
`tools.rs` now covers with its own test.
