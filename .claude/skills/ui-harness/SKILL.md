---
name: ui-harness
description: Drive, observe and visually QA quantick without clicks: env hooks for every UI surface, launch/capture workflow, live control plane, state matrix, defect checklist and evidenced PASS/FAIL. Use for app validation, screenshots, new UI surfaces, UI changes, "how does it look", or trader-ux-review.
---

# UI harness — drive, observe and QA the app without a mouse

> **Every user-visible surface (panel, layer, tab, popup, demo flow) is
> reachable from a fresh launch by environment hooks alone — zero clicks.**

A missing surface hook is a Should-fix. `QUANTICK_<SURFACE>_AUTOSTART=1`
reuses the manual toggle's exact path, never parallel activation; defaults off.

## Hooks

**Grep** `references/hook-registry.md` (one row per hook) for the surface you
need; read it whole only for an inventory or coverage audit:

```sh
grep -i 'heatmap\|book' .claude/skills/ui-harness/references/hook-registry.md
```

Registry rows are generated from `declare_hooks!` beside each read and
`docs/ui-harness/hook-prose.md`. Either a read without prose or prose without
a read fails `cargo test -p quantick-guards`. Edit the prose, then run
`cargo run -p quantick-app --features harness -- --dump-hook-registry`;
never hand-edit the registry. *Declared in* locates each hook; launch hooks
run in `crates/app/src/app/launch_hooks.rs` in its documented order.

**Hooks require a harness feature.** `--features harness` enables all four
families (`scenario-`, `control-`, `drawing-`, `quick-range-harness`). Default
builds read only operator config captured by `crates/app/src/launch.rs`.
Unregistered `QUANTICK_*` logs `UNKNOWN_HOOK` and disables session saving
(`SAVES OFF` in the status line).

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
- stateless setters and control/tab/replay/workspace clusters stay in
  `app/launch_hooks.rs`; stateful hooks needing only their own parsed value
  belong in their owner.

None calls `std::env::var`: `main` captures every declared name once and an
owner asks `crate::hooks::captured::var`.

Extend an existing hook with a defaulting struct field (`DrawingsDemo`,
`FrvpDemo`, `DrawingDraft`), never a new enum variant.

## Launch and capture

Raw captures stay outside Git; results and artifact links go in the PR.

1. **Own target dir with free space**: `CARGO_TARGET_DIR=D:\quantick-agent-target`,
   so the user's exe is never locked. Check `Get-PSDrive -PSProvider
   FileSystem` first; ENOSPC reads like a compile error.
2. **Fresh exe, proven**: run `cargo build -p quantick-app --features harness`
   just before capture; compare exe `LastWriteTime` with the last edit.
   Green tests do not rebuild it.
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

Prefer structured answers when available: they survive colour, font and
layout changes. Use screenshots for clipping, font, composition and readability. Launch a `--features harness` build with
`QUANTICK_CONTROL_ACCESS=1` and
the scopes, then use `quantick_get_scene` (controls by name, `selected`, the
coded reason one cannot be operated), `quantick_get_diagnostics` (frame and
tape numbers), and `quantick_capture_evidence` with `screenshot` (scene,
health, market and image at one revision, with `control_regions` per control).
For the manual client, PowerShell stdin trap and bundle fields, read
`references/control-plane.md`.

## The QA pass

1. **Scope** — every affected surface, including splitters moved by dock tabs
   or tape covered by popups. In doubt, include it.
2. **State matrix** — each in-scope surface in: default open (BTC dense tape
   preset); feature on via its hook; popup/menu open over live data; empty
   data (no session, fills or depth); dense data (fast replay, deep book);
   narrow window (~1000 px) and the user's normal size; disabled state (e.g.
   replay has no depth — is the *why* visible?). Prefer replay
   (`QUANTICK_REPLAY_*`, WINJ26 sessions) for reproducibility; presets from
   `config/bubbles.toml`, never bare defaults.
3. **Read the scene, then each capture** — answer explicitly; "it renders"
   is no verdict:
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
   - **Performance** — dense data: fps ≥ ~59, no change-induced
     `APP_SLOW_FRAMES` bursts. After ruling out occlusion, feature fps in the
     50s against ~59 on `main` (same hooks and tape) is FAIL.
   - A scene/image disagreement is a FAIL whichever half is wrong; say which
     you believe.
4. **Report** one verdict per surface × state, most severe first. **FAIL**:
   screenshot path, one-sentence defect, crop/coordinates and scene control ID
   where available. **PASS**: screenshot path and any structured reading
   (scene entry, diagnostics figure, evidence ID); without evidence, not run.
   **BLOCKED**: what was unobservable and what was validated; never PASS.
   PR: verdicts, surfaces, IDs, measurements and optional artifact link. Never
   put raw evidence under `.claude/evidence/` or `docs/workflow/evidence/`.
   Fix FAILs and re-run only failed cells. Keep a before/after pair outside Git
   only if another reviewer needs it. Finish when every cell is PASS or an
   accepted defect recorded in the PR body.
