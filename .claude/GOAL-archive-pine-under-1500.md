# Mission — split the Pine evaluator and compiler under 1,500 production lines

**Objective:** give `crates/pine/src/eval.rs` (1,669 production lines) and
`crates/pine/src/compile.rs` (1,540) owned sibling modules so every file in
the crate is at most 1,500 production lines, with no behaviour change, so an
agent asked to change one built-in family or one lowering pass reads one owner
instead of the whole subsystem.

**Tier:** `medium`. A campaign child of #367 (task key S9) for issue #376: a
pure structural move inside one headless crate, guarded by golden scripts, with
no contract, wire or financial rule in reach. It earns more than `small`
because the diff is large and the reviews have to see the whole of it; it does
not earn `high` because nothing here is a new behaviour, a trader-facing
surface or a call the trader has to make.

**Campaign base:** `origin/campaign/lean-a-plus` at `f574857b`. PR base is
exactly `campaign/lean-a-plus`.

## Request ledger

- **R1** — split `crates/pine/src/eval.rs` and `crates/pine/src/compile.rs`
  into owned sibling modules, each file at most 1,500 production lines as
  `crates/guards/src/size.rs` measures them.
- **R2** — *"no behaviour change"*: every evaluation result, compile
  diagnostic and built-in semantic is identical; every public path stays
  stable, so `crates/app` and the golden tests need no edit.
- **R3** — the moves are *pure*: every body moves unchanged, and the only
  widenings are the `pub(super)` marks a child or sibling module needs.
- **R4** — the seams follow the domain: built-in function families out of the
  evaluator core, lowering passes out of the compile driver.
- **R5** — both files' entries leave `crates/guards/size-baseline.txt` and the
  `!budget` line falls by exactly 3,209.
- **R6** — stay inside the files this child owns: the two roots, their new
  siblings, this child's baseline entries and the `mod` lines. Sibling
  missions own `crates/feed-mt5`, `bridge/mt5` and `crates/orderflow`.
- **R7** — the closing statement of purpose: the campaign's 1,500-line rule
  holds for `crates/pine` with no signed exception left behind.

## Decisions (from the coordinator, answering step 3)

- **D1** — pure moves only; a widening beyond `pub(super)`/`pub(crate)`, a new
  field or a consumer edit stops the work and is reported.
- **D2** — the ceiling is production lines as the size guard measures them;
  moving inline tests to sidecars is allowed, never required.
- **D3** — the seams named in R4; split further if any file would still exceed
  1,500.
- **D4** — A2's proof is a statement-line multiset before and after, with the
  command and its result in the PR body; A1's proof is the Pine golden tests
  passing unchanged, with counts.
- **D5** — performance: evaluation runs per bar, but moving methods between
  `impl` blocks adds no work; state the rate of each touched path in the PR.
- **D6** — `--tighten` after the moves; both entries leave the baseline,
  `!budget` falls by exactly 3,209, guards pass, no new module cycle or crate
  edge. Do not rebase if the campaign tip moves.
- **D7** — tier `medium`: `arch-review` with `code-review` at `low` and the
  full shape pass; `ai-review` completion; `delivery-review` completeness pass
  inline. At most two step-0 rounds.
- **D8** — `gh pr ready` once, from the Bash tool with the `cd` prefix; never
  merge, never touch main.

## Assumptions

- **S1** — the three `eval/` siblings (`kernels`, `pure`, `objects`) and the
  three `compile/` siblings (`declarations`, `fold`, `resolve`) are the right
  grain: each is one family or one numbered pass, and each is named by the
  section banner the file already carried. *Wanted to ask*; D3 names the
  families but not their count, and the code answers it in a reading.
- **S2** — the four `DEFAULT_*` draw-object constants, `style_of_tag`,
  `source_of_builtin` and `Scope` move with their single consumer rather than
  staying in the root. Safe: each has exactly one caller after the split, and
  a constant read from one module belongs to it.
- **S3** — a pre-existing misplaced doc comment in `eval.rs` (the `kind`
  documentation sitting above `pin_kernel_length`) is left exactly as it is.
  Safe: D1 forbids touching a body, and a grandfathered line is not this
  diff's to author.

## Acceptance criteria

- [ ] **A1** — `crates/pine/src/eval.rs`, `crates/pine/src/compile.rs` and
      every new sibling are at most 1,500 production lines.
      *Evidence:* `wc -l` per file and the guard's own numbers from
      `cargo run -p quantick-guards -- --tighten`.
      → the PR body's size table. *(R1)*
- [ ] **A2** — every production move is pure: the statement-line multiset
      before and after differs only by `pub(super)` prefixes and the new
      `impl` block headers.
      *Evidence:* the `bodies.py` diff, command and result.
      → the PR body's purity section. *(R2, R3)*
- [ ] **A3** — every Pine golden script test passes unchanged and no test
      expectation was edited.
      *Evidence:* `cargo test -p quantick-pine`, `cargo test -p
      quantick-indicators`, and `git diff --name-only` showing no test file.
      → the PR body. *(R2)*
- [ ] **A4** — the seams are the named families and passes, one owner per
      sibling, and no public path moved.
      *Evidence:* the moved-item table; `crates/app` untouched.
      → the PR body. *(R4)*
- [ ] **A5** — `crates/guards/size-baseline.txt` carries no entry for either
      file and `!budget` fell by exactly 3,209 (14,673 → 11,464).
      *Evidence:* the baseline diff; `cargo test -p quantick-guards`.
      → the PR body. *(R5, R7)*
- [ ] **A6** — the diff touches only the files this child owns.
      *Evidence:* `git diff --name-only campaign/lean-a-plus...HEAD`.
      → the PR body. *(R6)*
- [ ] **G1** — every artifact in English; conventional commits.
      *Evidence:* `cargo test -p quantick-guards` (the language guard) and the
      commit log. → the PR.
- [ ] **G2** — the four checks green on the final head, run one at a time, and
      CI green at the exact PR head.
      *Evidence:* command output in the PR body; `gh pr checks`. → the PR.
- [ ] **G3** — performance impact declared per touched path.
      *Evidence:* the PR body's performance section. → the PR.
- [ ] **G4** — `arch-review` with every Blocker and Should-fix resolved or
      deferred in the PR body; `ai-review` completion recorded.
      *Evidence:* review verdicts and the recorded markers. → the PR.

## Not applicable

- *Touches a hot path* — evaluation is per bar, but no call shape, loop or
  allocation changes; `cargo bench` numbers would measure the compiler's
  inlining, not this change. Declared in the PR instead (G3).
- *Touches anything user-visible* — `pine` is headless; no surface, no hook,
  no `visual-qa`, no `trader-ux-review`.
- *Adds a capability* / *adds something a trader does* — nothing is added.
- *Engine / determinism territory, test-first* — no fixture and no expected
  output changes; the existing golden scripts are the fixture, and they are
  what proves the move pure.
- *Docs/skills only* — this is runtime code.

## Closing steps

- **C1** — `delivery-review` returns PASS (completeness pass at this tier).
- **C2** — the PR is open against `campaign/lean-a-plus`, reviews recorded,
  CI green, and marked ready.
- **C3** — the handoff block returns to the coordinator: issue, branch,
  worktree, PR URL, head SHA, base tip, the production-line table, review
  verdicts, markers, the CI run, findings, repair batches, readiness, any
  `human_decision`, and the coordinator's next action.

## The request as received

> Campaign child mission **S9** of campaign #367: split `crates/pine/src/eval.rs`
> (1,669 production lines) and `crates/pine/src/compile.rs` (1,540) into owned
> sibling modules, each at most 1,500 production lines, with no behaviour change.
