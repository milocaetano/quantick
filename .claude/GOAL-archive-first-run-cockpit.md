# Mission: the first launch opens the cockpit the chart is for

**Objective:** on a machine with no saved cockpit, quantick's first launch opens
on Binance **BTCUSDT** in the **timeframe + flow** split with all four flow-layer
switches — bubbles, L2 heatmap, footprint, live strip — **on**; every later
launch keeps them on unless the trader switched one off; and the window the
trader actually works in shows them at all.

Branch: `fix/first-run-cockpit` · Worktree: `../quantick-worktrees/fix-first-run-cockpit`

## What the reproduction changed about this mission

The three settings asked for were **already the shipped answer** — `feeds.toml`
has `default_symbol = "BTCUSDT"` and `default_layout = "time+flow"`, and PR #229
put all four flow layers in `config/chart-layers.toml`. A launch on `main` with
every store pointed at a non-existent scratch path proved it: `feed=binance
symbol=BTCUSDT`, `canvas_layout=TimeAndFlow`, `CHART_LAYERS_RESTORED hidden=1`,
and all four lamps lit in the capture.

So the mission was never to change a default. It was to find why the shipped
answer does not reach the trader, and two independent causes were found.

**Bug A — the trader's file replaced the shipped default instead of covering
it.** `chart_layers::load` returned only the keys in the trader's file. A layer
absent from it fell through to the code baseline, which for all four flow layers
is *off*. Any cockpit written before PR #229 therefore answered "off" to a
question it had never been asked, on every launch — and `maintain_chart_layers`
rewrites the whole map on the first switch of a session, freezing that silence
into an explicit `false`. That is precisely why PR #229 changed nothing for the
trader who reported it.

**Bug B — a maximised window lays out larger than its own surface.** Measured:
client `2560x1369 px`, egui told `2560x1369 pt` at `scale=1.5`, painting
`3840x2054 px` into it. The right and bottom third fall outside the window —
exactly where the four lamps, the price axis, the live strip and the dock rail
live.

**Not fixed, and the reason is the deliverable.** Three corrections were built
and measured. Against `monitor_size`: that field is in *points*
(`egui-winit-0.29.1/src/lib.rs:970`), so the check would have shrunk every
honest window on a 150% display to 933x600 — caught by `arch-review` as a
Blocker. Against `inner_rect`: wrong together with `screen_rect`. Against the
platform's own `GetClientRect`: returns 3840x2052 *from inside the process*
where the same call from outside returns 2560x1369, because Windows virtualises
coordinates into the caller's DPI context. Every observable inside the process
is self-consistently wrong, because what is wrong is the process's coordinate
space. Upstream has it open and unfixed (`emilk/egui#7648` at 0.33.1); the
upgrade from 0.29 was priced at 262 compile errors for a release that still has
it. What ships is the measurement — `client_px` beside `screen_pt` and `scale`
in the health summary — plus `QUANTICK_WINDOW_MAXIMIZED=1` to reach the state,
and `window_scale`'s module doc recording all three dead ends for whoever picks
it up.

## Acceptance criteria

1. Reproduced on `main` before any code was written — capture plus log. **met**
2. BTCUSDT on a virgin launch, no env override. **met**
3. The time+flow split on a virgin launch. **met**
4. All four lamps lit on a virgin launch, in both focus states. **met**
5. The cause named and regression-tested — a test that fails on `main` with the
   observed wrong value and passes here. **met**
6. The trader's own choice still outranks the shipped default; the two #229
   tests still pass. **met**
7. A capability-blocked layer explains itself rather than reading as a switch
   the trader turned off. **met** — the lamp reads the switch, the button keeps
   its `disabled_explanation`.
8. The maximised window lays out inside its own surface. **not met** — three
   corrections built, measured and withdrawn; the diagnosis, the reproduction
   hook and the three dead ends ship instead. Taken to a follow-up issue.

### Standard gates

- [x] four checks green (`fmt`, `clippy -D warnings`, `build`, `test`)
- [x] performance declared: every touched path classified below
- [x] `ui-harness`: `QUANTICK_WINDOW_MAXIMIZED=1` added and registered
- [ ] `arch-review` with every Blocker/Should-fix resolved or deferred
- [ ] PR opened

## Performance, by rate

| Path | Rate | Cost |
| --- | --- | --- |
| `chart_layers::load` merge | rare (startup) | one `BTreeMap` extend |
| `apply_pending_layout` layer copy | rare (a layout change) | ~18 switch writes |
| `open_tab` inherited layers | rare (a new tab) | one map build |
| `CHART_LAYER_SWITCHED` log | rare | gated behind the mask compare that already existed |
| `SurfaceProbe::client_size_px` | rare (the 2 s health summary) | one `GetClientRect` |
| the four layers being on | per-frame | unchanged from PR #229, which declared and measured it |

## Out of scope

- Changing *which* layers ship on — PR #229's decision, and the trader is asking
  for it to hold rather than to change.
- The eframe upgrade: priced, proved not to fix the bug, reverted.
- `open_tab`'s pre-existing habit of resurrecting layers is fixed only where
  this branch's own change would otherwise have widened it.
