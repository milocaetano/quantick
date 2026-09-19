---
name: new-extension
description: Recipe for adding a new capability to quantick as an additive package — a new feed, bar type, indicator, chart layer, panel or crate that docks via a port instead of surgery on existing code. Use when a goal adds a capability, when scaffolding a new module, or when deciding where a new feature should live. Turns arch-review's docking test from a review gate into a build recipe.
---

# New extension — ship a package, not a patch

Design so `arch-review`'s docking question — a new file plus one registration
line? — answers itself.

## 1. Pick the docking port

| Adding a… | Port | Registration |
| --- | --- | --- |
| Market data source | `FeedEvent` channel + `FeedCapabilities` | new `feed-*` crate; entry in `crates/app/config/feeds.toml` |
| Bar/aggregation type | engine aggregator trait | new `engine` module, one registration |
| Indicator | `Indicator` trait (commit/preview + rollback) | a `.pine` script (data only), else a native file under `crates/indicators/src/native/` plus one `pub use` and one `NATIVES` entry (stable id like `native.sma`, the menu label printed verbatim, a `fn() -> Box<dyn Indicator>`) — nothing in `app` |
| Chart layer / overlay | layer registry (`QUANTICK_CHART_LAYERS`) | new layer module in `app` |
| Panel / dock tab | dock tab set | new `app` module, one tab registration |
| Floating surface (popup, toast, box) | `Surface` + `SurfaceEnv`/`SurfaceResponse` | new module in `app/src/surfaces/`, one field on `Surfaces` |
| Look / preset | config (`bubbles.toml`, drawing presets) | data only |
| Sim/backtest behaviour | `sim` fill model + metrics | `sim`, consumed by chart and runner alike |

Prefer a `.pine` script whenever the dialect can express the kernel. An id no
build ships becomes an error slot naming it, never a substitute.

**The shape to copy** — a surface: the `Surface` trait in
`crates/app/src/surfaces/mod.rs` (reads arrive in `SurfaceEnv`, requests leave
in `SurfaceResponse`, no reference to the host); a port: `TradingVenue` in
`crates/trading/src/venue.rs`; a decision: the pure plan in
`crates/feed/src/ohlcv_plan.rs` (inputs handed in, no clock or socket). Never a
new field, constructor line, `draw_frame` call or menu hotkey on `QuantickApp`
— `crates/guards/src/size.rs` fails that growth.

**No port fits?** Carve it first (trait, registry or capability flag) as its own
reviewable slice, then dock. Never inline now and abstract later. Name the
second concrete implementation; if you cannot, the abstraction is speculative —
keep the feature local and small. `ChartLayer` and `DockTab` are closed enums:
extend them only where they already apply, and read `arch-review` dimension 9
before adding a second variant of anything new.

## 2. Obey the frame

`CLAUDE.md`'s dependency direction and *one engine* hold; a feature needing a
reverse edge is in the wrong crate. Gate on capabilities (`FeedCapabilities`),
never identities — a consumer `match` arm on a source/type enum means the port
is broken.

## 3. Additive by default

- New options default to today's behaviour; existing files get registration
  lines, not rewrites.
- Config *presence* never activates anything — the user or an explicit hook
  does.
- Tunables are named config or unit-suffixed constants from birth (`_MS`,
  `_PX`, `_TICKS`).
- Blast radius before the PR: files added vs. edited **and lines added vs.
  lines poured into existing files**. Mostly edits → a port was missed or needs
  carving; say which in the PR body.

## 4. Performance is part of the port

Declare the rate class at design time (per-trade, per-depth, per-frame, rare):

- **Per-trade / per-depth** — zero allocation, no locks, bounded work per
  event; a per-tick allocation means redesign before code.
- **Per-frame** — recompute only what changed; cache projections, invalidate
  on event; batch into the existing meshes.
- **Rare** — clarity wins.

A hot-path package proves its budget before the PR (fixture bench or
`APP_HEALTH_SUMMARY` vs. `main` under a dense tape). Feed arithmetic saturates,
never panics.

## 5. Born testable, born drivable

- A fake second implementation exercises the registration path.
- Engine-adjacent work is test-first, golden-tested.
- A visible surface registers its `QUANTICK_*` hook per `ui-harness` in the
  same commit and passes its QA pass and `trader-ux-review` before the PR.
- Whatever a trader can *do* meets `arch-review`'s *The second operator* (act,
  read, discover) — read it there, not from a copy.

## 6. Done

Port named · registration the only edit to existing behaviour · defaults
preserve today · capabilities not identities · rate class declared and budget
proven · fake second implementation tested · golden test if determinism is
touchable · hook if visible · drivable without a mouse · four checks green ·
`arch-review` clean.
