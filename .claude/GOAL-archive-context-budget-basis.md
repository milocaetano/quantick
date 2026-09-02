# GOAL — recover the budget-basis fix that missed the merge, and answer its review

**Tier:** `medium`. The change is small and entirely inside `crates/guards`,
but the branch is 423 changed lines — over `SMALL_TIER_MAX_CHANGED_LINES`
(300) — so the `small` exemption would lapse at the gate and the branch pays
in full. `medium` is declared rather than discovered.

## Request (as made, in conversation)

> eu fiz um merge agora nao sei se fiz certo acho que falei com outra dsessao
>
> e acabei mergeando o seu pr tavlez

Preceded by `para eu mergear` — the instruction to close the gap that stopped
PR #280 being mergeable in good conscience.

## What happened

PR #280 was merged at `f480677`. The commit that fixed the budget basis,
`014e09a`, was pushed to the branch but GitHub still reported the older head
when the merge was made, so it did not go in. The merge itself was correct;
the timing caught an incomplete branch.

## Request ledger

- **R1** — the fix that missed the merge must reach `main`.
- **R2** — the findings raised against it must be answered, not carried.
- **R3** — the branch must be mergeable without the reviewer having to trust
  an unreviewed head, which is what stalled #280.

## Acceptance criteria

- [ ] **A1** (R1) — `main` gets the single-basis budget: moving prose out of a
      ceilinged file into a sub-threshold sibling is not reported as added
      weight. *Evidence:* `moving_prose_out_of_a_ceilinged_file_is_not_added_weight`
      in `crates/guards/src/context.rs`, plus a run of the binary against a
      scratch tree. → the PR body.
- [ ] **A2** (R2) — the four findings from the bug pass over `014e09a` are
      each fixed or deferred with a stated reason. *Evidence:* the commit
      message and the PR body name all four. → `293b4eb`.
- [ ] **A3** (R2) — the edit-time hook does not walk the repository for a
      verdict that ignores the walk. *Evidence:* `size::check_file` passes
      `&[]` on the baseline branch. → the diff.
- [ ] **A4** (R2) — a path this guard does not read stays silent whatever the
      tree looks like. *Evidence:* `quantick-guards --file crates/probe/src/a.rs`
      on a tree with no `.claude/skills/` exits 0 with no output. → the PR body.
- [ ] **A5** (R2) — the recorded budget equals the tree it ships with.
      *Evidence:* measured 231,973 against `!budget 231973`. → the PR body.

## Injected gates

- **G1** — `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test
  --workspace`.
- **G2** — `cargo test -p quantick-guards` and `sh .claude/hooks/guardrails_test.sh`.
- **G3** — `arch-review` over `git diff origin/main...HEAD`, full shape pass:
  this branch changes Rust.
- **G4** — `delivery-review`, completeness pass, as the `medium` tier sets.

## Closing steps

- **C1** — the PR is open.
