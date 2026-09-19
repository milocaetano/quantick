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
| `quantick_capture_chart` | `evidence.capture`, `evidence.read` | A native MCP PNG image of the entire Quantick window plus capture metadata. Validates every chunk and the assembled document/image before returning pixels; requires the screenshot and evidence grants. |
| `quantick_read_events` | `events.read` | A page of the semantic event journal after a cursor or from `oldest`/`latest`, with `dropped_before` when retention passed the cursor. |
| `quantick_wait_for_change` | `events.wait` | Parks (≤ 30 s) until the journal moves past the cursor, then the page that completes the call. |
| `quantick_search_capabilities` | `control.describe`, filtered | Capabilities and scopes by substring or module, with availability and the reason when one is unavailable. |
| `quantick_invoke` | any registered capability | The long tail, under the same authority checks as the named tools. Omit `capability_version` and it calls the newest version the instance registers for that ID, read from `control.describe`; the result's `capability_version` says which one answered, and a refused describe is returned as the answer, retryable as given. An explicit version is sent as given. |

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
live market transport, and `feed.deal_recording.set`, which starts or stops a
MetaTrader B3 tab's deal recording (the REC control, as a call). None of them
is destructive.

`feed.reload` is, and it sits outside what this adapter can reach: it also
requires the separately sensitive `cockpit.recover`, declares
`reversible: false` with the risk flag `timeline_rebuilt`, and closes any open
paper position and disarms every strategy. `main.rs` asks for `cockpit` and
`cockpit.layout` only, so a cockpit connection through `quantick-mcp` cannot
invoke it — the contract comment on `COCKPIT_RECOVER_PERMISSION_ID` is the
reason it is a separate permission at all.

The contract chains a fourth profile, `trader` (place, bracket and cancel
orders). Its `trade` permission is sensitive with `default_grant: Denied`, the
access panel filters it out, and `quantick-mcp` never requests it, so no
connection reaches it today. A capability the trader did not grant is refused
with `control.permission_denied` whatever the connection asked for and
whichever tool it came through — `quantick_invoke` is checked exactly like a
named tool.

Two things worth knowing before writing a client:

- **Park, do not poll.** A trader pressing the mark hotkey puts the resolved
  thing under the pointer into the journal, so the intended loop is *wait
  (`quantick_wait_for_change`), read the mark, answer about that bar* — not a
  screenshot every second.
- **The scene and the cursor share IDs.** The cursor scope answers with the
  scene's control IDs. Canvas rectangles are logical points: apply the display
  scale factor before composing them with a screenshot.

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

## Native chart image: observer setup and real smoke test

From the task checkout, build both executables. In PowerShell:

```powershell
cargo build -p quantick-app -p quantick-mcp
& .\target\debug\quantick-mcp.exe setup --client codex --profile observer
```

Run the `codex mcp add quantick -- "<absolute executable path>" --profile observer`
command printed by setup. Setup itself changes no configuration. If a Quantick
registration already exists, inspect it before replacing it. Restart the Codex
connection after registration or an executable update.

Start the tested Quantick executable with a real desktop/GPU. In **Tools >
Local agent access**, choose observer and enable only the required permissions:
`observe`, `observe.workspace`, `observe.attention`, `observe.market` (for
`scene.controls`), `observe.events`, `observe.system` (for the bundle),
`observe.evidence`, and `observe.screenshot`. The `observe` floor is automatic,
not a selectable checkbox or a token accepted by the scope hook.
Capture does not require paper, user-text, annotate,
cockpit or trade grants. For an isolated harness launch, the equivalent is:

```powershell
$env:QUANTICK_CONTROL_ACCESS = "1"
$env:QUANTICK_CONTROL_SCOPES = "observe.workspace,observe.attention,observe.market,observe.events,observe.system,observe.evidence,observe.screenshot"
```

Use the UI harness's isolated store overrides and a replay fixture when
launching a validation window; an ordinary unisolated launch can persist the
user's workspace. Do not reuse an occupied MT5 port or close another instance.

In Codex:

1. Call `quantick_describe` and select the tested window's `instance_id`.
2. Call `quantick_describe` with that ID; verify observer and the granted scopes.
3. Call `quantick_capture_chart` with `{"instance_id":"<selected ID>"}`.
4. Verify `isError: false`, one `content` item with `type: "image"` and
   `mimeType: "image/png"`, and decode its base64 with a PNG viewer. Check that
   it shows the tested window and its chart, not a blank framebuffer.
5. Check `structuredContent.image_available`, `capture_revision`,
   `screenshot.width_px/height_px`, `control_regions`, and `coverage`. The
   base64 must occur only in the image item. The window displays its normal
   capture notice. Confirm the observer tool list contains no order tools.
6. Revoke screenshot access and call again: it must refuse without an image.
   Close the validation window through its normal close path when finished.

Record executable SHA-256, source revision/dirty diff, dimensions, image hash,
tool metadata and the viewed PNG as smoke evidence outside the source tree.
Unit tests inject pixels and exercise MCP framing; they do not prove GPU
presentation or that a particular Codex version displays the returned image.

The tool requests only `scene.controls` and one recent event, reads the retained
resource in bounded pages, pins all reads to the instance that captured it,
checks chunk offsets/sizes/digests, the document digest and identities, and PNG
size/digest/header geometry. It forwards the already encoded PNG without
decompressing/re-encoding it. The gateway rechecks grants and lifetime on every
page; any refusal stops the call without retry or partial image. A missing
screenshot is an error with `image_available: false` and the gateway's coverage
reasons. No image or bundle is written to disk by the adapter.

**Privacy and timing:** the image covers the whole window, including visible
user text and account information. JSON redaction does not mask those pixels;
the separate screenshot grant is essential. `screenshot.state_skew` remains in
coverage: the image can precede numerical projections by one feed drain. A shared
capture revision correlates the scene and image; it does not prove identical
market values at the same instant. Capture is a read, but it displays a notice
and creates a short-lived evidence resource, so it is not marked idempotent.

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
