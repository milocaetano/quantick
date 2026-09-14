# Reproducible per-PR read cost

Record a reproducible, SHA-bound per-PR read cost in CI by counting production
lines in changed Rust files and their directly referenced in-crate modules.
This gives every pull request durable evidence for outside-score rubric C1
without changing the frozen scorer or product behavior.

**Tier:** medium. This adds an executable repository tool and CI workflow with
non-trivial Rust module-reference resolution, deletion/rename handling, and a
durable artifact contract. It does not touch a runtime hot path, product code,
or a trader-facing surface.

## Request ledger

- **R1** — Add a committed calculator that accepts explicit base and head
  revisions and computes read cost for any pull request.
- **R2** — Count production lines in changed files plus the in-crate modules
  those files directly reference, resolving references honestly and
  deduplicating counted modules.
- **R3** — Handle deleted and renamed files without silently dropping or
  double-counting their contribution.
- **R4** — Test independent Rust module layouts, relevant module/crate
  boundaries, reference deduplication, deletion, rename, and reproducibility.
- **R5** — Record a deterministic, explicit base/head-bound artifact for every
  pull-request CI run.
- **R6** — Leave `tools/outside_score/measure.py`, both rubrics, and both
  canonical/Codex score skills unchanged; do not alter product code to improve
  the metric.
- **R7** — Preserve every repository invariant and do not weaken tests,
  reviews, contracts, financial rules, order-ticket behavior, or replay.
- **R8** — Identify lexical aliases, re-exports, grouped imports, path
  attributes, unsupported syntax, and other resolution limitations honestly;
  do not claim semantic completeness for syntax the calculator does not
  support.
- **R9** — Preserve the independent B0 outside-score baseline report exactly
  at `docs/quality/outside-score/eb7bb039.md`, with adjacent provenance linking
  its separately published quantick-score companion; never represent it as a
  C1 assessment or change its grades or ledger.

## Decisions

- **D1** — The authenticated campaign source authorizes all defaults except a
  merge to `main` and GitHub settings changes; no trader question is required.
- **D2** — This campaign child uses branch `feat/pr-read-cost`, worktree
  `C:/src/quantick-worktrees/feat-pr-read-cost`, and exact base
  `origin/campaign/outside-eight` at
  `eb7bb039434667bb150be9cdf5237e172c4198fe`. The parent charter is the D1-D2
  authority source for a reviewed merge into that campaign branch.
- **D3** — The campaign assigned medium tier and `gpt-5.6-sol` because this is
  bounded tooling over an existing repository boundary.
- **D4** — Root coordination approved a direct-reference set and requires it
  to be labelled precisely rather than represented as transitive or fully
  semantic.
- **D5** — Root coordination authorized the bounded B0 evidence-storage
  addition after the independent assessor completed it at the campaign base.
  The report source is
  `C:/src/quantick-worktrees/outside-eight-assessment/outside-eb7bb039-independent.md`;
  the companion is durable at issue #473 comment `5668745129`.
- **D6** — Chronology correction to the historical coordinator quotation
  below: the supported fact is that a fresh-context assessor who authored none
  of the campaign code independently assessed the isolated
  `eb7bb039434667bb150be9cdf5237e172c4198fe` baseline while implementation
  missions ran in parallel. The quotation is retained as history; the report
  grades and ledger remain unchanged.

## Assumptions

- **S1** — “Modules those files reference” means direct one-hop source-module
  references, not the transitive dependency closure. This follows the rubric's
  singular change-reader framing and the coordinator confirmed it; recording
  that scope in the artifact makes the choice reversible and auditable.
- **S2** — A file present at head is measured from its post-image; a deleted
  file is measured from the merge-base pre-image that the PR diff actually
  removed, even when the explicit base tip diverged. A rename is one changed
  source at its head path, with the old path retained as change metadata. This
  is safe because it follows the actual diff while retaining the only source
  image a deletion removed.
- **S3** — Reusing the frozen outside-score lexer's production-code masking by
  importing it read-only is safer than creating a second production-line
  definition. Its blob remains frozen and tests pin the integration.
- **S4** — Canonical sorted JSON is the artifact format. It can retain
  file-level roles, revisions, unresolved references, and limitations while
  remaining byte-reproducible.
- **S5** — A dedicated pull-request job is the narrowest reliable CI wiring:
  trusted event SHA fields feed quoted command arguments, full Git history
  makes both objects available, and the artifact does not depend on the much
  slower workspace build job succeeding first.
- **S6** — Performance impact is rare CI-only work: two Git trees are read and
  small Python fixtures run once per pull request. Product per-trade,
  per-depth, and per-frame paths are untouched.

## Acceptance criteria

- [x] **A1** — A committed script accepts an explicit repository, base, and
  head; resolves them to commit SHAs; and emits deterministic structured read
  cost for the pull-request merge-base diff.
  *Evidence:* calculator CLI tests and two byte-identical executions at the
  same revisions, recorded in the PR body.
  → pull request evidence section. *(R1, R5)*
- [x] **A2** — The total is the deduplicated production-line sum of changed
  Rust production files and source files containing their directly referenced
  in-crate modules, with each file's touched/referenced role visible.
  *Evidence:* hand-counted nested, sibling, parent, grouped-import, and shared
  reference fixtures.
  → `tools/read_cost/test_measure.py` and pull request evidence. *(R2, R4)*
- [x] **A3** — Deleted files use actual merge-base pre-images even when the
  explicit base tip diverged, renamed files retain old/new identity without
  double counting, and non-production-only changes report an honest zero.
  *Evidence:* independent temporary-Git-repository fixtures for divergent-base
  deletion, rename, deduplication, and docs-only changes.
  → `tools/read_cost/test_measure.py`. *(R3, R4)*
- [x] **A4** — Unresolved or ambiguous local references and fixed parser
  limitations appear explicitly in the report; external-crate and test-only
  references do not inflate the total.
  *Evidence:* path-attribute, cross-crate, alias/re-export/grouped-use, cfg-test,
  and unsupported-layout fixtures plus report assertions.
  → `tools/read_cost/test_measure.py` and the tool module documentation.
  *(R2, R4, R8)*
- [ ] **A5** — Every pull-request workflow run invokes the calculator with
  quoted trusted event base/head SHAs and uploads a deterministically named,
  SHA-bound JSON artifact.
  *Evidence:* workflow inspection and the exact-head GitHub Actions artifact.
  → `.github/workflows/ci.yml`, PR check, and PR evidence section. *(R5)*
- [x] **A6** — Frozen measurement files retain their campaign-manifest blob
  IDs and the diff contains no product-code or score-gaming changes.
  *Evidence:* manifest blob verification and explicit name/status diff.
  → pull request evidence section. *(R6, R7)*
- [ ] **A7** — Repository behavior remains unchanged, with required Python and
  full Rust verification green in order.
  *Evidence:* targeted Python tests, ruff, and ordered fmt/clippy/build/test
  command receipts at the implementation head, followed by exact-head CI.
  → pull request evidence section and checks. *(R7)*
- [x] **A8** — The independent campaign-base outside-score report is committed
  byte-for-byte at its requested reusable path, and an adjacent provenance
  note labels its independent authorship, base SHA, companion publication, and
  separation from C1 success evidence.
  *Evidence:* source/target blob comparison and provenance-link inspection.
  → `docs/quality/outside-score/eb7bb039.md`, adjacent provenance note, and PR
  evidence. *(R9)*
- [ ] **G-ENGLISH** — Every authored artifact is English.
  *Evidence:* language guard and architecture review dimension 8.
  → review report and PR checks.
- [x] **G-CODE** — `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and
  `cargo test --workspace` pass in that order after implementation.
  *Evidence:* command receipts with exit codes.
  → pull request evidence section.
- [x] **G-PYTHON** — `ruff check --select F tools/ bridge/mt5/`, the existing
  outside-score tests, and the new read-cost fixture suite pass.
  *Evidence:* command receipts and exact-head CI.
  → pull request evidence section and checks.
- [x] **G-PERF** — Performance impact is declared as rare CI-only work with no
  runtime hot-path change.
  *Evidence:* path/rate table in the PR body and architecture review.
  → pull request evidence section and review report.
- [ ] **G-ARCH** — Architecture review runs on the exact campaign-base diff
  and every Blocker/Should-fix is resolved or explicitly deferred in the PR.
  *Evidence:* durable current-key architecture PASS report.
  → pull request review report and body.
<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Non-applicable gates

- Hot-path evidence is not applicable because only an offline PR calculator
  and CI configuration change; no product execution path imports this tool.
- UI harness, visual QA, and trader UX review are not applicable because no
  user-visible surface changes.
- `new-extension` and second-operator gates are not applicable because this is
  repository measurement tooling, not a feed, bar, indicator, layer, panel,
  crate, trader action, or control-plane capability.
- Engine test-first/golden determinism is not applicable because engine code
  and bar behavior are untouched; deterministic calculator fixtures still
  precede implementation where practical.
- Supply-chain validation is not applicable unless `Cargo.lock` moves; this
  mission adds no dependency.

## Closing steps

- [ ] **C1** — Medium-tier delivery review publishes a current-key PASS after
  the archived mission and architecture/AI reviews.
- [ ] **C2** — The child PR is open, non-draft, based on
  `campaign/outside-eight`, and green at its exact head; no merge to `main` is
  performed.
- [ ] **C3** — `mission_ship_gate.sh mission <pr>` publishes PASS and literal
  reconciliation before handoff to the campaign coordinator.

## Local verification evidence

The executable/configuration tree verified before the implementation commit is
Git tree `cc243547bbbce136520bc13fc5f8d979972b658c`, committed as
`c13b0a6e3e0c3190b0c8d5abac378018affa699c`. Its explicit campaign base was
`eb7bb039434667bb150be9cdf5237e172c4198fe`. The ordered Rust loop ran from
`C:/src/quantick-worktrees/feat-pr-read-cost` with
`CARGO_TARGET_DIR=C:/src/quantick-worktrees/fix-mutation-retry-truth/target`
under an exclusive coordinator lease and
`QUANTICK_BUBBLES=C:/src/quantick-worktrees/feat-pr-read-cost/config/bubbles.toml`.

- `cargo fmt --all`; `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets` — PASS.
- `cargo build --workspace` — PASS.
- `cargo test --workspace` — PASS.
- `.claude/hooks/guardrails_test.sh` under explicit Git Bash — PASS, 277
  passed and 0 failed. An earlier interrupted run is not evidence.
- `python tools/mt5/test_export_session.py` — PASS, 14 cases.
- Every `bridge/mt5/tests/test_*.py` suite — PASS.
- `python tools/outside_score/test_measure.py` — PASS, 17 tests.
- `python tools/read_cost/test_measure.py` — PASS, 9 tests.
- `ruff check --select F tools/ bridge/mt5/` — PASS.
- The frozen campaign manifest's seven paths retain their declared Git blob
  IDs. The copied independent report matches its assessor source byte-for-byte
  at SHA-256
  `C89F1AD2404B65607806E3367D93FB83989A024DCECD8447A3FA9A25E9A2CF82`.
- After the provenance-only wording correction, the current-root guard binary,
  `git diff --check`, relative-link existence, and the source/report hash
  comparison passed. Runtime evidence is reused from the executable tree above
  because only the mission record and provenance prose changed afterward.

After R1 integrated, this branch rebased cleanly onto campaign commit
`05bc95ecfa36339be75411b251edf67cffa79992`. The complete current-base loop ran
at commit `52b1e4362b47c67e8372842a72f1f818ac361967`, tree
`d5ebe289e68bdca7819d5c2690e374cb925998cb`, with the same explicit target and
tracked bubbles inputs. Ordered fmt, clippy, build, and workspace test all
exited 0; the workspace receipt contains 106 successful result summaries,
totalling 3,897 passed and 19 ignored tests with zero failures. The guardrail
hook reported 277 passed and 0 failed. Exporter, every bridge Python suite,
outside-score's 17 tests, read-cost's 9 tests, and Ruff F checks all passed.
Two calculator executions for exact base/head produced identical SHA-256
`C865C75D0A0E52ADF1AC8A2B83BE747308EB09774FE8B14C96ACDB240C7B60EC`
and honestly reported `production_change: not_applicable` with zero production
lines for this tooling/documentation-only branch. Durable selected output,
complete local-receipt digests, commands, inputs, and exits are recorded in
`.claude/evidence/pr-read-cost/workspace-final.log`; the calculator payload is
retained with repository LF line endings as `read-cost-52b1e436.json`.

## Verbatim delegated request

> Campaign #472 child C1 #476: implement reproducible per-PR read cost recorded in CI. Repository C:/src/quantick; initial base origin/campaign/outside-eight at eb7bb039. First read issue https://github.com/milocaetano/quantick/issues/476, parent #472 contains full authenticated source, and current canonical new-task, mission, ship plus integration/delivery rules. Assigned executor gpt-5.6-sol for bounded tooling implementation per issue/campaign routing. Own tools for new read-cost calculator (NOT tools/outside_score/measure.py) and CI wiring. F1 owns feed/app diagnostics; R1 control/mcp/gateway; coordinate .github workflow edits with root. User explicitly allows parallel PRs, all defaults except main/settings; never ask trader. Root owns parent journals/Project/merges. Do read-only design now and report proposed scope/map. Root will publish claim before authorizing worktree/edits. Plan medium mission unless risk raises tier; one branch feat/pr-read-cost at C:/src/quantick-worktrees/feat-pr-read-cost from campaign base, guards/cargo check before edits, private mission-base uses parent URL as D1-D2 grant. Frozen files: both rubrics, measure.py, both score skills/canonical+Codex. User requires no weakened tests/reviews/contracts/financial rules and no ticket/replay regression. Calculator for explicit base/head must count production lines of changed files plus referenced in-crate modules, resolve references honestly, deduplicate, cover deleted/renamed files, emit reproducible CI artifact per PR. No product code changes for score gaming. Inspect Rust module/use graph semantics and match the frozen rubric; tests should exercise independent reference layouts. Required full ordered fmt/clippy/build/test after implementation plus Python test/lint and exact-head CI/current reviews/ready/final verifier. No merge, no campaign score of own code. Coordinate expensive workspace build timing with root; use repository's existing target cache conventions safely. Complete to green ready PR once root confirms claim.

## Verbatim authenticated campaign request retained on parent #472

> $campaign  create Take Quantick to at least 8.0 on the outside-score rubric v1.0
> (docs/quality/outside-score-rubric.md), measured on main, with every dimension at
> least 8.0, from the committed baseline docs/quality/outside-score/d3d4b23d.md (6.5).
> Main has not moved materially since: at eb7bb039, ui_free_share_percent is 39.3,
> impl_spread.QuantickApp is 20, fns.over_200.per_100k is 29.9 and harness_hooks is
> 134. Only a fresh-context assessor may score, one that wrote none of the campaign's
> code and follows the rubric's Independence section. The campaign's own scorecards
> and PR claims are never success evidence, and a 9.0 stands only after the rubric's
> other-model-family reproduction at the same SHA. Freeze the measurement: do not
> change the rubric, measure.py or either score skill while the campaign runs. The
> work must also fix, not merely keep, the confirmed defects that quantick-score's
> A+ gates 4 and 5 block on:
> - automatic Binance reconnects and MT5 in-connection sequence gaps drop trades
>   without a FeedGap or health counter (feed-binance/src/stream.rs:77);
> - unkeyed timeouts and lost transports on mutating capabilities answer
>   retryable:true after the action may have run;
> - quantick_invoke cannot carry an idempotency key.
> Those gates must pass by blind assessment at the campaign head, and quantick-score
> must not fall. Keep every CLAUDE.md invariant: determinism, data honesty, one engine
> for chart, backtest and bot, one-way dependencies, headless crates below app, the
> ratchets, English in the repo, and no regression in the order ticket or replay. Do
> not weaken tests, guards, reviews, public contracts or financial rules. Main merges
> and GitHub settings belong to the trader: prepare settings changes as one-line human
> tasks on the parent issue and never block other work on them. Decide every other
> default yourself, and never wait on the trader for anything but the merge to main.

## Verbatim coordinator refinement

> Preserve lexical aliases/reexports/grouped imports and path attributes in explicit unresolved reporting; do not claim semantic completeness for syntax not supported. Direct referenced-module set fits stated rubric; label direct vs transitive precisely. For changed nonproduction/docs-only files return honest zero/not-applicable rather than parse prose. CI source refs must come from trusted event SHA fields, shell quoted; no arbitrary user text interpolation. Whole repo checks preserve frozen helper byte identity.

## Verbatim coordinator evidence-storage addition

> Small campaign-evidence addition under root authority: include the completed independent outside baseline report unchanged at docs/quality/outside-score/eb7bb039.md, copied via apply_patch from C:/src/quantick-worktrees/outside-eight-assessment/outside-eb7bb039-independent.md. Frozen rubric requires reports intended for reuse committed at that path. It was authored by independent baseline_assessor before any campaign code, SHA eb7bb039; it is NOT your/C1 score or evidence C1 succeeded. Full quantick companion remains durable at https://github.com/milocaetano/quantick/issues/473#issuecomment-5668745129; replace only broken relative companion reference with this explicit publication URL if report's link is otherwise unusable and label editorial linking, no assessment edits. Add scope/provenance to mission R/A bookkeeping before copying. If you prefer keep report fully unchanged, add separate adjacent provenance doc/link. Do not change its grades or ledger. This satisfies B0 storage convention without a separate implementation/worktree. All other frozen paths unchanged.

## Verbatim pre-freeze review refinement

> Pre-freeze inspection found concrete divergent-base concern: measure diffs merge_base..head but Snapshot(base) supplies deletion preimages; if base advanced deleting/modifying same deleted path, touched deletion is omitted or counted from wrong tree. Please add divergent-base regression fixture and use actual diff preimage with explicit report labeling. Also reference_paths omits ordinary bare qualified module calls (child::fn outside use), and bare local paths resolve only crate root, not current module. Please cover a changed nested module using child::fn where child mod is declared in unchanged parent: resolve unambiguous local module or expose uncertainty, do not silently undercount. No frozen files edits.
