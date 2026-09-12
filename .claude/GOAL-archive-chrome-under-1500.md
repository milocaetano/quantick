# Mission — split the tool rail, tab and layout wiring under 1,500 production lines

**Objective:** split `crates/app/src/toolrail.rs`, `crates/app/src/tab.rs` and
`crates/app/src/app/layout_wiring.rs` into owned sibling modules so every one
of them, and every sibling it creates, is at most 1,500 production lines, with
no behaviour change.

Why it matters: these three are trunk-adjacent files an agent opens for any
chrome change. Each carried a signed entry in `crates/guards/size-baseline.txt`,
which is debt: an agent that opens one to change one behaviour reads a whole
subsystem. The campaign's rule is one owner per file.

**Tier:** `medium` — campaign child S6 of
https://github.com/milocaetano/quantick/issues/367. A pure-move refactor over
UI chrome and the tab's pane state: mechanical, but wide enough that the shape
pass and a completeness pass are both worth their cost. Step 3 was answered by
the coordinator as D1..D8 below.

## Request ledger

- **R1** — `toolrail.rs` (2,114 production lines), `tab.rs` (1,661) and
  `app/layout_wiring.rs` (1,577) each end at most 1,500 production lines, and
  so does every new sibling.
- **R2** — every production move is a *pure* move: bodies move unchanged, the
  only widenings are the marks a child module needs.
- **R3** — no behaviour change: no UI, persistence-format, feed-drain or
  workspace-layout difference; no test expectation edited.
- **R4** — the three entries leave `crates/guards/size-baseline.txt` and the
  `!budget` line does not rise.
- **R5** — the four checks green on the final head, run one at a time, plus
  `cargo test -p quantick-guards`; CI green at the exact PR head.
- **R6** — performance stated per touched path; no per-trade, per-depth or
  per-frame path changes its complexity.
- **R7** — the work lands through a PR whose base is exactly
  `campaign/lean-a-plus`, reviewed under the campaign's shared key.

## Decisions (from the coordinator, in place of step 3)

- **D1** — pure moves only; the only widenings are the marks a child module
  needs. A `pub` beyond `pub(super)`/`pub(crate)`, or a new field on `ToolRail`,
  `Tab` or `QuantickApp`, stops the work and is reported.
- **D2** — the ceiling is production lines as `crates/guards/src/size.rs`
  measures them. Moving inline tests to sidecars is allowed, never required.
- **D3** — seams: `toolrail.rs` by rail section; `tab.rs` by feed drain versus
  settings and persistence; `layout_wiring.rs` by wiring group.
- **D4** — purity is proved by a statement-line multiset before/after, with the
  command and its result in the PR body; the round-trip tests are named.
- **D5** — performance is stated per touched path; only a changed call shape on
  a hot path needs a measurement.
- **D6** — `--tighten` after the moves; the three entries leave the baseline;
  `!budget` must not rise; `cargo test -p quantick-guards` passes.
- **D7** — tier `medium`: `arch-review` with `code-review` at `low`, full shape
  pass; `ai-review` completion; `delivery-review` completeness pass inline. At
  most two step-0 rounds.
- **D8** — one `gh pr ready` attempt through the Bash tool; if the gate denies
  a cd-prefixed command, stop at the draft PR and report.

## Assumptions

- **S1** — `tab.rs`'s feed drain already lives in `crates/app/src/tab/feed.rs`,
  so D3's "feed drain versus settings and persistence" seam is read as its
  in-file equivalent: the tab's pane addressing and its layout/spec persistence
  move out, and what stays is the tab's construction and feed attachment. The
  `drain_feed` opening-block arm the baseline comment cites was never in this
  file, so nothing of it is disturbed. Safe to assume: the code answers it in
  under a minute, and the outcome R1 asks for is unchanged.
- **S2** — a method that was `pub(super)` in `app/layout_wiring.rs` keeps the
  same visibility from its new child module as `pub(in crate::app)`. That is
  the *same* visibility, not a widening, so it stays inside D1.
- **S3** — the extension-boundary ratchet counts `impl QuantickApp` lines, and
  a new child module adds one `impl`/`}` pair to that count. Moving the layout
  strip and its delete confirmation out as one module — they were two adjacent
  `impl QuantickApp` blocks separated by a blank line — pays that pair back, so
  the root count falls rather than rises. Safe to assume: it is a file-local
  structural choice with no behaviour in it, and the guard proves the result.

## Acceptance criteria

- [ ] **A1** — each named file and every new sibling is at most 1,500
      production lines. *Evidence:* the guard's own measure printed per file.
      → PR body size table. *(R1)*
- [ ] **A2** — every production move is pure. *Evidence:* a statement-line
      multiset over each family before/after, whose only differences are module
      docs, `mod`/`use super::*`/`impl`/`}` scaffolding, visibility rewrites in
      pairs, and one rustfmt signature rewrap. → PR body purity proof. *(R2)*
- [ ] **A3** — behaviour is unchanged and no test expectation was edited.
      *Evidence:* `cargo test --workspace` green with no test file in the diff;
      the workspace and UI-state round-trip suites named. → PR body. *(R3)*
- [ ] **A4** — `size-baseline.txt` carries no entry for any named file and
      `!budget` fell. *Evidence:* the baseline diff. → PR body. *(R4)*
- [ ] **A5** — the four checks green on the final head, one at a time, plus
      `cargo test -p quantick-guards`; CI green at the PR head. *Evidence:*
      command output and the CI run URL. → PR body. *(R5)*
- [ ] **G1** — every artifact in English; conventional commits.
- [ ] **G2** — performance declared per touched path. *Evidence:* a per-path
      rate classification. → PR body. *(R6)*
- [ ] **G3** — `arch-review` run over the diff against `campaign/lean-a-plus`
      with its step 0 bug pass, every Blocker and Should-fix resolved or
      deferred in the PR body; `ai-review` completion recorded;
      `delivery-review` completeness pass. *Evidence:* review verdicts and the
      markers under the campaign's shared key. *(R7)*

## Not applicable

- *Touches a hot path* — the rail and the tab draw per frame and the tab's
  drain runs per trade batch, but no call shape on either changes: methods move
  between `impl` blocks of the same type, which is resolved at compile time.
  No measurement is owed; the classification is stated instead.
- *Touches anything user-visible* — nothing user-visible changes, so no new or
  changed surface needs a hook, and `visual-qa` / `trader-ux-review` have
  nothing to grade. The existing hook declaration
  (`QUANTICK_PANE_COLLAPSED` in `tab.rs`) stays beside its read, which did not
  move.
- *Adds a capability* — none added.
- *Adds something a trader does* — none added.
- *Engine / determinism territory* — no engine code is touched.
- *Docs/skills only* — this is runtime code.

## Closing steps

- **C1** — `delivery-review` returns PASS (completeness pass, tier `medium`).
- **C2** — the PR is open against `campaign/lean-a-plus` with the evidence in
  its body.

## The request as received

> You are executing campaign child mission **S6** of campaign #367 for
> milocaetano/quantick: split `crates/app/src/toolrail.rs` (2,114 production
> lines), `crates/app/src/tab.rs` (1,661) and
> `crates/app/src/app/layout_wiring.rs` (1,577) into owned sibling modules,
> each at most 1,500 production lines, with no behaviour change. You are a
> subagent: you cannot ask the trader; decisions D1..Dn below answer the
> mission's step-3 questions. A doubt that would need a new decision is
> reported back, never guessed.
