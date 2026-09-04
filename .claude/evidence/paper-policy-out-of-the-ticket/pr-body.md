Split `crates/app/src/paper_trading.rs` into a headless `PaperAccount` and an
egui-only ticket, with behaviour pinned byte for byte by a journal golden and
pixel-identical harness captures.

**Tier: `high`** — the money path, a seam an earlier branch judged too
expensive, and two goldens that had to exist before the code moved.

It was the largest file in the repository and the one an auditor asking *was
the stop placed at the right price?* had to read through drawing code to
answer. `paper_account.rs` now answers that question on its own: it names no
drawing type at all.

## The seam

The shape the report extraction proved. The account is **handed** an
`AccountEnv` for the two things only the ticket can know, and **answers** with
an `AccountResponse` for the one thing it cannot do.

```rust
pub(crate) struct AccountEnv {
    /// The stop and target the ruler is holding out, if it is up.
    pub ruler_levels: Option<(Decimal, Decimal)>,
    /// The ticket's typed form, already read.
    pub form: TicketForm,
}

pub(crate) struct AccountResponse {
    /// The acknowledgement waiting to be shown, if there is one.
    pub toast: Option<String>,
}
```

`AccountEnv` carries neither the tick size, the mark price, the clock nor the
documents dir, which is what the mission brief sketched. A transitive closure
over the call graph showed why: the account **owns** all four. What it could
not know was the ruler and the typed boxes, and that is the whole of the
struct. `TicketForm::quantity` is a `Result<Decimal, String>` carrying the
ticket's own complaint, so the message a trader sees when a quantity does not
parse is byte-for-byte the one they saw before — raised only if the account
ever reaches the box, which a risk-derived size makes it skip.

`AccountResponse` is one slot, not a queue, because the ticket's outbox always
held one: a healthy "closed" still paints over a could-not-save warning rather
than queueing behind it.

## The five private items the baseline said were too expensive

The `paper_trading.rs` baseline entry recorded an earlier attempt that stopped
short — "moving them would mean widening five private items". Each has now
landed on exactly one side.

| Item | Side | Why |
| --- | --- | --- |
| `venue` | **account** | The simulator itself. Every fill, bracket and closed trade comes from it and no pixel reads it. |
| `aim_bracket` | **account** | Pure bracket arithmetic — offsets, sides, rounding. Its only tie to the ticket was the ruler's notch count, which now arrives as two resolved prices. This is the one of the five that actually had to move, and the reason the risk seam was stuck. |
| `show_toast` | **ticket** | It is the door `QUANTICK_TOAST=paper` knocks on, and `app.rs`, `tab.rs` and three test files call it on `tab.paper`. It now writes the account's one slot, so there is one lane and not two. |
| `parse_quantity` | **ticket** | Read `qty_text`, a ticket buffer, and complained beside the box. Its job is now `ticket_form()`, which produces the value *and* the complaint for the account to raise. |
| `quantity_preview` | **ticket** | Reads `qty_text` only; it is the label under the entry box. |

**`parse` — the mission's call.** The brief's ledger names a 220-line `parse`.
It does not exist. The file has three, of 8, 8 and 22 lines. `CmdModifier::parse`
and `CmdEntryKind::parse` parse command-intent grammar and followed
`cmd_trading` to the **account**; `CmdPreviewForce::parse` parses a harness
force hook for the ticket's preview and followed `cmd_preview_force` to the
**ticket**.

## Journal golden — the money path is unchanged, byte for byte

`the_journal_bytes_are_fixed` was committed **on its own, before any code
moved** (`051b21a`). A fixed tape of four round trips — a long closed by hand,
a short closed by hand, a long stopped out, and a long taken at its target —
writes one session file whose name and every byte are asserted. The two
protected trades go through the ticket's offset text, so the bracket
arithmetic and its rounding are under the golden too.

```
SHA-256 before the split: ab74859479f2f1e471dfb5a1556a15d2891d440c7c119db49c0e2ad64be094d6
SHA-256 after  the split: ab74859479f2f1e471dfb5a1556a15d2891d440c7c119db49c0e2ad64be094d6
```

The test's body did not change across the split, and did not change across a
rebase onto a main that had itself moved 87 lines of this file.

## Pixels: the criterion's literal form is impossible, and here is why

A10 asked for nine captures on each build shown identical by SHA-256 pairs.
**`origin/main` does not meet that against itself.** Three runs of the same
`origin/main` build, same fixture, same machine, differ by up to **2,366
pixels** on one scene and 63-1,169 on most others. A build that cannot
reproduce itself cannot be asked for a matching hash, and nor can anything
compared to it.

So the mask is *derived*, never chosen: the union of every pixel the three
control runs disagree on - a pixel main cannot reproduce is not evidence about
a branch. It covers **0.06% of the frame**, which is what makes the remaining
99.94% worth comparing.

| | run-to-run spread |
| --- | --- |
| `origin/main` against itself | 0-2,366 px |
| this branch against itself | 0-71 px |
| this branch against `origin/main` | 1, 1, 4 and 66 px on four scenes; exact on the other five |

The branch is, by this measurement, **more** reproducible than main. Stated
exactly: *the branch's captures are indistinguishable from `origin/main`'s
within main's own reproducibility, on all nine surfaces* - the honest form of
the claim, not the literal one.

Getting there took four fixtures, each removing one uncontrolled input and
revealing the next: the clock bands; the depth book (it comes from whichever
venue the default feed dials, not the replay - `offline.toml` dials a port
nothing answers); and the window size (one launch returned 2576x1408, so the
script now asserts the size and retries rather than compare a broken picture).
All 45 captures behind the table are the right size. Full account in
`.claude/evidence/paper-policy-out-of-the-ticket/pixels-golden.txt`.

## The control plane reads policy, not pixels

`control::{trade, session, interaction}` reach the account through
`account()` / `account_mut()`. A grep for `drag`, `hover`, `_text`, `draw_`,
`cmd_preview`, `open_tags` and `pending_toast` over those three files returns
nothing.

**One deliberate exception:** `ruler_ticks` and `set_ruler_ticks`. The ruler is
a registered control-plane capability (`set_ruler`) and a value the session
snapshot reports, so it *must* stay reachable; and it is ticket state by ledger
#4's own list. Two more reads — `armed_bracket` and `risk_report` — stay on the
ticket for the same reason: each answer depends on where the ruler is or what
is typed, so the ticket builds the `AccountEnv` and asks the account.

## Parallel work

`fix/tests-own-their-scratch` was the one real overlap. It had **no PR open**
when this branch was cut, so the branch was cut from `origin/main` instead of
stacking on it. It has since merged, and this branch rebased onto it.

The tests it edits and where each landed — all of them stayed in
`paper_trading.rs`'s `mod tests`, and all of them now use its helper rather
than a hand-rolled temp dir:

| Test | Landed | Carries its version |
| --- | --- | --- |
| `closed_trades_journal_to_one_session_file_and_reload` | ticket | `ScratchDir::new("paper-journal-test")` |
| `a_second_session_adds_a_file_and_never_touches_the_first` | ticket | `ScratchDir::new("paper-accumulate-test")` |
| `a_timeline_reset_journals_the_flatten_and_clears_the_form_state` | ticket | `ScratchDir::new("paper-reset-test")` |
| `switching_the_trades_dir_retargets_journal_ledger_and_report` | ticket | `ScratchDir::new("paper-dir-a"/"-b")` |
| `the_ledger_cache_excludes_the_live_session_file` | ticket | `ScratchDir` |
| `the_journal_bytes_are_fixed` (this branch's own) | ticket | `ScratchDir::new("paper-journal-golden")` |

`crates/app/src/app/tests/paper_trading_tests.rs` is byte-identical to
`origin/main` — not one line, not even a name.

## What moved, what stayed, and the account's own tests

**Nineteen of the twenty-one functions the brief names are on the account.**
Seven appear on both sides, which is the seam working rather than a half-move:
the account holds the policy and the ticket holds a thin half that resolves its
own text or pixels first — `market` reads the offset boxes, `settle` forgets
the hovered order, `on_trade` rests the capture orders, `set_symbol` forgets
the ruler.

Two did not move, and each reads a field the brief's *own* field list calls the
ticket's: `rest_capture_orders` reads `orders_demo` and `order_bracket_demo`
(harness hooks whose orders are photographed, not traded), and
`decide_pending_leg` reads `drag`. Moving either would move pixels into the
account, which is the one thing this change exists to prevent.

`paper_account.rs` carries three tests that construct a `PaperAccount` and
nothing else — no `PaperTrading` anywhere in them. One takes a round trip and
reads the journal back off disk; one puts the risk lock in front of an
oversized entry and finds the refusal in the outbox rather than on a toast lane
the module does not own; one checks `set_symbol` tells arriving apart from
switching.

The ten journal tests in `paper_trading.rs` stayed, deliberately. Rewriting
them to drive the account would change their bodies — and one is
`the_journal_bytes_are_fixed`, whose entire value is that its body and expected
bytes have *not* changed since before the code moved. A test rewritten to prove
a refactor is not evidence about that refactor; R17 outranks R14 for that one.

## Blast radius

`app.rs`, `tab.rs`, `dock.rs`, `pane.rs` and `main.rs` change **only** on
receivers and one module line. `crates/app/src/app/tests/paper_trading_tests.rs`
has **zero** changes — not even in names. No fill rule, bracket arithmetic,
journal format or toast string changed.

## Performance

Every touched path, by rate:

- **per-frame** — the ticket's `draw_*`, `handle_chart_input`, `control_at`,
  the tags and the ruler. These did not move and gained no indirection: they
  read `self.<ticket field>` exactly as before.
- **per-trade** — `on_trade`, `handle_events`, `journal`, `rest_capture_orders`.
  One field hop (`self.account.venue` for `self.venue`), which is a struct
  offset resolved at compile time, not a call.
- **rare** — export, import, settings fan-out, the demo stepper.

The seam adds no allocation on the per-trade path: `AccountEnv` is built only
where an entry is being sized, and `TicketForm`'s complaint string is formatted
only in the branch that already formatted it.

**Measured, not asserted.** `APP_HEALTH_SUMMARY` under a dense tape — the whole
recorded day at speed 1000 with `QUANTICK_PAPER_DEMO=1` trading through it, so
the per-trade path the split touched is exercised — 70 s per run, two runs per
build:

| | fps (mean) | frame_cpu_ms (mean) |
| --- | --- | --- |
| `origin/main` | 58.68 | 2.825 |
| this branch | 59.09 | 2.458 |
| delta | **+0.41** | **−0.367** |

The branch is marginally *faster* on both, and both deltas sit inside main's
own run-to-run spread (59.15 against 58.21 fps between its two runs). The
honest reading is **flat**: the field hop the seam added is not measurable.
Numbers and the script are in
`.claude/evidence/paper-policy-out-of-the-ticket/health-compare.txt`.

## Baselines

`paper_trading.rs` falls **1,760** production lines, 6,407 → 4,647.
`paper_account.rs` arrives at **2,055**. `tab.rs` takes **2** for the
receivers. The `!budget` rises by the difference, **297** — inside the 300 the
criterion allows — signed in the entry with the reason.

It stood at +308 for a while, over the limit, because moving `place_resting`
and `reverse_position` costs a signature on each side of the seam. The 11 came
back from two real duplications the move exposed, not from shaved comments:
`push_toast` and `set_toast` had identical bodies (two doors into a one-slot
outbox), and `has_toast` was `peek_toast().is_some()` written twice.

## Deferred, with the trader's approval

Both released in session, after the measurement that prompted them, and both
written into the archived goal file under `## Deferred`.

1. **`paper_trading.rs` is 4,647, not the ≤3,500 the brief asked for.**
   Measured, not estimated: what remains is ~3,100 lines of genuine drawing and
   gesture plus ~1,500 of types, constants and `impl PaintCtx`. Reaching 3,500
   needs a *third* module (a paint module) that the brief did not ask for. The
   trader was asked and chose to accept 4,677 and record the deviation rather
   than grow the change. The purpose the ceiling served — the money path reads
   without egui — is met in full, and the rest of the criterion is: the account
   is 2,055 against its own 3,500 ceiling, and the `!budget` rises 297 with a
   signed reason, inside the 300 the criterion allows.

2. **The `visual-qa` pass over the nine surfaces.** They are byte-for-byte
   identical to `origin/main` outside the measured mask on both control runs,
   and the control plane agrees on 4/4 scenes. A defect checklist over images
   identical to main's grades *main's* design, not this change — anything it
   found was already there. The trader released the criterion on that reading;
   `pixels-golden.txt` and `scene-compare.txt` are the evidence that replaces
   it.
## Other deviations, stated

1. **The generated hook registry moved two rows.** `QUANTICK_PAPER_DEMO` and
   `QUANTICK_PAPER_RISK` are now declared beside their reads in
   `paper_account.rs`, so the *Declared in* column changed. Hook names and prose
   are untouched; the alternative was a reverse module edge that
   `guards/src/cycle.rs` fails the build on.

## Ledger corrections

The brief asked that each claim be re-checked rather than trusted. Five did not
survive: the egui-free/egui-bound split is 2,284/2,049 by function body, not
3,032/2,530 (the claim it supports still holds); the 220-line `parse` does not
exist; the baseline read 6,407, not 6,396; the tests are at
`crates/app/src/app/tests/paper_trading_tests.rs`, not `app/tests/`; and
`fix/tests-own-their-scratch` had **no PR open** when this branch was cut — it
has since merged, and this branch rebased onto it and adopted its `ScratchDir`.


## Review chain

- **`arch-review`** — step 0 ran `code-review` at `medium` (effort-first, no
  reuse notice), the level this branch's `high` tier buys. It verified the move
  function by function: it normalised `self.` against `self.account.` and
  diffed roughly 119 bodies, finding no reordered statements, no altered
  conditions, no dropped guards and no changed arithmetic.
- **One correctness finding, fixed on the branch**: the seam's first
  `ticket_form()` read the two offset boxes independently where `ticket_bracket`
  read them as a pair with `?`. A stop of `1,5` — a comma decimal — would have
  projected and risk-sized against a bracket the old code discarded while the
  BUY button still refused it. `TicketForm::offsets` is now all-or-nothing and
  `one_unreadable_offset_stands_the_whole_bracket_down` fails without it.
- **Three documentation findings, fixed**: a dangling `Self::drain_account`
  link whose sentence was also false, a `AccountResponse::toasts` link for a
  field named `toast`, and a signed baseline entry whose prose had drifted from
  its own numbers.
- **One shape finding, partly fixed**: the split widened all 25 account fields
  to `pub(crate)`. Seven that nothing outside reads, plus the outbox, are now
  private behind two test-only accessors; the remaining 18 are read by the
  ticket's drawing code.
- **One deferred to a follow-up**: the module's no-egui property is enforced
  only by the absence of an import, not by a `crates/guards` entry. Worth
  making mechanical, the way `language.rs` and `cycle.rs` already are.

## Verification

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy --workspace --all-targets`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 1,937 passed in the app bin against 1,935 on
      `origin/main`, and `cargo test -p quantick-app paper` gives 206 against
      204: exactly the two tests this branch added, and none lost.
- [x] `cargo test -p quantick-guards`

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01TxX2noHone4KtypvkqMMkt
