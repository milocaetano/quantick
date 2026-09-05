# Mission: move the inline tests out of the six largest hosts

Move the inline `#[cfg(test)] mod` blocks out of the six production files that
carry the most of them, into a `<file>/tests/mod.rs` sibling beside each, so a
session opening one of these files to change a ticket no longer reads 13,300
lines of tests it did not ask for.

**Why it matters:** the size ratchet counts production lines only, so these
files read as finished to the guard while `paper_trading.rs` is 8,249 lines on
disk against 4,589 production. The 3,651 lines an agent does not need are the
most expensive kind to skim, because a test reads like the code it exercises.
This is the largest read-cost cut left that changes no behaviour, no signature
and no production line.

**Tier:** `medium`. Thirteen thousand lines move across two crates under a
zero-tolerance criterion on both production line counts and test counts, so the
completeness pass is worth its cost; but the change is mechanical, has no
design surface, touches no UI and adds no capability, so the full `high` round
would buy nothing.

## Request ledger

| # | Ask |
| --- | --- |
| R1 | Move the test module in `crates/app/src/paper_trading.rs` (both `risk_tests` and `tests`, 3,651 lines) into `paper_trading/tests/`. |
| R2 | Move the `tests` module in `crates/orderflow/src/projection.rs` (3,040 lines) into `projection/tests/mod.rs`. |
| R3 | Move the `tests` module in `crates/app/src/drawings/mod.rs` (2,134 lines) into `drawings/tests/mod.rs`. |
| R4 | Move the `tests` module in `crates/app/src/orderflow_render.rs` (1,878 lines) into `orderflow_render/tests/mod.rs`. |
| R5 | Move the `tests` module in `crates/app/src/toolrail.rs` (1,323 lines) into `toolrail/tests/mod.rs`. |
| R6 | Move the `tests` module in `crates/app/src/paper_report.rs` (1,305 lines) into `paper_report/tests/mod.rs`. |
| R7 | Follow the shape PR #288 used for `pane.rs` and `app.rs` already uses: each host ends with `#[cfg(test)] mod tests;` where the inline module was. |
| R8 | Indentation preserved — verbatim fragment: *"never de-indent the moved items — the diff must read as a move"*. |
| R9 | `use super::*` kept as the moved module's first line. |
| R10 | Test-only production helpers left where they are — the two `#[cfg(test)] use` lines aside, which move with the tests if nothing else in the host uses them. |
| R11 | A nested module named after a real crate module is suffixed `_tests`, so it does not shadow the real one. |
| R12 | Production line counts do not change by one line. |
| R13 | Test counts per crate do not change by one test. |
| R14 | Nothing widened: no `pub`/`pub(crate)` added to reach a moved test. A path fix forced by the move is allowed and listed in the PR body. |
| R15 | Per-crate test counts recorded before and after for `quantick-app` and `quantick-orderflow`. |
| R16 | `wc -l` before and after for all six hosts in the PR body; `paper_trading.rs` under 4,700 lines and `projection.rs` under 1,900. |
| R17 | The guards report diffed before and after: nothing moves but the `#[cfg(test)]` site count. |
| R18 | `git diff --color-moved` shows moves; every non-move hunk is a `mod tests;` line, a `use` line, or a listed path fix. |
| R19 | Verify each of the twelve evidence-ledger claims against the tree before acting, rather than trusting the brief. |
| R20 | Stay out of the stated out-of-scope: the other 239 files, the test-only helpers and the toolrail instrumentation, splitting `pane/tests` or `app/tests` further, and any change to what a test asserts. |
| R21 | The purpose that judges the rest: cut the read cost of the six files without changing behaviour, a signature or a production line. |

## Decisions taken by the trader

None. The brief settles every ambiguity it raises, including delegating the
`paper_trading.rs` layout to the mission, so no question qualified under the
`medium` budget.

## Assumptions

- **S1** — The `medium` budget allowed two questions and none was asked. Every
  doubt below is recorded here instead.
- **S2** — *Wanted to ask.* R17's criterion says the report's `site.cfg_test`
  moves "by exactly the number of modules moved (seven)". It cannot: the report
  counts every literal `#[cfg(test)]` occurrence in a non-test file
  (`report.rs:308`), not modules, and each host keeps one such attribute on the
  `#[cfg(test)] mod tests;` line that replaces the module. The predicted delta
  is therefore not −7. Measured, it is −5 across the six hosts: `paper_trading`
  −1 (two modules folded behind one declaration), `drawings` −3 and
  `paper_report` −1 (attributes that sat *inside* the moved test modules and
  left with them), and `projection`, `orderflow_render` and `toolrail` zero
  each, every one of them trading its module's attribute for the one on
  `mod tests;`. The report shows −4 rather than −5 because the doc comment
  this mission adds to `headless.rs` writes the literal `#[cfg(test)]` once in
  prose, and the metric counts substrings, not items.
  Going with the criterion's
  intent — nothing in the report moves except the `#[cfg(test)]` site count —
  and reporting the arithmetic against the measured delta. A wrong guess here
  throws away no work: the refactor is byte-identical either way, only the
  number reported differs.
- **S3** — *Wanted to ask.* R8 says never de-indent, but the precedent it names
  does: `pane/tests/mod.rs` and `app/tests/*.rs` sit at column zero, and
  `cargo fmt --check` would reject a top-level item indented four spaces. Read
  as forbidding reflow, not the single uniform dedent that unwrapping a module
  into a file requires. Every relative indentation inside the moved bodies is
  preserved byte for byte, and `--color-moved-ws=allow-indentation-change`
  shows the result as a move.

  The dedent has one exception, found the expensive way. A first pass stripped
  four spaces from every line that had them, including the interior of
  multi-line string literals — which is data, not code, so six tests changed
  what they asserted and only one of the six said so
  (`the_report_numbers_are_fixed`, comparing a `{:#?}` dump against a 260-line
  raw string). The extractor now scans the source one character at a time,
  tracking normal, raw, byte and char literals as well as both comment forms,
  and leaves any line that *begins* inside a literal byte-identical. It asserts
  both invariants before writing each module: every line's `strip()` is
  unchanged, and every literal-interior line is byte-equal to the original.
  266 lines are preserved that way. Rustfmt then rejoined nine lines that fit
  once they were four columns shallower; none is inside a literal, and each is
  listed in the PR body.
- **S4** — `paper_trading.rs` gets `tests/mod.rs` (the `tests` body, plus a
  `mod risk_tests;` line) and `tests/risk_tests.rs`, following `app/tests/`
  rather than nesting both inside one file. The brief left the layout to the
  mission; this is the shape the repo already proves at twelve files, and
  `risk_tests` already carries the `_tests` suffix R11 asks for.
- **S5** — The brief measured against `origin/main` at `343c658`; this branch
  cuts from `d2ba64a`. All six files carry identical line counts and module
  offsets at both, re-verified before any edit, so the brief's numbers stand.
- **S7** — The two `#[cfg(test)] use` lines in `paper_trading.rs` (`:26`, `:37`)
  stay in the host rather than moving with the tests. R10 sends them along only
  "if nothing else in the host uses them", and after the move they are still
  used: the child module reaches them through `use super::*`, which carries an
  ancestor's private imports down — the mechanism `app/tests/mod.rs` documents
  and `app.rs:35` already proves, keeping a `#[cfg(test)] use` in the host with
  its tests in `app/tests/`. Moving them would also have taken their two
  explanatory comment lines, and a comment sitting above a `#[cfg(test)]`
  attribute *is* a production line by `production_source` (`size.rs:230`), so
  the move would have cut three production lines and broken R12. Leaving an
  orphaned comment describing an absent import was the other option and is
  worse than both.
- **S8** — A necessary detour, taken rather than asked: the move breaks the
  headless guard, and fixing the guard is part of this mission. `headless.rs`
  states in its own header that it does not report test code, and it delivered
  that by reading `size::production_source`, which drops an *inline*
  `#[cfg(test)]` module — every test in the repository, while every test lived
  inside its host. `projection.rs`'s tests carry an `Instant::now` bench, so
  the moment they became their own file the guard read them as shipping
  production source and failed the workspace. Ledger #9 records PR #288 hitting
  the same class of thing ("route the relocated pane tests and drop the guard's
  exemption"). The fix excludes a `tests/` path component in `in_scope` — the
  same exclusion `size::is_tracked` already makes, for the reason `headless.rs`
  already gives — and routes the directory walk through `in_scope` so the walk
  and `check_file` cannot disagree, which the doc comment claimed they could
  not while the walk carried its own `.rs` test. The alternative, signing the
  bench into `headless-allowlist.txt`, was rejected: that file is for
  production sites that ship, and its own header argues against a fifth entry.
  This is the only production code the branch changes, and it changes no
  behaviour for any file that is not under `tests/`.
- **S6** — The moved files are invisible to the size guard because its walk
  skips any path component named `tests` (`size.rs:281`), not because of
  `report.rs`'s `is_test_file`, which matches only a file named `tests.rs`.
  Ledger #11 holds, by that mechanism.

## Acceptance criteria

- [ ] **A1** — Each of the six hosts ends with `#[cfg(test)] mod tests;` where
      its inline module was, and holds no other test module.
      *Evidence:* `tail` of each host and a grep proving no `mod tests {`
      remains in the six. → PR body. *(R1-R7)*
- [ ] **A2** — The seven moved modules live in six new `tests/` directories,
      each opening with the `use super::*` and `use` lines the inline module
      had. *Evidence:* `head` of each new file. → PR body. *(R1-R6, R9)*
- [ ] **A3** — Production line counts do not move: the guards report before and
      after differs in nothing but the `#[cfg(test)]` site count, with the
      arithmetic of S2 shown. *Evidence:* the report diff, quoted in the PR
      body. → PR body. *(R12, R17)*
- [ ] **A4** — All six hosts shorter on disk by their test lines, with
      `paper_trading.rs` under 4,700 and `projection.rs` under 1,900.
      *Evidence:* `wc -l` before and after for all six. → PR body. *(R16)*
- [ ] **A5** — `cargo test -p quantick-app` and `cargo test -p quantick-orderflow`
      report the same number of tests as before, all green.
      *Evidence:* the "test result" lines before and after. → PR body. *(R13, R15)*
- [ ] **A6** — Nothing widened: the diff adds no `pub` or `pub(crate)` to a
      production item. Any path fix the move forced is listed.
      *Evidence:* a grep over the diff for added visibility keywords.
      → PR body. *(R14)*
- [ ] **A7** — `git diff --color-moved=zebra --color-moved-ws=allow-indentation-change`
      shows the bodies as moves; every non-move hunk is a `mod tests;` line, a
      `use` line, or a listed path fix. *Evidence:* the non-move hunk list.
      → PR body. *(R8, R18)*
- [ ] **A8** — The test-only production items named in ledger #2, #4, #6 and #7
      are still in their hosts, and no file outside the six hosts and their new
      `tests/` directories is touched. *Evidence:* `git diff --stat` and a grep
      for each named helper. → PR body. *(R10, R20)*
- [ ] **A9** — Each of the twelve evidence-ledger claims re-verified against the
      tree before editing, with the one correction S2 records.
      *Evidence:* the verification notes. → PR body. *(R19)*
- [ ] **G1** — Four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`, each run on its own.
      *Evidence:* the four exit codes. → PR body.
- [ ] **G2** — `cargo test -p quantick-guards` green, the language guard
      included. *Evidence:* its "test result" line. → PR body.
- [ ] **G3** — Performance impact declared: no production path is touched, so
      the rate classification is *no runtime path changed*; compile-time only.
      *Evidence:* this line, restated in the PR body.
- [ ] **G4** — Every artifact in English. *Evidence:* `arch-review` dimension 8
      and the language guard.
- [ ] **G5** — `arch-review` run over the final branch, every Blocker and
      Should-fix resolved or deferred in the PR body with its severity.
      *Evidence:* its verdict.

## Not applicable

- **Hot path** — no production line changes; nothing executes differently.
- **User-visible** — no surface changes, so `ui-harness`, `visual-qa` and
  `trader-ux-review` do not apply.
- **Adds a capability** — nothing is added; `new-extension` does not apply.
- **Something a trader does** — no new action, tool, trade or lock.
- **Engine / determinism** — no engine behaviour changes; the tests that guard
  determinism move unchanged and still run.
- **Docs/skills only** — this is code, so the full shape pass applies.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, naming the tier, with CI green.

## The request as received

> Quoted verbatim from the trader, in the language it was written
> (an attributed quotation under `CLAUDE.md`'s language exemption).

> medium refactor/tests-out-of-the-hosts — 67,568 lines of the workspace are
> inline `#[cfg(test)] mod` blocks inside 245 production files, and six files
> carry 13,300 of them: crates/app/src/paper_trading.rs (3,651 test lines of
> 8,249), crates/orderflow/src/projection.rs (3,040 of 4,901),
> crates/app/src/drawings/mod.rs (2,134 of 4,454),
> crates/app/src/orderflow_render.rs (1,878 of 4,953),
> crates/app/src/toolrail.rs (1,323 of 3,448) and
> crates/app/src/paper_report.rs (1,305 of 4,610). Move each file's test module
> into `<file>/tests/mod.rs` beside it, the way PR #288 did for pane.rs and
> app.rs already does, indentation preserved, `use super::*` kept, test-only
> helpers left where they are. Production line counts do not change by one
> line; test counts per crate do not change by one test. Read
> C:\src\mission-tests-out-of-the-hosts.md in full before anything else and
> build the request ledger from it.

The brief that invocation points to, `C:\src\mission-tests-out-of-the-hosts.md`,
is the mission's real request and is quoted above only by reference: its twelve
evidence-ledger claims, its five scope points, its five acceptance criteria,
its four out-of-scope exclusions and its parallel-work note are decomposed into
R1-R21 above. The brief itself is not committed: it is a working file outside
the repository, and every ask it carries is on the ledger.
