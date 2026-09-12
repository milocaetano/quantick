# Mission: let the campaign merge and ready gate accept a cd-prefixed command

Make `pr-gate` in `.claude/hooks/guardrails.sh` accept exactly one leading
`cd <worktree> &&` before the otherwise bare, head-pinned campaign
`gh pr merge` / `gh pr ready` statement, so the documented campaign integration
commands are reachable from a Bash tool whose working directory resets between
calls.

**Why it matters.** Claude Code's Bash tool resets `cwd` between calls, so a
campaign child reaches its worktree only through a leading `cd <dir> &&`.
`effective_dir` already reads the worktree from that prefix, but the merge and
campaign-ready form checks then require the whole command to equal the bare
statement. The result, observed on 2026-09-11 while integrating PR #382, is a
gate no honest agent can satisfy: the cd-prefixed form is denied as a compound
command, and the bare form resolves to the main checkout and is denied as a
merge to main. A gate that cannot be satisfied is a gate that invites a route
around it.

**Tier:** `medium`. The change is one small, well-bounded shell function plus
its call sites, but it moves a security-shaped gate, so it takes the full
request ledger, the full injected gate table, `arch-review` with its step 0 bug
pass at `low`, and a `delivery-review` completeness pass.

## Request ledger

- **R1** — `pr-gate` accepts, for the campaign `merge` and `ready` forms,
  exactly one leading `cd <dir> &&` before the otherwise bare statement, where
  `<dir>` is the string `effective_dir` already extracts, quoted or not, and the
  remainder equals today's exact pinned form. *(issue #386 Scope, bullet 1)*
- **R2** — every other spelling stays denied with the existing messages: a
  second `&&`, a `;`, a `||`, a trailing statement, `--auto`, `--admin`, `-R` /
  `--repo`, merge-queue shortcuts. *(issue #386 Scope, bullet 1;
  "still reject any other statement")*
- **R3** — what the gate *checks* does not change at all: markers, AI
  completion, open threads, head pin, base advance, draft state.
  *(issue #386, "Out of scope")*
- **R4** — the bare forms stay accepted where they already were. *(D1)*
- **R5** — `docs/campaign/integration.md` and `.claude/hooks/README.md` show the
  accepted form and say why it exists, and state that the PowerShell tool is
  outside the `Bash` matcher and must never be used to route around the gate;
  a denial is reported to the coordinator. *(issue #386 Scope, bullet 2; D2)*
- **R6** — the hook suites carry positive and negative cases for the new form.
  *(issue #386 Scope, bullet 2; A1; D3)*

## Decisions

- **D1** — the accepted forms on a campaign base become exactly
  `cd <dir> && gh pr merge N --merge|--squash --match-head-commit <HEAD>` and
  `cd <dir> && gh pr ready N`; the bare forms stay accepted; everything else is
  denied with the existing messages; what the gate checks does not change.
- **D2** — the documented reason is the Bash tool's `cwd` reset, with the
  2026-09-11 PR #382 observation; both documents also state the PowerShell
  prohibition.
- **D3** — the tests live where the merge/ready form cases live today
  (`campaign_context_test.sh`), with the main-base cases in
  `guardrails_test.sh`: cd-prefixed merge accepted; cd-prefixed merge plus a
  trailing statement denied; two `cd`s denied; cd-prefixed ready accepted; bare
  merge from a main-based worktree still denied as a merge to main.
- **D4** — validation is the delivery contract's second row plus behavioural
  proof, and the shell script and test changes additionally take the first row
  once.
- **D5** — tier `medium` review set: `arch-review` with `code-review` at `low`,
  `ai-review` completion, `delivery-review` completeness pass inline.

## Assumptions

- **S1** — the new form check is expressed as one helper that strips a single
  leading `cd <dir> &&` and leaves everything else in place, so the existing
  string-equality comparisons keep doing the rejecting. Safe to assume: it adds
  no new accept path, and any spelling the helper does not understand survives
  the strip and fails the same equality check as today. *(Conventional in this
  file, which already parses by narrow, documented patterns.)*
- **S2** — the helper mirrors `effective_dir`'s directory pattern rather than
  inventing a second one, so the directory the gate *judges* and the directory
  it *strips* can never disagree. Safe: a divergence between the two would be
  the only way a prefix could smuggle something past the check.
- **S3** — the denial messages are unchanged, because they already describe the
  rejected shapes correctly and the campaign contract quotes them.

## Acceptance criteria

- [ ] **A1** — `sh .claude/hooks/guardrails_test.sh` and
      `sh .claude/hooks/campaign_context_test.sh` pass, with the new cases and
      every existing case green.
      *Evidence:* both suites' final counts, before and after.
      → the PR body. *(R1, R2, R4, R6)*
- [ ] **A2** — from a campaign child worktree,
      `cd <worktree> && gh pr merge N --merge --match-head-commit <HEAD>` passes
      the gate when every other check holds, and the same command followed by
      `&& echo x`, or preceded by a second `cd`, is denied.
      *Evidence:* named cases in `campaign_context_test.sh`.
      → the PR body. *(R1, R2)*
- [ ] **A3** — bare `gh pr merge N …` from a main-based worktree is still denied
      as a merge to main, and a cd-prefixed one is too.
      *Evidence:* named cases in `guardrails_test.sh`.
      → the PR body. *(R2, R3)*
- [ ] **A4** — `docs/campaign/integration.md` and `.claude/hooks/README.md` show
      the accepted cd-prefixed form, give the reason, and carry the PowerShell
      prohibition.
      *Evidence:* the quoted sections in the diff.
      → the PR body. *(R5)*
- [ ] **A5** — the set of checks the gate performs is unchanged: no marker, AI
      completion, thread, head-pin, base-advance or draft-state condition is
      added, removed or relaxed.
      *Evidence:* the diff touches only the two form comparisons plus the new
      helper; every pre-existing gate case stays green.
      → the PR body. *(R3)*

## Injected gates

- **G1** — every artifact in English; conventional English commits.
  *Evidence:* `arch-review` dimension 8, `cargo test -p quantick-guards`.
- **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets`, `cargo build --workspace`, `cargo test --workspace`, each run
  separately on the final head, plus both hook suites.
  *Evidence:* command output in the PR body.
- **G3** — `arch-review` over the exact diff against `origin/campaign/lean-a-plus`
  with its step 0 bug pass at `low`, every Blocker and Should-fix resolved or
  deferred in the PR body; `ai-review` completion recorded; `delivery-review`
  completeness pass.
  *Evidence:* review verdicts and the markers at the shared campaign key.
- **G4** — the context ratchet stays satisfied: `cargo run -p quantick-guards --
  --report` unchanged except for context bytes, and any growth in
  `README.md` / `integration.md` paid for by trimming prose in the same files
  rather than raising a ceiling, or the raise named explicitly.
  *Evidence:* the guards report and, if raised, the signed baseline entry.
- **G5** — the branch writes only inside its own worktree, and only the files
  this child owns: `.claude/hooks/guardrails.sh`, the two hook test scripts,
  `.claude/hooks/README.md`, `docs/campaign/integration.md`, plus the goal
  archive the mission's step 8 mandates. Three sibling missions run in
  parallel. *(Added by the completeness pass.)*
  *Evidence:* `git diff origin/campaign/lean-a-plus...HEAD --stat`.

### Not applicable, and why

- **Hot path** — the change is a shell hook, not a per-trade, per-depth or
  per-frame code path. No cargo runtime behaviour changes.
- **User-visible surface** — no UI; `ui-harness`, `visual-qa` and
  `trader-ux-review` do not apply.
- **Adds a capability** — nothing docks; `new-extension` does not apply.
- **Something a trader does** — the gate is an agent-facing guardrail, not a
  trader action.
- **Engine / determinism** — no engine code is touched.
- **Issue #386's A4** — "merged into `campaign/lean-a-plus` through a PR whose
  base is exactly that branch, with the merge read back". The PR's base is that
  branch, which is this mission's half; the merge itself is not. The campaign
  integration contract gives the merge to the coordinator, and this mission is
  instructed never to merge, so A4 stays **pending with the coordinator** and
  is not gradeable here. *(Named by the completeness pass, which found it in
  the retained request with no line behind it.)*

## Closing steps

- **C1** — `delivery-review` returns PASS (completeness pass at `medium`).
- **C2** — the draft PR is open against `campaign/lean-a-plus`, with the
  evidence in its body.
- **C3** — `gh pr ready <n>` is attempted once from the worktree through the
  Bash tool, and its outcome — accepted or denied — is reported rather than
  worked around. The hook that runs is the main checkout's, so a denial is the
  expected result until this branch merges. The gated commands are never run
  through the PowerShell tool. *(Added by the completeness pass: an operational
  obligation of this mission that the first map carried no `C` line for.)*
- **C4** — the coordinator receives the handoff block: issue, branch, worktree,
  PR URL, head SHA, base tip, review verdicts, markers, CI, suite results,
  findings closed/open, the `gh pr ready` outcome and the next action.
  *(Added by the completeness pass, same reason.)*

## The request as received

> You are executing campaign child mission **Q7** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick:
> make the `pr-gate` hook accept exactly one leading `cd <worktree> &&` before
> the otherwise bare, head-pinned campaign `gh pr merge` / `gh pr ready`
> statement. You are a subagent: you cannot ask the trader; decisions D1..Dn
> below answer the mission's step-3 questions. A doubt that would need a new
> decision is reported back, never guessed.
>
> ## Decisions
>
> - D1: accepted forms on a campaign base become exactly
>   `cd <dir> && gh pr merge N --merge|--squash --match-head-commit <HEAD>` and
>   `cd <dir> && gh pr ready N`, where `<dir>` is the string `effective_dir`
>   extracts (quoted or not) and the remainder equals today's exact bare form;
>   the bare forms stay accepted. Anything else (a second `&&`, `;`, `||`, a
>   trailing statement, `--auto`, `--admin`, `-R`, merge queue) is denied with
>   the existing messages. What the gate checks (markers at the shared key, AI
>   completion, zero open threads, head pin, base advance, draft state) does not
>   change at all.
> - D2: the reason, for the docs: Claude Code's Bash tool resets `cwd` between
>   calls (observed 2026-09-11 while integrating PR #382:
>   `cd <wt> && gh pr merge 382 --merge --match-head-commit 01a001c7...` denied
>   as compound; a bare command would resolve to the main checkout and be denied
>   as a merge to main). Say in README and integration.md that the PowerShell
>   tool is outside the `Bash` matcher and must never be used to route around
>   the gate; a denial is reported to the coordinator.
> - D3: tests: add to `guardrails_test.sh` (or `campaign_context_test.sh`,
>   whichever owns the merge/ready form cases today) at least: cd-prefixed merge
>   accepted when all other checks hold; cd-prefixed merge followed by
>   `&& echo x` denied; `cd a && cd b && gh pr merge ...` denied; cd-prefixed
>   ready accepted; bare merge from a main-based dir still denied as merge to
>   main. Keep every existing case green.
> - D4: this is a workflow/gate change: the delivery contract's second row plus
>   behavioural proof applies (`sh .claude/hooks/guardrails_test.sh`,
>   `sh .claude/hooks/campaign_context_test.sh`, `cargo test -p quantick-guards`,
>   `cargo run -p quantick-guards -- --report` unchanged except context bytes;
>   the context ratchet in `crates/guards/context-baseline.txt` may need its
>   number if README/integration.md grow, pay for it by trimming prose in the
>   same files rather than raising unless impossible, and say which).
> - D5: tier `medium`: `arch-review` with `code-review` at `low` (step 0
>   applies; shape dimensions 1-7 and 9 waived for prose but NOT for the shell
>   script and tests, which take the full pass; dimension 8 always); `ai-review`
>   completion; `delivery-review` completeness pass inline.
