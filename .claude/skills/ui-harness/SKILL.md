---
name: ui-harness
description: Drive, observe and visually QA the quantick desktop app without a human clicking — env-var hooks that reach every UI surface, the launch and screenshot workflow, reading the live control plane, and the QA pass (state matrix, defect checklist, PASS/FAIL with evidence). Use when launching the app for validation, capturing screenshots, adding a new UI surface, when a change touches UI, when the user asks "how does it look", or when trader-ux-review needs to see the app.
---

# UI harness — drive, observe and QA the app without a mouse

> **Every user-visible surface (panel, layer, tab, popup, demo flow) is
> reachable from a fresh launch by environment hooks alone — zero clicks.**

A surface shipped without a hook is a Should-fix. `QUANTICK_<SURFACE>_AUTOSTART=1`
reuses the manual toggle's exact code path — never a parallel activation — and
defaults off.

## Hooks

**Grep** `references/hook-registry.md` (one row per hook) for the surface you
need; read it whole only for an inventory or coverage audit:

```sh
grep -i 'heatmap\|book' .claude/skills/ui-harness/references/hook-registry.md
```

The registry is generated: rows come from the `declare_hooks!` line beside each
read plus `docs/ui-harness/hook-prose.md`, and a hook read but not described, or
described but not read, fails `cargo test -p quantick-guards`. Edit the prose,
then `cargo run -p quantick-app --features harness -- --dump-hook-registry`;
never hand-edit the registry. Its *Declared in* column says where each hook
lives; launch-phase hooks apply in `crates/app/src/app/launch_hooks.rs`, in
the order its doc comment fixes.

**Hooks compile only with a harness feature.** `--features harness` enables
all four families (`scenario-`, `control-`, `drawing-`, `quick-range-harness`);
a default build reads only the operator configuration `crates/app/src/launch.rs`
captures. An unregistered `QUANTICK_*` logs `UNKNOWN_HOOK` and turns saving
off for the session (`SAVES OFF` in the status line).

**Adding one** — a new surface gets its hook in the same commit: read the var,
call the manual toggle's function, default off; add the name to that module's
`crate::hooks::declare_hooks![…]` and a row to `hook-prose.md`; regenerate.
Where the read lives:

- a hook the window owns (menu opened, pointer parked, demo staged, history
  page asked, a frame budget) → `crates/app/src/harness.rs`: one `Harness`
  field, one `Harness::capture` line, one accessor named for its purpose;
- a floating surface's hook → that surface's module under
  `crates/app/src/surfaces/`, as an `apply_env_hook` the registry calls
  (`size.rs` fails one added to the trunk);
- stateless launch setters and the control/tab/replay/workspace clusters stay
  in `app/launch_hooks.rs`; a hook that keeps a field and needs only its own
  parsed value belongs in its owner.

None calls `std::env::var`: `main` captures every declared name once and an
owner asks `crate::hooks::captured::var`.

A second dimension on an existing hook is a defaulting field on its struct
(`DrawingsDemo`, `FrvpDemo`, `DrawingDraft`), never a new enum variant.

## Launch and capture

Raw captures stay outside Git; results and artifact links go in the PR.

1. **Own target dir with free space**: `CARGO_TARGET_DIR=D:\quantick-agent-target`,
   so the user's exe is never locked. Check `Get-PSDrive -PSProvider
   FileSystem` first; ENOSPC reads like a compile error.
2. **Fresh exe, proven**: `cargo build -p quantick-app --features harness`
   right before capturing,
   then compare the exe `LastWriteTime` with your last edit — green tests do
   not rebuild it.
3. **Launch with PowerShell `Start-Process`**, hooks set, `RUST_LOG=quantick=info`,
   stderr to a log. A bash background job never presents (white captures).
4. **Capture by PID**, never window title: `tools/capture_window.ps1`
   (PrintWindow, PW_RENDERFULLCONTENT) filtered to your PID.
5. **Gate on health**: `APP_HEALTH_SUMMARY` every 2 s. `fps≈59 /
   frame_avg≈16.7` is real; `fps≈19 / frame_avg≈52 / frame_cpu≈3` is an
   occluded or idle desktop — wait for fps ≥ 50 and recapture. A blank capture
   is environment; run a `main` control build before blaming the change.
6. **Verify by pixel** where the eye is fooled (counting marks, dash
   signatures, colours, frame diffs) — e.g. `System.Drawing`;
   `readable_min_radius` in `config/bubbles.toml` is the "too small" reference.
7. **Be a guest**: minimized window or an active mouse (`GetLastInputInfo`
   idle ≈ 0) means stop and keep your evidence. No `SendInput`. Never bind a
   second MT5 listener on port 9100 — `QUANTICK_CONFIG` with another
   `listen_addr`. Close every instance you opened.

## Ask the app, then look

A structured answer beats a pixel answer whenever both exist — it survives
colour, font and layout nudges; keep screenshots for clipping, font,
composition and "does this read". Launch a `--features harness` build with
`QUANTICK_CONTROL_ACCESS=1` and
the scopes, then use `quantick_get_scene` (controls by name, `selected`, the
coded reason one cannot be operated), `quantick_get_diagnostics` (frame and
tape numbers), and `quantick_capture_evidence` with `screenshot` (scene,
health, market and image at one revision, with `control_regions` per control).
The hand-driven client, its PowerShell stdin trap and the bundle fields:
`references/control-plane.md`.

## The QA pass

1. **Scope** — every surface the diff can affect, not only the target (a dock
   tab moves a splitter; a popup covers the tape). In doubt, in scope.
2. **State matrix** — each in-scope surface in: default open (BTC dense tape
   preset); feature on via its hook; popup/menu open over live data; empty
   data (no session, fills or depth); dense data (fast replay, deep book);
   narrow window (~1000 px) and the user's normal size; disabled state (e.g.
   replay has no depth — is the *why* visible?). Prefer replay
   (`QUANTICK_REPLAY_*`, WINJ26 sessions) for reproducibility; presets from
   `config/bubbles.toml`, never bare defaults.
3. **Read the scene first, then each capture** — answer explicitly; "it
   renders" is no verdict:
   - **Integrity** — nothing clipped, overlapping or off-window; splitters and
     neighbours intact.
   - **Readability** — text at the app's own readable size; contrast on the
     dark canvas; no truncated numbers (`1234…` on a price fails).
   - **Occlusion** — nothing covers live price, tape or forming bar.
   - **State honesty** — disabled controls explain themselves; inferred data
     labelled; an empty panel says why.
   - **Motion** — two captures ~1.2 s apart in one PowerShell call: flow
     advanced, no layout jump, the live region never frozen.
   - **Consistency** — the existing chip/button language; no one-off widget.
   - **Performance** — under dense data fps ≥ ~59 and no `APP_SLOW_FRAMES`
     bursts from the change; fps in the 50s with the feature against ~59 on a
     `main` control run (same hooks, same tape) is a FAIL once occlusion is
     ruled out.
   - A scene/image disagreement is a FAIL whichever half is wrong; say which
     you believe.
4. **Report** one verdict per surface × state, most severe first. **FAIL**:
   screenshot path, one-sentence defect, crop/coordinates, the control ID where
   the scene names it. **PASS**: the screenshot path and any structured
   reading (scene entry, diagnostics figure, evidence ID) — a PASS without
   evidence counts as not run. **BLOCKED**: what could not be observed and what
   was validated otherwise — never reported as PASS. PR gets verdicts,
   surfaces, IDs and measurements, optionally an artifact link; raw evidence
   never under `.claude/evidence/` or `docs/workflow/evidence/`. Fix FAILs,
   re-run only failed cells (a before/after pair, outside Git, only when
   another reviewer needs it); done when every cell is PASS or an accepted
   defect noted in the PR body.
