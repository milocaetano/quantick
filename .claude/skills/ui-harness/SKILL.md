---
name: ui-harness
description: How an agent drives and observes the quantick desktop app without a human clicking — env-var hooks to reach every UI surface, the screenshot capture workflow, and the rule that every new surface must register a hook. Use when launching the app for validation, capturing screenshots, adding a new UI surface, or when another skill (visual-qa, trader-ux-review) needs to see the app.
---

# UI harness — drive the app without a mouse

The contract that makes autonomous visual work possible:

> **Every user-visible surface (panel, layer, tab, popup, demo flow) must be
> reachable from a fresh launch via environment hooks alone — zero clicks.**

A PR that adds a surface without a hook leaves that surface untestable by
agents; that is a Should-fix in review. Hooks follow the existing family:
`QUANTICK_<SURFACE>_AUTOSTART=1` reuses the exact code path of the manual
toggle — never a parallel activation path — and defaults to off, so a hook
never changes behaviour for a user who did not set it.

## Hook registry

The registry lives in `references/hook-registry.md`, beside this file. It is
one row per hook — `| QUANTICK_… | what it reaches |` — so **grep it for the
surface you need** rather than reading it whole:

```sh
grep -i 'heatmap\|book' .claude/skills/ui-harness/references/hook-registry.md
```

It was moved out of this file rather than shortened, and no row was dropped.
The table is 61KB — four fifths of this skill — and it is *data*, looked up one
row at a time by a run that drives one or two surfaces. Loading it whole to
answer "what turns the heatmap on" was the single largest token cost in this
repository's whole agentic flow, paid on every capture. A grep answers the same
question from the same rows.

Read it whole when you genuinely need the whole thing: taking inventory of what
is reachable, or auditing coverage before a release. That is the rare case, and
it now costs the same as it always did instead of being charged to every run.

Code is the source of truth either way — `grep env::var crates/app/src` — and
the table is the index. A hook the registry does not list is a hook nobody can
find; see *Adding a new hook* below, which is unchanged and still requires the
row.

## Launch and capture workflow

1. **Own target dir, on a drive with room**: build with
   `CARGO_TARGET_DIR=D:\quantick-agent-target` so the user's running exe is
   never locked and rust-analyzer never poisons fingerprints. It was `F:` until
   that drive stopped existing — check `Get-PSDrive -PSProvider FileSystem`
   before trusting this line, and pick the drive with free space: `C:` runs
   into single-digit gigabytes with a few worktrees on it, and a build that
   dies of ENOSPC looks like a compile error until you read the message.
2. **Fresh exe, proven fresh**: `cargo build -p quantick-app` immediately
   before capturing, then compare the exe `LastWriteTime` against your last
   edit. `cargo test` green does **not** imply the exe was rebuilt.
3. **Launch via PowerShell `Start-Process`** with hooks set and
   `RUST_LOG=quantick=info`, stderr to a log file. A bash background job
   produces a window whose GL surface never presents (pure-white captures).
4. **Capture by PID, never by window title**: use
   `heatmap-design-ref/capture_window.ps1` (PrintWindow with
   PW_RENDERFULLCONTENT) adapted to filter by the PID you launched — title
   matching grabs the wrong window when other instances or editors are open.
5. **Gate on health before trusting a capture**: `APP_HEALTH_SUMMARY` prints
   every 2 s. `fps≈59 / frame_avg≈16.7` → surface presents, capture is real.
   `fps≈19 / frame_avg≈52 / frame_cpu≈3` → occluded or idle desktop, capture
   will be blank; wait for fps ≥ 50 in the log and recapture. Blank capture
   is an environment state, not a render regression — run a `main` control
   build before blaming the change.
6. **Verify by pixel when the eye can be fooled**: read the PNG (e.g.
   `System.Drawing`) to count/locate marks, match dash signatures, or compare
   two frames; use `readable_min_radius` from `config/bubbles.toml` as the
   "too small to read" reference.
7. **Be a guest on the desktop**: never fight the user — if the window gets
   minimized or the mouse is active (`GetLastInputInfo` idle ≈ 0), stop
   driving, keep the evidence you have. Do not inject input with `SendInput`.
   Never bind a second MT5 listener on the user's port (9100) — use
   `QUANTICK_CONFIG` with an alternate `listen_addr`. Close every instance
   you opened when done.

Landing with the MT5 older-history goal (`feat/mt5-load-older`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_LOAD_OLDER=<pages>` | the toolbar's `+ older` button **pressed**, that many times, once the chart has bars to page back from. The button's point is what happens after the click — the prints prepended in front of what is already drawn — so a capture that can only photograph the enabled button proves the affordance exists and nothing about whether it works. Goes through `Tab::request_older_history`, the very function the click calls, so a hooked run drives the loading indicator too. Fires one page per frame and waits for each reply: the feed serves one request at a time, so pressing them together would photograph the refusal path instead of the feature. Waits up to `LOAD_OLDER_HOOK_FRAMES` (~10 s) for a first block, then gives up and logs `LOAD_OLDER_AUTOSTART_GAVE_UP` rather than hanging a capture run on a bridge that never connected. On MetaTrader it needs a bridge that declares `history_paging` (`bridge/mt5/quantick_bridge.py`, not the Expert Advisor); on a feed that cannot page, each press is answered empty and the chart is unchanged |
Once merged, move it into `references/hook-registry.md`.

Landing with the one-week candle default (`fix/frvp-candles-window-and-history`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_LOAD_OLDER_CANDLES=<spans>` | the history menu's `+ older candles` entry **pressed**, that many times, once the opening span has landed. A chart now opens on one week of venue candles (`feed::TIME_HISTORY_SPAN_MS`) and reaches the quarter a week at a time, so "what does a deep chart look like" is a state no capture reaches without a hand on the menu. Goes through `Tab::request_older_ohlcv_history`, the function the menu entry calls, so a hooked run drives the loading indicator and the prepend too. One span per frame, and it waits: the tab serves one candle request at a time. Spends `LOAD_OLDER_CANDLES_HOOK_FRAMES` (~60 s at 60 fps) across the whole run — much larger than the trade twin's, because a span is several slices of several pages and the reach is documented in thirteen of them — and every waiting frame costs a tick, so a venue that never answers gives up and logs `LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP` with the reason rather than hanging the run. The trade twin is `QUANTICK_LOAD_OLDER` — two records, two capabilities, two hooks: a feed can serve candle history without paging its tape |

Once merged, move it into `references/hook-registry.md`.

Landing with the load-older outcome (`fix/history-reach-speaks`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_HISTORY_NOTE=<ending>` | the **outcome** of a `+ older` press, in the loading lane where the spinner was — the one line that tells a trader their press reached nothing. Named by the ending's own log token and resolved through `CampaignEnd::from_action`: `nothing_coming_back` (the venue answered empty and the run gave up), `venue_exhausted` (the record is spent), `page_budget_spent` / `print_budget_spent` / `span_cap_covered` (stopped on a budget, press again), `nothing_charted`. `reach_met` raises nothing and says so in the log — a press that worked has the chart as its answer. Raised through `Tab::raise_history_note`, the same call a settled run makes, so the picture is the picture a refusing venue gives. Without this the surface is invisible to a capture: on any feed a validation run can arrange, the reach either lands its session or the source declares it cannot page and the button never takes a press. An unknown token raises no note rather than the wrong one |

Once merged, move it into `references/hook-registry.md`.

## Reading the running app through the control plane

A screenshot shows what a window looks like. It does not say what the
application *believes* — which market, which revision, how late the tape is,
whether a control is disabled and why. The control plane answers that in
structured data, and an assertion against it is worth more than an assertion
against pixels: it does not move when a colour does.

Prefer this over reading a capture whenever the question has a structured
answer. Keep the screenshot for the questions only pixels can answer — clipping,
font, composition, "does this read".

**The fixture.** Launch with local access enabled and the scopes the read
needs, per the table above:

```powershell
$env:QUANTICK_CONTROL_ACCESS = "1"
$env:QUANTICK_CONTROL_SCOPES = "all-reads,observe.evidence,observe.screenshot"
```

**The client.** `quantick-mcp` is a STDIO MCP server; feeding it JSON-RPC lines
is a complete client, no extra tooling. It discovers the running instance
itself and never starts one.

Build it first — step 1 of the launch workflow builds `quantick-app` only, and
the adapter is a separate binary in the same target directory:

```powershell
$target = "D:\quantick-agent-target"     # the same one the launch used
cargo build -p quantick-mcp
$mcp = Join-Path $target "debug\quantick-mcp.exe"
```

```powershell
$lines = @(
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ui-harness","version":"1"}}}',
  '{"jsonrpc":"2.0","method":"notifications/initialized"}',
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"quantick_get_scene","arguments":{}}}'
) -join "`n"
$lines | & $mcp --profile observer
```

Every answer is one JSON line on stdout; `result.structuredContent` is the
capability's own result, and `result.isError` with a `control.*` code is a
refusal you can branch on. Useful calls:

| Ask | Call |
| --- | --- |
| What is on screen, by name | `quantick_get_scene` |
| Which market, which bars, which layout | `quantick_get_snapshot` with the scopes |
| Is the frame healthy, is the tape late | `quantick_get_diagnostics` |
| What changed since I looked | `quantick_read_events` / `quantick_wait_for_change` |
| Everything at one instant, hashed | `quantick_capture_evidence` |

**Evidence bundles.** `quantick_capture_evidence` freezes the named scopes, the
events around them and the effective configuration into one hashed bundle and
answers with a manifest. Read it back with `quantick_invoke` on
`evidence.read`, page by page, and concatenate the base64 chunks: the bytes are
the bundle's canonical JSON and their SHA-256 is the manifest's
`content_digest`. Two fields decide whether an assertion is sound:

- `coverage` — what the capture left out, and why, as codes. A scope you did
  not name is in `omitted_scopes`; a field the application could not fill is in
  `unavailable_fields` with the JSON Pointer that finds it. `complete` is never
  true, and a capture never pretends to be the whole session.
- `screenshot.capture_revision` — equal to the bundle's own `capture_revision`,
  which is what makes `screenshot.control_regions` trustworthy: each named
  control's rectangle in the image, in physical pixels, with `within_image`
  saying whether the window was clipping it. That is the pair a visual defect
  is diagnosed from — the picture plus the names.

Without a client on the socket, `QUANTICK_CONTROL_EVIDENCE` takes the same
capture from a launch and logs the manifest as `CONTROL_EVIDENCE_CAPTURED`.
Bundles live in memory for fifteen minutes, are cleared when access is turned
off, and are never written to disk.

Landing with the history-reach goal (*a load-older press that reaches the previous session*):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_HISTORY_REACH=<token>` | pins how far one `+ older` press reaches, overriding what the workspace saved: `page` (one request of the page size — the press every release before this one had), `previous-session` (keep asking until the tape reaches past the market's last close plus a lead into the session before it), or `span` (keep asking until the chart holds `QUANTICK_HISTORY_REACH_SPAN_MINUTES` more of *traded* time). The tokens come from `HistoryReach::ALL`, the same list the history menu is drawn from, so a hook can reach every reach a trader can. A token this build does not know is refused out loud (`HISTORY_REACH_HOOK_UNKNOWN`) and leaves the current reach alone — a silent fallback would look like a press ignoring the run it was told to make. Pair it with `QUANTICK_LOAD_OLDER=1` to photograph a run in flight: with `previous-session` a single press keeps paging, so the loading indicator stays up across several replies |
| `QUANTICK_HISTORY_REACH` note for `QUANTICK_LOAD_OLDER` | with `previous-session` set, **one hooked press is one run**, not one request: the hook waits on the same loading task the run holds, so `QUANTICK_LOAD_OLDER=3` is three runs and not three pages. The `+ older` button itself is drawn disabled while a run is in flight (a press during one does nothing, and a live button that swallows it reads as broken), so a capture of the button mid-run photographs the greyed state and its reason — which is the state to photograph |
| `QUANTICK_HISTORY_REACH_SPAN_MINUTES=<n>` | how far one press of the `span` reach pulls, in minutes of **traded** time — nights and weekends are crossed to find them and add nothing to them. Pair with `QUANTICK_HISTORY_REACH=span`: the reach and how far it goes are one choice, and a hook that could pick `by time` but not say how much time would leave the operator setting half of it. Goes through `set_history_reach_span_minutes`, the same call the menu's box makes, and is clamped there to the campaign's own span cap — promising a reach the budgets forbid is worse than refusing it. A value that is not a whole number of minutes is logged (`HISTORY_REACH_SPAN_HOOK_UNREADABLE`) and ignored, never silently defaulted. Read back over the control plane as `history_reach_span_minutes` in the workspace summary |
| `QUANTICK_MENU=history` | the toolbar's **history menu** open on the first drawn frame — the reach chips, the `by time` span box, the page size, the trades-backfilled readout and the candle reach. Everything in that group sits behind a caret button, which a scripted run cannot press, so without this the whole menu — including the two reaches that shipped before it — is invisible to a capture. Same mechanism as `QUANTICK_MENU=workspace` and the same registry (`ScriptedMenu::ALL`): the click is delivered on the button's own published rectangle through the app's input path, so what opens is what a trader's click opens. A feed that pages nothing has no menu to open and the hook photographs that rather than forcing it; an unknown token opens nothing rather than the wrong menu |
| `QUANTICK_VENUE_LEAD_IN=1` | pins the View → *venue candles on charts cut by trades* switch on. Off by default and off for anything but `1`, because that is the whole point of the switch: a tick, volume, dollar or imbalance chart has always opened holding only the prints this session saw, and nothing goes in front of them unasked. On, the venue's own 1-minute candles are installed unfolded in front of a chart cut by trades — the only state in which a tick chart shows yesterday, and one no capture reaches without a hand in the View menu. Reaches `Tab::set_venue_lead_in`, the function the checkbox calls, so a hooked run refolds exactly as a click does |

Once merged, move these into `references/hook-registry.md`.

## Adding a new hook

New surface → new `QUANTICK_*` env hook in the same commit: read the var next
to the existing autostart block in `crates/app/src/app.rs`, call the same
function the manual toggle calls, default off. Then add one row to
`references/hook-registry.md`. That row is part of the feature's definition
of done: a hook nobody can find is a surface nobody can reach.
