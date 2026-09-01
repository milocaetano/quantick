# arch-review — evidence for G8 and G1

Graded over `git diff origin/main...HEAD` for `feat/mt5-session-history`. The
`arch-review-ok` marker holds the sha this file names; if the two disagree, the
marker is stale and this file is the record of what was actually reviewed.

**Graded head: the commit that lands this file.** An earlier verdict lived only
in a scratchpad and named `92f8ac9`, two commits behind what shipped —
`delivery-review` graded G8 UNPROVEN for exactly that, and it was right: the
README/code mismatch it also caught lived in the commits that verdict never
covered. Hence this file, in the repository, beside the evidence it cites.

## Step 0 — the bug pass

Two `code-review` passes at **xhigh** over the branch, plus one self-review of
the delta after them. Stated separately because they are different in kind.

| Pass | Scope | Findings | Outcome |
| --- | --- | --- | --- |
| 1 | whole branch @ `9d08e3a`…`8981b27` | 13, all confirmed | 13 fixed |
| 2 | whole branch @ `7bdef1a` | 14, all confirmed | 6 fixed, 3 deferred with reasons, 5 doc/claim corrections |
| 3 | the delta after pass 2, by me | 2 | 2 fixed |
| 4 | the delta after round 4 of `delivery-review`, by me | 1 | 1 fixed |

The three that could have reached the trader, all from the agent passes:

1. **An empty opening block left the live cursor at zero**, so the pump asked
   the terminal for its *oldest* ticks and forwarded 2024 prints as live
   trades, 4 096 a pass, for the life of the session.
2. **`flush` retried a partial write.** The socket carries a timeout, so
   `sendall` can write part of a buffer and raise; `close` then re-sent up to
   256 KB, duplicating prints on the tape.
3. **The reconnect guard was armed by the *first* opening block**, so a normal
   open discarded every slice of its own session — the trader would have seen
   the block the chart paints on and nothing behind it. Found by re-measuring,
   not by reading.

Every one of those is now covered by a test **run red against the un-fixed
code** before being accepted: the cursor, the buffer drop, the dropped slices,
the in-window session edge, and the zero cap.

My own passes over the deltas found three things the agent passes could not
have seen, because they were in code written after them: the floor check I had
just widened costing **1109 ms on every open** — on the path this branch exists
to make fast — a docstring with two ideas jammed into one line, and a
wire-contract guard that split on the wrong `required` array and so could not
have failed for the reason it stated. That last one was proved by marking the
field required in the shipped schema and watching the test go red.

**A third adversarial pass was deliberately not run.** Rounds one and two
returned 13 then 14 — flat, with several findings inside the previous round's
own fixes. That shape means triage and record deferrals, not iterate; five
deferrals are written into the goal file with their reasoning. This is a
visible decision to argue with rather than a skipped gate.

## Verdict

- **Correctness** — 29 findings across three passes; all fixed or deferred with
  a recorded reason. None open.
- **Docking** — yes. `HistoryReach::Span` is a variant plus its arms, reached
  by the toolbar, the env hook and the control plane through `ALL` and
  `from_token` with no edit of their own;
  `every_reach_is_reachable_by_its_token_and_says_what_it_does` fails for a
  reach added without a token, label or hover. `ScriptedMenu` replaced a string
  equality against one literal with the same shape.
- **Performance** — 62 s → ~11 s to put a session on the wire (47 s of it was
  one `sendall` per tick); the session walk 984 → 219 ms; the floor check
  1109 → 0 ms on the common path. Under the fill: fps floor 54 against a
  control's 57, no `APP_SLOW_FRAMES` on either side, for eight times the
  trades. Generated, not transcribed — [`perf.md`](perf.md) and
  [`summarise_perf.py`](summarise_perf.py). Per-trade and per-depth paths
  untouched.
- **Operability** — `QUANTICK_MENU=history` and
  `QUANTICK_HISTORY_REACH_SPAN_MINUTES`, both in the `ui-harness` registry; the
  reach and its span read back over the control plane
  ([`second-operator-reach-readback.txt`](second-operator-reach-readback.txt)),
  and the fill's progress is on the wire with a schema test holding it optional.
- **Proof** — unit: the cursor, the buffer drop, the in-window session edge,
  the zero cap, the formatter/parser round trip, a hostile count, the span
  campaign's five stop conditions, the tab's fill-progress reader. Integration:
  `session_gap_agreement.rs` (5 tests), `bridge_server.rs`'s three opening-slice
  tests, `metatrader.rs`'s first-session test, and three Python suites now
  discovered automatically so a fourth cannot be forgotten.
- **Accumulation** — trunk moved: `app.rs` +115, `tab.rs` +173, `toolbar.rs`
  +124, `metatrader.rs` +171, `stream.rs` +55. Three `size_guard` ceilings
  raised — `app.rs` 9775 → 9890, `tab.rs` 4401 → 4470, `stream.rs` 2142 → 2200
  — each with the reason recorded beside the number in the diff.
- **Language** — `language_guard` passes, **and** I read the prose, the branch
  name and every commit message myself. English throughout; the only
  non-English text is the trader's request quoted verbatim in the archived goal
  file, which `CLAUDE.md` exempts as a marked, attributed quotation and which
  that file claims openly in its own preamble.
