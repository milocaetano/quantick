---
name: new-extension
description: Recipe for adding a new capability to quantick as an additive package — a new feed, bar type, indicator, chart layer, panel or crate that docks via a port instead of surgery on existing code. Use when a goal adds a capability, when scaffolding a new module, or when deciding where a new feature should live. Turns arch-review's docking test from a review gate into a build recipe.
---

# New extension — ship a package, not a patch

`arch-review` asks after the fact: *could a second implementation be added
as a new file plus one registration line?* This skill is the same idea
before the fact — design the feature so the review question answers itself.

## 1. Pick the docking port

Find where things of this kind already plug in, and use that port. The
repo's existing ports:

| You are adding a… | Port | Registration point |
| --- | --- | --- |
| Market data source | `FeedEvent` channel + `FeedCapabilities` | new `feed-*` crate; config entry in `crates/app/config/feeds.toml` |
| Bar/aggregation type | engine aggregator trait | new module in `engine`, one registration |
| Indicator / kernel | `Indicator` trait (commit/preview + rollback) | `indicators` crate, or a `.pine` script compiled by `pine` |
| Chart layer / overlay | chart layer registry (`QUANTICK_CHART_LAYERS` set) | new layer module in `app` |
| Panel / dock tab | dock tab set | new module in `app`, one tab registration |
| Look / preset | config file (`bubbles.toml`, drawing presets) | data only — no code |
| Sim/backtest behaviour | `sim` fill model + metrics | `sim` crate, consumed by chart and runner alike |

**No port fits?** Then the goal has two parts: first carve the port (a
trait, a registry, a capability flag) as its own reviewable slice, then
dock the feature into it. Never inline the feature and promise the
abstraction later. If unsure the port is right, state the second concrete
implementation you can imagine — if you cannot name one, the abstraction is
speculative; keep the feature local and small instead.

## 2. Obey the frame

- **Dependency direction is law**: `app` → `pine` → `indicators` →
  `engine`; `app` also → `orderbook`/`replay`/`sim`/`feed-*`; `feed-*` →
  `engine`/`orderbook` only. Feeds never see each other. If your feature
  needs a reverse edge, the feature is in the wrong crate.
- **Capabilities, not identities**: downstream behaviour gates on what a
  component *can do* (`FeedCapabilities`), never on which one it is. Adding
  a `match` arm on a source/type enum in consumer code means the port is
  broken — fix the port.
- **One engine**: chart, backtest and bot consume the same aggregator path.
  A per-consumer copy of bar logic is never a package, it is a fork.

## 3. Additive by default

- New options default to today's behaviour; the diff to existing files is
  registration lines, not rewrites.
- Config *presence* never activates anything — the user (or an explicit
  hook) turns it on.
- Everything tunable is named config or a unit-suffixed constant from birth
  (`_MS`, `_PX`, `_TICKS`) — retrofitting costs a review round.
- Measure blast radius before opening the PR: files added vs. files
  edited. Mostly edits → you missed a port or need to carve one; say which
  in the PR body.

## 4. Performance is part of the port

Declare at design time — not at review — which rate class the package runs
in (`arch-review` table: per-trade, per-depth, per-frame, rare), and build
to that budget from the first line:

- **Per-trade / per-depth**: zero allocation, no locks, bounded work per
  event. If the design needs an allocation per tick, the design is wrong —
  restructure before writing code.
- **Per-frame**: recompute only what changed since the last event; cache
  projections and invalidate on event, never rebuild stable data at 60 Hz.
  Batch draws into the existing meshes — no per-element draw calls.
- **Rare (config, startup, panel edits)**: clarity wins freely.

A hot-path package proves its budget before the PR: a bench over a fixture
or an `APP_HEALTH_SUMMARY` comparison against `main` under a dense tape.
"It felt smooth" is not evidence. Overflow-prone feed arithmetic saturates,
never panics.

## 5. Born testable, born drivable

- **Prove the port**: a test with a second (fake) implementation exercises
  the registration path. One implementer never proves a trait is a port.
- **Prove the behaviour**: engine-adjacent work is test-first — fixture
  trades in, expected output out, golden-tested for determinism.
- **Prove it on screen**: any user-visible surface registers a
  `QUANTICK_*` hook per `ui-harness` in the same commit, and passes a
  `visual-qa` + `trader-ux-review` pass before the PR.
- **Prove it without a mouse**: whatever the package lets a trader *do* has
  to satisfy `arch-review`'s *The second operator* — act, read, discover —
  which is the door the embedded assistant will come through. Read the rule
  there rather than working from a summary here; a second copy of it drifts,
  which is a finding under its own Discover bullet.

## 6. Definition of done for a package

Port named · registration is the only edit to existing behaviour · defaults
preserve today · capabilities not identities · rate class declared and its
budget proven (bench or health-summary vs. `main`) · fake second
implementation tested · golden test if determinism is touchable ·
ui-harness hook if visible · drivable without a mouse (*The second
operator*) · four checks green · arch-review clean.
