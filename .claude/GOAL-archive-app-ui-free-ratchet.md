# Goal: ratchet the UI-free code that lives in the app crate

Add a ratchet to `crates/guards` that caps `app.lines.without_egui` — the
production lines in `crates/app` files that never name the UI library — at
today's value, with the size and extension ratchets' mechanism (a recorded
ceiling, a `!budget`, teeth both ways, `--tighten`) and an explicit, reasoned
exemption list, so UI-free logic stops accumulating in the UI crate where the
headless guard cannot see it; and name the rule in `CLAUDE.md` in one
sentence, paid for in the context ratchet.

**Tier:** medium — assigned by the campaign coordinator. A new guard with a
data file, a report definition change and one instruction sentence: more than
`small`'s ceremony, no trader call (no money, safety or irreversibility).

Campaign task Q18 of #367, issue #443. Base: `campaign/lean-a-plus` at
`317772a5`. Scope of this PR: issue A1 plus the `CLAUDE.md` sentence; issue
A2's lower ceiling is recorded after Q16 (#441) and Q17 (#442) land.

## Request ledger

- **R1** — A ratchet on `app.lines.without_egui` in `crates/guards`: a
  recorded ceiling at the current tree value, teeth both ways, `--tighten`,
  "same mechanism as the size and extension ratchets".
- **R2** — "exempt only wiring files that must be UI-free by nature, listed
  explicitly with a reason".
- **R3** — `guards` keeps zero dependencies.
- **R4** — Issue A1: "a guard test fails when a new UI-free production file is
  added to `crates/app` without lowering the ceiling elsewhere, and passes on
  the current tree".
- **R5** — The one-sentence `CLAUDE.md` mention, "paid for in the context
  ratchet" (a growth paid by trimming elsewhere in the tracked instruction
  set).
- **R6** — Issue A2's lower ceiling is recorded later, after Q16/Q17 land;
  the PR body says so and issue A2 stays open.
- **R7** — Do not touch `crates/app` sources.
- **R8** — Test-first: the failing guard test is committed before the
  implementation.
- **R9** — The purpose: UI-free logic stops accumulating in the UI crate out
  of reach of the headless guard (the finding behind Q16 and Q17).

Operational instructions (four checks run each on its own, draft PR against
`campaign/lean-a-plus`, arch-review, ai-review, delivery-review, CI,
`gh pr ready` once, the ship gate, never merge) map to the `G` and `C` items.

## Decisions

- **D1** — From the coordinator: the `CLAUDE.md` sentence ships in this PR;
  issue A2's lower number does not.

## Assumptions

- **S1** — *Wanted to ask* (double meaning). "`app.lines.without_egui`"
  names a report row that counts **lines** not containing `egui` (121,269 of
  125,137 `app` lines at `317772a5`, 97%), while the issue describes it as
  lines "in `crates/app` files that never reference the UI library", and its
  A1 speaks of "a new UI-free production **file**". Read at file level: a
  ratchet on the line-level number would cap the whole UI crate and bill
  every UI feature, which is not the finding Q16/Q17 act on (they measure by
  file). Safe: the file-level reading is the issue's own sentence; reversing
  it is one function.
- **S2** — The report row keeps its label and now prints the ratcheted
  measurement, so the name the issue uses and the number the guard enforces
  are one definition. Earlier evidence files keep the old line-level values;
  the PR body states the break. Safe: `--report` enforces nothing.
- **S3** — "Names the UI library" means what the headless guard forbids as
  the UI toolkit (`egui`, `eframe`; Q16/Q17 measure "egui/eframe"), asked
  through its own matcher: whole identifiers, comments stripped with quotes
  respected. A comment does not make a file UI code, so a `// egui` cannot
  launder one; and `timeframe` is not `eframe` — a bare substring test
  charged every file naming a timeframe as UI code. Safe: one list and one
  matcher for both guards (changed after the ai-review found the first
  version kept a second one).
- **S4** — Production is the size guard's definition (top-level
  `#[cfg(test)]` items stripped, `tests/` skipped): a test sidecar outside
  `tests/` counts, exactly as it counts toward size. Safe: one owner of
  "production"; an inline `#[cfg(test)]` module stays free.
- **S5** — One aggregate entry, `crates/app`, plus the `!budget`, slack 200
  as the size ratchet's; not a per-file baseline, which would fail every
  one-line fix in any of 130 files. Safe: issue A1 is about the total.
- **S7** — The `CLAUDE.md` sentence is paid inside `CLAUDE.md` itself (a
  duplicated "run the guards" sentence and three wordier clauses trimmed,
  9,439 → 9,436 bytes), and `AGENTS.md`'s crate map names the new ratchet
  at a net −1 byte. Safe: no rule removed, only repeated or reworded prose.
- **S6** — The exemption list holds `crates/app/src/scratch.rs` only: the
  scratch guard requires each crate's tests to mint directories through the
  crate's own scratch module, so it must live in `app` and has nothing to do
  with the UI. `*_wiring.rs`, `hooks.rs` and `control/mod.rs` are not
  exempt: they are glue a port could shrink. Safe: an exemption is one
  reviewed data line.

## Acceptance criteria

- [x] **A1** — `crates/guards/src/ui_free.rs` registers an `app-ui-free`
      ratchet in `GUARDS` on `ratchet::Policy`: `ui-free-baseline.txt`
      records `crates/app` and `!budget` at the value measured at the branch
      head; a total over the ceiling fails, a total more than 200 below asks
      for `--tighten`, and `--tighten` lowers both and never raises.
      *Evidence:* unit tests `ui_free::tests::*` and the registry test
      `app_ui_free_code_stays_within_its_ceiling` green; `--report` row
      `ratchet.app-ui-free.measured` equals the recorded ceiling.
      → the PR body. *(R1, R9)*
- [x] **A2** — Exemptions live in `crates/guards/ui-free-exemptions.txt` as
      `<path> <reason>`; a line without a reason, outside `crates/app/src`,
      naming a file that is gone or that names the UI library is a finding.
      *Evidence:* the exemption unit tests. → the PR body. *(R2)*
- [x] **A3** — A test adds a new UI-free production file to a fixture
      `crates/app` at its ceiling and gets a finding, and passes again once
      the same number of UI-free lines leaves another file; the registry
      test passes on the current tree. *Evidence:* the named test output.
      → the PR body. *(R4)*
- [x] **A4** — `crates/guards/Cargo.toml` still has empty dependency
      tables. *Evidence:* the file at the head. → the PR body. *(R3)*
- [x] **A5** — `CLAUDE.md` names the ratchet in one sentence, and its size
      does not exceed its `context-baseline.txt` ceiling: the bytes are
      trimmed inside the tracked instruction set in the same change.
      *Evidence:* `cargo test -p quantick-guards` (context guard) green and
      `wc -c CLAUDE.md` before/after. → the PR body. *(R5)*
- [x] **A6** — The test commit precedes the implementation commit and fails
      on its own. *Evidence:* `git log` order and the red run.
      → the PR body. *(R8)*
- [x] **A7** — No file under `crates/app/` changes. *Evidence:*
      `git diff --stat origin/campaign/lean-a-plus...HEAD -- crates/app`
      empty. → the PR body. *(R7)*
- [x] **A8** — The PR body says the ceiling is lowered after Q16/Q17 land
      and leaves issue A2 open. → the PR body. *(R6)* Superseded by A9
      under the amendment below: Q16/Q17 landed before this PR merged.
- [ ] **A9** — After the rebase onto the campaign tip holding Q16 and Q17,
      `--tighten` records the lower ceiling in `ui-free-baseline.txt` with a
      comment naming Q16/Q17, and an independent count agrees; the PR body
      reports issue A2 closed. *Evidence:* the baseline diff, `--report`
      rows, the Python count. → the PR body. *(R10)*
- [ ] **A10** — The rebase keeps both sides of every conflict and raises no
      context ceiling or budget: `CLAUDE.md` and `AGENTS.md` stay under
      their `context-baseline.txt` entries, which this branch does not edit.
      *Evidence:* `git diff origin/campaign/lean-a-plus...HEAD --
      crates/guards/context-baseline.txt` empty, `wc -c`, the context guard
      green. → the PR body. *(R10)*
- [ ] **G1** — Every artifact in English (`arch-review` dimension 8, the
      language guard). → the arch-review report on the PR.
- [x] **G2** — Four checks green, each run on its own:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test
      --workspace`; plus `cargo test -p quantick-guards`. → the PR body.
- [x] **G3** — Performance impact declared: every touched path is *rare*
      (CI, `--report`, `--tighten`, and the edit-time `--file` hook on a
      `crates/app/src` write, which now walks `crates/`); nothing per-trade,
      per-depth or per-frame. The hook's cost measured. → the PR body.
- [ ] **G4** — `arch-review` over `origin/campaign/lean-a-plus` with step 0
      at `low`; every Blocker/Should-fix resolved or deferred in the PR body.
      → the arch-review report on the PR.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI-review evidence lands on the PR: the durable report comment, the thread
list, and the private `ai-review-complete` projection.

## Status at archive

A1-A7 and G2-G3 met locally at `4fd44d83` (tests `0effff8a`, red: 18 unit
and 2 registry tests failing; implementation `8c1c0e18`; doc note
`4fd44d83`). Ceiling 50,795 lines in 130 files at `317772a5`, equal to
`ratchet.app-ui-free.measured` and `app.lines.without_egui` in `--report`
and to an independent Python count. Exemptions: `crates/app/src/scratch.rs`.
Four checks each on its own: fmt rc 0, clippy rc 0, build rc 0,
`env -u QUANTICK_BUBBLES cargo test --workspace` rc 0 (3,834 passed, 18
ignored); `cargo test -p quantick-guards` green. `CLAUDE.md` 9,439 -> 9,436
bytes, `AGENTS.md` 13,170 -> 13,169. No `crates/app` or
`crates/guards/Cargo.toml` change. Edit-time hook: `--file` on an app file
364-420 ms after vs 358-401 ms before. A8, G1, G4, G-AI1-4 and the closing
steps land on the PR after this archive; they are not claimed here.

## Amendment, after Q16 and Q17 landed

The coordinator's follow-up (verbatim below) adds one ask, numbered on
without renumbering anything:

- **R10** — Rebase onto `campaign/lean-a-plus` at `89bf713b` (Q16 #453 and
  Q17 #452 merged), keeping both sides of the conflicts (`CLAUDE.md`,
  `AGENTS.md`, `context-baseline.txt`, the guards registry) and trimming
  prose rather than raising a context ceiling or budget; run `--tighten`
  for the lower UI-free ceiling with a baseline comment naming Q16/Q17,
  verified by the independent count, closing issue A2; update the PR body
  and this goal; the four checks each on its own plus `cargo test -p
  quantick-guards`; push with `--force-with-lease`; delta follow-ups of
  arch-review (dimension 8 and step 0 on the rebase and tighten delta),
  ai-review and delivery-review at the new key; CI green; the ship gate.

It supersedes R6's "leave A2 open" (D1's deferral): A8 is kept as history,
A9 and A10 are added. Only `CLAUDE.md` conflicted in practice (the headless
bullet now names `control-host`, `paper` and `civil`); `AGENTS.md`,
`context-baseline.txt` and the registry merged cleanly.

Status at the amendment: rebased onto `89bf713b`; `--tighten` wrote
`crates/app 47665` and `!budget 47665` (50,795 -> 47,665, 125 files); the
independent Python count gives 47,665 in 125 files. `CLAUDE.md` 9,489 on the
base -> 9,486, `AGENTS.md` 13,613 -> 13,612, `context-baseline.txt`
untouched.

## Closing steps

- **C1** — `delivery-review` (tier medium: completeness pass) PASS recorded
  for the current key.
- **C2** — The PR is open, non-draft, base `campaign/lean-a-plus`, CI green
  at the head.
- **C3** — `mission_ship_gate.sh` run from the worktree, its output handed to
  the coordinator.

## Not applicable

- *Touches a hot path*: guard tooling only; no app code runs differently.
- *Touches anything user-visible*: no UI surface changes.
- *Adds a capability*: a guard registered in `GUARDS` is the repository's
  existing registration line, not a feed/bar/indicator/layer/panel/crate.
- *Adds something a trader does*: no trader action.
- *Engine / determinism territory*: not the engine; the guard is still
  test-first (R8).

## Verbatim request

> The coordinator's brief for campaign task Q18 (session of 2026-09-13):
>
> Execute campaign task **Q18**, issue https://github.com/milocaetano/quantick/issues/443
> (read it in full with `gh issue view 443`), of campaign #367, phase 2,
> mission tier **medium**, through the `/mission` workflow of this
> repository. You are a subagent and cannot ask the trader; make routine
> judgment calls, record them in the goal file and PR body.
>
> Scope for this PR: A1 — the ratchet on `app.lines.without_egui` in
> `crates/guards` (recorded ceiling at the current tree value, teeth both
> ways, `--tighten`, explicit exemption list with reasons, same mechanism as
> the size and extension ratchets; `guards` keeps zero dependencies), plus
> the one-sentence `CLAUDE.md` mention paid for in the context ratchet. A2's
> lower ceiling is recorded later, after Q16 (#441) and Q17 (#442) land —
> they are running now in parallel and will lower the number; say so in the
> PR body and leave A2 open. Do not touch `crates/app` sources.
>
> Rules: English in the repo; test-first (commit the failing guard test
> before the implementation); context ratchet — a CLAUDE.md growth must be
> paid for by trimming elsewhere in the tracked instruction set; run the four
> checks each on its own plus `cargo test -p quantick-guards`. Never truncate
> test output with head.
>
> Delivery: draft PR against `campaign/lean-a-plus` (tier medium, "Campaign
> task Q18 of #367", `Refs #443`). Reviews as the tier requires: arch-review
> (step 0 code-review), ai-review with resolved threads (two repair rounds
> budget), delivery-review last from fresh context. Then CI green,
> `gh pr ready <n>` once, `sh .claude/hooks/mission_ship_gate.sh ship <n>`.
> Never merge; never touch main.
>
> Issue #443, *Scope*: "A ratchet on `app.lines.without_egui` (same
> mechanism as the size and extension ratchets: a recorded ceiling, teeth
> both ways, `--tighten`), so a change can only lower it; exempt only wiring
> files that must be UI-free by nature, listed explicitly with a reason.
> After Q16 and Q17 land, `--tighten` records the lower number."
>
> Issue #443, *Acceptance criteria*: "A1: a guard test fails when a new
> UI-free production file is added to `crates/app` without lowering the
> ceiling elsewhere, and passes on the current tree. A2: the ceiling is
> recorded after Q16/Q17 and documented in `CLAUDE.md` in one sentence (paid
> for in the context ratchet). A3: merged into `campaign/lean-a-plus` after
> the first consolidated PR is cut."
>
> The coordinator's follow-up (same session, after Q16 and Q17 merged):
>
> Q16 (#453) and Q17 (#452) are both merged into `campaign/lean-a-plus`,
> and the tip is `89bf713bf321b54fc017cdf15f142a0260661610`. Please finish
> #450 now, including A2. The trader merged #449 into main as `d3d4b23d`.
>
> 1. Fetch, then `git rebase origin/campaign/lean-a-plus`. Expect conflicts
>    in CLAUDE.md, where the headless sentence now names `paper`, `civil`
>    and `control-host`, and in the AGENTS.md map and table, which is
>    exactly at its 13,613-byte ceiling. Also expect them in
>    `crates/guards/context-baseline.txt` and the guards registry. Keep both
>    sides, and trim prose rather than raise a context ceiling or budget.
> 2. Run `cargo run -p quantick-guards -- --tighten` to record the lower
>    UI-free ceiling after the two moves. Add a baseline comment saying the
>    drop came from Q16/Q17, and verify the number with your independent
>    Python count. This closes issue A2. Update the PR body and the goal
>    file, and use a goal amendment if the archive is already committed.
> 3. Run the four checks each on its own, plus `cargo test -p
>    quantick-guards`. Push with `--force-with-lease`.
> 4. Run delta follow-ups for arch-review (dimension 8 plus step 0 on the
>    rebase and tighten delta), ai-review and delivery-review at the new
>    key, with markers recorded through `review_report.sh`. Wait for CI
>    green, then run `sh .claude/hooks/mission_ship_gate.sh ship 450`.
>
> Never merge, never touch main, never use PowerShell for `gh pr`. Return
> the head, the key, the new ceiling, CI and the ship-gate output.
