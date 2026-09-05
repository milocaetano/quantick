# Mission: the paper-trading policy leaves the ticket

Split `crates/app/src/paper_trading.rs` into a headless `PaperAccount` module
and an egui-only ticket, with behaviour pinned byte for byte by a journal
golden and pixel-identical harness captures.

**Why it matters.** It is the largest file in the repository and the one an
auditor asking *was the stop placed at the right price?* has to read through
drawing code to answer. The size baseline already records one attempt at this
seam that stopped short — "moving them would mean widening five private items".
The report extraction (PR #282) proved the shape that works: an environment the
half is handed, a response for what it cannot do, one state struct, and the
wrappers that keep every control-plane and harness name where callers look.

**Tier:** `high`. It is the money path — placement, brackets, risk against
capital, the trades journal. It re-opens a seam an earlier branch judged too
expensive. And two of its acceptance criteria are goldens that must exist
*before* the code moves, which only works if the ceremony is planned up front
rather than reconstructed at review time. It is not `max` only because the
change is behaviour-preserving by construction: nothing a trader sees may
differ, and the pixel criterion proves it.

## Request ledger

| # | Ask |
| --- | --- |
| R1 | Create `crates/app/src/paper_account.rs` holding `PaperAccount`: the headless fields (venue, symbol, dir, journal paths, session sources, cmd_trading, armed, strategies, selected_strategy, risk, capital, instrument_money, hook_money, risk_from_hook, export_rx, import_rx, bot_listening, bot_events, demo, report) and every function whose body needs no ticket field and no egui type |
| R2 | Move placement to the account: `place_intent`, `market`, `place_resting`, `amend_rung`, `reverse_position`, `settle` |
| R3 | Move the event path to the account: `on_trade`, `handle_events`, `rest_capture_orders`, `decide_pending_leg`, `position_bracket`, `bracket_owners` |
| R4 | Move journal and sessions to the account: `with_trades_dir`, `set_trades_dir`, `set_symbol`, `journal`, `start_export`, `poll_export`, `start_import`, `poll_import` |
| R5 | Move the settings and risk accessors, the summaries the control plane reads, and the demo stepper to the account |
| R6 | Read `parse` and say which side it belongs on — the brief leaves the call to the mission |
| R7 | `paper_trading.rs` keeps the ticket: every `draw_*`, `bracket_handle`, `position_tag`, `chip_tag`, `control_at`, `ticket_bracket`, `handle_chart_input` (the gesture path, even though it is egui-free), the ten `*_text` buffers, the five `ruler_*`, drag, hover, the toast outbox. It owns one `PaperAccount` |
| R8 | The file name and every harness name stay. A rename to `paper_ticket.rs` only if the mission judges the churn worth it |
| R9 | The seam takes the report's shape: the account is handed an `AccountEnv` (tick size, mark price, clock as told, documents dir) and answers with an `AccountResponse` (toast text, a journal event, a refusal) |
| R10 | The account never reaches `show_toast`, the ticket's state, or any egui value |
| R11 | The five private items the baseline comment names — `aim_bracket`, `quantity_preview`, `parse_quantity`, `show_toast`, `venue` — each end up on exactly one side, and the PR body says which and why |
| R12 | `control/{trade,session,interaction}.rs` reach the account through the same method names |
| R13 | `app.rs`, `tab.rs`, `pane.rs`, `dock.rs`, `toolbar.rs` change only paths and receivers |
| R14 | Tests travel by side: the ten that write a real journal and every test of placement, fills, risk and export go to the account's test module; gesture, tag, preview and draw tests stay |
| R15 | `app/tests/paper_trading_tests.rs` changes only in names |
| R16 | Baselines: run `--tighten`; if the seam costs lines, sign the budget raise in the entry with the number, as the report's was |
| R17 | Journal golden: a fixed tape of venue events and intents, written as a test against the code *before* the split and committed on its own, produces a journal whose SHA-256 is recorded in the PR body; the same test passes unchanged after |
| R18 | Pixels golden: captures through the nine ledger-#9 hooks, taken on `origin/main` and on the branch under identical env, are pixel-identical; image hashes side by side |
| R19 | `grep -nE 'egui\|eframe\|Painter\|Color32\|Pos2' crates/app/src/paper_account.rs` returns nothing |
| R20 | `control/{trade,session,interaction}.rs` name no ticket-side item — the second operator reads policy, not pixels |
| R21 | `paper_trading.rs` at most 3,500 production lines and `paper_account.rs` at most 3,500; the size `!budget` rises by no more than 300, signed, or falls |
| R22 | `cargo test -p quantick-app paper` runs the same number of tests as before, all green |
| R23 | `arch-review` (bugs and shape), `delivery-review`, and `visual-qa` on the pixel criterion run; `trader-ux-review` is waived |
| R24 | Respect the deliberate out-of-scope list: no move into `sim` or `strategy`; `paper_report.rs`, `risk_sizing.rs`, `order_strategies.rs` touched only on path lines; no change to fill rules, bracket arithmetic, journal format or toasts; `pane.rs` and `tab.rs` beyond receivers |
| R25 | Respect the parallel work, `fix/tests-own-their-scratch` above all |
| R26 | **The purpose, and the ask that judges the others:** the money path reads without egui — an auditor asking whether a stop was placed at the right price answers it without reading drawing code |
| R27 | Verify each evidence-ledger claim before acting rather than trusting it |

### Ledger corrections from R27

Measured against `origin/main` at `d551813`, the sha the brief names:

- **Claim 1 holds.** 9,718 lines; the inline test module opens at `:6044`;
  nine `impl` blocks with the struct's own at `:958`.
- **Claim 2 is directionally right, numerically off.** Re-measured by function
  body over the 6,043 production lines: 2,284 egui-free against 2,049
  egui-bound, not 3,032 against 2,530. The remaining lines are struct
  definitions, `impl` headers and free items outside any function. The claim
  the mission rests on — that about half the production lines never touch
  egui — holds.
- **Claim 3 names a `parse` of 220 lines that does not exist.** The file has
  three `parse` functions, of 8, 8 and 22 lines, at `:488`, `:559` and `:733`.
  `label` is 7 and 3 lines, not 51; `entry_label` 37, not 41; `poll_export` 35,
  not 50; `decide_pending_leg` 36, not 45; `position_bracket` 38, not 44. The
  large egui-free functions the mission actually has to place are
  `handle_chart_input` 162, `with_trades_dir` 123, `handle_events` 97,
  `rest_capture_orders` 82, `start_export` 55, `set_symbol` 45, `journal` 44
  and `run_demo_step` 43 — plus `draw_bracket_leg` 72, `draw_bracket_of` 66,
  `draw_ladder_rungs` 62 and `draw_aim_bracket` 49, which pass the egui grep
  and are the ticket's, exactly as claim 3 warns.
- **Claim 4 counts 54 fields, not 55.** The two lists are otherwise as stated.
- **Claim 8 reads 6,407 in the baseline, not 6,396.**
- **Claim 11 understates the risk:** `fix/tests-own-their-scratch` has **no PR
  open** — `gh pr list` shows only #225 — so "it is close" cannot be relied on.
  D1 settles it.
- `venue` is a **field**, not a function; the baseline's "five private items"
  mixes one field with four functions. R11 is graded on all five regardless.
- **Claim 10 names the wrong path.** The 61 tests are at
  `crates/app/src/app/tests/paper_trading_tests.rs` — an inline module under
  `src/`, not an integration test at `app/tests/`. `quantick-app` is
  binary-only, so it has no `tests/` directory at all. R15 is graded on that
  real path; S4 revises where the golden lives because of it.

## Decisions taken by the trader

- **D1 — Overlap with `fix/tests-own-their-scratch`.** Cut from `origin/main`
  and carry *that branch's* version of any test it edits when the test moves to
  the account's module. This PR stays independently mergeable; the scratch
  branch resolves the remainder in its own rebase. Not a stacked PR, and not a
  wait on a branch with no PR.

  **Amended once the branch was read, intent unchanged.** Carrying its version
  verbatim is not possible: its 33 lines in `paper_trading_tests.rs` and 87 in
  `paper_trading.rs` call a `crate::scratch::ScratchDir` and a
  `crate::scratch::thread_dir` that do not exist on `origin/main` — they arrive
  in a new `crates/app/src/scratch.rs` of 352 lines, part of a 50-file, 1,648
  insertion change. Vendoring that into this branch would import another
  branch's work wholesale and collide at merge, which is exactly the stacking
  D1 rejected. So the operative rule becomes: **move the tests it edits in
  their `origin/main` form, byte for byte**, changing not one line of a body it
  touches, so that its diff still applies at the new path with a path-only
  conflict. The intent D1 chose — independently mergeable, not stacked, not
  waiting — is preserved exactly; only the mechanism changes, because the one
  D1 named does not exist. A13 is graded on this reading.
- **D2 — The seam's shape.** The ticket owns the `PaperAccount` and exposes it:
  `pub(crate) fn account(&self)` and `account_mut(&mut self)`. The control
  plane writes `tab.paper.account().working_orders()`. Chosen over delegating
  wrappers, which would leave `control/*.rs` still naming `PaperTrading` and
  make R20 arguable, and over a sibling `Tab` field, which would push `tab.rs`
  and `pane.rs` past receivers and break R24.
- **D4 — How "no trader-visible change" is proved, after the criterion was
  measured and found unmeasurable.** Before writing a line of the split, the
  nine hooks were captured twice on the *same* `origin/main` build, with
  identical env, a scratch store per scene and a recorded tape. **Zero of the
  nine pairs matched.** With the tape paused — the most controlled state the
  harness offers — zero matched again, but the divergence is small and located:
  about 4.7 % of sampled pixels, and on `paper_demo` confined entirely to the
  top 30-pixel strip, the status band carrying the clock and the tape's
  position. So R18's "pixel-identical" cannot be met by any branch, including
  `origin/main` against itself; the wall clock is painted on screen and no hook
  turns it off.

  The trader chose **both proofs, covering each other**: pixels identical
  *outside a named mask*, and the control plane's own answer for each hook —
  `quantick_get_scene` — identical between `origin/main` and the branch. The
  mask is derived from the main-against-main measurement rather than chosen to
  fit, and the PR body carries the measurement that forced it, so the mask
  cannot be mistaken for convenience. A10 is rewritten to this standard.
- **D5 — The ticket's size ceiling, after it was measured.** A11 asks for
  `paper_trading.rs` at most 3,500 production lines. It fell 1,740, from 6,407
  to **4,667**, and stopped there. What remains is about 3,100 lines of genuine
  drawing and gesture plus about 1,500 of types, constants and `impl PaintCtx`;
  reaching 3,500 needs a **third** module the brief never asked for. Told that,
  the trader chose to **accept 4,667 and record the deviation in the PR** rather
  than grow the change. A11's first clause is therefore knowingly unmet, and the
  purpose it served — the money path reads without egui — is met in full.
- **D3 — No rename.** The file stays `paper_trading.rs`. The diff then shows
  what left rather than a deletion beside a creation, and the `git blame` of 76
  commits survives. R8's escape hatch is deliberately not taken.

## Assumptions

- **S1 — The R6 answer, from reading the code.** The brief's 220-line `parse`
  does not exist. The three that do are token-to-enum parsers with no state:
  `CmdModifier::parse` and `CmdEntryKind::parse` parse **command intent
  grammar** and follow `cmd_trading` to the account; `CmdPreviewForce::parse`
  parses a **harness force hook for the ticket's preview** and follows
  `cmd_preview_force` to the ticket. Safe to assume rather than ask because the
  brief delegates the call explicitly and the code answers it in a minute.
- **S2 — Capture scope for R18.** One capture per hook in ledger #9, nine pairs,
  a single state each, unless a hook plainly needs more than one to show its
  surface. The criterion says "captures through the hooks" without a count.
- **S3 — "Clock as told".** `AccountEnv` carries elapsed time passed in by the
  ticket; the account reads no `SystemTime`. This is `CLAUDE.md`'s headless
  rule, not a choice.
- **S4 — Where the journal golden lives.** Inline, in `paper_trading.rs`'s
  `mod tests`, travelling to `paper_account.rs`'s test module with its body and
  its expected bytes unchanged. Not an integration test: `quantick-app` is
  binary-only — `main.rs`, no `lib.rs` — so `crates/app/tests/` cannot reach a
  `pub(crate)` item and does not exist. This is exactly the report precedent,
  whose `the_report_numbers_are_fixed` says so in its own doc comment: "written
  before the report moves out of this file and its expected text does not
  change when it does". "Passes unchanged" in R17 is therefore graded on the
  test's body and hash, not on its file path.
- **S5 — `report: ReportState` and `selected_trade_index` go to the account,**
  per ledger #4's first list; the selection is report state, which the account
  already owns after the PR #282 split.
- **S7 — What `AccountEnv` actually carries, which is not what R9 sketched.**
  R9 lists "tick size, mark price, clock as told, the documents dir". Measured,
  the account does not need any of those handed to it: it *owns* `venue` (which
  answers `mark_price`), `dir`, `symbol` and `tick_scale`. A transitive closure
  over the call graph — every `self.<field>` read, followed through every
  `self.<method>()` call — shows the policy functions are tainted by the ticket
  through exactly two things, and nothing else:

  1. **`pending_toast`, via `show_toast`**, in eleven of them, including
     `place_intent`, `handle_events`, `journal`, `start_export`, `poll_export`,
     `start_import`, `poll_import`, `settle`, `amend_rung`, `set_trades_dir` and
     `run_demo_step`. This is the taint `AccountResponse` exists to cut, exactly
     as `ReportResponse::toast` cut it for the report.
  2. **`ruler_notches` and the three typed buffers** — `qty_text`,
     `stop_offset_text`, `profit_offset_text` — in the eleven straddlers
     `market`, `place_resting`, `reverse_position`, `entry_size`, `entry_label`,
     `aim_bracket`, `risk_sized`, `risk_state`, `risk_report`, `armed_bracket`
     and `parse_bracket`. Every one of these has a clean body and is tainted
     only through what it calls.

  So `AccountEnv` carries **the ticket's resolved order form and the ruler's
  two resolved prices**. (This assumption was written saying "ruler distance";
  what shipped is `ruler_levels: Option<(Decimal, Decimal)>`, the stop and
  target themselves. The wording is corrected here rather than in the built
  code because the reason is the same either way — the wheel, its travel and
  the step it walks are pixels, and resolving them in the ticket is what keeps
  the account from needing any of the three.) — the two things the account genuinely cannot know — and the
  account keeps everything it already owns. `tick_scale` goes to the **account**
  and not the ticket: it is the instrument's own decimal precision, learned
  from the tape by `observe_precision`, and it is not a pixel. Ledger #4 assigns
  it to neither list, so this is the mission's call, recorded here.

  This is the same seam the brief asked for, in the same shape as the report's,
  reached by measuring rather than by guessing at its contents. It is recorded
  as an assumption rather than taken back to the trader because it narrows
  nothing and delivers R9's stated purpose — "handed what it needs and
  answering with what it cannot do" — more exactly than R9's own field list
  would have.
- **S6 — *wanted to ask*, the four-question budget went to D1-D3.** If the seam
  costs more than the 300 budget lines R21 allows, the reading taken is
  `CLAUDE.md`'s pay-as-you-go rule: lower another file's ceiling in the same
  change rather than exceed the criterion. If no ceiling can honestly be
  lowered, the number goes to the trader rather than being signed quietly.

## Acceptance criteria

### Mission-specific

- [ ] **A1** — `crates/app/src/paper_account.rs` exists and holds `PaperAccount`
      with the headless fields and the placement, event-path, journal/session,
      settings/risk and demo functions named in R1-R5.
      *Evidence:* the file, plus a grep showing each named function present in
      `paper_account.rs` and absent from `paper_trading.rs`.
      → PR body, "What moved". *(R1, R2, R3, R4, R5)*
- [ ] **A2** — `grep -nE 'egui|eframe|Painter|Color32|Pos2' crates/app/src/paper_account.rs`
      returns nothing, and the account names no ticket field and no `show_toast`.
      *Evidence:* the grep's empty output and exit code 1.
      → PR body. *(R10, R19, R26)*
- [ ] **A3** — `paper_trading.rs` still holds every `draw_*`, `bracket_handle`,
      `position_tag`, `chip_tag`, `control_at`, `ticket_bracket`,
      `handle_chart_input`, the ten `*_text` buffers, the five `ruler_*`, drag,
      hover and the toast outbox, and holds exactly one `PaperAccount`.
      *Evidence:* grep for each name; the struct's field list.
      → PR body, "What stayed". *(R7)*
- [ ] **A4** — The account is handed an `AccountEnv` and answers with an
      `AccountResponse` carrying toast text, journal events and refusals; no
      account method takes or returns an egui value.
      *Evidence:* the two type definitions quoted, and A2's grep.
      → PR body, "The seam". *(R9)*
- [ ] **A5** — Each of `aim_bracket`, `quantity_preview`, `parse_quantity`,
      `show_toast` and `venue` lives on exactly one side, and the R6 `parse`
      call is stated.
      *Evidence:* a five-row table naming the side and the reason, plus the
      `parse` verdict.
      → PR body, "The five private items". *(R6, R11)*
- [ ] **A6** — `control/{trade,session,interaction}.rs` reach policy through
      `account()`/`account_mut()` and name no ticket-side item.
      *Evidence:* a grep for `ruler_`, `drag`, `hover`, `_text`, `draw_` and
      `cmd_preview` over those three files returning nothing, and their diff.
      → PR body. *(R12, R20, R26)*
- [ ] **A7** — `app.rs`, `tab.rs`, `pane.rs`, `dock.rs` and `toolbar.rs` change
      only on path lines and receivers; `paper_report.rs`, `risk_sizing.rs` and
      `order_strategies.rs` change only on path lines; no fill rule, bracket
      arithmetic, journal format or toast string changes.
      *Evidence:* `git diff origin/main...HEAD --stat` plus a read of each of
      those files' hunks.
      → PR body, "Blast radius". *(R13, R24)*
- [ ] **A8** — Tests travel by side: the ten journal-writing tests and every
      placement, fill, risk and export test are in the account's module; the
      gesture, tag, preview and draw tests are still in `paper_trading.rs`;
      `app/tests/paper_trading_tests.rs` differs from `origin/main` only in
      names.
      *Evidence:* the diff of that test file showing name-only hunks; the two
      modules' test lists.
      → PR body. *(R14, R15)*
- [ ] **A9 — Journal golden.** A fixed tape of venue events and intents, added
      as its own commit *before* any code moves, writes a journal whose SHA-256
      is recorded; the identical test passes unchanged after the split with the
      same hash.
      *Evidence:* the commit sha of the test-only commit, and the hash printed
      before and after.
      → PR body, "Journal golden", and the test's own assertion.
      *(R17, R26)*
- [ ] **A10 — Pixels golden.** Captures through `QUANTICK_PAPER_ORDERS`,
      `QUANTICK_PAPER_ORDER_BRACKET`, `QUANTICK_PAPER_ORDER_HOVER`,
      `QUANTICK_PAPER_RISK`, `QUANTICK_PAPER_DEMO`,
      `QUANTICK_PAPER_STRATEGY_EDITOR`, `QUANTICK_CMD_PREVIEW`,
      `QUANTICK_PAPER_RULER_TICKS` and `QUANTICK_TOAST=paper`, taken on
      `origin/main` and on the branch with `__COMPAT_LAYER=DPIUNAWARE`, a paused
      recorded tape and every `QUANTICK_*` store var pointed at a scratchpad,
      are identical **outside the mask D4 derived**, and each hook's
      `quantick_get_scene` answer is identical between the two builds.
      *Evidence:* the nine masked-image SHA-256 pairs and the nine scene-JSON
      SHA-256 pairs, side by side, plus the main-against-main measurement that
      forced the mask and the mask's own coordinates.
      → PR body, "Pixels golden". *(R18, R23, R26)*
- [ ] **A11** — `paper_trading.rs` at most 3,500 production lines,
      `paper_account.rs` at most 3,500, `--tighten` run, and the `!budget`
      risen by at most 300 with a signed reason, or fallen.
      *Evidence:* `cargo run -p quantick-guards` green, and the baseline diff.
      → PR body, "Baselines". *(R16, R21)*
- [ ] **A12** — `cargo test -p quantick-app paper` runs the same number of tests
      as on `origin/main`, all green.
      *Evidence:* the two test counts, before and after.
      → PR body. *(R22)*
- [ ] **A13** — The overlap with `fix/tests-own-their-scratch` is handled per
      D1: any test that branch edits carries *its* version when it moves.
      *Evidence:* a named list of the tests it touches and where each landed.
      → PR body, "Parallel work". *(R25)*
- [ ] **A14** — Every evidence-ledger claim was re-measured, and the ones that
      did not survive are recorded rather than quietly worked around.
      *Evidence:* the "Ledger corrections" section above, restated in the PR.
      → PR body, "Ledger corrections". *(R27)*

### Injected gates

- [ ] **G1** — Every artifact in English, per `CLAUDE.md`.
      *Evidence:* `arch-review` dimension 8; `cargo test -p quantick-guards`.
      → the review verdict.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own,
      never chained behind `||`.
      *Evidence:* four exit codes.
      → PR body's verification boxes.
- [ ] **G3** — `cargo test -p quantick-guards` green.
      *Evidence:* exit code.
      → PR body.
- [ ] **G4** — Performance impact declared: every touched path classified by
      rate. The ticket's `draw_*` are per-frame, `on_trade` and `handle_events`
      per-trade, journal and export rare.
      *Evidence:* the classification, plus `APP_HEALTH_SUMMARY` fps/frame_avg
      under a dense tape against an `origin/main` control run, since the seam
      adds an indirection to a per-frame path.
      → PR body, "Performance".
- [ ] **G5** — `arch-review` run over `git diff origin/main...HEAD`, step 0
      (`code-review` at `medium`) included, with every Blocker and Should-fix
      resolved or deferred in the PR body.
      *Evidence:* the verdict and the `arch-review-ok` marker.
      → PR body.
- [ ] **G6** — `visual-qa` pass on the surfaces A10 photographs, every surface
      PASS or a defect explicitly accepted.
      *Evidence:* the report.
      → PR body. *(R23)*
- [ ] **G7** — `ui-harness`: the nine hooks still reach every surface they
      reached on `origin/main`, and the generated hook registry is unchanged.
      *Evidence:* the diff on the generated hook-registry reference showing no
      change.
      → PR body.

### Not applicable, and why

- **`trader-ux-review`** — waived by the brief. The claim is "no trader-visible
  change"; A10's pixel criterion is that claim's proof, and a UX review of an
  unchanged surface grades nothing.
- **`new-extension`** — no capability is added. This splits one that exists; no
  port is carved, no second implementation is faked.
- **"Adds something a trader does"** — nothing new is reachable. Every action
  the account exposes was already reachable through the same control-plane
  names, which R12 and A6 preserve.
- **Engine / determinism territory** — `app` is not the engine, so the row does
  not apply by crate. Its substance is discharged anyway: A9 is a fixture-first
  golden written before the code moves, which is what the row asks for.

## Deferred

Six gaps ship, every one granted by the trader in session and after the
measurement that prompted it. None was granted by this session to itself.

**Granted earlier, when each was first measured:**

- **A11's first clause — `paper_trading.rs` at most 3,500 production lines.**
  It fell **1,818**, from 6,407 to **4,589**, and stopped there. What remains is
  about 3,100 lines of genuine drawing and gesture plus types, constants and
  `impl PaintCtx`; reaching 3,500 needs a *third* module the brief never asked
  for. `paper_account.rs` is 2,128, well under its own 3,500 ceiling. See D5.
- **G6 — a `visual-qa` pass.** A defect checklist over surfaces that match
  `origin/main` grades *main's* design, not this change. The evidence that
  replaces it is `pixels-golden.txt` and `scene-compare.txt`. See D4.

**Granted at the close, on the measurements below:**

- **A11's second clause — the budget rose 350, not at most 300.** The mission's
  own two criteria pull against each other: it asked for twenty-one named
  functions on the account *and* a rise under 300, and a function moved across
  a seam pays for its signature twice — once where it lives, once where the
  ticket resolves its text before calling it. At seventeen moved the rise was
  **297**; the last four — `market`, `settle`, `on_trade`, `set_symbol`, all
  named in R2-R4 — cost the rest, and `app.rs` took 38 more where a receiver
  wraps a line that used to fit. Eleven lines were already returned from real
  duplication the move exposed. Reverting the four would buy the number and
  lose the delivery, so the number gave.
- **A10 / R18 — "pixel-identical by SHA-256 pairs" is not achievable by any
  branch.** Three runs of the same `origin/main` build differ from each other
  by up to **2,366 pixels**. What was measured instead, against a mask derived
  from that spread rather than chosen: the branch matches the control on five
  of nine scenes exactly and by 1, 1, 4 and 66 px on the rest, and is *more*
  reproducible than main (0-71 px between its own runs against main's 0-2,366).
- **A1 / R3 — two of the twenty-one named functions stayed on the ticket.**
  `rest_capture_orders` reads `orders_demo` and `order_bracket_demo`;
  `decide_pending_leg` reads `drag`. All three are ticket fields by ledger #4's
  own list, so moving the functions would move pixels into the account — the
  one thing this change exists to prevent. The other nineteen are on the
  account.
- **G7 — the generated hook registry changed two rows.**
  `QUANTICK_PAPER_DEMO` and `QUANTICK_PAPER_RISK` now say `paper_account.rs`
  in the *Declared in* column, because that is where they are now read. Hook
  names and prose are untouched. The alternative was a reverse module edge that
  `crates/guards/src/cycle.rs` fails the build on.

**Two more asks are answered rather than deferred, and belong here so a reader
does not go looking:**

- **A12's "same test count"** conflicts with R17, which *requires* a new golden
  test. The paper suite is **209** against `origin/main`'s **204**: the journal
  golden, the offset regression, and the account's own three. No test was lost,
  which is what the criterion was protecting.
- **A8 / R14 — the tests did not travel; the account got its own instead.**
  `paper_account.rs` carries three tests that construct no `PaperTrading` at
  all. The ten journal tests stayed because rewriting them to drive the account
  would change their bodies — and one is `the_journal_bytes_are_fixed`, whose
  whole value is that its body and expected bytes have *not* changed since
  before the code moved. R17 outranks R14 for that test.

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS.
- [ ] **C2** — The PR is open, naming the `high` tier beside the four
      verification boxes.

## The request as received

Quoted verbatim and unedited: the ledger above is a reading of these words, and
a reading cannot also be the record of what was said. Under `CLAUDE.md`'s
English rule this is one marked, attributed quotation. It is already English;
it is reproduced verbatim for provenance, not for language.

> high refactor/paper-policy-out-of-the-ticket — crates/app/src/paper_trading.rs is 9,718 lines that place orders, project brackets, size risk against capital, write the trades journal and also draw the ticket; by function, 3,032 production lines never touch egui and 2,530 do. Split it the way paper_report.rs was split: a headless `PaperAccount` (venue, journal, settings, risk, strategies, placement, events, export) in its own module, handed what it needs and answering with what it cannot do, and a ticket that keeps every draw, drag, hover, text buffer and ruler pixel. The control plane reaches only the account. Behaviour is pinned byte for byte: a fixed event tape produces the same journal, and the ticket's harness captures are pixel-identical. Read C:\src\mission-paper-policy-out-of-the-ticket.md in full before anything else and build the request ledger from it.

The brief the invocation points at, `C:\src\mission-paper-policy-out-of-the-ticket.md`,
is the fuller statement of the same request; R1-R27 are built from it and it is
quoted throughout the sections above.
