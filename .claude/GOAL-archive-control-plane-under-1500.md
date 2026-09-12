# Mission: split the control-plane contract, evidence and gateway files under 1,500 production lines

Split `crates/app/src/control/contract.rs` (1,718 production lines),
`crates/app/src/control/evidence.rs` (1,708) and
`crates/app/src/control/gateway.rs` (1,696) into owned sibling modules, each at
most 1,500 production lines, with no behaviour change and no authority path
moved by one code path — so that the later AP1 and AP4 missions of campaign
#367 edit a file that has one owner, and so that the size ratchet carries no
signed exception for any of the three.

Campaign child of https://github.com/milocaetano/quantick/issues/367 (task key
S5); task issue https://github.com/milocaetano/quantick/issues/372. Base:
`origin/campaign/lean-a-plus` at `e8eb23e5`; PR base exactly
`campaign/lean-a-plus`.

**Tier:** `high` — the files carry the observer authority boundary (permission
ceilings, refusal rules, evidence retention and its grant recheck, the local
action door with its trace). A pure move can still move a check by one code
path if a seam is cut through the wrong place, and the campaign parent classes
the risk as high, so the full interrogation budget, the full shape pass, a
`medium` bug pass and `delivery-review` in full are owed.

## Request ledger

- **R1** — Each of the three named files, and every new sibling, ends at most
  1,500 production lines as `crates/guards/src/size.rs` measures them.
  *"each at most 1,500 production lines"*
- **R2** — Every production move is a pure move: bodies unchanged, the only
  widenings `pub(super)` marks. No change to permission ceilings, refusal
  rules, envelope validation, idempotency handling, evidence capture or
  verification, published schemas or released v1 contract fixtures.
  *"no behaviour change and no authority path moved by one code path"*
- **R3** — Seams as named: `contract.rs` permission ceilings and refusal rules
  versus envelope validation; `evidence.rs` capture versus verification;
  `gateway.rs` `invoke_local_action` and `service_replay_trace` out to siblings
  under `control/gateway/`.
- **R4** — Every pinned or golden test in the touched crates passes unchanged
  with no expectation edited and no snapshot regenerated;
  `published_schema_compatibility` stays green.
- **R5** — `crates/guards/size-baseline.txt` no longer carries an entry for any
  named file, `!budget` does not rise, `cargo test -p quantick-guards` passes,
  no new module cycle.
- **R6** — Performance stated per touched path (per-request / per-frame /
  rare); no call shape on the frame path changes, or `APP_HEALTH_SUMMARY`
  frame_avg is measured before and after.
- **R7** — Delivery through the campaign process: draft PR against
  `campaign/lean-a-plus`, `arch-review` with `code-review` at `medium`,
  `ai-review` completion, `delivery-review` in full, markers with the shared
  key, green CI at the head, one `gh pr ready` attempt, a handoff block.
- **R8** — Write only inside the worktree; only the three named files, new
  files under their three sibling directories, the baseline entries and `mod`
  lines if strictly needed. *"You own only the three named files ..."*
- **R9** — The closing purpose: an agent that opens one of these files to change
  one behaviour reads one owner and not the whole subsystem.

## Decisions (from the coordinator; step 3 is answered by the prompt)

- **D1** — Pure moves only; `pub(super)` is the only widening. Anything that
  would need `pub`, a new trunk-struct field or a reordered check in
  `ObserverContract::prepare` or a dispatcher stops and is reported as a
  human_decision.
- **D2** — The ceiling is production lines per `size.rs`; moving inline tests
  to sidecars is allowed, never required.
- **D3** — Seams as in R3; if a file would still exceed 1,500, split further by
  phase.
- **D4** — Proof of A2 is the statement-line multiset before/after, command and
  result in the PR body; the compiler drives the `pub(super)` widenings. Proof
  of A1 names every authority/refusal test and
  `published_schema_compatibility` with counts.
- **D5** — Performance: per-request / per-frame / rare stated per touched path;
  moving methods across `impl` blocks adds no work.
- **D6** — After the moves `cargo run -p quantick-guards -- --tighten`; the
  three entries leave the baseline; `!budget` must not rise; the `--report`
  "largest" list is a fixed top-N and an untouched file may appear in its
  diff, to be explained not "fixed".
- **D7** — Tier `high`: `arch-review` with `code-review` at `medium`, full
  shape pass; `ai-review` completion; `delivery-review` in full; at most two
  step-0 rounds, the rest as named PR follow-ups.
- **D8** — `gh pr ready` is tried exactly once through the Bash tool; if the
  gate denies it, stop at the draft PR with reviews, markers and green CI.

## Assumptions

- **S1** — *Which items are "envelope validation" in `contract.rs`.* Read as
  the per-capability prepare handlers (`prepare_*`, `decode_payload`,
  `read_capability`, `register_capability`) and the invocation types they
  build, which decode and validate each request's payload; the permission
  ceilings (`ObserverContract::new`) and the refusal rules
  (`ObserverContract::prepare`, `default_grant`, `readable_scopes`) stay in the
  root, so no check in `prepare` moves. Safe to assume: the file has three
  owners in its own section order, the root is the one that holds the
  authority table and the refusal sequence, and the choice is reversible in
  one edit.
- **S2** — *Which items are "verification" in `evidence.rs`.* Read as the
  retention store: `EvidenceStore`, its state, the retained bundle and the read
  that rechecks the grant, the cursor and the retention on every page. The
  screenshot encoding with its control correlation is the second sibling,
  because it is the one part of capture with its own vocabulary and no
  authority in it. Coverage derivation and configuration redaction stay in the
  root beside `into_manifest`, which is their only caller. Safe to assume: the
  file's own section banners draw these lines already.
- **S3** — *`invoke_local_read` travels with `invoke_local_action`.* The two are
  the same door (the in-process invocation of a registered capability), and
  the baseline note names the action as the next cut. `local_actor` stays in
  the root because `hook_agent_actor` also calls it, which keeps the widening
  count down. Safe to assume: file placement with a conventional default.
- **S4** — *Re-exports keep every external path.* `RawScreenshot`,
  `ScreenshotPixels`, `EvidenceStore` and `EventsReadInvocation` are
  re-exported from their roots at their existing `pub(crate)` visibility,
  exactly as `gateway.rs` already re-exports `server::runtime_id_bytes`. Not a
  widening. Safe to assume: the precedent is in the same directory.
- **S5** — *Section banners travel with the section they title*, as PR #384
  did, so the line multiset stays whole.
- **S6** — *Nothing on the frame path changes shape.* `service_replay_trace`
  and `begin_frame` keep their signatures and bodies; a method moved to
  another `impl` block of the same type monomorphises identically. No
  `APP_HEALTH_SUMMARY` measurement is owed; stated as such in the PR body.

## Acceptance criteria

- [x] **A1** — Each named file and every new sibling is at most 1,500
      production lines, and every authority, refusal and schema-compatibility
      test passes unchanged.
      *Evidence:* production-line table before/after (script over
      `size.rs`'s law); named test list with counts from
      `env -u QUANTICK_BUBBLES cargo test -p quantick-app control`,
      `cargo test -p quantick-control`, and the `published_schema_compatibility`
      test, with no snapshot regenerated. → PR body. *(R1, R4)*
- [x] **A2** — Every production move is pure: statement-line multiset over
      the moved items before/after differs only in `pub(super)` marks.
      *Evidence:* the command and its output. → PR body. *(R2, R3)*
- [x] **A3** — Every pinned or golden test in the touched crates passes
      unchanged; `git diff` shows no edit under any test expectation or
      fixture. *Evidence:* `git diff --stat` and the test run. → PR body.
      *(R4)*
- [x] **A4** — `size-baseline.txt` no longer carries an entry for any named
      file; `!budget` did not rise. *Evidence:* the diff of the baseline and
      `ratchet.size.*` lines from `--report` before/after. → PR body. *(R5)*
- [ ] **A5** — The four checks are green on the final head, one at a time; CI
      is green at the exact PR head; `cargo test -p quantick-guards` passes.
      *Evidence:* exit codes listed in the PR body; CI run URL. → PR body.
      *(R5)*
- [ ] **A6** — The PR's base is exactly `campaign/lean-a-plus`; merge and
      read-back are the coordinator's, recorded on issue #372.
      *Evidence:* `gh pr view --json baseRefName`. → handoff block. *(R7)*
- [x] **A7** — The diff touches only the paths R8 allows.
      *Evidence:* `git diff --name-only <base>...HEAD`. → PR body. *(R8)*
- [x] **A8** — Each new sibling opens with a doc comment naming what it owns
      and why it is a seam. *Evidence:* the files' first lines. → the diff.
      *(R9)*

## Injected gates

- [x] **G1** — Every artifact in English (`CLAUDE.md` owns the rule);
      `crates/guards/src/language.rs` passes in `cargo test -p quantick-guards`.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`,
      `env -u QUANTICK_BUBBLES cargo test --workspace` green on the final head,
      each run alone.
- [x] **G3** — Performance impact declared per touched path: per-request
      (`contract/reads.rs`, `evidence/store.rs`, `evidence/image.rs`,
      `gateway/local_action.rs`), per-frame (`gateway/trace_replay.rs`: one
      comparison per live tab), rare (contract construction). No complexity or
      call shape changes.
- [ ] **G4** — `arch-review` over `campaign/lean-a-plus...HEAD` with
      `code-review` at `medium`; every Blocker/Should-fix resolved or deferred
      in the PR body. `ai-review` complete with zero open threads.

## Not applicable, and why

- *Touches a hot path* — no per-trade or per-depth path is touched; the frame
  path keeps its call shape (S6), so no `APP_HEALTH_SUMMARY` run is owed.
- *Touches anything user-visible* — no UI surface changes; no hook is added,
  moved or removed.
- *Adds a capability / adds something a trader does* — nothing is added.
- *Engine / determinism territory* — the engine is untouched.
- *Docs/skills only* — this is runtime code; the full shape pass applies.

## Evidence at archive time

- A1: production lines (script over `size.rs`'s law): contract.rs 1,718 → 1,261,
  evidence.rs 1,708 → 1,221, gateway.rs 1,686 → 1,219; reads.rs 514, store.rs
  327, image.rs 219, local_action.rs 282, trace_replay.rs 234. Named tests:
  `cargo test -p quantick-app -- control::contract:: control::evidence::
  control::gateway:: published_observer_schemas_remain_compatible …` → 72
  passed, 0 failed; `cargo test -p quantick-control --test
  published_schema_compatibility` → 5 passed; `cargo test -p quantick-app
  control` → 175 passed, 0 failed, 5 ignored.
- A2: `statements.py` multiset, base vs. head: contract 1,320 = 1,320,
  evidence 1,189 = 1,189, gateway 975 = 975, all `identical`.
- A3: workspace test log has 0 `FAILED` lines; no file under any `tests/`
  or fixture directory is in the diff; the one test-module change is an
  import line.
- A4: `size-baseline.txt` diff removes the three entries; `--tighten` wrote
  `!budget 45739 -> 40383`; `--report` `ratchet.size.recorded` 45,505 → 40,383.
- A5 (local half): fmt 0, clippy 0, build 0, test 0, each on its own;
  `cargo test -p quantick-guards` 162+7+12+20+5 passed. CI: pending at the PR
  head.
- A7: `git diff --name-only origin/campaign/lean-a-plus...HEAD` lists the
  three roots, five siblings, the baseline and this archive.
- A8: each sibling opens with a `//!` doc comment naming its owner and seam.
- G3: per request — reads.rs, store.rs, image.rs, local_action.rs; per
  frame — trace_replay.rs (same body, same call site); rare — contract
  construction. No call shape changed.
- Environment note: drive C: hit 0 bytes free mid-mission (a write to
  contract.rs was truncated and regenerated from the split script); the
  coordinator was told, space was freed outside this mission, nothing outside
  this worktree's own `target/` was deleted by it.

## Closing steps

- **C1** — `delivery-review` returns PASS and records its marker with the
  shared key.
- **C2** — Draft PR open against `campaign/lean-a-plus`, `arch-review` and
  `ai-review` recorded, CI green at the head, one `gh pr ready` attempt made.

## The request as received (verbatim, attributed quotation — coordinator prompt for campaign #367 child S5)

> You are executing campaign child mission **S5** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick:
> split `crates/app/src/control/contract.rs` (1,718 production lines),
> `crates/app/src/control/evidence.rs` (1,708) and
> `crates/app/src/control/gateway.rs` (1,696) into owned sibling modules, each
> at most 1,500 production lines, with no behaviour change and no authority
> path moved by one code path. You are a subagent: you cannot ask the trader;
> decisions D1..Dn below answer the mission's step-3 questions. A doubt that
> would need a new decision is reported back, never guessed.
>
> [Read first, in this order: CLAUDE.md; the mission skill as a campaign child
> at tier `high` following steps 1, 2, 4, 5, 7, 8, 9; the integration contract
> and the delivery contract; issue #372; the size-baseline history and PR
> #384's purity proof; the control-plane contract docs as read-only context.]
>
> Worktree (the only place you write): `refactor/control-plane-under-1500`
> from `origin/campaign/lean-a-plus` at `e8eb23e5`. Siblings running in
> parallel own the order-flow renderers and `drawings/`; you own only the three
> named files, new files under `crates/app/src/control/contract/`,
> `crates/app/src/control/evidence/`, `crates/app/src/control/gateway/`, your
> entries in `crates/guards/size-baseline.txt`, and `mod` lines if strictly
> needed. Do not edit `crates/app/src/app/tests/control_plane_tests.rs` or any
> file outside `crates/app/src/control/`.
>
> Decisions: D1 pure moves only, `pub(super)` the only widening, stop and
> report a human_decision if more is needed; D2 the ceiling is production lines
> per `size.rs`; D3 seams — `contract.rs`: permission ceilings and refusal
> rules versus envelope validation; `evidence.rs`: capture versus verification;
> `gateway.rs`: `invoke_local_action` and `service_replay_trace` out to
> siblings; D4 statement-line multiset proof and named authority/schema tests
> with counts; D5 performance stated per touched path; D6 `--tighten`, the
> three entries leave, `!budget` must not rise, guards pass; D7 tier `high`
> reviews with at most two step-0 rounds; D8 one `gh pr ready` attempt through
> the Bash tool, never PowerShell, never merge, never touch main.
>
> Delivery: implement with the verification loop; the four checks before every
> commit, each separately; conventional English commits with the attribution
> lines; archive `GOAL.md` (slug `control-plane-under-1500`) as the last commit
> before reviews; push and open a draft PR against `campaign/lean-a-plus` with
> the named title and body; `/arch-review`, `/ai-review`, `/delivery-review`;
> watch CI; one `gh pr ready` attempt; return a HANDOFF BLOCK.
