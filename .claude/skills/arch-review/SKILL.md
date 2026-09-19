---
name: arch-review
description: The full pre-PR shape review for quantick — bugs first via the bundled code-review, then docking, performance, tests, operability without a mouse, hardcoded values, the trunk and the English rule. Use when the user types /arch-review, asks for a code review or a bug pass before shipping, or asks whether a change is modular, fast enough, drivable by a script, or properly tested. Reviews a diff; it does not design the assistant.
---

Campaign children override the main-based examples via the
[integration contract](../../../docs/campaign/integration.md), including review keys.

# Architecture-first code review

Step 0 runs the bundled bug review; then grade shape. Read a dimension's
`references/` file before filing a finding in it; skip it for clean dimensions.

## Step 0 — the bug pass runs first, always

Correctness outranks architecture, so the review never closes without it. Run
the bundled `code-review` — never the plugin `code-review:code-review`, which
posts to the PR by itself:

```
Skill(code-review), args: "<effort> <target>"      one string, effort FIRST
  <effort>   never omitted — a first token that is not a level silently reuses
             another session's cached level and swallows the target
  <target>   a PR number once one exists, else a branch, or omitted for the
             working diff; never a revision range
```

- **Level from the tier**, one notch below its name: `low` for `small` and
  `medium`, `medium` for `high`, `high` for `max` (say in the header that
  `/code-review ultra` exists; only the trader triggers it). No tier, or one
  the hook would not honour: `high` for a branch or PR, `medium` for a working
  diff. Read the tier from the file `pr-gate` reads, and report a disagreement
  with `GOAL.md`'s `**Tier:**` line as a finding:

  ```sh
  WT=/path/to/worktree
  cd "$WT" && cat "$(git rev-parse --absolute-git-dir)/mission-tier"
  ```

- **Never re-run a level that ran clean**; when to stop follows [the delivery
  contract](../../../docs/workflow/delivery.md) (finding IDs, bounded repairs,
  delta follow-up when inputs qualify, else the full diff).
- **The bug pass keeps the strong model.**
- **Prove the level** — effort-first by construction, and no reuse notice in
  the report (one present means the invocation failed). Ran **below** the
  tier's level: re-invoke once. Ran **above**: accept, record the overspend in
  the header and PR body. Unsettled: write *unverified*. Detail:
  `references/step-0.md`.
- **It returns in the background.** Read for shape meanwhile, publish nothing.
  No notification: re-invoke once; failing again, do the bug pass yourself and
  say so.
- **Check its scope.** Findings on files the branch never touched, or a
  suspiciously empty pass: re-invoke with an explicit target.
- **Wrong behaviour** (crash, wrong output, broken determinism, race) becomes
  a **Blocker**, listed first. A cleanup is graded in its own dimension — an
  efficiency finding on a per-frame path is still a Blocker there. Confirm
  uncertain findings (`high`+) under *Verify* item 2. Cite `file:line` plus the
  severity assigned here; never restate. Step 0 never publishes: no `--fix`,
  `--comment` or `--post`.

A docs/skills change still runs this skill for `arch-review-ok`, reporting step
0 through it; only the shape pass is waived.

## Publish and record

Write the complete verdict to a scratch report whose last line is exactly
`ARCH-REVIEW: PASS`. Once every Blocker and Should-fix is resolved or validly
deferred and no edit is pending, from the clean reviewed worktree:

```sh
cd "$WT" &&
  sh .claude/hooks/review_report.sh publish arch-review "$PR" "$REPORT_PATH"
```

The producer verifies worktree, branch, HEAD, base, review key and PR before and
after publishing, reads the report back, then records `arch-review-ok`; a
hand-written marker has no receipt and readiness rejects it. No PR: print the
report, record nothing. A later tracked edit needs a current follow-up, never a
marker refresh.

Conformance (*is it what was asked?*) is `delivery-review`'s, after this; a gap
noticed here gets a sentence, never a severity.

## Priority order

State it when a trade-off is at stake. **0** correctness, determinism and
authority (no operator reaches a market or safety action by a path the trader
does not cross) — a precondition, not a trade-off. **1** performance — never
spend runtime to read better. **2** modularity and extensibility, including the
second operator's operability half. **3** tests that prove behaviour. **4**
standardisation. **5** human-friendliness — mandatory wherever free at runtime.

## Scope

```sh
git fetch origin                       # the ranges read origin/main
git diff origin/main...HEAD --stat     # branch
git diff --stat                        # working diff
gh pr diff <n>                         # PR
```

Never `main...HEAD` — a stale local `main` credits other PRs' files to the
branch. Read neighbouring code first; the repo's existing pattern is the
standard, and a second way to do a solved thing is a finding.

At `small`, read only the dimensions the diff reaches — **8 always**, step 0 in
full; three or more applying means say the branch outgrew its tier. Every other
tier reads all nine. A branch over the `small` ceiling in `guardrails.sh` gets
all nine whatever the tier file says.

## The nine dimensions

1. **Docking** — could a second implementation be a new file plus one
   registration line, editing no existing behaviour? If not, name the file the
   next author must open and the port that would prevent it. A port looks like
   `Surface` (`crates/app/src/surfaces/mod.rs`) or `TradingVenue`
   (`crates/trading/src/venue.rs`). Hunt: growing type switches (`match` on a
   closed enum, `if is_replay`, `if feed == "binance"`); a consumer naming a
   concrete producer; a reverse edge (Blocker); a per-consumer bar-building
   copy (Blocker); a change mostly of edits; a capability that activates
   itself. `references/docking.md`.
2. **Performance** — every touched path classified per trade, per depth
   update, per frame or rare, judged at that rate. Never "slow" without rate
   and concrete cost; a guess stated as a measurement is a finding against the
   reviewer. Cold-path micro-optimisation that costs clarity is a finding in
   reverse. `references/performance.md`.
3. **Nothing hardcoded** — every behaviour-configuring literal is named or in
   config, and the finding names its tier: config file (anything a user tunes,
   including what a fresh launch draws), shared module (two places must
   agree), or module top (`SCREAMING_SNAKE_CASE`, unit in the name, doc comment
   saying why). Message and assertion text is not configuration.
   `references/hardcoded.md`.
4. **Tests** — every new behaviour has a test that fails without it, or it is
   unproven (Blocker). Engine work is test-first with golden fixtures. Test the
   port with a fake second implementer. Name the existing test that would catch
   this change regressing; none is the finding. Cover the domain's edges: empty
   book, one-tick spread, zero quantity, update-id gap, saturating feed
   arithmetic, session ending mid-bar. `#[cfg(test)]` that changes behaviour is
   a Blocker. A decision worth testing is a pure plan function, like
   `crates/feed/src/ohlcv_plan.rs`. `references/tests.md`.
5. **Standardisation** — one way to do a thing: errors, layout, config,
   naming, logging, public API shape. A new convention needs a stated reason.
6. **Human-friendly at zero runtime cost** — names carry intent and unit
   (`cluster_window_ms`); complex algorithms get an English comment on the
   *objective*; inferred data labelled; newtypes over bare `f64`, exhaustive
   `match` over `_ =>`, early returns; no comment restating code, no dead code.
7. **The second operator** — could a script, test or assistant trigger this,
   read back what it did, and discover it exists, without a click? **Act** (a
   named call taking data and an actor), **read** (results enumerable as data),
   **discover** (a stable id in the registry that feeds the UI, with declared
   parameters) — each its own finding. What the trader authors is data, not a
   rebuild. Authority is priority 0: a market or safety action a non-human
   reaches by a shorter path than the trader's is a Blocker, and what the
   assistant placed is labelled. `references/second-operator.md`.
8. **English** — `CLAUDE.md` owns the rule; grade only lines the diff
   **authors** (moving or deleting a foreign line is not authoring). An
   authored foreign line is a Blocker. The guard (`language.rs`) scans code;
   you read the branch name, commits, PR title and body, prose its keyword list
   misses, and whether an exemption is honest. Report both verdicts.
   `references/language.md`.
9. **The trunk** — where did the registration lines land? Growth in the trunk
   (a ceiling raised without a comment is a finding), a closed-enum registry,
   blast radius in lines not only files, root-struct state only one surface
   reads. Say whether the fix is an extraction (mechanical) or a redesign.
   Execution artifacts or `GOAL-archive-*` in the diff are Should-fix: remove
   them and link external evidence; fixtures, contracts and docs are exempt.
   `references/docking.md`.

## Verify before reporting

Precision over volume. (0) Step 0's findings are in hand, or the review is
incomplete. (1) Read the surrounding code — most "missing" findings die one
function up. (2) Argue each finding's opposite; drop it if the refutation
holds. (3) Verify outputs for the delivery contract's applicable local path;
code changes need the four checks; final-head CI is mandatory. A clean change
gets a short review saying why. Never pad.

## Severity

- **Blocker** — confirmed step 0 correctness finding; reverse edge; forked
  aggregator; broken determinism; hot-path regression; new behaviour untested;
  self-activating feature; a market/safety action a non-human reaches by a
  shorter path, or with no record of who acted; behaviour-changing
  `#[cfg(test)]`; any authored non-English line.
- **Should fix** — hardcoded value (tier named); user-tunable value as a
  `const`; a constant duplicated across a boundary; an extension point forcing
  edits; missing regression cover; test module without `#[cfg(test)]`;
  undocumented `pub` used only by tests; unexplained complex algorithm;
  misleading name or missing unit; a second way to do a solved thing; a
  capability reachable only from a click handler; state only as pixels; a
  capability registered nowhere, or a hand-kept list beside a registry;
  something the trader should vary compiled in; root-struct field for one
  surface's state; a registry-enum variant where a trait object would absorb
  it; a ceiling raised without a comment.
- **Consider** — clarity with no correctness, performance or extensibility
  consequence.

## Output

One header line for step 0, always one of three shapes: `step 0: code-review at
high (effort-first, no reuse notice), 12 findings, 3 confirmed`; a divergence
(`at xhigh (tier bought medium; reuse notice, accepted per the asymmetric
rule)`); or unsettled (`at medium (effort-first; level unverified)`). It goes
into the PR body too.

Findings via `ReportFindings` when available, most severe first, categories
`correctness`, `modularity`, `performance`, `hardcoded-values`,
`test-coverage`, `test-layout`, `standardisation`, `agent-surface`,
`accumulation`, `language`, `readability`; otherwise markdown by severity. Each:
`file:line`, what is wrong, why in priority terms, and the concrete fix — never
"consider refactoring". Name the commit graded, after it exists.

Close with seven lines, none dropped: **Correctness** (step 0, anything open);
**Docking**; **Performance** (what moved, at what rate); **Operability** (act,
read, discover — or "no surface"); **Proof** (which test fails on regression,
unit or integration); **Accumulation** (tracked files moved and by how much,
ceilings raised — or "trunk flat"); **Language** (the guard's verdict, and
whether you read prose, branch and commits yourself).
