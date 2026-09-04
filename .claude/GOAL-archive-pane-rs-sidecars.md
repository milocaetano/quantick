# Mission: `pane.rs` sheds its sidecars

Move four cohesive groups out of `crates/app/src/pane.rs` into sibling modules
under `crates/app/src/pane/`, bodies unchanged and method names kept, so the
file drops from 7,771 production lines to under 5,700.

**Why it matters.** `pane.rs` is the largest file in the repository: 77 fields
and one `impl ChartPane` block running from `:1395` to the end. Every chart bug
— a hit-test, a menu entry, an axis label — costs a read through navigation and
drawing code it does not touch. The two functions that *are* the pane
(`handle_navigation`, 961 lines; `draw_chart`, 939) stay for the mission that
gives them a shape; this one takes the groups around them so that mission reads
5,500 lines instead of 7,800.

**Tier:** `medium`. A ~2,100-line mechanical move with a hard number to hit
(`pane.rs` ≤ 5,700, budget down ≥ 2,000) and one judgement call the brief
delegates (`interact_shared`). No new behaviour, no new surface, nothing a
capture could see — so it does not earn `high`; but it is far past the `small`
diff ceiling, and "did it actually move everything that was asked for" is
exactly the question `delivery-review` exists to answer.

## Request ledger

The mission brief is `C:\src\mission-pane-rs-sidecars.md`, read in full before
any work; its evidence ledger #1–#10 is the source of every line number below.

| # | Ask |
| --- | --- |
| **R1** | Move the **context menus** group, `pane.rs:2042-2357` (`layer_checkbox`, `draw_chart_layer_entries`, `draw_tape_menu_section`, `draw_layer_menu`, `draw_drawing_menu_section`), into a sibling module. |
| **R2** | Move the **strategy badges and their lifecycle**, `:2378-2709` (`badge_text_for` through `remove_strategy_for_drawing`). |
| **R3** | Move the **drawing gestures**, `:3332-3397` and `:3557-4241`. |
| **R4** | Move the **axes and chrome painters**, `:6173-6455` and `:7274-7770`. |
| **R5** | The modules are siblings under `crates/app/src/pane/`, "the way `crates/app/src/app/` holds QuantickApp's sidecars" — `use super::…` plus `impl ChartPane` blocks, declared by `mod` lines beside the existing test module. |
| **R6** | **Bodies unchanged.** `git diff --color-moved=zebra` shows moves, not edits; every non-move hunk is quoted and explained in the PR body. |
| **R7** | **Method names kept.** |
| **R8** | **Ceiling tightened**: `pane.rs` at most 5,700 lines, its baseline entry lowered to the new size via `--tighten`. |
| **R9** | **Budget lowered**: the size `!budget` line down by at least 2,000. |
| **R10** | Every new file stays under the 1,500 production-line threshold — "split rather than sign" a raise. |
| **R11** | Free functions that travel are re-exported per brief ledger #8; the mission picks one approach and applies it everywhere, and the PR body lists every `pub(super)` added and why. |
| **R12** | Read `interact_shared` (`:3409-3546`) and say which side it belongs to. |
| **R13** | Generated hook registry and capability inventory unchanged — nothing to move (brief ledger #7); `cargo test -p quantick-guards` green and the generated files' diff empty. |
| **R14** | `pane/tests/mod.rs` changes only imports, if at all; `app/tests/panes_layout_tests.rs` and `drawings_tests.rs` change nothing. |
| **R15** | `cargo run -q -p quantick-guards -- --report` before and after, diffed: only `pane.rs`-related lines and the new files move. |
| **R16** | The four-check loop green, and `cargo test -p quantick-app` runs the same number of tests. |
| **R17** | Verify each of the brief's evidence-ledger claims #1–#10 against the tree before acting, rather than trusting them. |
| **R18** | Respect the out-of-scope list: `handle_navigation` `:4254-5214` and `draw_chart` `:5223-6161` untouched; `ChartPane`'s 77 fields untouched; no change to hit-testing, magnet or placement arithmetic; no behaviour a capture could see; the gesture helpers of brief ledger #10 (`pane_divider_gesture`, `pane_pan_gesture`, `axis_zoom_gesture`) and `draw_dashed_vertical` stay. |
| **R19** | Re-run the `refactor/paper-policy-out-of-the-ticket` diff before the PR (brief ledger #6) to confirm the one line it edits, `:5996`, is still inside code that does not move. |
| **R21** | Respect the other parallel branch, `refactor/app-rs-workspace-and-indicators` (proposed): a different file, meeting this mission only at the `!budget` line of `size-baseline.txt` — "whichever lands second re-runs `--tighten`". |
| **R20** | *Purpose, and the ask that judges the others:* the next `pane` mission — the one that gives `handle_navigation` and `draw_chart` a shape — opens a file of ~5,500 lines, not 7,800, with the four groups gone from around them. |

## Decisions taken by the trader

None. Nothing in the brief met the bar for a question at this tier: the four
judgement calls it leaves open are explicitly delegated to the mission, and no
reading of any of them throws away work. They are recorded as `S1`–`S4`.

## Assumptions

- **S1** — *(wanted to ask; brief ledger #4 delegates it explicitly)*
  `interact_shared` (`:3409-3546`, 138 lines) **stays in `pane.rs`**. It is
  cross-pane shared-mark interaction, not a drawing gesture, and its only caller
  is `handle_navigation` (`:4791`), which stays. Safe rather than asked: the
  brief instructs the mission to read it and decide, and both readings cost the
  same work — the drawing group moves as two ranges either way. Keeping it out
  also keeps R10 comfortable.
- **S2** — *(wanted to ask)* The two painter ranges of R4 become **one file**,
  not two. At roughly 709 production lines it sits well under the 1,500
  threshold, and the crosshair, compass and axis-tag painters read the same
  price/time axis machinery the strip and the gutter do. Safe: the brief offers
  the split as an option ("may become two files if the mission judges…"), not a
  requirement, and a later split is one file move.
- **S3** — *(wanted to ask)* On R11, the rule picked and applied everywhere is
  **free functions do not travel**. `paint_placement_hint`, `snap_bar_to_tape`
  and `magnet_price_of` (`:7702-7770`) sit at module scope *outside* the
  `impl ChartPane` block this mission moves; `snap_bar_to_tape` and
  `magnet_price_of` are called from R3's group, and `paint_placement_hint` from
  code that stays (`:7266`). Leaving all three in `pane.rs` costs **zero**
  re-exports and **zero** widened visibility — a child module sees its
  ancestor's private items — and leaves `pane/tests/mod.rs` untouched. Safe:
  brief ledger #8 asks the mission to pick one rule and apply it everywhere;
  this is that rule, and it is the one that changes fewest lines.
- **S4** — Module names are the brief's own: `menus.rs`, `strategy_badges.rs`,
  `drawing_gestures.rs`, `axes_and_chrome.rs`. Conventional default; the brief
  says names are the mission's to adjust, and none needed adjusting.
- **S5** — R15's "before" `--report` is captured from `origin/main` in this
  worktree before the first edit, and both reports are written into the
  branch's evidence directory, so the diff is reproducible rather than recalled.

## Amendment, after the delivery review

`R21` and `A19` were added after `delivery-review`'s completeness pass found
the brief's second "Parallel work to respect" bullet carried by no `R` line:
the ledger had taken the paper branch (`R19`) and dropped the
`app-rs-workspace-and-indicators` one. The numbers are stable — nothing was
renumbered — and the gap is recorded here rather than quietly closed, because
an ask that reached the branch without reaching the ledger is the one failure
the rest of the pipeline is blind to.

## Acceptance criteria

- [ ] **A1** — The context-menu group is gone from `pane.rs` and present, bodies
      identical, in a sibling module under `crates/app/src/pane/`.
      *Evidence:* `grep -c` for each of the five method names showing one
      definition, in the child; the `--color-moved=zebra` hunks.
      → `.claude/evidence/pane-rs-sidecars/moves.md` *(R1, R5, R7)*
- [ ] **A2** — Same for the strategy badges and lifecycle group.
      *Evidence:* as A1, over `badge_text_for` … `remove_strategy_for_drawing`.
      → same file *(R2, R5, R7)*
- [ ] **A3** — Same for the drawing gestures group, both ranges, with
      `interact_shared` left behind in `pane.rs`.
      *Evidence:* as A1, plus `grep -n interact_shared crates/app/src/pane.rs`
      showing the definition still there. → same file *(R3, R5, R7, R12)*
- [ ] **A4** — Same for the axes and chrome painters group, both ranges, minus
      the three module-scope free functions of S3.
      *Evidence:* as A1. → same file *(R4, R5, R7)*
- [ ] **A5** — **Bodies unchanged.** `git diff --color-moved=zebra
      origin/main...HEAD -- crates/app/src` classifies every moved line as a
      move; each remaining non-move hunk is quoted with the reason it exists.
      *Evidence:* the zebra diff's non-move hunks, quoted in full.
      → `.claude/evidence/pane-rs-sidecars/moves.md` and the PR body *(R6)*
- [ ] **A6** — `wc -l crates/app/src/pane.rs` ≤ 5,700, and
      `crates/guards/size-baseline.txt`'s `pane.rs` entry equals its new
      production-line count.
      *Evidence:* the `wc -l` output and the baseline diff.
      → `.claude/evidence/pane-rs-sidecars/numbers.md` *(R8)*
- [ ] **A7** — The `!budget` line in `size-baseline.txt` is at least 2,000 lower
      than on `origin/main`. *Evidence:* the baseline diff, both values quoted.
      → same file *(R9)*
- [ ] **A8** — Every new module under `crates/app/src/pane/` is under 1,500
      production lines, and no baseline ceiling was raised to make room.
      *Evidence:* `--report`'s per-file lines for the new files, and the
      baseline diff showing no raised entry. → same file *(R10)*
- [ ] **A9** — The rule of S3 is stated and applied everywhere; the PR body lists
      every `pub(super)` added, or states that none was needed and why.
      *Evidence:* `git diff origin/main...HEAD | grep 'pub(super)'`, quoted.
      → `.claude/evidence/pane-rs-sidecars/moves.md` and the PR body *(R11)*
- [ ] **A10** — `interact_shared`'s side is decided in writing, with the reason.
      *Evidence:* the S1 entry above, and the PR body's paragraph on it. *(R12)*
- [ ] **A11** — The generated hook registry and capability inventory are
      byte-identical to `origin/main`.
      *Evidence:* `git diff origin/main...HEAD --stat` over the generated paths
      showing no entry, and `cargo test -p quantick-guards` green.
      → `.claude/evidence/pane-rs-sidecars/numbers.md` *(R13)*
- [ ] **A12** — `crates/app/src/pane/tests/mod.rs` shows no diff, or a diff
      confined to `use` lines; `crates/app/tests/panes_layout_tests.rs` and
      `crates/app/tests/drawings_tests.rs` show no diff at all.
      *Evidence:* `git diff origin/main...HEAD --stat` over those three paths.
      → same file *(R14)*
- [ ] **A13** — The before/after `--report` diff contains only `pane.rs`'s own
      numbers and the new files' entries.
      *Evidence:* both reports saved, and their `diff` quoted in full.
      → `.claude/evidence/pane-rs-sidecars/report-before.txt`,
      `report-after.txt`, `report.diff` *(R15)*
- [ ] **A14** — `cargo test -p quantick-app` reports the same number of tests as
      on `origin/main`, all passing.
      *Evidence:* both runs' test-count lines, quoted side by side.
      → `.claude/evidence/pane-rs-sidecars/numbers.md` *(R16)*
- [ ] **A15** — Each of the brief's evidence-ledger claims #1–#10 was checked
      against the tree, with the check recorded and any correction noted.
      *Evidence:* a claim-by-claim table naming the command that verified each.
      → `.claude/evidence/pane-rs-sidecars/ledger-check.md` *(R17)*
- [ ] **A16** — Nothing out of scope moved: `handle_navigation` and `draw_chart`
      remain in `pane.rs`, `ChartPane`'s field list is untouched, and the three
      gesture helpers of brief ledger #10 plus `draw_dashed_vertical` stay.
      *Evidence:* `grep -n` for each of those symbols in `pane.rs` after the
      move, and `git diff` over the `struct ChartPane` block showing no change.
      → `.claude/evidence/pane-rs-sidecars/numbers.md` *(R18)*
- [ ] **A17** — The paper branch's diff, re-run before the PR, still touches
      `pane.rs` at exactly one line, inside code this branch did not move.
      *Evidence:* the re-run diff, quoted, with the line's post-move location.
      → same file *(R19)*
- [ ] **A18** — The next `pane` mission's read is measurably smaller: `pane.rs`
      under 5,700 lines with all four groups gone, and the two hot functions
      contiguous and intact within it.
      *Evidence:* `wc -l` plus the line ranges of `handle_navigation` and
      `draw_chart` after the move. → same file *(R20)*

- [ ] **A19** — The `!budget` coordination with
      `refactor/app-rs-workspace-and-indicators` is settled in writing: which
      branch lands first, and who therefore owes the re-run of `--tighten`.
      *Evidence:* `git log --oneline origin/main..refactor/app-rs-workspace-and-indicators`
      showing whether it is ahead, and the resulting statement in the PR body.
      → `.claude/evidence/pane-rs-sidecars/numbers.md` and the PR body *(R21)*

### Injected gates

- [ ] **G1** — Every artifact on this branch is in English — code, comments,
      commit messages, PR title and body, and the evidence files.
      *Evidence:* the `arch-review` dimension 8 verdict, and
      `cargo test -p quantick-guards` (which runs `language.rs`) green.
      → the arch-review transcript and the PR body.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own,
      never chained behind `|| echo`.
      *Evidence:* the four commands' output and exit codes, quoted.
      → `.claude/evidence/pane-rs-sidecars/four-checks.md` and the PR body.
- [ ] **G3** — Performance impact declared. Expected: none. Every touched path is
      a code move with bodies unchanged, so the per-frame drawing and
      per-gesture paths keep identical instruction sequences and only their
      source location changes.
      *Evidence:* the classification written in the PR body, with the
      `--color-moved` result as its warrant. → the PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, with every
      Blocker and Should-fix resolved or deferred in the PR body.
      *Evidence:* the review's verdict and the `arch-review-ok` marker.
      → the PR body.

### Not applicable, and why

- **Touches a hot path** (measured evidence that performance is flat or better):
  the *touched paths* are hot — `draw_chart`'s painters run per frame — but
  nothing this mission does changes an instruction. A capture run would measure
  the compiler, not the branch. G3 declares the impact; a measured comparison
  would be theatre. If a single non-move hunk turns out to change a body, this
  row stops being N/A.
- **Touches anything user-visible** (`ui-harness`, `visual-qa`,
  `trader-ux-review`): no surface is new or changed. The menus, badges and
  painters that move draw the same pixels from the same code. Brief ledger #7:
  `pane.rs` declares and reads no `QUANTICK_*` hook, so no harness hook is owed.
- **Adds a capability** (`new-extension`): nothing docks. This is subtraction
  from one file into siblings of it — not a new feed, bar type, indicator, layer
  or panel.
- **Adds something a trader does** (`arch-review`'s *The second operator*): no
  new act, tool, trade or lock. Every method keeps its name and its caller.
- **Engine / determinism, test-first**: `crates/app` is not the engine, and the
  mission writes no new behaviour to test first. The existing `pane` tests are
  the golden here — A14 requires the same count, green.
- **Docs/skills only**: not applicable in the other direction. This is a code
  change, so the full shape pass applies and nothing is waived.

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS.
- [ ] **C2** — The PR is open, naming the tier, with CI green.

## The request as received

Quoted verbatim, in the trader's own words, as this skill requires: the ledger
above has to be auditable against the words it was built from, not against a
paraphrase. The request was written in English, so no exemption is in play.

> medium refactor/pane-rs-sidecars — crates/app/src/pane.rs is 7,771 production
> lines in one `impl ChartPane` block that runs from line 1,395 to the end; PR
> #288 moved its tests out and nothing has moved since. Four cohesive groups lie
> away from the two 950-line hot functions and from the one line the open paper
> branch edits: the context menus (`pane.rs:2042-2357`), the strategy badges and
> their lifecycle (`:2378-2709`), the drawing gestures (`:3332-3397` and
> `:3557-4241`), and the axes and chrome painters (`:6173-6455` and
> `:7274-7770`), about 2,180 lines. Move them into sibling modules under
> crates/app/src/pane/, the way crates/app/src/app/ holds QuantickApp's
> sidecars. Bodies unchanged, method names kept, ceiling tightened, budget
> lowered. Read C:\src\mission-pane-rs-sidecars.md in full before anything else
> and build the request ledger from it.

The brief that invocation points at, `C:\src\mission-pane-rs-sidecars.md`, is
the request's full text: a file outside the repository, read in full before any
work, and every line number in the ledger above comes from its evidence table.
Its own summary of the tier reads *"Tier `medium`: a 2,200-line move with a hard
number to hit and one judgement call (`interact_shared`)."*
