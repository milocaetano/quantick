# Split the paper-trading files under 1,500 production lines

`crates/app/src/paper_trading.rs` (4,589), `crates/app/src/paper_report.rs`
(3,300) and `crates/app/src/paper_account.rs` (2,128) are the second, third
and fourth largest production files in the repository. Give their contents
owners — sibling modules under each file's own directory, every file at most
1,500 production lines — with no behaviour change on the money path, so that
an agent opening one of them to change one behaviour reads one owner instead
of a whole subsystem.

**Tier:** `high`. Campaign child S2 of #367 (issue #369), on the money path:
order placement, bracket prices, risk arithmetic, journal bytes and report
text must not change by one byte. Two byte-pinned tests are the safety net,
and the full shape pass and `delivery-review` both run.

## Request ledger

- **R1** — split `paper_trading.rs`, `paper_report.rs` and `paper_account.rs`
  into owned sibling modules, each at most 1,500 production lines as
  `crates/guards/src/size.rs` measures them.
- **R2** — pure moves: every production body moves unchanged; the only
  widenings are the `pub(super)`/`pub(crate)` marks a child module needs. No
  behaviour, contract, financial-rule or wire change. Verbatim:
  *"Order placement, bracket prices and risk arithmetic must not change by one
  byte."*
- **R3** — the two byte-pinned tests report the same SHA-256 as before the
  move, and no test expectation is edited.
- **R4** — `crates/guards/size-baseline.txt` carries no entry for any of the
  three named files afterwards, and the `!budget` line does not rise.
- **R5** — the four checks green on the final head, run one at a time, and CI
  green at the exact PR head; `cargo test -p quantick-guards` passes.
- **R6** — the change lands through a PR whose base is exactly
  `campaign/lean-a-plus`.
- **R7** (the purpose that judges the rest) — an agent that opens one of these
  files to change one behaviour reads one owner, not the whole subsystem.
- **R8** — the diff stays inside the files this mission owns, because two
  sibling missions are editing the repository in parallel. Verbatim: *"you own
  only the three paper files, new files under `crates/app/src/paper_trading/`,
  `crates/app/src/paper_report/`, `crates/app/src/paper_account/`,
  `crates/guards/size-baseline.txt` (your entries only) and, if strictly
  needed, `mod` lines. Do not edit any other file."* Added by this mission's own
  completeness pass, which found the constraint in the request with no
  criterion discharging it.

## Decisions (from the campaign coordinator)

- **D1** — pure moves only. A widening beyond `pub(super)`/`pub(crate)`, or a
  new field on `PaperTrading`/`PaperAccount`/`ReportState`, stops the work and
  is reported as a `human_decision` instead.
- **D2** — the ceiling counts production lines as `size.rs` measures them
  (inline tests excluded). Moving inline tests to sidecars is allowed, never
  required.
- **D3** — candidate seams to confirm by reading: the ticket surface versus
  the outbox/toast and the bracket projection versus the launch hooks in
  `paper_trading.rs`; calendar tinting and equity-curve plotting versus the
  ledger and the report text in `paper_report.rs`; fills and settlement versus
  risk sizing in `paper_account.rs`. The hook declarations stay beside the
  reads.
- **D4** — if any file or new sibling would still exceed 1,500 production
  lines, split further until every file is under.
- **D5** — proof of R3 is the two pinned tests' SHA-256 before and after;
  proof of R2 is a sorted line multiset of the moved bodies.
- **D6** — after the moves, `--tighten`; the three entries must leave
  `size-baseline.txt` and `!budget` must not rise. The `--report` "largest"
  list is a fixed top-N, so an untouched file may appear in its diff.
- **D7** — performance: state each touched path's rate in the PR body; no
  measurement is required unless a call shape changes.
- **D8** — tier `high`: `arch-review` with `code-review` at `medium`, full
  shape pass; `ai-review` completion; `delivery-review` in full. At most two
  step-0 rounds.

## Assumptions

- **S1** — the section banner comments the item cut leaves on the wrong side
  of a seam travel to the module they title rather than being deleted, so the
  line multiset stays whole. *Safe because a comment is not behaviour and the
  alternative — deleting them — would break the multiset proof D5 asks for.*
- **S2** — three dead section banners in `paper_trading.rs` (`Import`,
  `Export`, `Events, journal, parsing`), left empty by the earlier account
  extraction, stay in the parent rather than travelling with the last method
  above them. *Safe because they title nothing in either file; leaving them
  where they are is the smaller change.*
- **S3** — a test sidecar that reached its parent's imports through
  `use super::*` gets explicit `use` lines for what moved away. *Safe because
  test code is outside the ratchet and no expectation changes.*
- **S4** — three files that already existed inside the owned trees are edited
  rather than only added to: `paper_trading/leg_tag.rs` and the two
  `tests/mod.rs` sidecars, every change an import line pointing at a moved
  item's new home. R8 names new files and `mod` lines; these are neither.
  *Safe because the alternative is a branch that does not compile — an import
  naming a type that moved is not an optional edit — and because no sibling
  mission owns a file under the three paper trees. Recorded rather than asked
  because the tier's question budget was spent before the compiler raised it.*

## Acceptance criteria

- [ ] **A1** — `paper_trading.rs`, `paper_report.rs`, `paper_account.rs` and
      every new sibling are at most 1,500 production lines.
      *Evidence:* `cargo run -p quantick-guards -- --report` and a per-file
      count. → the PR body's size table. *(R1, R7)*
- [ ] **A2** — every production move is a pure move.
      *Evidence:* a sorted line multiset over the moved bodies, before and
      after, with the command. → the PR body. *(R2)*
- [ ] **A3** — `the_journal_bytes_are_fixed` and `the_report_numbers_are_fixed`
      pass with the same expected bytes, and no test expectation was edited.
      *Evidence:* the test run plus the SHA-256 of each expectation.
      → the PR body. *(R3)*
- [ ] **A4** — `size-baseline.txt` carries no entry for the three files and
      `!budget` did not rise.
      *Evidence:* the baseline diff and `cargo test -p quantick-guards`.
      → the PR body. *(R4)*
- [ ] **G1** — every artifact in English; conventional commits.
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace`
      green on the final head, run one at a time; CI green at the exact PR
      head. *(R5)*
- [ ] **G3** — `arch-review` (step 0 at `medium`) with every Blocker and
      Should-fix resolved or deferred in the PR body; `ai-review` completion
      recorded; `delivery-review` PASS. *(R5)*
- [ ] **G4** — performance: no touched path changes its complexity, stated per
      path in the PR body. *(R2)*
- [ ] **A5** — the PR's base is exactly `campaign/lean-a-plus`. *(R6)*
- [ ] **A6** — every file the diff touches is one this mission owns: the three
      paper files, files under their three directories,
      `crates/guards/size-baseline.txt`, and this goal archive. Nothing a
      sibling mission is editing — `pane.rs`, `worker_progress`,
      `app/tests/control_plane_tests.rs` — is in the diff.
      *Evidence:* `git diff --name-only origin/campaign/lean-a-plus...HEAD`.
      → the PR body. *(R8)*

## Not applicable

- *Touches a hot path* — the ticket and the chart layer draw per frame, but a
  method moved between `impl` blocks compiles to the same call; no call shape
  changes, so D7 asks for a declaration rather than a measurement.
- *Touches anything user-visible* — no surface changes; `visual-qa` and
  `trader-ux-review` have nothing new to see.
- *Adds a capability* — nothing is added.
- *Engine / determinism territory* — the engine is untouched; the byte-pinned
  tests already guard the determinism this change could break.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — the draft PR is open against `campaign/lean-a-plus` and made ready.

## The request as received

> You are executing campaign child mission **S2** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for the repository
> milocaetano/quantick: split `crates/app/src/paper_trading.rs` (4,589
> production lines), `crates/app/src/paper_report.rs` (3,300) and
> `crates/app/src/paper_account.rs` (2,128) into owned sibling modules, each at
> most 1,500 production lines, with no behaviour change on the money path. You
> are a subagent: you cannot ask the trader anything; the coordinator has
> answered the mission's step-3 questions below as decisions D1..Dn. A doubt
> that would need a new decision is reported back, never guessed.
>
> ## Ground rules (read these files first, in this order)
>
> 1. `C:\src\quantick\CLAUDE.md` (all rules apply; English everywhere; the size
>    ratchet and `--tighten` under *Keeping the trunk small*).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign
>    child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9 exactly. Step 3 is
>    answered below. Step 6 is already done (worktree, `mission-base`,
>    `mission-tier`, guards armed) but you still run `cargo check -p quantick-app
>    --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` — base is
>    `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`,
>    review keys from `sh .claude/hooks/campaign_context.sh key "$WT"`.
> 4. `C:\src\quantick\docs\workflow\delivery.md`.
> 5. The task: issue #369 (`gh issue view 369`): scope, acceptance criteria
>    A1..A6, gates.
> 6. The history of the two earlier cuts of these files in the header comments
>    of `crates/guards/size-baseline.txt` (the report extraction, the account
>    extraction, and the two byte-pinned tests `the_journal_bytes_are_fixed` and
>    `the_report_numbers_are_fixed` with their SHA-256).
>
> ## Your worktree (the only place you write)
>
> - Worktree: `C:\src\quantick-worktrees\refactor-paper-trading-under-1500`
>   (Git Bash `/c/src/quantick-worktrees/refactor-paper-trading-under-1500`),
>   branch `refactor/paper-trading-under-1500`, cut from
>   `origin/campaign/lean-a-plus` at
>   `e8eb23e543dcc827c16c7c552478e7ebf2150a4f`.
> - Never write to `C:\src\quantick` or any other worktree. Every command starts
>   with `cd /c/src/quantick-worktrees/refactor-paper-trading-under-1500 &&`.
> - Two sibling missions run in parallel on this machine (`pane.rs`, and two
>   test files under `worker_progress` and `app/tests/control_plane_tests.rs`);
>   you own only the three paper files, new files under
>   `crates/app/src/paper_trading/`, `crates/app/src/paper_report/`,
>   `crates/app/src/paper_account/`, `crates/guards/size-baseline.txt` (your
>   entries only) and, if strictly needed, `mod` lines. Do not edit any other
>   file.
>
> ## Decisions from the coordinator (record as D1..Dn in GOAL.md)
>
> - D1: pure moves only. Every function body moves unchanged; the only
>   widenings are `pub(super)` marks a child module needs. Order placement,
>   bracket prices, risk arithmetic, journal bytes and report text must not
>   change by one byte. If a move would require a `pub` widening beyond
>   `pub(super)`/`pub(crate)`, or a new field on
>   `PaperTrading`/`PaperAccount`/`ReportState`, stop and report it in the
>   handoff as a human_decision instead of doing it.
> - D2: the ceiling is production lines as `crates/guards/src/size.rs` measures
>   them (inline tests excluded). Moving inline tests to sidecars is allowed
>   when it helps, never required.
> - D3: candidate seams to confirm by reading: in `paper_trading.rs` the ticket
>   surface versus the outbox/toast and the bracket projection versus the launch
>   hooks and `declare_hooks!` (the hook declarations must stay beside the
>   reads, `hooks::OWNERS` names the file a reader opens and the guard checks
>   they agree); in `paper_report.rs` calendar tinting and equity-curve plotting
>   versus the ledger and the report text; in `paper_account.rs` fills and
>   settlement versus risk sizing. Prefer siblings under each file's own
>   directory (`crates/app/src/paper_trading/*.rs` already exists with
>   `leg_tag.rs` and `tests/`).
> - D4: if any file or new sibling would still exceed 1,500 production lines,
>   split further by phase until every file is under.
> - D5: proof of A1: the two byte-pinned tests report the same SHA-256 before
>   and after (`ab74859479f2f1e471dfb5a1556a15d2891d440c7c119db49c0e2ad64be094d6`
>   for the journal per the baseline note; verify the report's from the test
>   itself). Proof of A2: a sorted line multiset of moved bodies before/after,
>   command and result in the PR body.
> - D6: after the moves, `cargo run -p quantick-guards -- --tighten`; the three
>   entries must leave `size-baseline.txt`; `!budget` must not rise; `cargo test
>   -p quantick-guards` passes (including the hook-declaration guard). The
>   `--report` "largest" list is a fixed top-N, so an untouched file may appear
>   in that report's diff; explain, do not "fix".
> - D7: performance: the ticket draws per frame; moving methods across `impl`
>   blocks adds no work. State per touched path (per-frame / per-trade / rare)
>   in the PR body; no measurement is required unless you change a call shape,
>   in which case measure `APP_HEALTH_SUMMARY` frame_avg before/after.
> - D8: tier `high`: `arch-review` with `code-review` at `medium`, full shape
>   pass; `ai-review` completion; `delivery-review` in full. At most two step-0
>   rounds; remaining minor findings ship as PR follow-ups named in the PR body.
>
> ## Environment notes
>
> - Use `python`, not `python3`, for scripted edits of large Rust files. If
>   `cargo fmt` is blocked, run `rustfmt` directly then `cargo fmt --all --
>   --check`.
> - Run the four checks one at a time, never chained with `||` or `| head`; read
>   the whole failure output.
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`.
> - When splitting: never de-indent moved items; child modules need `pub(super)`
>   on moved helpers; let the compiler drive; a new test module must be suffixed
>   `_tests`. `crates/guards/src/cycle.rs` fails the build on a new module
>   cycle; do not create one.
> - If writing the review markers into the git dir is denied by the permission
>   classifier, do not work around it: put the exact `printf` lines in your
>   handoff and mark the step pending.
>
> ## Delivery
>
> 1. Implement with the verification loop between edits (`cargo check -p
>    quantick-app`, `env -u QUANTICK_BUBBLES cargo test -p quantick-app paper`,
>    `cargo test -p quantick-guards`).
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy
>    --workspace --all-targets`, `cargo build --workspace`, `env -u
>    QUANTICK_BUBBLES cargo test --workspace`, each separately. Conventional
>    English commits ending with `Co-Authored-By: Claude Fable 5.1
>    <noreply@anthropic.com>` and `Claude-Session:
>    https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 3. Archive `GOAL.md` per mission step 8 (slug `paper-trading-under-1500`) as
>    the last commit before reviews.
> 4. Push and open a **draft** PR: `cd
>    /c/src/quantick-worktrees/refactor-paper-trading-under-1500 && gh pr create
>    --draft --base campaign/lean-a-plus --title "refactor(app): split the
>    paper-trading files under 1,500 production lines" --body-file -` with a
>    heredoc body following the PR template: mission tier, "Campaign child of
>    #367 (S2); closes on integration: #369" (no `Closes` keyword), the
>    moved-item table, the multiset proof, the pinned hashes before/after, the
>    size report before/after, local verification, then the generated-with
>    footer and the session link.
> 5. Run `/arch-review` (step 0 bundles `code-review` against the campaign
>    base), then `/ai-review`, then `/delivery-review`. Resolve Blockers and
>    Should-fixes; record markers with the shared key exactly as
>    `docs/campaign/integration.md` shows.
> 6. Watch CI at the exact PR head (`gh pr checks <n> --watch`, bounded). A
>    failure of `delayed_producer_bookkeeping...` or
>    `gateway_client_reads_the_running_application...` is a known load flake
>    being fixed by a sibling mission: report it, do not rerun blindly more than
>    once.
> 7. When reviews, markers, AI completion and CI are green: `cd
>    /c/src/quantick-worktrees/refactor-paper-trading-under-1500 && gh pr ready
>    <n>` as its own command. **Never merge.** Never touch main.
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; final head SHA;
>    campaign base tip at review time; production-line table before/after per
>    file; pinned hashes; review verdicts with URLs; markers recorded yes/no; CI
>    run URL and conclusion; findings closed/open with IDs; repair batches used;
>    anything pending/blocked (including any human_decision) and the exact next
>    action for the coordinator.
