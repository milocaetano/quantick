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

## Pixels golden — nothing a trader sees moved

The criterion as written could not be met by **any** branch, and that was
measured before a line was moved: the nine hooks captured twice on the *same*
`origin/main` build, identical env, matched **zero of nine**. The wall clock
and the tape position are painted on screen and no hook turns them off.

So the mask was measured rather than chosen. Two runs of the same build differ
in exactly two horizontal bands — rows **0–30** and **662–672** — and nowhere
else. Outside them the chart, the ticket, the orders, the brackets, the risk
panel, the strategy editor and the toast are bit-identical run to run, which is
what makes it fair to demand they be bit-identical build to build.

| Comparison | Result |
| --- | --- |
| `origin/main` vs `origin/main` (the control) | **9/9 identical outside the mask** |
| branch vs `origin/main`, run 1 | **9/9 identical outside the mask** |
| branch vs `origin/main`, run 2 | **9/9 identical outside the mask** |

Fixture: a 4,000-print recording played to its end (a drained tape is a
deterministic screen; a playing one is not), `__COMPAT_LAYER=DPIUNAWARE`, every
`QUANTICK_*` store at a per-scene scratch path. Hashes, the mask and the
scripts are in `.claude/evidence/paper-policy-out-of-the-ticket/`.

### And the same answer from the application itself

A picture says what the window looks like; the control plane says what the
application *believes* is there, and that does not move when a colour does. The
`session.paper` snapshot — orders, position, risk, ruler, tick size, trades —
read from both builds through `quantick-mcp`:

| Scene | Stable fields | Result |
| --- | --- | --- |
| `paper_orders` | 64 | **identical** |
| `paper_order_bracket` | 72 | **identical** |
| `paper_risk` | 42 | **identical** |
| `paper_demo` | 84 | **identical** |

Four fields are excluded as environment rather than state, and they are the
only ones that differed: `instance_id` (a fresh process per launch),
`captured_at_unix_ms` and `capture_elapsed_us`.

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

## Baselines

`paper_trading.rs` falls **1,740** production lines, 6,407 → 4,667.
`paper_account.rs` arrives at **1,981**. `tab.rs` takes **2** for the receivers.
The `!budget` rises by the difference, **243**, signed in the entry with the
reason.

## Deviations, stated

1. **`paper_trading.rs` is 4,667, not the ≤3,500 the brief asked for.**
   Measured, not estimated: what remains is ~3,100 lines of genuine drawing and
   gesture plus ~1,500 of types, constants and `impl PaintCtx`. Reaching 3,500
   needs a *third* module (a paint module) that the brief did not ask for. The
   trader was asked and chose to accept 4,667 and record the deviation rather
   than grow the change. The purpose the ceiling served — the money path reads
   without egui — is met in full.
2. **The generated hook registry moved two rows.** `QUANTICK_PAPER_DEMO` and
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

## Verification

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy --workspace --all-targets`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 1,936 passed in the app bin against 1,935 on
      `origin/main`: exactly the one test this branch added.
- [x] `cargo test -p quantick-guards`

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01TxX2noHone4KtypvkqMMkt
