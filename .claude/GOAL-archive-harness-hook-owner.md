# Mission — give the harness hooks one owner

Extract the env-driven agent-harness fields out of `QuantickApp` behind one
typed owner, so the trunk asks that owner instead of holding two dozen flags
the chart never trades on.

**Tier:** `high`. The work is a refactor of the trunk of a 9,885-line file that
every review surface depends on: `ui-harness` is how `visual-qa` and
`trader-ux-review` see the app, so a hook broken silently disarms both at once.
Four readings of the request led to materially different code (which fields,
where the mutable countdown lives, how far the documentation reaches, what
"demonstrated" means), which is what the full interrogation round buys. `max`
was not taken: the change adds no capability and no user-visible surface.

## Why it matters

`QuantickApp` holds 98 fields. Twenty-three of them exist only so an agent can
drive the app — they are read from environment variables at startup, consumed
once or counted down over a handful of frames, and are not state the chart
trades on. Because they sit in the same struct as the state it does, every
module that touches the trunk sees them. This mission is the third application
of a pattern that already works here (`dock.rs`, then `surfaces/mod.rs`), and
the remaining clusters — tabs/layouts, workspace persistence, control plane,
perf counters — are each their own mission that follows the shape this one
proves.

## Request ledger

| # | Ask |
| --- | --- |
| R1 | Extract the env-driven harness fields out of `QuantickApp` behind **one owner**. |
| R2 | Copy the pattern that already works here rather than inventing a second one: *"an env read once, a typed struct that names its members, and a trunk that asks it instead of holding thirty flags"* — as `surfaces/mod.rs` did for chrome and `dock.rs` before it. |
| R3 | Where the response shape matters, make it *"a struct with defaulting fields, not an enum whose new arm reopens every call site"*. |
| R4 | No behaviour change. Every hook that works today works identically after, under the same environment variable name. |
| R5 | The trunk loses net field count, and the before and after numbers are stated in the PR body. |
| R6 | `ui-harness`'s SKILL.md is part of this change, not a follow-up: after it there is one file to point at for *"where does a new hook go"*. |
| R7 | Non-goal, to be honoured: no further splitting of `app.rs` into files — *"file count is not the problem"*. |
| R8 | Non-goal, to be honoured: do not touch the remaining clusters (tabs/layouts, workspace persistence, control plane, perf counters). |
| R9 | A new hook can be added by editing the hook owner and nothing else in `app.rs`, *"demonstrated by actually adding or moving one"*. |
| R10 | `visual-qa` captures the same surfaces before and after. |
| R11 | Purpose that judges the rest: *"this one proves the shape they follow"* for the four remaining cluster missions. |

## Decisions taken by the trader

- **D1 — the owner takes 23 fields: the 22 named in the request plus
  `pending_maximize`.** The request said "thirty"; the fields it enumerates
  come to twenty-two. The gap is the five `pending_control_*` flags plus
  `pending_maximize`. The trader took `pending_maximize` (a window hook, not
  control-plane machinery) and left every `pending_control_*` for the
  control-plane mission R8 reserves. The twenty-three are: nine `scripted_*`
  (`scripted_footprint`, `scripted_menu`, `scripted_menu_release`,
  `scripted_context_menu`, `scripted_context_menu_release`, `scripted_pointer`,
  `scripted_candle_width`, `scripted_pan_px`, `scripted_indicator_settings`),
  five `pending_*_demo` (`pending_drawing_demo`, `pending_frvp_demo`,
  `pending_avwap_demo`, `pending_venue_history_demo`, `pending_strategy_demo`),
  then `pending_load_older`, `pending_load_older_candles`,
  `pending_history_note`, `pending_replay_restart`, `pending_drawing_draft`,
  `control_evidence_hook_frames`, `settings_autostart`,
  `layout_picker_autostart` and `pending_maximize`.
- **D2 — the owner holds the parsed configuration *and* the live countdown, and
  the trunk mutates through it.** About half these fields are not consumed
  once: `pending_load_older`, `pending_load_older_candles`,
  `pending_history_note`, `control_evidence_hook_frames` and
  `pending_strategy_demo` are multi-frame budgets the draw path decrements
  every frame. Holding only the immutable half would leave six flags in the
  trunk and set an ambiguous precedent for the four missions that copy this
  shape. The trunk calls methods on the owner; it does not read its fields.
- **D3 — the documentation covers the owner, and the registry is unchanged.**
  SKILL.md gains an "Adding a hook" section naming the owner module as the
  single place a startup hook is parsed, with the `hook-registry.md` row still
  required. Hooks that already live beside the surface they reach
  (`surfaces/*`, `feed/*`, `tab.rs`) are documented as the second and already
  correct home — a surface parses its own — rather than being moved.
- **D4 — the new-hook property is demonstrated by moving an existing stray
  hook, not by adding a new one.** R9 and R4 pull against each other; moving a
  mid-function `std::env::var` read into the owner proves the property with a
  diff that touches only the owner, and changes no behaviour under any name.

## Assumptions

- **S1 — the owner is a new module `crates/app/src/harness.rs` (or
  `harness/mod.rs` if it earns a directory), not a sub-module of `app.rs`.**
  Conventional file placement in this repo; R7 forbids splitting `app.rs` into
  more files, and adding one *new* module beside the seventy that already exist
  is the opposite of that — it is where `dock.rs` and `surfaces/mod.rs` live.
- **S2 — the trunk keeps exactly one field, the owner itself.** "One owner" is
  the request's own word; twenty-three fields become one, which is the net
  field loss R5 asks to be stated.
- **S3 — the module doc comment carries the design argument**, the way
  `surfaces/mod.rs` does: what a harness hook is, why the response type is a
  struct and not an enum, and why the registry names its members. This is
  repository convention for a new port, and it is where R11's "proves the
  shape" is actually written down for the next mission.
- **S4 — `visual-qa`'s before/after comparison is made against `origin/main`
  built in this worktree**, not against remembered captures. *Wanted to ask*,
  ranked fifth: the reading taken is the one the repository's own guidance
  ("diagnose against `origin/main`") already prescribes.
- **S5 — the hook parsing keeps its current tolerance for malformed values.**
  Every hook today fails soft (an unparseable `QUANTICK_PAN_PX` leaves the flag
  unset rather than panicking); R4 makes that behaviour, so it is preserved
  rather than tightened. *Wanted to ask*, ranked sixth.
- **S6 — no new dependency and no change to `crates/app/Cargo.toml`.** A field
  move needs none, and a refactor that grows the manifest is one that did
  something else.

## Acceptance criteria

### Mission-specific

- [x] **A1** — One new module owns every one of the twenty-three fields named
      in D1: each is parsed from its environment variable inside that module,
      and no `std::env::var` call for those hooks remains in `app.rs`.
      *Evidence:* the module source, plus `grep -n 'env::var' crates/app/src/app.rs`
      showing none of the D1 hooks' variable names.
      → PR body, quoted grep output. *(R1)*
- [x] **A2** — The owner is a typed struct that names its members as fields,
      constructed by one env read at startup, and its module doc comment states
      why it copies `dock.rs`/`surfaces/mod.rs` rather than inventing a second
      pattern.
      *Evidence:* the module's doc comment, quoted in the PR body. *(R2, R11)*
- [x] **A3** — Every response type the owner hands back to the trunk is a
      struct with defaulting fields; no new enum is introduced whose arms the
      call sites must match on.
      *Evidence:* a grep for enum declarations over the new module returning
      nothing, or each hit justified in the PR body. *(R3)*
- [x] **A4** — `QuantickApp` holds one field where it held twenty-three; the
      trunk reads no field of the owner directly, only its methods.
      *Evidence:* field count before and after from the same counting script,
      both numbers in the PR body; a grep over `app.rs` for the owner field
      showing method calls only. → PR body. *(R1, R5, D2)*
- [x] **A5** — No behaviour change: every hook works identically under the same
      environment variable name. Every environment variable name that reached a
      D1 field before the change reaches the same effect after.
      *Evidence:* a name-by-name table (before → after) in the PR body, plus
      `cargo test -p quantick-app` green and the visual-qa run of A8. *(R4)*
- [x] **A6** — `.claude/skills/ui-harness/SKILL.md` names the owner module as
      the single place a startup hook is parsed, and answers "where does a new
      hook go" — including the second, already-correct home for a hook that
      lives beside its surface.
      *Evidence:* the diff of SKILL.md, quoted in the PR body. *(R6, D3)*
- [x] **A7** — The "add a hook by editing the owner alone" property is
      demonstrated: one existing mid-function `std::env::var` read in `app.rs`
      is moved into the owner, and the demonstration commit's diff touches the
      owner module and the removed read only.
      *Evidence:* the commit hash and its `git show --stat`, in the PR body. *(R9, D4)*
- [x] **A8** — `visual-qa` captures the same surfaces before and after, with
      the before run built from `origin/main` in this worktree.
      *Evidence:* the visual-qa report with both runs and a PASS verdict.
      → `.claude/evidence/harness-hook-owner/visual-qa.md`, screenshots beside it. *(R10, S4)*
- [x] **A9** — The non-goals are honoured: `app.rs` is not split further (one
      new module is added, no existing content is redistributed across files),
      and no field belonging to the tabs/layouts, workspace-persistence,
      control-plane or perf-counter clusters is moved.
      *Evidence:* `git diff --stat origin/main...HEAD` showing which files
      changed, and the D1 list matched against the moved fields. → PR body. *(R7, R8)*

### Injected gates

- [x] **G1** — Every artifact in this branch is English, per `CLAUDE.md`.
      *Evidence:* `cargo test -p quantick-guards` green; `arch-review`
      dimension 8 clean. → PR body.
- [x] **G2** — The four checks pass after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own,
      never chained behind a trailing echo.
      *Evidence:* the four exit codes, quoted separately in the PR body.
- [x] **G3** — Performance impact declared: every touched path classified by
      rate. The harness hooks are read in `draw_frame`, so this change touches
      **per-frame** paths, and the claim to prove is that one struct
      indirection costs nothing measurable.
      *Evidence:* the classification in the PR body, plus an
      `APP_HEALTH_SUMMARY` fps/frame_avg comparison against an `origin/main`
      control run. → `.claude/evidence/harness-hook-owner/perf.md`.
- [x] **G4** — `arch-review` run over `git diff origin/main...HEAD` with every
      Blocker and Should-fix resolved, or deferred in the PR body with its
      severity.
      *Evidence:* the review verdict and the `arch-review-ok` marker. → PR body.
- [x] **G5** — `ui-harness` is followed: every hook this change touches stays
      reachable from a fresh launch by environment variable alone, and the
      registry row for any moved hook is still correct.
      *Evidence:* the visual-qa launch of A8 driving hooks from the owner.
- [x] **G6** — Repository guards green, including the size ratchet over the new
      module and the shrunken `app.rs`; if `app.rs` shrinks, the ratchet is
      tightened rather than left slack.
      *Evidence:* `cargo test -p quantick-guards`, and
      `cargo run -p quantick-guards -- --tighten` if it reports a shrink. → PR body.

#### Evidence, as recorded

- **A1** — `grep -n 'env::var' crates/app/src/app.rs` returns fifty-one hits and
  not one of them names a D1 hook's variable; every one belongs to a boot hook
  that keeps no trunk field, or to a cluster R8 reserves.
- **A2** — `crates/app/src/harness.rs:1-72`, the module header.
- **A3** — the four enums in the module (`StrategyDemoMode`, `ContextMenuPane`,
  `VenueHistoryDemo`, `ScriptedMenu`) are hook *values* moved from `app.rs`, not
  new response shapes, and each is matched at one site. Every response the owner
  hands back — `DrawingsDemo`, `FrvpDemo`, `DrawingDraft`, `HookFrame` — is a
  struct with defaulting fields. Discussed in the PR body.
- **A4** — 98 fields before, 76 after, counted with the same script over
  `git show origin/main:crates/app/src/app.rs` and the branch's file.
  `grep -o 'self\.harness\.[a-z_]*' crates/app/src/app.rs | sort -u` returns
  forty-one names, every one of them a method call; the field-access grep
  returns nothing.
- **A5** — the name-by-name table is in the PR body;
  `cargo test -p quantick-app` 2174 passed / 0 failed; `visual-qa.md` PASS.
- **A6** — `.claude/skills/ui-harness/SKILL.md`, *Hook registry* and *Adding a
  new hook*.
- **A7** — commit `790fbca`, `git show --stat`: `app.rs | 2 +-`,
  `harness.rs | 10 ++++++++++`.
- **A8** — `.claude/evidence/harness-hook-owner/visual-qa.md`, verdict PASS,
  with fourteen screenshots under `shots/`. The control-plane leg is reported
  BLOCKED there rather than counted as a pass, and an incident is recorded.
- **A9** — `git diff --stat origin/main...HEAD`: 26 files, one of them the new
  module; no file was split, and no field outside D1 moved.
- **G1** — `cargo test -p quantick-guards` green (the language and encoding
  scans are two of its four).
- **G2** — each check run on its own: `cargo fmt --all -- --check` exit 0;
  `cargo clippy --workspace --all-targets -- -D warnings` exit 0;
  `cargo build --workspace` exit 0; `cargo test --workspace` exit 0.
- **G3** — `.claude/evidence/harness-hook-owner/perf.md`: 32 paired scenes,
  frame_avg mean 16.6670 ms on `main` against 16.6649 ms on the branch, no
  `APP_SLOW_FRAMES` line on either.
- **G4** — see the PR body.
- **G5** — every hook driven from a fresh launch by environment variable alone
  across the 64 captures; no registry row changed, because no hook was renamed.
- **G6** — the ratchet reported `app.rs` down to 9,355 from 9,890 and
  `cargo run -p quantick-guards -- --tighten` wrote it; `harness.rs` is under
  the 1,500-line threshold, so it earns no baseline entry.

## Not applicable, and why

- **`trader-ux-review`** — the gate table injects it for a change a trader
  touches mid-session. This change moves no pixel and adds no interaction: the
  surfaces it reaches are reached identically, by the same hooks, and a trader
  cannot observe the refactor at all. Grading it would produce a review with
  nothing to grade. `visual-qa` still runs, because R10 asks for it and because
  it is what proves the hooks still work.
- **`new-extension`** — no capability is added. No feed, bar type, indicator,
  chart layer, panel or crate; the module added is a home for state that
  already existed, not a new port a second implementation docks at. The
  *shape* rules that skill teaches are still honoured through A2 and A3.
- **Engine / determinism territory** — nothing under `crates/engine` or any
  other domain crate is touched; the change is confined to `crates/app` and one
  skill file.
- **Docs/skills-only waiver** — not claimed. SKILL.md changes alongside Rust
  code, so the full nine-dimension shape pass applies.

## Closing steps

- **C1** — `delivery-review` returns PASS over the final branch.
- **C2** — the pull request is open, with CI green.

## The request as received

Quoted verbatim, in the language it was written, because a paraphrase is
exactly what this section exists to guard against: `delivery-review` reads this
file and never sees the session, so it re-derives the asks from the trader's
own words and reports what the ledger above failed to carry. This is the single
marked, attributed quotation `CLAUDE.md`'s language rule permits; every other
line of this file is English. Received 2026-09-02, from the trader, as the
argument to `/mission high`.

> Give the harness hooks one owner, and take them out of the trunk.
>
> After the test extraction, `QuantickApp` still holds 98 fields and 144 methods.
> Thirty of those 98 fields exist only so an agent can drive the app: every
> `scripted_*`, every `pending_*_demo`, `pending_load_older`, `pending_history_note`,
> `pending_replay_restart`, `pending_drawing_draft`, `control_evidence_hook_frames`,
> `settings_autostart`, `layout_picker_autostart`. They are read from env vars at
> startup, consumed once, and are not state the chart trades on — but they sit in the
> same struct as the state it does, so every module that touches the trunk sees them.
>
> Extract them behind one owner, the way `surfaces/mod.rs` did for chrome and `dock.rs`
> did before it: an env read once, a typed struct that names its members, and a trunk
> that *asks* it instead of holding thirty flags. Copy the pattern that already works
> here rather than inventing a second one — and where the response shape matters, make
> it a struct with defaulting fields, not an enum whose new arm reopens every call site.
> The reason that constraint is in this prompt is the one `surfaces/mod.rs` already
> argues: `ChartLayer`'s 21 variants across 264 call sites.
>
> Constraints:
> - No behaviour change. Every hook that works today works identically after, under the
>   same env var name — `ui-harness` is how every other review sees the app, so breaking
>   a hook silently disarms `visual-qa` and `trader-ux-review` at the same time.
> - The trunk loses net field count. Say the before and after numbers in the PR body.
> - `ui-harness`'s SKILL.md is part of the change, not a follow-up: it documents the
>   hooks, and after this there is one file to point at for "where does a new hook go".
>
> Non-goals: splitting `app.rs` into more files (70+ modules already exist; file count is
> not the problem), touching the remaining clusters (tabs/layouts, workspace persistence,
> control plane, perf counters) — each is its own mission, and this one proves the shape
> they follow.
>
> Acceptance beyond the standard gates: a new hook can be added by editing the hook owner
> and nothing else in `app.rs`, demonstrated by actually adding or moving one; and
> `visual-qa` captures the same surfaces before and after.
