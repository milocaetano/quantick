# visual-qa — workspace persistence has one owner

Two builds, driven through the same hooks against the same live Binance feed:

- **control** — `origin/main` at `dc6396e`, the commit this branch is based on.
- **branch** — `refactor/workspace-persistence-owner` at that base plus the one
  commit.

Every `QUANTICK_*` store pointed at a per-case scratch folder, so no run read
or rewrote the trader's real cockpit. Captured by PID (two builds of the same
app are on the desktop at once, so filtering by process name would grab the
wrong window), `__COMPAT_LAYER=DPIUNAWARE` so the client area stays inside the
monitor.

## The four flows the mission names

| Flow | Case | How it is reached |
| --- | --- | --- |
| **save** | `save` | `QUANTICK_WORKSPACE_SAVE=1` — the menu entry's own path |
| **rename** | `namebox-full` | `QUANTICK_WORKSPACE_NAME_BOX=1` — the Save-as box, against a file that already holds two names |
| **delete** | `menu-bm` | the Workspace menu's *Delete* submenu, enabled because the seeded file holds two named arrangements |
| **reopen** | `reopen` | `QUANTICK_WORKSPACE_IMPORT=<bundle>` after `QUANTICK_WORKSPACE_EXPORT` wrote it |

Plus `menu-cold` (nothing saved — every entry in its empty state). The cases
suffixed nothing (`name-box`, `menu-warm`) ran against the first seed, the ones
named `-full` and `-bm` against the corrected seeds described at the foot of
this file; both builds always saw the same seed as each other, which is what
the comparison rests on.

## Verdict: PASS — no difference attributable to the change

### What the two builds wrote to disk

The strongest evidence, and stronger than any screenshot: the files themselves.

| Case | Files written | Content |
| --- | --- | --- |
| `menu-cold` | none, both | — |
| `save` | `ui-state.toml`, both | **byte-identical** (SHA-256 match) |
| `name-box` (first seed) | `ui-state.toml`, both | **byte-identical** |
| `menu-warm` (first seed) | `ui-state.toml`, both | **byte-identical** |
| `export` | `ui-state.toml`, both | one line differs: `recent_workspaces` holds the bundle path each build was *told* to export to (`bundle-control` vs `bundle-branch`) — a fixture difference, not a behaviour one |
| `reopen` | `ui-state.toml`, both | same one line, same reason |
| bundle | `.qws.toml`, both | one line differs: `name = "bundle-control"` vs `"bundle-branch"`, which is the filename each was given |

### What the two builds logged

Across ten runs each, the workspace event ledgers are identical — same codes,
same counts:

```
control : CHART_LAYERS_RESTORED x10  UI_STATE_SAVED x4  WORKSPACE_EXPORTED x1  WORKSPACE_IMPORTED x1
branch  : CHART_LAYERS_RESTORED x10  UI_STATE_SAVED x4  WORKSPACE_EXPORTED x1  WORKSPACE_IMPORTED x1
```

Same writes, the same number of times, in the same places.

### What the two builds painted

The Workspace menu's own rectangle (x 78–258, y 55–300), control vs branch:

| Case | Differing pixels | Reading |
| --- | --- | --- |
| `menu-cold` | **0 / 44100** | identical |
| `menu-warm` | **0 / 44100** | identical |
| `menu-bm` | **0 / 44100** | identical |
| `menu-full` | 5 / 44100 | the menu's antialiased bottom-left corner at (78,296)–(79,299), where the live chart bleeds through |

Every entry, and every enabled/disabled state, matches: Save workspace, Reset
startup layout, Save as…, Open ▸, Delete ▸, Export to file…, Open from file…,
Open recent ▸, Show where it's saved, and the ticked *Save on exit*.

### Three differences chased down, none the change's

A same-build control run established the noise floor first: **control vs
control, 0/9520 differing pixels** in the menu bar. So a difference between
builds is real and had to be explained rather than waved at.

1. **Workspace button highlight** (`menu-bm`, 1373 px, x 80–159). The branch
   capture painted the menu-bar button's hover background, the control did not.
   Re-running both: **0/2520 px differ**. Pointer state at capture time — the
   desktop mouse, which the harness rules already warn about. Not the change,
   and the other three menu cases showed zero difference from the same hook.
2. **"loading venue history" toast** (`menu-bm` rerun, 3116 px, y 135–163). The
   control run had the feed's history-loading indicator painted over the menu
   at capture time; the branch run had finished loading. The rows underneath —
   Save as…, Open, Delete — are identical in both. Feed timing.
3. **Status bar and chart body** (8–15 % of the frame in every case). Live
   Binance tape: different trades, different prices, different trade counts and
   timestamps between two runs seconds apart.

### Performance — the per-frame claim (G3)

`maintain_layouts` is the one per-frame path the diff touches. Ten runs per
build, four `APP_HEALTH_SUMMARY` samples each:

| | median fps | frame_avg | frame_cpu median | `APP_SLOW_FRAMES` |
| --- | --- | --- | --- | --- |
| control | **59** | 16.71–17.02 ms | 1.49–1.71 ms | **0** |
| branch | **59** | 16.68–17.06 ms | 1.41–1.90 ms | **0** |

Flat. The ranges overlap and neither build logged a single slow-frame burst.

## Captures kept

- `control-menu-bm.png` / `branch-menu-bm.png` — the Workspace menu with
  bookmarks, delete and recents all live, 0/44100 pixels apart.
- `control-namebox-full.png` / `branch-namebox-full.png` — the Save-as box.

The remaining captures, the per-case scratch stores and the logs live in the
session scratchpad; the numbers above are quoted rather than pointed at,
because that directory does not outlive the session.

## One fixture defect found and fixed, worth recording

The first seed appended `recent_workspaces` to the end of a workspace file,
which put a top-level key *inside* the file's `[chrome]` table. `ui_state`
refuses a file it cannot parse **whole** rather than half-reading it — the
documented rule — so the app opened on defaults and the menu photographed
empty. The second seed edits the key in place. Correct app behaviour throughout,
and on both builds identically; recorded because a QA fixture that silently
photographs the wrong state is the way this pass fails without saying so.

A second seed defect in the same vein: bookmarks with no tabs are dropped by
`Workspace::restore`'s stale-tab filter, so the Open and Delete submenus stayed
greyed until the seeded arrangements were given real tabs. Again correct
behaviour, again identical on both builds.
