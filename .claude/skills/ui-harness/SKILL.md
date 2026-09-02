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
find; see *Adding a new hook* below, which still requires the row.

For the code itself there is now one file to open rather than a grep:
**`crates/app/src/harness.rs`** owns every hook the window reads at launch —
the parse, the value, and the budget a multi-frame hook counts down. The
exception is a hook belonging to a floating surface, which parses itself beside
that surface. *Adding a new hook* below says which of the two a new hook is.

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
needs. The scope IDs, and what each one reaches, are the
`QUANTICK_CONTROL_ACCESS` / `QUANTICK_CONTROL_SCOPES` /
`QUANTICK_CONTROL_EVIDENCE` rows of `references/hook-registry.md` — named
rather than pointed at, because "the table above" stopped being true the day
the registry moved out of this file:

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

**Send a blank line first.** Windows PowerShell 5.1 writes a UTF-8 preamble to
a child's stdin the moment `Process.StandardInput` is touched, and it lands on
line 1 — so the `initialize` frame comes back `-32700 parse error: expected
value at line 1 column 1` while every line after it parses, which reads as "the
adapter is broken" and is not. A leading newline takes the preamble:

```powershell
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $mcp; $psi.Arguments = "--profile observer"
$psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true
$psi.UseShellExecute = $false
$m = [System.Diagnostics.Process]::Start($psi)
$nl = [char]10
$bytes = [System.Text.Encoding]::ASCII.GetBytes($nl + ($lines -join $nl) + $nl)
$m.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
$m.StandardInput.BaseStream.Flush(); $m.StandardInput.Close()
$m.StandardOutput.ReadToEnd()
```

**One instance at a time**, or discovery answers `control.instance_ambiguous`
and names the ids rather than choosing. Clear strays by **path**, never by
process name: `Get-Process quantick-app | Stop-Process` takes the trader's own
window down with yours, which is the *be a guest on the desktop* rule above,
broken by a one-liner.

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

## Adding a new hook

New surface → new `QUANTICK_*` env hook in the same commit: read the var, call
the same function the manual toggle calls, default off. Then add one row to
`references/hook-registry.md`. That row is part of the feature's definition
of done: a hook nobody can find is a surface nobody can reach.

**Where the var is read depends on what it reaches**, and getting this wrong is
now a build failure rather than a style note. There are exactly two homes, and
neither of them is the trunk:

- **A hook the window owns** — a menu pressed open, a pointer parked, a demo
  staged, a page of history asked for, a budget counted down over frames —
  lives in **`crates/app/src/harness.rs`**. One field on `Harness`, one line in
  `Harness::from_env`, one accessor named for what the hook is *for*. The
  trunk's own line is then the single call that asks for it. That module's
  header carries the argument; the short version is that twenty-three of
  `QuantickApp`'s ninety-eight fields used to be this wiring, so every module
  that touched the trunk saw them.
- **A hook a floating surface owns** — its hook lives **in that surface's own
  module** under `crates/app/src/surfaces/`, as an `apply_env_hook` the
  registry calls. Not another line in `app.rs`. That line is the fifth
  hand-written edit per feature the `Surface` port removes — the other four
  being the field, the initialiser, the draw call and the hotkey — and
  `crates/guards/src/size.rs` fails a branch that adds it to the trunk instead.

So: **a surface's hook goes beside the surface; every other hook goes in
`harness.rs`.** If you are about to add a `std::env::var` call to `app.rs`, the
answer is one of those two files instead.

**Prefer a defaulting field to a new variant.** A hook that already exists and
needs a second dimension — "the same demo, but shared across the split", "the
same profile, but left selected" — becomes a field on that hook's struct
(`DrawingsDemo`, `FrvpDemo`, `DrawingDraft`), defaulting to "did not ask". It
does not become a new arm of an enum, which reopens every call site that
matches on it. `ChartLayer`'s 21 variants across 264 call sites are what that
rule is written against.

That surface rule used to live at the bottom of the registry, which is now a
61KB data file this skill tells you to `grep` rather than read. An authoring
rule nobody reads is one the size guard enforces by failing you instead, so it
belongs here, beside the instruction it is the exception to.
