# Mission: leave the size baseline with no signed per-file exception

**Objective.** Retire the last per-file entry in
`crates/guards/size-baseline.txt` and set `!budget 0`, so that "no production
file over 1,500 production lines" is a rule the size guard enforces rather
than a state the tree happens to be in; and make the guards' report honest when
a scan fails, instead of printing the most flattering number it has.

**Tier:** `medium`. Campaign child S10 of #367 (issue #377), assigned at
`medium` by the coordinator: a guard change whose failure mode is a guard that
reports clean when it could not look, but with no runtime, money or UI path in
reach. `medium` buys the full gate table, `code-review` at `low` inside
`arch-review` with the full shape pass, and `delivery-review`'s completeness
pass inline. Step 3's questions were answered by the coordinator as D1–D7.

## Request ledger

- **R1** — "the baseline holds no per-file entries and `!budget 0`"; "Remove
  `crates/app/src/app.rs 769`". The mechanism for a future legitimate
  exception stays: "a signed entry plus a budget raise in the same change is
  still possible and still reviewed". *(A1, A2)*
- **R2** — A file over 1,500 production lines fails the guard "without an
  escape hatch other than an explicit, reasoned budget raise". *(A2, A3)*
- **R3** — "a guard test that a fixture file of 1,501 production lines fails
  and one of 1,500 passes with an empty baseline and `!budget 0`". *(A3)*
- **R4** — "a test that a signed entry above the threshold without a matching
  budget raise fails (if such a test does not already exist, add it; if it
  exists, name it)". *(A4)*
- **R5** — Rewrite the baseline header: "one accurate paragraph that states
  the rule", then "a short, accurate history of how the file reached zero in
  campaign #367" naming the twenty entries and the PRs that retired them,
  "with no stale arithmetic"; fold the per-cut paragraphs in; recover S5's lost
  rationale from `29bff61b` for the control-plane line; drop the stale 40,121,
  1,297 and 5,891. *(A5)*
- **R6** — `extension_boundary.rs` `measured()` "must not report a failed scan
  as 0": propagate the error so `--report` prints the failure, with
  `size::measured` and `context::measured` consistent; a test with an
  unreadable or missing root. *(A6, A7)*
- **R7** — `CLAUDE.md`: "at most one sentence in *Keeping the trunk small*"
  saying the baseline is empty and every production file is at or under
  1,500, its bytes paid in the context ratchet by trimming redundant prose in
  the same section, or the exact overage reported. *(A8)*
- **R8** — Purpose, the ask that judges the rest: "so that 'no production file
  over 1,500 production lines' becomes a rule the guard enforces, and make the
  guards report honest when a scan fails". *(A2, A6)*
- **R9** — Write only in `crates/guards/*` and one sentence of `CLAUDE.md`
  (plus this mission's own archive); never the sibling-owned control, worker,
  state, health or layers-test files. *(A9)*

Operational instructions (worktree, draft PR, reviews, markers, CI, one
`gh pr ready`, never merge) map to G/C below, not to R/A.

## Decisions (from the coordinator, answering step 3)

- **D1** — No per-file entries, `!budget 0`; the exception mechanism stays.
- **D2** — Header: one rule paragraph, then a short accurate history of the
  twenty entries and PRs #383, #384, #388, #390, #391, #392, #395, #396, #399,
  and app.rs retired here; S5's rationale recovered from `29bff61b`.
- **D3** — Tests: 1,501 fails / 1,500 passes with an empty baseline and
  `!budget 0`; a signed over-threshold entry without a budget raise fails.
- **D4** — `measured()` propagates errors; `--report` prints the failure
  consistently with `scan.unreadable`/`scan.blind`; size and context
  consistent; a test with a missing or unreadable root.
- **D5** — At most one `CLAUDE.md` sentence, paid in the context ratchet.
- **D6** — Guard change: the delivery contract's first validation row;
  `medium` review shape; at most three step-0 rounds.
- **D7** — `gh pr ready` once, cd-prefixed, from the Bash tool; stop and
  report if denied; never merge, never touch `main`.

## Assumptions

- **S1** — The issue body's "pure moves" scope and its A2/A3 (line multiset,
  pinned tests unchanged) are the campaign's template for S1–S9. S10 moves no
  production code, so A2's multiset has nothing to prove; A3's "no test
  expectation edited" is honoured except for the one test that asserted the
  `app.rs` entry exists (`baseline_parsing_ignores_comments`), which R1
  retires by definition. Safe: the coordinator's brief and D1 name that entry
  for removal, and the test's comment-parsing property is kept on a fixture.
- **S2** — "Consistent" (D4) is read as: every `Ratchet::measured` returns a
  `Result`, and a walk that could not see every tracked path — an unlistable
  directory, an unreadable file, a file that does not decode — is a failed
  measurement, not a smaller total. `cycle::measured` shares the registry
  signature and is made consistent too. Safe: the alternative is a signature
  that lets one ratchet keep the defect the issue names.
- **S3** — `--report` on a failed scan keeps printing the whole table, with
  `failed` in place of the number, a `scan.failed` count row beside
  `scan.unreadable`/`scan.blind`, the reasons on stderr, and exit code 1. D4
  offers "exits non-zero or prints an explicit `scan.failed` line"; doing both
  costs nothing and a script checking the status is not misled. Reversible in
  one edit.
- **S4** — Budget and the BUDGET_SLACK: with an empty baseline, the recorded
  total is 0 and `!budget 0` sits at it, so the slack rule is quiet. Code
  comments that cite "eighteen entries recorded today" are updated only where
  they state a current count as fact.

## Acceptance criteria

- [x] **A1** — `crates/guards/size-baseline.txt` has no `<path> <count>` line
      and its only directive is `!budget 0`.
      *Evidence:* `grep -vE '^(#|$)' crates/guards/size-baseline.txt` prints
      exactly `!budget 0`; `--report` shows `ratchet.size.budget 0` and
      `ratchet.size.recorded 0`. → PR body. *(R1)*
- [x] **A2** — The guard passes on the real tree with that baseline, and
      `cargo run -p quantick-guards -- --tighten` changes nothing.
      *Evidence:* `cargo test -p quantick-guards` green;
      `--tighten` output and a clean `git status`. → PR body. *(R1, R2, R8)*
- [x] **A3** — A fixture file of 1,501 production lines fails the size guard
      and one of 1,500 passes, both under an empty baseline with `!budget 0`.
      *Evidence:* the named tests in `crates/guards/src/size.rs`. → PR body.
      *(R2, R3)*
- [x] **A4** — A signed entry above the threshold without a matching budget
      raise fails, and the same entry with the budget raised passes.
      *Evidence:* named tests in `size.rs` (existing and new). → PR body.
      *(R4)*
- [x] **A5** — The baseline header is one rule paragraph plus a history of
      the twenty entries and the PRs that retired them, carrying S5's
      recovered rationale and none of 40,121, 1,297 as current, or 5,891.
      *Evidence:* the file's text and `grep -c` for those numbers. → PR body.
      *(R5)*
- [x] **A6** — `--report` over a root whose scan fails prints `failed`, not
      `0`, for every affected ratchet, a non-zero `scan.failed`, the reasons
      on stderr, and exits non-zero; on the real tree it still exits 0 with
      `scan.failed 0`.
      *Evidence:* an integration test in `crates/guards/tests/guards.rs`
      running the binary against a missing root; the command's output. → PR
      body. *(R6, R8)*
- [x] **A7** — `extension_boundary::measured`, `size::measured`,
      `context::measured` (and `cycle::measured`) return an error for a scan
      that failed; each has a unit test over a missing or unreadable root.
      *Evidence:* named tests. → PR body. *(R6)*
- [x] **A8** — `CLAUDE.md` carries exactly one new sentence in *Keeping the
      trunk small* with the rule, and `cargo test -p quantick-guards` passes
      the context ratchet without raising `CLAUDE.md`'s ceiling or the
      budget — or the exact overage is reported.
      *Evidence:* the diff of `CLAUDE.md` and `context-baseline.txt`. → PR
      body. *(R7)*
- [x] **A9** — The diff touches only `crates/guards/*`, `CLAUDE.md` and this
      mission's archive. *Evidence:* `git diff --stat` against the base. → PR
      body. *(R9)*
- [x] **G1** — Every artifact English; conventional commits with the session
      trailers. *Evidence:* `arch-review` dimension 8; guards. → review report.
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`,
      `env -u QUANTICK_BUBBLES cargo test --workspace` green, run one at a
      time, plus `sh .claude/hooks/guardrails_test.sh`; CI green at the exact
      PR head. *Evidence:* outputs and the CI run URL. → PR body.
- [x] **G3** — Performance impact declared: every touched path is the guards
      binary or its tests (rate: rare, developer-time); no per-trade,
      per-depth or per-frame path is touched. *Evidence:* PR body.
- [ ] **G4** — `arch-review` over the diff against `origin/campaign/lean-a-plus`
      with `code-review` at `low` and the full shape pass; every Blocker and
      Should-fix resolved or deferred in the PR body; `ai-review` completion
      recorded with zero unresolved threads. *Evidence:* review reports on the
      PR. → PR comments.

## Evidence at the implementation commit

- A1: `grep -vE '^(#|$)' crates/guards/size-baseline.txt` prints `!budget 0`;
  `--report` rows `ratchet.size.budget 0`, `ratchet.size.recorded 0`,
  `ratchet.size.measured 184691`.
- A2: `cargo test -p quantick-guards` green; `--tighten` printed "nothing to
  tighten" for all four ratchets and left `git status` clean.
- A3: `size::tests::a_file_one_line_over_the_threshold_fails_an_empty_baseline`,
  `size::tests::a_file_at_the_threshold_passes_an_empty_baseline`.
- A4: new `size::tests::a_signed_exception_without_a_budget_raise_fails` and
  `size::tests::a_signed_exception_with_its_budget_raise_passes`; existing
  `size::tests::a_raise_that_pays_for_nothing_is_over_budget`.
  `size::tests::the_baseline_holds_no_exception_and_a_zero_budget` pins A1.
- A5: `grep -cE "40,121|1,297|5,891" crates/guards/size-baseline.txt` is 0;
  the header is the rule, the budget's reason, and the per-PR history.
- A6: `tests/guards.rs::the_report_says_failed_rather_than_zero_when_a_scan_fails`
  and `tests/extension_boundary.rs::a_failed_scan_is_not_measured_as_zero`;
  the real tree's report carries `scan.failed 0` and exits 0
  (`the_report_is_byte_identical_across_runs`).
- A7: `tests/guards.rs::every_ratchet_fails_to_measure_a_tree_it_cannot_scan`
  (all four through the registry), `size::tests::measured_is_a_failure_when_the_walk_missed_a_file`,
  `context::tests::measured_is_a_failure_when_an_instruction_directory_is_missing`,
  `ratchet::tests::a_walk_that_missed_a_path_has_no_total`.
- A8: `CLAUDE.md` 9,551 -> 9,548 bytes, under its 9,551 ceiling; the context
  baseline and budget unchanged.
- A9: `git diff --stat origin/campaign/lean-a-plus...HEAD` lists only
  `CLAUDE.md`, `crates/guards/*` and this archive.
- G2 (local): fmt check, clippy, build and
  `env -u QUANTICK_BUBBLES cargo test --workspace` (3,614 passed) each exit
  0, run one at a time; `sh .claude/hooks/guardrails_test.sh` 227 passed.
  CI at the PR head is recorded in the PR.

## Not applicable

- *Touches a hot path*, *user-visible*, *adds a capability*, *adds something
  a trader does*, *engine/determinism*: the diff is the guards crate, its
  baselines and one `CLAUDE.md` sentence — no app, engine or UI code.
- *Docs/skills only*: tests and guard source change, so the full shape pass
  and the first validation row apply.

## Closing steps

- **C1** — `delivery-review` completeness pass (inline, tier `medium`) returns
  PASS and its marker is recorded with the campaign key.
- **C2** — Draft PR against exactly `campaign/lean-a-plus`, then one
  `gh pr ready` after reviews, markers and green CI; the coordinator merges.

## The request as received

The coordinator's delegated task request, quoted in full and verbatim:

> You are executing campaign child mission **S10** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: leave the size baseline with no signed per-file exception, so that "no production file over 1,500 production lines" becomes a rule the guard enforces, and make the guards report honest when a scan fails. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision is reported back, never guessed.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (*Keeping the trunk small*: the size ratchet, teeth both ways, pay-as-you-go growth, the budget; *Keeping the instructions small*: the context ratchet covers CLAUDE.md, so any sentence you add is paid for there).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `medium`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-guards --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` (base `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`, review key from `sh .claude/hooks/campaign_context.sh key "$WT"`), `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #377 (`gh issue view 377 --comments`): scope, acceptance criteria, and the two comments carrying inputs from the integrations (the lost S5 rationale paragraph recoverable from `git show 29bff61b:crates/guards/size-baseline.txt`; stale numbers in S1's block, 40,121, and S9's block, a 1,297 fall; S8's block closing on 5,891).
> 5. Issue #365 item 3 (`gh issue view 365`): `crates/guards/src/extension_boundary.rs` `measured()` swallows inventory errors with `unwrap_or(0)`, so `cargo run -p quantick-guards -- --report` prints 0 when the scan fails.
> 6. The guard: `crates/guards/src/size.rs` (THRESHOLD 1,500, `production_flags`, the baseline parser, `--tighten`, the budget rules and their tests), `crates/guards/size-baseline.txt` (header prose and the one remaining entry `crates/app/src/app.rs 769`, which is below the threshold and only exists as history), `crates/guards/src/context.rs` and `crates/guards/context-baseline.txt` (the context ratchet), `crates/guards/tests/guards.rs`.
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\feat-size-baseline-no-exceptions` (Git Bash `/c/src/quantick-worktrees/feat-size-baseline-no-exceptions`), branch `feat/size-baseline-no-exceptions`, cut from `origin/campaign/lean-a-plus` at `f36cc022`. Every command starts with `cd /c/src/quantick-worktrees/feat-size-baseline-no-exceptions &&`. Never write to `C:\src\quantick` or other worktrees.
> - You own `crates/guards/*` (sources, tests, baselines) and one sentence in `CLAUDE.md`. Siblings in flight own `crates/app/src/control/*` (Q2), worker/state/health files (Q3) and `crates/app/src/app/tests/layers_tests.rs` (Q8); do not edit those.
>
> ## Decisions
> - D1: the baseline holds no per-file entries and `!budget 0`. The mechanism for a future legitimate exception stays (a signed entry plus a budget raise in the same change is still possible and still reviewed); only the current data goes to zero. Remove `crates/app/src/app.rs 769` (it is below the threshold).
> - D2: rewrite the baseline header: one accurate paragraph that states the rule (every production file at or under 1,500 production lines; an exception needs a signed entry and a budget raise, and the budget only ever goes down on its own), followed by a short, accurate history of how the file reached zero in campaign #367 (the twenty entries and the PRs that retired them: #383 pane, #384 paper trading, #388 renderers, #390 drawings, #391 control plane, #392 chrome, #395 MT5 stream, #396 orderflow, #399 pine, and app.rs retired here), with no stale arithmetic. Fold the per-cut paragraphs into that history; keep what a reader needs (why an entry left, where to look) and drop what was only true on the day. Recover S5's lost rationale from `git show 29bff61b:crates/guards/size-baseline.txt` for the control-plane line.
> - D3: tests: a guard test that a fixture file of 1,501 production lines fails and one of 1,500 passes with an empty baseline and `!budget 0`; a test that a signed entry above the threshold without a matching budget raise fails (if such a test does not already exist, add it; if it exists, name it). Keep every existing guard test green.
> - D4: `extension_boundary.rs` `measured()` must not report a failed scan as 0: propagate the error so `--report` prints the failure (and exits non-zero or prints an explicit `scan.failed` line consistent with how `scan.unreadable`/`scan.blind` are reported today); make `size::measured` and `context::measured` consistent with that. Add a test with an unreadable or missing root.
> - D5: `CLAUDE.md`: at most one sentence in *Keeping the trunk small* saying the baseline is empty and every production file is at or under 1,500 production lines; pay its bytes in `crates/guards/context-baseline.txt` by trimming an equivalent amount of redundant prose in the same section if the context ratchet requires it, or report the exact overage.
> - D6: this is a guard change: the delivery contract's first row applies (tests changed). Tier `medium`: `arch-review` with `code-review` at `low`, full shape pass; `ai-review` completion; `delivery-review` completeness pass inline. At most three step-0 rounds.
> - D7: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved or the PR is DIRTY, stop and report; the coordinator rebases. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Environment notes
> - Use `python`, not `python3`. If `cargo fmt` is blocked, run `rustfmt` directly then `cargo fmt --all -- --check`. Run the four checks one at a time, never `||` or `| head`; read whole failures. Do not wait on background commands; foreground with an adequate timeout.
> - `sh .claude/hooks/guardrails_test.sh` also reads the guards' outputs; run it before committing.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> - Known CI load flake being fixed by Q8: `layers_tests::the_trade_paint_layer_switch_stops_the_marks`; report, rerun the job once at most.
>
> ## Delivery
> 1. Implement with the verification loop (`cargo check -p quantick-guards`, `cargo test -p quantick-guards`, `cargo run -p quantick-guards -- --report`, `cargo run -p quantick-guards -- --tighten` changing nothing).
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately, plus `sh .claude/hooks/guardrails_test.sh`.
> 3. Archive `GOAL.md` per mission step 8 (slug `size-baseline-no-exceptions`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/feat-size-baseline-no-exceptions && gh pr create --draft --base campaign/lean-a-plus --title "feat(guards): leave the size baseline with no signed per-file exception" --body-file -` with a heredoc body per the PR template: tier, "Campaign child of #367 (S10); closes on integration: #377; addresses #365 item 3", the before/after of the baseline, the new tests and what each proves, the `--report` output on a failed scan, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. `gh pr ready <n>` once (D7).
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; head SHA; base tip; the final `--report` size lines; tests added; the context-ratchet outcome; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; repair batches; ready accepted or denied; any human_decision; the coordinator's next action.
