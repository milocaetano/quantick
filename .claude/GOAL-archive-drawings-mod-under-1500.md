# Mission — split `drawings/mod.rs` under 1,500 production lines

**Objective:** split `crates/app/src/drawings/mod.rs` (2,283 production lines)
into owned sibling modules, each at most 1,500 production lines, with no
behaviour change, so an agent that opens the file to change one drawing
behaviour reads one owner and not the whole subsystem.

**Tier:** `medium`. Campaign child S4 of
https://github.com/milocaetano/quantick/issues/367, task
https://github.com/milocaetano/quantick/issues/371. The work is a mechanical
move over an already-modular directory, but it touches the object model every
drawing tool and the layout store depend on, so it earns the full shape pass
and a completeness `delivery-review` — not `high`, because no contract, wire
format or money path is in scope.

**Base:** `origin/campaign/lean-a-plus`; PR base exactly `campaign/lean-a-plus`.

## Request ledger

- **R1** — `crates/app/src/drawings/mod.rs` and every new sibling under
  `crates/app/src/drawings/` is at most 1,500 production lines as
  `crates/guards/src/size.rs` measures them. *(A1)*
- **R2** — the split is by responsibility: object model versus placement rules
  versus serialisation versus the tool registry, and the registry stays the
  single registration point so a new tool still docks as one file plus one
  line. *(A1, A2)*
- **R3** — pure moves only: "every body moves unchanged; the only widenings are
  `pub(super)` marks". No drawing behaviour, placement, hit-testing,
  persistence format or serialisation change. *(A2)*
- **R4** — the persisted drawing format does not change and the drawings
  fixtures round-trip unchanged; no test expectation is edited. *(A1, A3)*
- **R5** — `crates/guards/size-baseline.txt` no longer carries an entry for
  `crates/app/src/drawings/mod.rs`, and the `!budget` line did not rise. *(A4)*
- **R6** — the four checks are green on the final head, run one at a time, CI
  is green at the exact PR head, and `cargo test -p quantick-guards` passes.
  *(A5)*
- **R7** — no per-trade, per-depth or per-frame path changes its complexity,
  stated per touched path in the PR body. *(A2, G4)*
- **R8** — the diff stays inside this mission's owned paths: `drawings/mod.rs`,
  new files under `crates/app/src/drawings/`, this mission's
  `size-baseline.txt` entry and the goal archive. Siblings own the order-flow
  renderers and the control plane. *(A2)*
- **R9** — the purpose the others are judged against: the file stops being the
  whole subsystem's single owner, so "an agent that opens a file to change one
  behaviour reads one owner". *(A1, A2)*

## Decisions (from the coordinator, D1..D8)

- **D1** — pure moves only; a widening beyond `pub(super)`/`pub(crate)`, or a
  new field on a trunk struct, stops the work and is reported as a
  human_decision.
- **D2** — the ceiling is production lines as `size.rs` measures them; moving
  inline tests to sidecars is allowed, never required.
- **D3** — split by responsibility; the registry stays the single registration
  point; split further if any file would still exceed 1,500.
- **D4** — A2 is proved by a statement-line multiset before/after, with command
  and result in the PR body; A1's round-trip by naming the tests.
- **D5** — drawings paint per frame; state each touched path's rate in the PR
  body; measure `APP_HEALTH_SUMMARY` only if a call shape changes.
- **D6** — after the moves, `--tighten`; the `drawings/mod.rs` entry leaves the
  baseline; `!budget` must not rise; `cargo test -p quantick-guards` passes.
- **D7** — tier `medium`: `arch-review` with `code-review` at `low`, full shape
  pass; `ai-review` completion; `delivery-review` completeness pass inline. At
  most two step-0 rounds.
- **D8** — one `gh pr ready` attempt through the Bash tool; if the gate denies
  it, stop at the draft PR and report.

## Assumptions

- **S1** — serialisation of drawings does not live in `drawings/mod.rs` (the
  persisted format is written by the layout store), so the split's
  serialisation axis is discharged by leaving the object model's shape
  untouched rather than by carving a serialisation sibling. *Safe to assume:
  the code answers it in under a minute of reading, and D3's other axes still
  give every moved line an owner.*
- **S2** — the new siblings are named after what they own (`tool.rs`,
  `collection.rs`, `placement.rs`) and live beside the tool files under
  `crates/app/src/drawings/`. *Conventional file placement and naming in this
  repo; reversible in one edit.*
- **S3** — the inline `#[cfg(test)] mod tests;` sidecar stays where it is; only
  the `use` lines it needs to name moved items may change, never an
  expectation. *D2 makes test moves optional, and R4 forbids editing
  expectations.*

## Acceptance criteria

- [ ] **A1** — `crates/app/src/drawings/mod.rs` and every new sibling is at
      most 1,500 production lines, and the drawings persisted-format fixtures
      round-trip unchanged.
      *Evidence:* `cargo run -p quantick-guards -- --report` size table, and
      the named round-trip tests passing.
      → the PR body. *(R1, R2, R4, R9)*
- [ ] **A2** — every production move is a pure move: `git diff --stat` shows
      the lines leaving and arriving, and a statement-line multiset over the
      moved items proves nothing was lost or duplicated.
      *Evidence:* the command and its output.
      → the PR body. *(R2, R3, R7, R8, R9)*
- [ ] **A3** — every pinned or golden test in the touched crates passes
      unchanged, and no test expectation was edited.
      *Evidence:* `cargo test --workspace` output and the diff of the test
      files.
      → the PR body. *(R4)*
- [ ] **A4** — `crates/guards/size-baseline.txt` no longer carries an entry for
      `crates/app/src/drawings/mod.rs`, and the `!budget` line did not rise.
      *Evidence:* the baseline diff.
      → the PR body. *(R5)*
- [ ] **A5** — the four checks are green on the final head, run one at a time,
      CI is green at the exact PR head, `cargo test -p quantick-guards` passes.
      *Evidence:* command output and the CI run URL.
      → the PR body and the handoff. *(R6)*
- [ ] **A6** — merged into `campaign/lean-a-plus` through a PR whose base is
      exactly that branch, with the merge read back and recorded on the issue.
      *Evidence:* the coordinator's merge readback. → issue #371. *(R6)*
      *(Not this child's action: the coordinator serialises campaign merges.)*
- [ ] **G1** — every artifact in English; conventional commits.
      *Evidence:* `arch-review` dimension 8. → the review verdict.
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace`, each
      run on its own, green on the final head.
      *Evidence:* command output. → the PR body.
- [ ] **G3** — `arch-review` over the exact diff against the campaign base with
      its step 0 bug pass; every Blocker and Should-fix resolved or deferred;
      `ai-review` completion recorded; `delivery-review` completeness pass.
      *Evidence:* review verdicts and markers. → the PR and the git dir.
- [ ] **G4** — performance: no per-trade, per-depth or per-frame path changes
      its complexity, stated per touched path.
      *Evidence:* the declaration. → the PR body.

## Not applicable

- *Touches a hot path* — the per-frame paint path is touched only by moving its
  methods between `impl` blocks in the same crate; no call shape changes, so
  the complexity statement under G4 replaces a measured run.
- *Touches anything user-visible* — no surface changes; `visual-qa` and
  `trader-ux-review` have nothing to grade that a pure move could alter.
- *Adds a capability* — nothing docks; the registry keeps its one registration
  point.
- *Adds something a trader does* — no new action, tool, trade or lock.
- *Engine / determinism territory* — `drawings` is UI-only; the deterministic
  engine never learns about UI marks.
- *Docs/skills only* — this is runtime code; the full shape pass applies.

## The request as received

> You are executing campaign child mission **S4** of campaign #367 for
> milocaetano/quantick: split `crates/app/src/drawings/mod.rs` (2,283
> production lines) into owned sibling modules, each at most 1,500 production
> lines, with no behaviour change. [...] D1: pure moves only. Every body moves
> unchanged; the only widenings are `pub(super)` marks. No drawing behaviour,
> placement, hit-testing, persistence format or serialisation change. [...]
> D3: split by responsibility: object model versus placement rules versus
> serialisation versus the tool registry; the registry stays the single
> registration point so a new tool still docks as one file plus one line. If
> any file or sibling would still exceed 1,500, split further.
