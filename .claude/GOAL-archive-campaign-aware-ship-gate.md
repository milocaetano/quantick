# Mission: make the mission ship gate campaign-aware

Make `.claude/hooks/mission_ship_gate.sh` tell a mission PR, a main
synchronization PR into a campaign branch and a consolidated campaign-to-main
PR apart, so the last two can pass it on their own terms instead of relying on
coordinator decision D26 (campaign #367, child Q15, issue #439).

**Tier:** medium. A hook change with behavioural tests and contract prose; no
crate file, no hot path, no UI. Campaign child: base `origin/campaign/lean-a-plus`.

## Request ledger

- **R1** — Sync PRs: count only the archives the branch itself adds relative to
  both its base and `origin/main`, "or accept a sync with zero or one own
  archive" (#439 scope; coordinator D1b).
- **R2** — Consolidated campaign PRs (base main, head `campaign/*`): no single
  goal; require the parent campaign marker instead and skip the single-goal
  literal checks, keeping every other check (#439 scope; coordinator D1c).
- **R3** — The mission PR is unchanged: exactly one own archive; zero or two
  still fail (#439 AC1; coordinator D1a, D2).
- **R4** — The kind is detected from facts the gate already reads (head ref,
  base ref, `mission-base`), not from a caller flag (coordinator D1).
- **R5** — `guardrails_test.sh` covers the three PR kinds, pass and fail, and
  every existing case stays green (#439 AC1; coordinator D2).
- **R6** — `docs/campaign/integration.md` steps 6 and 7 and the ship skill say
  which checks apply to which PR kind, in as few words as possible, paying the
  context ratchet by trimming (#439 AC2; coordinator D3).

## Decisions

- **D1** — Coordinator D1: three kinds, detection from head, base and
  `mission-base`; consolidated PRs verify the parent campaign marker with "the
  least ceremony that is still verifiable".
- **D2** — Coordinator D4: tier `medium`; `arch-review` step 0 at `low`, shape
  dimensions on the script and tests, dimension 8 always; `ai-review`
  completion; `delivery-review` completeness pass inline; at most three step-0
  rounds.
- **D3** — Coordinator D5: draft PR into `campaign/lean-a-plus`; `gh pr ready`
  once via Bash with the cd prefix; never merge; never touch main.

## Assumptions

- **S1** — The consolidated PR's parent marker is a body line
  `Campaign-parent: <issue URL>`. It is the least ceremony that is verifiable:
  the gate checks the URL is an issue in the PR's repository and that the
  issue body carries `<!-- quantick-campaign:v1 -->` and the head branch in
  backticks, which the state contract (`docs/campaign/state.md`) already
  requires of every charter. The branch's first commit carries no campaign URL
  and a consolidated worktree has no `mission-base` (integration.md), so
  neither could verify it. Safe: one line in a PR body the coordinator writes.
- **S2** — "A merge commit whose second parent is `origin/main`" is detected as
  "HEAD carries main commits its campaign base lacks" (the merge base of
  `origin/main` and HEAD is not an ancestor of the base). It is the same fact,
  and it stays true after main advances past the synced commit. Safe: a mission
  child never contains main commits its base lacks unless it merged main, which
  is a sync.
- **S3** — A sync's single own archive still goes through the literal
  four-line `G-AI` comparison; zero own archives skip it. Safe: stricter than
  D26, which applied no goal check at all.
- **S4** — Only archives the consolidated PR itself names are skipped; the
  readiness gate, identity, CI, reviews, clean tree, threads and What done means
  clauses stay for every kind. Safe: the literal reading of coordinator D1c.

## Acceptance criteria

- [x] **A1** — A sync PR carrying main's archives plus zero or one of its own
      passes; two of its own fails; its own archive still needs the four `G-AI`
      lines.
      *Evidence:* named cases in `guardrails_test.sh`, suite green.
      → PR body test output. *(R1, R4, R5)*
- [x] **A2** — A consolidated `campaign/*` PR into main with several child
      archives passes when its body names a same-repository charter that names
      the branch, and fails without the line, with a foreign repository, with a
      non-charter, or with a charter naming another branch; threads and CI still
      block it.
      *Evidence:* named cases in `guardrails_test.sh`, suite green.
      → PR body test output. *(R2, R4, R5)*
- [x] **A3** — A mission PR (main-based and campaign child) still needs exactly
      one archive: zero and two fail, one passes; every pre-existing case green.
      *Evidence:* named cases plus the unchanged suite, 0 failed.
      → PR body test output. *(R3, R5)*
- [x] **A4** — The new cases bite: against the pre-change gate the sync and
      consolidated cases fail.
      *Evidence:* mutation run output. → PR body. *(R5)*
- [x] **A5** — `integration.md` steps 6 and 7, the ship skill and the hooks
      README name the checks per PR kind; the context ratchet is paid by
      trimming in the same files.
      *Evidence:* the diff; `cargo test -p quantick-guards` green; byte delta.
      → PR body. *(R6)*

### Injected gates

- [ ] **G1** — Every artifact in English (`arch-review` dimension 8, language
      guard). → arch-review report.
- [ ] **G2** — The four checks green after rebasing on the campaign tip;
      performance impact declared: rare (delivery-time script only).
      → PR body and CI.
- [ ] **G3** — `arch-review` run with every Blocker/Should-fix resolved or
      deferred in the PR body. → arch-review PR report.
- [x] **G4** — Hook suites green: `sh .claude/hooks/guardrails_test.sh`,
      `sh .claude/hooks/campaign_context_test.sh`. → PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Local evidence (run at d88c27e9's tree, rebased on 5222c911)

- `sh .claude/hooks/guardrails_test.sh`: 273 passed, 0 failed (253 before + 20 new).
- Mutation: the 20 new cases against the pre-change gate: 15 fail (every sync
  and consolidated case); the 5 that pass are the unchanged mission-child and
  thread cases.
- `sh .claude/hooks/campaign_context_test.sh`: 67 passed.
- `cargo test -p quantick-guards`: green; context measured 295,587 of 295,668
  before the trims; ratcheted docs net -19 bytes (integration.md 9,550 -> 9,444,
  ship/SKILL.md 5,268 -> 5,355).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
  `cargo build --workspace`, `cargo test --workspace` (with `QUANTICK_BUBBLES`
  unset): each exit 0, run separately.

- arch-review repair (literal repository prefix for `Campaign-parent:`, two
  more cases): `sh .claude/hooks/guardrails_test.sh` 275 passed, 0 failed; the
  pre-repair regex accepted `owner/reXpo` for a PR in `owner/re.po`.

- ai-review thread 4000092270 repair (the branch matches as a whole ref token,
  in backticks or plain text; two more cases): `sh .claude/hooks/guardrails_test.sh`
  277 passed, 0 failed; `cargo test -p quantick-guards` green at the repaired
  tree. No Rust input changed after d88c27e9, so the four cargo checks from
  that tree are reused; CI runs them again at the final head.

## Closing steps

- **C1** — `delivery-review` completeness pass, inline (medium).
- **C2** — Draft PR open into `campaign/lean-a-plus`, then `gh pr ready` once.
- **C3** — `mission_ship_gate.sh mission <pr>` PASS, now reachable by this
  mission PR's own kind, or the coordinator is told why not.

## Not applicable

- Hot path, UI, new capability, trader action, engine determinism: the diff is
  a delivery-time shell script, its tests and prose.

## Verbatim request

> You are executing campaign child mission **Q15** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for
> milocaetano/quantick: make `.claude/hooks/mission_ship_gate.sh`
> campaign-aware, so a main-synchronization PR and a consolidated
> campaign-to-main PR can pass it on their own terms (issue #439).
>
> Decisions
> - D1: three PR kinds, each detected from facts the gate already reads (head
>   ref, base ref, `mission-base`), not from a flag the caller sets: (a)
>   mission PR: unchanged (exactly one goal archive the branch adds); (b)
>   main-sync PR into a campaign branch (head `sync/*` or a merge commit whose
>   second parent is `origin/main`): count only archives the branch adds
>   relative to both its base and `origin/main` (zero or one); (c)
>   consolidated campaign PR (head `campaign/*`, base main): no single goal;
>   require the parent campaign marker instead (the campaign issue URL from the
>   branch's first commit, the charter, or a `campaign-parent` line the
>   coordinator provides in the PR body; choose the least ceremony that is
>   still verifiable) and skip the single-goal literal checks, keeping every
>   other check (exact-head CI, reviews, clean tree, threads).
> - D2: tests: add cases to the hook suite for each kind (pass and fail), and
>   keep every existing case green; a mission PR with zero or two own archives
>   still fails.
> - D3: docs: `integration.md` steps 6 and 7 and the ship skill name which
>   checks apply to which PR kind, in as few words as possible; pay the context
>   ratchet by trimming, or report the overage.
> - D4: tier `medium`: `arch-review` (step 0 `code-review` at `low`; shape
>   dimensions apply to the shell script and tests; dimension 8 always),
>   `ai-review` completion, `delivery-review` completeness pass inline. At most
>   three step-0 rounds.
> - D5: open a **draft** PR [...] Watch CI; `gh pr ready <n>` once via Bash
>   with the cd prefix. **Never use the PowerShell tool for `gh pr` commands;
>   never merge; never touch main.**
>
> — coordinator of campaign #367, 2026-09-13; issue #439 by milocaetano.
