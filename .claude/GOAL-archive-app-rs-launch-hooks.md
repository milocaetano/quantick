# Mission: the launch hooks leave the constructor, the paper wiring leaves `app.rs`

Move the 715-line launch-hook sequence out of `new_with_workspace` into
`crates/app/src/app/launch_hooks.rs` as `apply_launch_hooks(&mut self)`, and
move the paper wiring into `crates/app/src/app/paper_wiring.rs`, leaving
`app.rs` at most 1,300 lines: the window's definition and nothing else.

**Why it matters.** A screener opens `app.rs` first. Today they find a 56-field
struct and a constructor whose second half is the ui-harness: 68 environment
reads that exist so an agent can open any surface without a click. That is a
strength of the repository, buried inside `new_with_workspace` where it reads
as startup logic. Given its own module and its own name it becomes the thing
`ui-harness/SKILL.md` can point at — "every launch hook is applied here, in
this order" — and the constructor becomes the 190 lines that build a window.

**Tier:** `medium`. One receiver rename across 715 lines carrying an order
invariant, plus a ~380-line pure move. Mechanical in kind, but the volume and
the ordering invariant put it past `small`: a silent reordering changes launch
behaviour and no compiler catches it. It is not `high` — no design decision is
open, the target shape is named in the brief, and nothing user-facing changes.

## Measurements taken on the branch before any edit

Against `origin/main` at `cc4c92f`, in this worktree:

- `crates/app/src/app.rs` is 1,961 lines; the size baseline entry is 1,932
  production lines.
- `new_with_workspace` spans `:494`-`:1425` (932 lines).
- The hook sequence spans `:689` (the comment "Dev/ops can open the map
  without a click", `QUANTICK_BOOK_AUTOSTART`) to `:1409` (the close of the
  `QUANTICK_TOAST` block) — 721 lines, 68 `QUANTICK_*` sites, 52 distinct
  names.
- The block reads no outer local: `config` appears only as a `crate::config::`
  path and in comments, `workspace` only as `app.workspace` and in comments,
  `consolidated` not at all. Every other reference is to `app`. So it becomes
  a method on `&mut self` taking no parameters.
- The paper wiring is `:1427`-`:1702` (less `attach_surface` at `:1703`) plus
  `arm_strategy_instance` at `:1803`-`:1904` — about 380 lines.
- Existing siblings under `crates/app/src/app/` total 9,247 lines across 15
  modules; the largest is `layout_wiring.rs` at 1,557.
- `.claude/skills/ui-harness/SKILL.md` is 13,045 bytes against a ceiling of
  13,377 — 332 bytes of slack.

### Two corrections to the brief's evidence ledger

- **Ledger #6 is stale by one item.** `carry_strategy_to_duplicate` is no
  longer in `app.rs`; the final cut (PR #303) moved it to
  `crates/app/src/app/replay_and_history.rs:463`. The paper wiring to move is
  the remaining nine functions. The brief's "about 330 lines" measures about
  380 because `adopt_tab` is larger than the brief's estimate.
- **Scope item 2's proof recipe does not work as written.** A plain
  `app.` → `self.` substitution misses three multi-line receivers where `app`
  ends a line and `.method` opens the next (`:1071`, `:1263`, `:1287`), while a
  broader word-boundary substitution on `app` would corrupt the 18
  `target: "quantick::app"` string literals inside the block. The transform
  actually applied, and the one the PR body will prove, is `app.` → `self.`
  plus those three bare receivers, with string literals untouched. See S4.

## Request ledger

Atomic asks decomposed from `C:\src\mission-app-rs-launch-hooks.md`, read in
full before any work.

| # | Ask |
| --- | --- |
| R1 | Move the launch-hook sequence out of `new_with_workspace` into `crates/app/src/app/launch_hooks.rs` as `pub(super) fn apply_launch_hooks(&mut self)` |
| R2 | Split that sequence by surface **in the same order** — about seven to ten functions, each under 150 lines, named for the surface group |
| R3 | The only textual change inside the block is the receiver rename; verbatim "The only textual change inside the block is `app.` → `self.`" |
| R4 | The module's doc comment states the order rule and that this is the ui-harness's application point |
| R5 | The constructor calls `apply_launch_hooks()` where the block was |
| R6 | Move the paper wiring into `crates/app/src/app/paper_wiring.rs` as a pure move; `git diff --color-moved=zebra` shows moves |
| R7 | `app.rs` ends at most 1,300 lines — verbatim "the window's definition and nothing else" |
| R8 | `new_with_workspace` ends at most 220 lines |
| R9 | The 52 launch names may move to a `declare_hooks!` in `launch_hooks.rs`; the generated registry is byte-identical either way |
| R10 | `ui-harness/SKILL.md` gains at most one line naming the module, paid within the context budget |
| R11 | Baselines tightened; both new files under 1,500 production lines |
| R12 | Hook names in file order, from the old block and from `launch_hooks.rs`, are identical 68-entry lists |
| R13 | Generated hook registry and capability inventory unchanged |
| R14 | `--report` before and after: only `app.rs`-related lines and the two new files move; `site.cfg_test` unchanged |
| R15 | The receiver-rename proof in the PR body; every other non-move hunk quoted and explained |
| R16 | Four-check loop green; `cargo test -p quantick-app` runs the same number of tests |
| R17 | The three ui-harness smoke hooks (`QUANTICK_LAYOUT`, `QUANTICK_CONTROL_PANEL`, `QUANTICK_REPLAY_AUTOSTART`) each still open their surface, evidenced in the PR body |
| R18 | Respect what is out of scope: the struct's 56 fields, what any hook does or parses or refuses, `new`, the `eframe::App` impl, the struct's doc comments |
| R19 | *(purpose, and the ask that judges the others)* So that a screener opening `app.rs` finds the window's definition, and the launch hooks become a named thing `ui-harness/SKILL.md` can point at |

## Decisions taken by the trader

- **D1 — smoke evidence is a control-plane readout, not an image hash.** For
  R17 the app is launched under each of the three hooks and asked, over the
  control plane, what it believes is on screen; the hash goes over that
  deterministic state readout, which a reviewer can reproduce. The three
  screenshots ship alongside as visual attachment, unhashed. *(Asked because a
  PNG hash varies with the clock, the live tape and font rasterisation, so the
  number the brief asked for would prove nothing to a reviewer.)*

## Assumptions

- **S1 — the `declare_hooks!` slice moves to `launch_hooks.rs`.** R9 leaves
  this open and names the feed crate's shape (the read and its declaration in
  one file) as the precedent. Safe to assume: the generated registry is
  byte-identical either way, guarded crate-wide, and reversing it is one cut
  and paste.
- **S2 — `attach_surface` stays in `app.rs`.** It sits between the paper
  wiring functions but belongs to the window, not the paper surface, and R7
  keeps the window's definition in `app.rs`.
- **S3 — the paper wiring moves as the nine functions that remain**, since
  `carry_strategy_to_duplicate` already left. Delivering the brief's tenth name
  is impossible; moving the nine is what the ask means.
- **S4 — the receiver rename is `app.` → `self.` plus the three bare
  multi-line receivers, with string literals untouched.** R3's intent is "the
  receiver changes and nothing else"; the substitution recipe was a proof
  sketch, not the specification. The PR body will state the exact transform
  and show the normalised diff empty.
- **S5 — the surface split follows the brief's own grouping** (book/live-strip,
  control, rail/drawing, history, tape/bubbles, indicators, layout, replay,
  dock and report, workspace, toast), which is the file order already, so the
  split introduces no reordering.
- **S6 — `ui-harness/SKILL.md`'s one line is paid from the 332 bytes of
  existing slack**, with no deletion, unless the line exceeds it. *(Wanted to
  ask: the brief predicted no slack and asked for a replacement; the branch's
  own measurement says a line fits. Recorded rather than asked because
  measurement, not preference, decides it.)*
- **S7 — no `pub(super)` widening is expected**, since child modules see the
  parent's private fields; any free item left behind and called from a moved
  body gets `pub(super)` as the sibling precedent does.

## Acceptance criteria

- [ ] **A1** — `crates/app/src/app/launch_hooks.rs` exists, holding
      `pub(super) fn apply_launch_hooks(&mut self)` which calls one function
      per surface group in the block's original order; each is under 150
      lines and there are between seven and ten. *Evidence:* the file, plus
      `grep -c 'fn '` and a per-function line span table. → PR body. *(R1, R2)*
- [ ] **A2** — The hook names extracted in file order with `grep -o
      'QUANTICK_[A-Z_0-9]*'` from the pre-change block and from
      `launch_hooks.rs` are identical 68-entry lists. *Evidence:* `diff` of the
      two lists, empty, with both counts shown. → PR body. *(R2, R12)*
- [ ] **A3** — Normalising the pre-change block by the receiver rename of S4
      diffs empty against the concatenated new function bodies, modulo
      function boundaries. *Evidence:* the normalising command and its empty
      `diff` output; every non-move hunk in the whole diff quoted and
      explained. → PR body. *(R3, R15)*
- [ ] **A4** — `launch_hooks.rs` carries a module doc comment stating the
      order rule (that replay day-before is read before anything loads a
      session, and the active layout lands on the first tab's panes before any
      autostart) and naming the module as the ui-harness's application point.
      *Evidence:* the doc comment quoted. → PR body. *(R4)*
- [ ] **A5** — `new_with_workspace` calls `apply_launch_hooks()` at the point
      the block occupied, and is at most 220 lines. *Evidence:* the call site
      and the function's measured line span. → PR body. *(R5, R8)*
- [ ] **A6** — `crates/app/src/app/paper_wiring.rs` holds the nine remaining
      paper-wiring functions as a pure move. *Evidence:* `git diff
      --color-moved=zebra` showing moves, and a line-multiset comparison of
      the removed and added regions. → PR body. *(R6)*
- [ ] **A7** — `crates/app/src/app.rs` is at most 1,300 lines and contains
      only the module's declarations and consts, the struct, `new`,
      `new_with_workspace`, `attach_surface`, the `eframe::App` impl and `mod
      tests`. *Evidence:* `wc -l` and the file's full `fn` inventory. → PR
      body. *(R7, R19)*
- [ ] **A8** — The generated hook registry and capability inventory are
      byte-identical to `origin/main`. *Evidence:* `git diff origin/main --
      <generated files>` empty, and `cargo test -p quantick-guards` green. → PR
      body. *(R9, R13)*
- [ ] **A9** — `.claude/skills/ui-harness/SKILL.md` gains at most one line
      naming `launch_hooks.rs`, and the context ratchet passes. *Evidence:* the
      diff of that file and the guards run's context section. → PR body. *(R10)*
- [ ] **A10** — Size and context baselines tightened; `launch_hooks.rs` and
      `paper_wiring.rs` are each under 1,500 production lines. *Evidence:* the
      baseline diff and `--report` line counts. → PR body. *(R11)*
- [ ] **A11** — `cargo run -p quantick-guards -- --report` before and after
      differ only in `app.rs`-related lines and the two new files;
      `site.cfg_test` is unchanged. *Evidence:* `diff` of the two reports. → PR
      body. *(R14)*
- [ ] **A12** — `cargo test -p quantick-app` runs the same number of tests as
      on `origin/main`, all passing. *Evidence:* both run summaries, counts
      quoted. → PR body. *(R16)*
- [ ] **A13** — Launching under `QUANTICK_LAYOUT`, `QUANTICK_CONTROL_PANEL`
      and `QUANTICK_REPLAY_AUTOSTART` each still opens its surface, proven by
      the control plane's own readout of what the app believes is on screen,
      hashed; three screenshots attached unhashed. *Evidence:* the three
      readouts with their hashes, and the screenshot paths. → PR body. *(R17,
      D1)*
- [ ] **A14** — Nothing out of scope changed: the struct's fields and doc
      comments, every hook's behaviour, parsing and refusal messages, `new`
      and the `eframe::App` impl are untouched except for the receiver rename
      and the block's excision. *Evidence:* the diff, with the `fn`-level
      inventory of what moved and what did not. → PR body. *(R18)*
- [ ] **G1** — Every artifact this branch authors is in English, per
      `CLAUDE.md`. *Evidence:* `arch-review` dimension 8 verdict and
      `cargo test -p quantick-guards` green. → PR body.
- [ ] **G2** — After rebasing on latest `main`, all four checks green run
      individually — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace`;
      performance impact declared; `arch-review` run with every Blocker and
      Should-fix resolved or deferred in the PR body. *Evidence:* each
      command's own exit status, the rate classification of every touched
      path, and the review verdict. → PR body.

## Performance impact, declared

Every path this branch touches is **rare** — launch-time, executed once per
process before the first frame. The launch-hook block moves from the
constructor's body into a method called from the same point in the same order;
the paper wiring's functions keep their bodies and their callers. No per-trade,
per-depth or per-frame path is touched, and the change adds one non-inlined
call per process. No measurement of a hot path is therefore owed under the
injected table's hot-path row; the branch will still show `--report` and the
test counts unchanged.

## Injected gates that do not apply, and why

- **Touches a hot path** — no. Launch-time only, as declared above. No
  `APP_HEALTH_SUMMARY` control run is owed.
- **Touches anything user-visible** — no surface is added or changed; every
  hook keeps its behaviour, parsing and refusal messages (R18). `visual-qa` and
  `trader-ux-review` are therefore not owed. A13's three smoke launches are a
  *regression* check that the moved hooks still reach their surfaces, not a
  visual review; they discharge R17, not this row.
- **Adds a capability** — no feed, bar type, indicator, layer, panel or crate
  is added. Two modules appear, both holding code that already existed.
  `new-extension` does not apply to a move.
- **Adds something a trader does** — no new action, tool, trade or lock. The 68
  launch hooks are already operable without a mouse and stay so, by A13.
- **Engine / determinism territory** — no. This is `crates/app`, above the
  headless line; no engine code is touched, so the test-first rule does not
  bind. The behaviour-preservation proof is A3 and A12 instead.
- **Docs/skills only** — no. One skill line changes (A9), but the branch is
  code, so the full shape pass applies with no waiver.

## Corrections found while doing the work

Two criteria above were written from the brief's expectations and the work
proved both wrong. The criteria are left as written -- rewriting a criterion to
match the result is grading yourself -- and the true outcome is recorded here
and in the PR body.

- **A8 is not achievable as stated, and the brief's ledger #5 is wrong.** The
  brief says "the generated registry is byte-identical either way". It is not.
  `crates/app/src/hooks.rs` holds an `OWNERS` table naming *the file a reader
  should open to find each hook*, and a guard checks that the file registering
  a slice is the file that reads the `QUANTICK_*`. The reads moved, so the
  declaration had to move with them -- this is forced, not the free choice S1
  assumed -- and the generated registry's *Declared in* column changed in
  exactly 50 rows, `app.rs` to `app/launch_hooks.rs`, +650 bytes. The hook
  *names* are byte-identical, which is the half that matters: 68 sites, 52
  distinct, same list in the same order (A2). The +650 is paid for inside the
  context budget by cutting the same number of bytes of history and reasoning
  from `ui-harness/SKILL.md`, signed in `context-baseline.txt`.
- **A11's "`site.cfg_test` unchanged" is off by three.** It went 622 -> 625.
  The moved code took the last *production* read of five names with it, and
  `app::tests` still reaches them through `use super::*`; `app.rs` already
  documents this exact case and answers it with `#[cfg(test)]`-gated imports
  rather than edits to `app/tests/`. Three attributes were added on that
  precedent, one removed. Every other line of the report moved only for
  `app.rs` and the two new files, as the criterion asked.

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS (at `medium`, the completeness
      pass, inline).
- [ ] **C2** — The PR is open, naming the tier beside the four verification
      boxes.

## The request as received, verbatim

> *(Quoted in full and attributed, under `CLAUDE.md`'s marked-quotation
> exemption. The mission's own language is English.)*

> medium refactor/app-rs-launch-hooks — after the final cut,
> crates/app/src/app.rs is the struct, a 912-line `new_with_workspace`, the
> paper wiring and the `eframe::App` impl. The constructor is two things: 190
> lines that build the window, then 715 lines that read 68 `QUANTICK_*` launch
> hooks (52 distinct) and apply them to the built app, one `if let Ok(..) =
> std::env::var(..)` block after another, from `QUANTICK_BOOK_AUTOSTART` to
> `QUANTICK_TOAST`. Move that sequence into crates/app/src/app/launch_hooks.rs
> as `apply_launch_hooks(&mut self)`, split by surface in the same order, with
> `app.` becoming `self.` and nothing else changing; move the paper wiring (the
> trades-dir pickers, `persist_*`, `adopt_tab`, `arm_strategy_instance`,
> `carry_strategy_to_duplicate`, about 330 lines, free since PR #301) into
> app/paper_wiring.rs as a pure move. `app.rs` ends at most 1,300 lines: the
> window's definition and nothing else. Read
> C:\src\mission-app-rs-launch-hooks.md in full before anything else and build
> the request ledger from it.

The mission brief at `C:\src\mission-app-rs-launch-hooks.md` is the request's
long form; its evidence ledger, scope list, acceptance criteria and
out-of-scope list are decomposed above as R1-R19, with the two corrections
recorded under *Measurements taken on the branch before any edit*.
