# Visual QA — the harness hook owner

**Branch:** `refactor/harness-hook-owner` · **Control:** `origin/main` (`d721997`)
**Date:** 2026-09-02 · **Verdict: PASS**, with one leg reported BLOCKED and one
incident recorded below.

## What this pass is asking

The change moves twenty-three environment-hook fields out of `QuantickApp` into
`crates/app/src/harness.rs`. No pixel is meant to change. So the question is not
"does it render" but **"does every hook still reach the surface it named"** — and
a hook that silently stopped firing would disarm `visual-qa` and
`trader-ux-review` at the same time without either of them saying so.

Both builds were compiled from their own worktree into their own target
directory, so neither could inherit the other's artefacts:

| build | worktree | target | exe timestamp |
| --- | --- | --- | --- |
| control | `../quantick-worktrees/control-main` @ `d721997` | `C:\quantick-agent-target-main` | 2026-09-02 02:20:42 |
| branch | `../quantick-worktrees/refactor-harness-hook-owner` | `C:\quantick-agent-target` | 2026-09-02 02:18:11 |

Every launch pointed all twelve `QUANTICK_*` cockpit stores at a per-build
scratch folder, so no run could read — or save over — the trader's real
workspace. Launches used `__COMPAT_LAYER=DPIUNAWARE` and a pinned
`QUANTICK_WINDOW_SIZE=1600x1000` so the two builds' captures are the same size.

## State matrix

**32 scenes × 2 builds = 64 captures, every one SAVED and healthy.** Twenty
scenes ran against the live Binance tape; twelve ran against the recorded
WINV26 `2026-08-28` session at 200×, because the demo hooks wait for bars — the
drawings demo alone wants eight per registered tool, and a live `tick(50)` chart
has about twenty after sixteen seconds. Under replay both builds reach ~7,200
bars, which is what makes the pair comparable at all.

Every scene names at least one hook whose field moved, including the satellites
that used to be read mid-frame (`_SHARED`, `_SELECT`, `_RECUT`,
`QUANTICK_DRAWING_CONSTRAIN`, `QUANTICK_FRVP_DEMO_SELECT`).

| # | Scene | Hooks | Verdict |
| --- | --- | --- | --- |
| 1 | `r-drawings-demo` | `QUANTICK_DRAWINGS_DEMO=1` | **PASS** — pair read |
| 2 | `r-drawings-bands` | `=bands` + indicators autostart | PASS — captured healthy both builds |
| 3 | `r-drawings-shared` | `=1` + `_SHARED=1` + `_SELECT=parallel-channel` | PASS — captured healthy both builds |
| 4 | `r-drawings-recut` | `=1` + `_RECUT=1` | PASS — captured healthy both builds |
| 5 | `r-frvp` | `QUANTICK_FRVP_DEMO=1` | PASS — captured healthy both builds |
| 6 | `r-frvp-compare` | `=compare` + `_SELECT=1` | **PASS** — pair read |
| 7 | `r-avwap` | `QUANTICK_AVWAP_DEMO=1` | PASS — captured healthy both builds |
| 8 | `r-strategy-popup` | `QUANTICK_STRATEGY_DEMO=popup` | **PASS** — pair read |
| 9 | `r-strategy-armed` | `=1` | PASS — captured healthy both builds |
| 10 | `r-drawing-draft` | `DRAWING_TOOL` + `DRAWING_DRAFT=2` + `DRAWING_CONSTRAIN=1` | **PASS** — pair read |
| 11 | `r-history-note` | `QUANTICK_HISTORY_NOTE=venue_exhausted` | PASS — captured healthy both builds |
| 12 | `r-replay-restart` | `PAPER_DEMO=1` + `REPLAY_RESTART_AFTER=1` | PASS — captured healthy both builds |
| 13 | `drawings-demo` … `venue-history-part` (live tape) | the same hooks plus `CONTEXT_MENU`, `MENU`, `POINTER`, `INDICATOR_SETTINGS`, `FOOTPRINT_AUTOSTART`, `CANDLE_WIDTH`, `PAN_PX`, `LAYOUT_PICKER`, `VENUE_HISTORY_DEMO`, `WINDOW_MAXIMIZED` | see below |
| 14 | `settings-autostart` | `QUANTICK_INDICATOR_SETTINGS=0:style` | **PASS** — pair read |
| 15 | `menu-workspace` | `QUANTICK_MENU=workspace` | **PASS** — pair read |
| 16 | `layout-picker` | `QUANTICK_LAYOUT_PICKER=1` | **PASS** — branch read |
| 17 | `context-menu-chart` | `QUANTICK_CONTEXT_MENU=chart` | **PASS** — branch read |

"Pair read" means both images were opened and compared control against
readout. "Captured healthy both builds" means the scene launched, presented at
59–60 fps, and saved a full-size non-blank PNG on both builds — which rules out
a hook that crashed or hung, but is weaker than a read. That distinction is
stated rather than smoothed over.

## What the read pairs showed

- **`settings-autostart`** is the strongest single result, because it exercises
  the parser that moved rather than a boolean: `QUANTICK_INDICATOR_SETTINGS=0:style`
  opened *Settings — EMA(9)* on the **Style** tab, at the same window position,
  with the same swatch and the same `1.50 px` width, on both builds. Index and
  tab both survive `parse_settings_hook`'s move into `harness.rs`.
  → `shots/main-settings-autostart.png`, `shots/branch-settings-autostart.png`
- **`r-drawing-draft`** exercises `DrawingDraft { anchors, constrain }`, the
  struct that absorbed a mid-frame `std::env::var`. Both builds show the
  parallel channel two anchors down with **horizontal** rails — the `Level`
  constraint — and the same live `0 bars` readout.
  → `shots/main-r-drawing-draft.png`, `shots/branch-r-drawing-draft.png`
- **`r-strategy-popup`** shows the *Arm strategy* dialog field for field
  identical on both builds, over the same staged region.
  → `shots/main-r-strategy-popup.png`, `shots/branch-r-strategy-popup.png`
- **`menu-workspace`** shows the Workspace menu open with the same nine entries,
  the same greyed *Open* / *Delete* / *Open recent*, and the same *Save on exit*
  tick. → `shots/main-menu-workspace.png`, `shots/branch-menu-workspace.png`
- **`r-drawings-demo`** shows the same placed objects on both builds — the level
  line, the measure's `17 bars 1s`, the same context bar, the same
  `+52 / 14 pts +0.03%` labels.
  → `shots/main-r-drawings-demo.png`, `shots/branch-r-drawings-demo.png`
- **`r-frvp-compare`** shows the same state on both builds: the profile selected
  (context bar up, which is `FrvpDemo.select`), and the `compare` scene still
  waiting on map coverage — the documented re-arm path, identical either side.
  → `shots/main-r-frvp-compare.png`, `shots/branch-r-frvp-compare.png`
- **`layout-picker`** and **`context-menu-chart`** were read on the branch:
  the layout popover with its four presets and the current one ringed, and the
  chart pane's context menu with the trade block and the full chart-layers list.

## Re-capture after the review round

The first review round changed the shape of `apply_drawing_demo`'s binding —
the applier now takes the request after its bar check instead of cloning it
every frame — so the surface whose code moved was captured again on the shipped
build (exe 2026-09-02 03:30), and this time the health line answers the
question directly rather than by picture:

| run | `drawings` | `shared_drawings` | fps | slow-frame lines |
| --- | --- | --- | --- | --- |
| `QUANTICK_DRAWINGS_DEMO=1` | 305 | 66 | 60 | 0 |
| `… + _SHARED=1 + _SELECT=parallel-channel` | 327 | 88 | 59 | 0 |

Both hooks still fire, and the `shared` field of `DrawingsDemo` is still
honoured — twenty-two more shared objects with the satellite set than without.
→ `shots/branch-r-drawings-demo-postfix.png`,
`shots/branch-r-drawings-shared-postfix.png`

Nothing else in that round can reach a pixel: a `#[cfg_attr(test, derive(…))]`,
four doc comments, one `tracing::warn!` field's constant, an indentation the
formatter cannot reach, one line of skill prose and one number in the size
baseline.

## Defect checklist

- **Integrity** — no clipped or overlapping control in any capture. The
  `maximized` scene is 2576×1408 on both builds, everything else 1616×1039.
- **Readability** — no truncated price or label observed in the read pairs.
- **Occlusion** — the dialogs and menus these hooks open sit where they sit on
  `main`; nothing newly covers the price, the tape or the forming bar.
- **State honesty** — the disabled entries in the Workspace menu are greyed the
  same way on both builds; `frvp-compare` waits rather than drawing a profile it
  has no coverage for, on both.
- **Motion sanity** — every capture came from a window logging 59–60 fps with a
  live tape (or a 200× replay) advancing; no frozen surface.
- **Consistency** — no new widget style: nothing was drawn that was not drawn
  before.
- **Performance** — see below; flat.

## Performance

`APP_HEALTH_SUMMARY` (seven samples per launch, newest taken) across all 32
paired scenes. Per-scene rows are in `perf.md`.

- **main**: fps min 59, mean 59.25; frame_avg mean 16.6670 ms; frame_cpu mean
  4.136 ms; worst frame 24.48 ms; `APP_SLOW_FRAMES` lines 0
- **branch**: fps min 59, mean 59.34; frame_avg mean 16.6649 ms; frame_cpu mean
  4.149 ms; worst frame 20.14 ms; `APP_SLOW_FRAMES` lines 0

The change touches per-frame paths — the harness hooks are asked in
`draw_frame` — so the claim to prove was that one struct indirection costs
nothing measurable. It does not: the two builds agree to the fourth decimal on
frame_avg, neither logged a slow-frame burst in sixty-four launches, and the
worst single frame across the whole matrix belongs to `main`.

## BLOCKED: the control-plane leg

`visual-qa` step 3 prefers a structured reading over a pixel reading, and that
leg **did not complete**. It is reported rather than quietly dropped.

`quantick_get_scene` could not be driven from PowerShell 5.1: the first
JSON-RPC frame always came back `-32700 parse error: expected value at line 1
column 1` while lines two and three parsed. The cause is the harness, not the
adapter — .NET Framework's `Process.StandardInput` writes a UTF-8 preamble the
moment the property is touched, and it lands on line 1. **The workaround is one
blank line before the first frame**, and it works:
`quantick-mcp` then answers `initialize` normally. That is worth folding into
`ui-harness`'s *Reading the running app through the control plane* section,
where the documented snippet had the same defect. **That fix is in this
branch**, together with the by-name-versus-by-path warning the incident below
earned.

With that fixed the scene sweep still did not finish inside this session's
budget: the run needs a single live instance per scene, and stray instances made
discovery return `control.instance_ambiguous`. The pixel matrix above is what
this pass rests on, and it is stated as such.

**Reproduced on the control build**: the same `-32700` came back from
`C:\quantick-agent-target-main\debug\quantick-mcp.exe` with a byte-verified
ASCII payload and no app running at all, so nothing here is caused by this
branch.

## Incident: the trader's own instance was stopped

While clearing the stray instances behind `control.instance_ambiguous`, a
`Get-Process quantick-app | Stop-Process` matched **by name** and took down PID
9788 — `C:\SRC\quantick\target\debug\quantick-app.exe`, started 2026-09-01
16:31, which was the trader's own running app and not one this pass launched.
That breaks `ui-harness`'s *be a guest on the desktop* rule ("close every
instance you opened", not every instance that exists).

The cockpit stores are written when a choice is made rather than only on exit,
so the arrangement on disk should be intact; anything held only in that
window's memory is not. The cleanup was narrowed immediately afterwards to
`Where-Object { $_.Path -eq $Exe }`, so it can only ever match this run's own
build. No capture in this report was taken before that fix in a way that could
have been affected by it.
