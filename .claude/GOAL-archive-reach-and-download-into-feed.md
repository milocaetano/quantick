# Mission: history reach and replay download join `quantick-feed`

Move the two remaining pure feed-side modules out of `crates/app` into the
`quantick-feed` crate as a pure move, with the one reach into the app's
documents folder handed in as a parameter.

Why it matters: `refactor/feed-crate` (PR #291) moved the feed contract and its
adapters but left these two behind, because they imported the contract and that
mission was scoped to the eight files under `feed/`. They are the follow-up it
named. Both are egui-free, both are about getting history into the chart, and
both are the last feed-side code an agent still has to find inside the
176k-line UI crate.

**Tier:** `medium`. Roughly two thousand lines change crates, across fifteen
consumer files and a guard that reads one of them by path — past the `small`
ceiling, and past the point where a completeness pass is optional. It is not
`high`: no behaviour is authored, no surface moves, and every dependency the
two files need is already on the receiving crate.

## Request ledger

Derived from `C:\src\mission-reach-and-download-into-feed.md`, whose every
claim was re-verified against `origin/main` at `d3cf317` before this file was
written. The brief measured against `2254039`; `origin/main` has since advanced
by PR #293, which merged the `refactor/native-indicator-docking` work the brief
listed as parallel.

| # | Ask |
| --- | --- |
| R1 | Move `crates/app/src/history_reach.rs` to `crates/feed/src/history_reach.rs`; body unchanged except paths. |
| R2 | Move `crates/app/src/replay_download.rs` to `crates/feed/src/replay_download.rs`; body unchanged except paths. |
| R3 | Register both with two `pub mod` lines in `crates/feed/src/lib.rs`. |
| R4 | The documents-folder edge: `replay_download` takes the shelf directory as an `Option<&Path>` from its caller instead of reaching `crate::paper_home`, *"the way `mt5_bridge` already takes it"*. |
| R5 | Consumers change `use` paths only — *"A pure move"*, no logic change. |
| R6 | Persistence names stay identical: *"a saved workspace must load unchanged"*. |
| R7 | Tests travel inside the files; the app test files change paths only. |
| R8 | No new crate, no new edge — the feed crate already depends on `engine`. |
| R9 | `AGENTS.md`'s table row for `feed` gains the two responsibilities *"in a few words"*. |
| R10 | `--tighten`: *"baselines tighten"*. |
| R11 | `cargo test -p quantick-feed` runs as many more tests as the two files held, green, **without building `quantick-app`**. |
| R12 | No definition or module of either file remains in `crates/app/src`; what remains are feed-crate references and the call site that passes the shelf directory. |
| R13 | Graph guard green; guards green; four-check loop green; `!budget` unchanged or lower. |
| R14 | Deliberately out of scope: `replay_get_data.rs` / `replay_view.rs` beyond the call sites; any change to reach rules, campaign steps or the exporter protocol; `paper_home.rs`. |
| R15 | **The judging ask.** After this, feed-side code lives in the feed crate: these two are the last of it inside the UI crate. |
| R16 | **Derived — the brief's ledger missed it.** `crates/guards/tests/session_gap_agreement.rs` reads `crates/app/src/history_reach.rs` by hardcoded path (`fs::read_to_string`, line 146) and names it in three more places (lines 6, 103, 115). Its path and prose follow the move, or R13 cannot hold. Not invented scope: R13 forces it. |

## Decisions taken by the trader

None. No question qualified — see the assumptions below, each of which had a
conventional default in this repo or an answer the code gave in under a minute.

## Assumptions

| # | Assumption | Why it was safe to assume rather than ask |
| --- | --- | --- |
| S1 | Consumers import the module and use short paths — `use quantick_feed::history_reach;` then `history_reach::HistoryReach` — rather than fully-qualified `quantick_feed::history_reach::` inline. | The repo has a dominant convention for exactly this: `crates/app/src/tab.rs:42` reads `use quantick_feed::stall::{self, Stall, StallInput};`, and nineteen other `use quantick_feed::…` lines across `crates/app/src` follow it. |
| S2 | *Wanted to ask.* The brief's acceptance criterion — `grep -rn` over `crates/app/src` *"shows only `use` lines and the two call sites"* — is literally unachievable and is restated, not narrowed. With the module imported per S1, roughly eighty short-path mentions (`history_reach::HistoryReach`) remain in `app.rs`, `toolbar.rs` and `tab.rs` and still match a grep for the word. A2 states the checkable form of the same intent. | The intent is unambiguous — no logic change, nothing feed-side left defined in `app` — and only the proxy was imprecise. A wrong guess costs nothing, because the restatement is strictly harder to pass than the literal one is to satisfy. |
| S3 | `AGENTS.md`'s `feed` row absorbs the two responsibilities **net-neutral or smaller**, by compressing the sentence already there, so no ceiling raise and no budget spend. | `AGENTS.md` sits at exactly its recorded ceiling (13,179 bytes, `context-baseline.txt:103`), and the context budget has only 886 bytes of headroom left (measured 233,999 against a 232,885 budget with 2,000 headroom). The `app` row offers no offset — it is one short generic line. `CLAUDE.md` already settles the direction: raising one ceiling means lowering another in the same change. |
| S4 | `Mt5SessionSource::new` gains the shelf directory as a second parameter; its one production caller, `replay_get_data.rs:263`, passes `crate::paper_home::shelf_dir().as_deref()`. The `#[cfg(test)]` `with_roots` takes it too, so the test constructor stays honest. | Verified: those are the only two call sites in the workspace. The brief's *"`replay_get_data.rs` / `replay_view.rs` pass what they already have"* is inaccurate — `replay_view.rs` has no such call site, referencing only `DEFAULT_JOIN_DAY_BEFORE` and a doc link. Recording the correction rather than acting on the wrong half. |
| S5 | The two Rust-path references in `bridge/mt5/quantick_bridge.py:918` and `bridge/mt5/tests/test_session_backfill.py:14`, which name `crate::history_reach`, become `quantick_feed::history_reach`. | Two comment words, squarely in the cross-surface hazard this repo treats as its own bug class. A diff that leaves behind a path it just made wrong is authoring the staleness, not inheriting it. |
| S6 | Archived goal files and everything under `.claude/evidence/` keep the old path untouched. | They are frozen historical records of what was true when written; editing them would falsify the record. |
| S7 | Both modules sit at the feed crate root, siblings of `mt5_bridge` and `stall`. | The brief specifies two `pub mod` lines in `lib.rs`, which is that placement. |

## Acceptance criteria

- [ ] **A1** — `crates/feed/src/history_reach.rs` and `crates/feed/src/replay_download.rs` exist, are registered by two `pub mod` lines in `crates/feed/src/lib.rs`, and no longer exist under `crates/app/src`. Their bodies differ from the originals only in import paths and in R4's parameter.
      *Evidence:* `git diff --find-renames --stat origin/main...HEAD` showing both as renames, plus the full diff of each moved file.
      → `.claude/evidence/reach-and-download-into-feed/move.txt`. *(R1, R2, R3)*
- [ ] **A2** — No `mod history_reach`, `mod replay_download`, `crate::history_reach` or `crate::replay_download` remains anywhere in `crates/app/src`; every surviving mention resolves to the feed crate, and no consumer's logic changed.
      *Evidence:* the two greps, empty; and `git diff origin/main...HEAD -- crates/app/src` reviewed line by line for path-only changes.
      → `.claude/evidence/reach-and-download-into-feed/consumers.txt`. *(R5, R12, R15)*
- [ ] **A3** — `Mt5SessionSource::new` takes the shelf directory as `Option<&Path>` and reaches `paper_home` nowhere; `replay_get_data.rs` passes it. A grep for `paper_home` over `crates/feed/src` is empty.
      *Evidence:* the grep, plus the diff of the two constructors and the one call site.
      → `.claude/evidence/reach-and-download-into-feed/shelf-param.txt`. *(R4)*
- [ ] **A4** — `cargo test -p quantick-feed` is green and runs at least as many more tests than on `origin/main` as the two moved files held, and its build does not compile `quantick-app`.
      *Evidence:* both test counts, before and after, and a `cargo tree` inversion showing `quantick-app` is not in `quantick-feed`'s build graph.
      → `.claude/evidence/reach-and-download-into-feed/test-counts.txt`. *(R7, R11)*
- [ ] **A5** — A workspace saved by today's build loads on the branch with the same reach settings: every persistence name is byte-identical, and the `ui_state` round-trip test passes unchanged.
      *Evidence:* the round-trip test's name and result, plus a diff over `ui_state.rs` and `config.rs` showing no serialised name changed.
      → `.claude/evidence/reach-and-download-into-feed/persistence.txt`. *(R6)*
- [ ] **A6** — `crates/guards/tests/session_gap_agreement.rs` names `crates/feed/src/history_reach.rs` in all four places and passes; `crates/guards/Cargo.toml`'s dependency tables stay empty.
      *Evidence:* the test's result and the four updated references.
      → `.claude/evidence/reach-and-download-into-feed/agreement-guard.txt`. *(R16, R13)*
- [ ] **A7** — The graph guard is unchanged and green: `crates/pine/tests/workspace_deps.rs`'s `feed` entry gains no edge, and `crates/feed/Cargo.toml` gains no dependency.
      *Evidence:* an empty diff over both files, and the guard's result.
      → `.claude/evidence/reach-and-download-into-feed/graph.txt`. *(R8)*
- [ ] **A8** — `AGENTS.md`'s `feed` row names both responsibilities, and `AGENTS.md` is no larger than its recorded ceiling of 13,179 bytes, so `context-baseline.txt` needs no raise and `!budget` is unchanged or lower.
      *Evidence:* the row, the file's byte count, and the `ratchet.context.*` lines of the guards report before and after.
      → `.claude/evidence/reach-and-download-into-feed/agents-and-budget.txt`. *(R9, R10, R13)*
- [ ] **A9** — Nothing in R14's out-of-scope list changed: `paper_home.rs` is untouched, and `replay_get_data.rs` / `replay_view.rs` differ only at their `use` lines and the one call site. No reach rule, campaign step or exporter argument changed.
      *Evidence:* a diffstat over those three paths, with the whole diff of each.
      → `.claude/evidence/reach-and-download-into-feed/out-of-scope.txt`. *(R14)*

### Injected gates

- [ ] **G1** — Every artifact this branch authors is in English, per `CLAUDE.md`.
      *Evidence:* `arch-review` dimension 8's verdict, and `crates/guards/src/language.rs` green.
      → the arch-review record.
- [ ] **G2** — The four checks are green after rebasing on the latest `main`: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `cargo test --workspace`. Each run on its own, never chained behind a shell operator that can swallow a failure.
      *Evidence:* the four commands' output and exit codes, separately.
      → `.claude/evidence/reach-and-download-into-feed/checks.log`.
- [ ] **G3** — Performance impact declared. Every path this branch touches is classified by rate, as part of the plan: **`history_reach`'s campaign is `rare`** (one press of *load older*), **`replay_download` is `rare`** (one session export), and the consumer edits are compile-time path changes that author no call. Nothing per-trade, per-depth or per-frame is touched, so no measurement is owed and none is claimed.
      *Evidence:* this paragraph, restated in the PR body.
      → the PR body.
- [ ] **G4** — `arch-review` run over the branch diff against `origin/main`, with every Blocker and Should-fix resolved, or deferred in the PR body with its severity.
      *Evidence:* the `arch-review-ok` marker and the review's verdict.
      → the PR body.
- [ ] **G5** — `cargo test -p quantick-guards` green, and the `PostToolUse` guard-watch armed in this worktree before the first edit.
      *Evidence:* the command's output; `cargo build -p quantick-guards` already run here.
      → `.claude/evidence/reach-and-download-into-feed/checks.log`.

### Closing steps

- [ ] **C1** — `delivery-review` returns PASS.
- [ ] **C2** — The PR is open, naming the tier beside the four verification boxes.

## Not applicable, and why

- **Hot path evidence.** Injected only where a diff reaches a hot path. This one authors no call and changes no call shape; both moved modules are `rare`-rate by G3's classification. A measured fps comparison would be theatre over a diff that cannot move a frame.
- **`ui-harness`, `visual-qa`, `trader-ux-review`.** Nothing user-visible changes. The `QUANTICK_HISTORY_REACH*` hooks are declared app-side in `app.rs` and are untouched; `history_reach.rs`'s only hook mention is a doc comment, and it declares none.
- **`new-extension`.** No capability is added. A module changes crates; no port is created and none is docked against.
- **The second operator / drivable without a mouse.** No action, tool, trade or lock is added. The control-plane surface is unchanged, which A5 pins.
- **Test-first engine work.** No behaviour is authored. The tests travel with the files unchanged, which is the stronger guarantee here — a test that had to be rewritten would mean the move was not pure.
- **The docs/skills waiver.** Explicitly *not* claimed. This is a code change and takes the full shape pass; `AGENTS.md` riding along does not buy a waiver.

## The request as received

Quoted verbatim and in full, as the mission skill requires, so that no ask can
be lost in the act of writing the ledger above. Reproduced in the wording it
arrived in — this is the marked, attributed quotation `CLAUDE.md`'s English
rule exempts; every other line of this file is English.

> medium refactor/reach-and-download-into-feed — move the two remaining pure feed-side modules out of crates/app into the new quantick-feed crate: history_reach.rs (1,204 lines, the by-time reach and its campaign of history requests, depends on engine only) and replay_download.rs (872 lines, the session exporter driver, depends on feed::mt5_bridge and one config enum the feed crate already owns). A pure move: consumers change `use` paths only, the one reach into the app's documents folder is handed in as a parameter the way mt5_bridge already takes it, tests travel with the files, baselines tighten. Read C:\src\mission-reach-and-download-into-feed.md in full before anything else and build the request ledger from it.

The brief that invocation points at,
`C:\src\mission-reach-and-download-into-feed.md`, is the mission's second
source and is not reproduced here; it lives outside the repository. Its scope
list, acceptance criteria, out-of-scope list and evidence ledger are carried
above as R1–R15, and the one coupling it missed as R16.
